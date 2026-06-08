use crate::{
    backend::{BackendError, Result},
    device::output_device_by_id,
};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, SampleFormat, SampleRate as CpalSampleRate, Stream, StreamConfig,
};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer, RingBuffer};
use seraph_core::{EventBus, PlayerEvent};
use seraph_decoder::{open_decoder, Decoder, StreamInfo};
use seraph_dsp::{resample_interleaved_linear, StatefulSincResampler};
use seraph_visualizer::{SimpleVisualizer, Visualizer};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tracing::{debug, warn};

const TARGET_BUFFER_SECONDS: usize = 3;
const QUEUE_SLEEP: Duration = Duration::from_millis(5);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const SPECTRUM_INTERVAL: Duration = Duration::from_millis(66);
const SPECTRUM_FFT_SIZE: usize = 1024;
const SPECTRUM_BINS: usize = 32;
const DEFAULT_EXCLUSIVE_PERIOD_FRAMES: u32 = 512;

#[derive(Clone)]
pub struct PlaybackController {
    tx: Sender<PlaybackCommand>,
}

enum PlaybackCommand {
    PlayFile {
        path: PathBuf,
        track_id: String,
        start_seconds: f64,
    },
    Resume,
    Pause,
    Stop,
    Seek(f64),
    SetOutputDevice(String),
    SetDriver(String),
    SetVolume(f32),
}

impl PlaybackController {
    pub fn new(event_bus: EventBus) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut engine = PlaybackEngine::new(event_bus.clone());
            while let Ok(command) = rx.recv() {
                let result = match command {
                    PlaybackCommand::PlayFile {
                        path,
                        track_id,
                        start_seconds,
                    } => engine.play_file(path, track_id, start_seconds),
                    PlaybackCommand::Resume => engine.resume(),
                    PlaybackCommand::Pause => engine.pause(),
                    PlaybackCommand::Stop => engine.stop(),
                    PlaybackCommand::Seek(seconds) => engine.seek(seconds),
                    PlaybackCommand::SetOutputDevice(device_id) => {
                        engine.set_output_device(device_id)
                    }
                    PlaybackCommand::SetDriver(driver) => engine.set_driver(driver),
                    PlaybackCommand::SetVolume(volume) => engine.set_volume(volume),
                };

                if let Err(err) = result {
                    event_bus.publish(PlayerEvent::Error {
                        message: err.to_string(),
                    });
                    event_bus.publish(PlayerEvent::PlaybackStopped);
                }
            }
        });

        Self { tx }
    }

    pub fn play_file(&self, path: PathBuf, track_id: String, start_seconds: f64) -> Result<()> {
        self.send(PlaybackCommand::PlayFile {
            path,
            track_id,
            start_seconds,
        })
    }

    pub fn resume(&self) -> Result<()> {
        self.send(PlaybackCommand::Resume)
    }

    pub fn pause(&self) -> Result<()> {
        self.send(PlaybackCommand::Pause)
    }

    pub fn stop(&self) -> Result<()> {
        self.send(PlaybackCommand::Stop)
    }

    pub fn seek(&self, seconds: f64) -> Result<()> {
        self.send(PlaybackCommand::Seek(seconds))
    }

    pub fn set_volume(&self, volume: f32) -> Result<()> {
        self.send(PlaybackCommand::SetVolume(volume))
    }

    pub fn set_output_device(&self, device_id: String) -> Result<()> {
        self.send(PlaybackCommand::SetOutputDevice(device_id))
    }

    pub fn set_driver(&self, driver: String) -> Result<()> {
        self.send(PlaybackCommand::SetDriver(driver))
    }

    fn send(&self, command: PlaybackCommand) -> Result<()> {
        self.tx
            .send(command)
            .map_err(|_| BackendError::Internal("audio thread is not available".into()))
    }
}

pub struct PlaybackEngine {
    event_bus: EventBus,
    session: Option<PlaybackSession>,
    volume: f32,
    selected_device_id: Option<String>,
    driver: OutputDriver,
}

struct PlaybackSession {
    path: PathBuf,
    track_id: String,
    duration_seconds: f64,
    shared: Arc<PlaybackShared>,
    decode_worker: Option<JoinHandle<()>>,
    render_worker: Option<JoinHandle<Result<()>>>,
    _stream: Option<Stream>,
}

struct PlaybackShared {
    seek_request: Mutex<Option<f64>>,
    paused: AtomicBool,
    stopped: AtomicBool,
    frame_position: AtomicU64,
    volume_bits: AtomicU32,
    buffer_generation: AtomicU32,
    output_rate: u32,
    output_channels: usize,
    max_buffer_samples: usize,
}

#[derive(Clone, Copy)]
struct QueuedSample {
    // L-4: generation 用 u32 而非 u64，配合 f32 value 后单样本 8 字节（vs 16 字节）。
    // 768kHz×2ch×3s 缓冲下，内存占用从 ~72MB 降到 ~36MB；普通 192kHz×2×3s 从 ~18MB → ~9MB。
    // u32 可表示 ~42 亿个 seek/clear，远超合理使用次数（折算到 1ms 一次也要 49 天）。
    generation: u32,
    value: f32,
}

struct DecodeWorkerInput {
    decoder: Box<dyn Decoder>,
    path: PathBuf,
    track_id: String,
    info: StreamInfo,
    shared: Arc<PlaybackShared>,
    producer: Producer<QueuedSample>,
    event_bus: EventBus,
    start_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputDriver {
    Shared,
    WasapiExclusive,
    Asio,
}

impl OutputDriver {
    fn from_frontend_value(value: &str) -> Result<Self> {
        match value {
            "wasapi" => Ok(Self::WasapiExclusive),
            "direct" => Ok(Self::Shared),
            "asio" => Ok(Self::Asio),
            other => Err(BackendError::UnsupportedFormat(format!(
                "unknown output driver: {other}"
            ))),
        }
    }
}

impl PlaybackEngine {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            session: None,
            volume: 0.7,
            selected_device_id: None,
            driver: OutputDriver::WasapiExclusive,
        }
    }

    pub fn play_file(&mut self, path: PathBuf, track_id: String, start_seconds: f64) -> Result<()> {
        // 同一首歌「无缝」复用 session 的优化路径：
        // 仅当 worker 仍存活、track 未结束时才走 resume；否则一律重建 session，
        // 避免 H-3 描述的「UI 显示在播但 worker 已退出」假象。
        let can_resume = self.session.as_ref().is_some_and(|session| {
            session.path == path
                && session.track_id == track_id
                && !session.shared.stopped.load(Ordering::Acquire)
                && session
                    .decode_worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
        });
        if can_resume {
            if start_seconds > 0.0 {
                self.seek(start_seconds)?;
            }
            return self.resume();
        }

        // 先停旧 session，再 open 解码器：
        // 即使 open 失败也保证旧的播放真的停了（否则前端 UI 已切歌但实际仍在播旧曲）。
        self.stop_session();

        let decoder = match open_decoder(&path) {
            Ok(decoder) => decoder,
            Err(err) => {
                let backend_err = BackendError::UnsupportedFormat(err.to_string());
                self.event_bus.publish(PlayerEvent::Error {
                    message: backend_err.to_string(),
                });
                self.event_bus.publish(PlayerEvent::PlaybackStopped);
                return Err(backend_err);
            }
        };
        let info = match decoder.info().cloned() {
            Some(info) => info,
            None => {
                let backend_err =
                    BackendError::Internal("decoder opened without stream info".into());
                self.event_bus.publish(PlayerEvent::Error {
                    message: backend_err.to_string(),
                });
                self.event_bus.publish(PlayerEvent::PlaybackStopped);
                return Err(backend_err);
            }
        };
        let duration_seconds = info.duration_seconds;

        if self.driver == OutputDriver::Asio {
            return Err(BackendError::NotImplemented);
        }

        let device = self.output_device()?;
        let device_name = device
            .name()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let (sample_format, config) = select_output_config(&device, &info, self.driver)?;
        let output_rate = config.sample_rate.0;
        let output_channels = usize::from(config.channels).max(1);
        let shared = Arc::new(PlaybackShared::new(
            output_rate,
            output_channels,
            self.volume,
        ));
        let (producer, consumer) = RingBuffer::new(shared.max_buffer_samples);
        shared.frame_position.store(
            seconds_to_frames(start_seconds, output_rate),
            Ordering::Relaxed,
        );

        let (stream, render_worker) = match self.driver {
            OutputDriver::WasapiExclusive => {
                let worker = spawn_wasapi_exclusive_render_worker(
                    self.selected_device_id.clone(),
                    device_name,
                    config.clone(),
                    sample_format,
                    shared.clone(),
                    consumer,
                )?;
                (None, Some(worker))
            }
            OutputDriver::Shared => {
                let stream =
                    build_output_stream(&device, &config, sample_format, shared.clone(), consumer)?;
                stream
                    .play()
                    .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
                (Some(stream), None)
            }
            OutputDriver::Asio => unreachable!("ASIO checked above"),
        };

        let worker = spawn_decode_worker(DecodeWorkerInput {
            decoder,
            path: path.clone(),
            track_id: track_id.clone(),
            info,
            shared: shared.clone(),
            producer,
            event_bus: self.event_bus.clone(),
            start_seconds,
        });

        debug!(
            "started playback: {} @ {} Hz / {} ch / {:?}",
            path.display(),
            output_rate,
            output_channels,
            self.driver
        );
        self.session = Some(PlaybackSession {
            path,
            track_id: track_id.clone(),
            duration_seconds,
            shared,
            decode_worker: Some(worker),
            render_worker,
            _stream: stream,
        });
        self.event_bus.publish(PlayerEvent::TrackChanged {
            track_id: track_id.clone(),
        });
        self.event_bus
            .publish(PlayerEvent::PlaybackStarted { track_id });
        Ok(())
    }

    pub fn set_driver(&mut self, driver: String) -> Result<()> {
        let next = OutputDriver::from_frontend_value(&driver)?;
        if self.driver == next {
            return Ok(());
        }

        self.driver = next;
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };

        let path = session.path.clone();
        let track_id = session.track_id.clone();
        let seconds = session.shared.progress_seconds();
        let was_paused = session.shared.paused.load(Ordering::Acquire);
        self.stop_session();
        self.play_file(path, track_id, seconds)?;
        if was_paused {
            self.pause()?;
        }

        Ok(())
    }

    pub fn resume(&mut self) -> Result<()> {
        let Some(session) = self.session.as_ref() else {
            return Err(BackendError::Internal("no loaded track".into()));
        };

        session.shared.paused.store(false, Ordering::Release);
        self.event_bus.publish(PlayerEvent::PlaybackResumed);
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };

        session.shared.paused.store(true, Ordering::Release);
        self.event_bus.publish(PlayerEvent::PlaybackPaused);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.stop_session();
        self.event_bus.publish(PlayerEvent::PlaybackStopped);
        Ok(())
    }

    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };

        let seconds = seconds.max(0.0);
        session.shared.frame_position.store(
            seconds_to_frames(seconds, session.shared.output_rate),
            Ordering::Relaxed,
        );
        session.shared.next_buffer_generation();
        *session.shared.seek_request.lock() = Some(seconds);
        self.event_bus.publish(PlayerEvent::Progress {
            track_id: session.track_id.clone(),
            seconds,
            total: session.duration_seconds,
        });
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<()> {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(session) = self.session.as_ref() {
            session.shared.set_volume(self.volume);
        }
        self.event_bus.publish(PlayerEvent::VolumeChanged {
            volume: self.volume,
        });
        Ok(())
    }

    pub fn set_output_device(&mut self, device_id: String) -> Result<()> {
        output_device_by_id(&device_id)?;
        if self.selected_device_id.as_deref() == Some(device_id.as_str()) {
            return Ok(());
        }

        self.selected_device_id = Some(device_id);
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };

        let path = session.path.clone();
        let track_id = session.track_id.clone();
        let seconds = session.shared.progress_seconds();
        let was_paused = session.shared.paused.load(Ordering::Acquire);
        self.stop_session();
        self.play_file(path, track_id, seconds)?;
        if was_paused {
            self.pause()?;
        }

        Ok(())
    }

    fn output_device(&self) -> Result<cpal::Device> {
        if let Some(device_id) = self.selected_device_id.as_deref() {
            return output_device_by_id(device_id);
        }

        cpal::default_host()
            .default_output_device()
            .ok_or_else(|| BackendError::DeviceLost("no default output device".into()))
    }

    fn stop_session(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };

        session.shared.stopped.store(true, Ordering::Release);
        if let Some(worker) = session.decode_worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = session.render_worker.take() {
            if let Ok(Err(err)) = worker.join() {
                warn!("audio render worker stopped with error: {err}");
            }
        }
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        self.stop_session();
    }
}

impl PlaybackShared {
    fn new(output_rate: u32, output_channels: usize, volume: f32) -> Self {
        let max_buffer_samples = output_rate as usize * output_channels * TARGET_BUFFER_SECONDS;
        Self {
            seek_request: Mutex::new(None),
            paused: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            frame_position: AtomicU64::new(0),
            volume_bits: AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()),
            buffer_generation: AtomicU32::new(0),
            output_rate,
            output_channels,
            max_buffer_samples,
        }
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }

    fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn buffer_generation(&self) -> u32 {
        self.buffer_generation.load(Ordering::Acquire)
    }

    fn next_buffer_generation(&self) {
        self.buffer_generation.fetch_add(1, Ordering::AcqRel);
    }

    fn progress_seconds(&self) -> f64 {
        self.frame_position.load(Ordering::Relaxed) as f64 / self.output_rate.max(1) as f64
    }
}

fn build_output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
    mut consumer: Consumer<QueuedSample>,
) -> Result<Stream> {
    let err_fn = |err| warn!("audio output stream error: {err}");
    let mut observed_generation = shared.buffer_generation();
    match sample_format {
        SampleFormat::F32 => device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    render_output_f32(data, &shared, &mut consumer, &mut observed_generation)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::I16 => device
            .build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    render_output_i16(data, &shared, &mut consumer, &mut observed_generation)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::U16 => device
            .build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    render_output_u16(data, &shared, &mut consumer, &mut observed_generation)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::F64 => device
            .build_output_stream(
                config,
                move |data: &mut [f64], _| {
                    render_output_f64(data, &shared, &mut consumer, &mut observed_generation)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        other => Err(BackendError::UnsupportedFormat(format!(
            "output sample format {other:?}"
        ))),
    }
}

fn select_output_config(
    device: &cpal::Device,
    info: &StreamInfo,
    driver: OutputDriver,
) -> Result<(SampleFormat, StreamConfig)> {
    if driver == OutputDriver::WasapiExclusive {
        let channels = info.channels.0.max(1);
        let sample_format = if info.bit_depth.0 <= 16 {
            SampleFormat::I16
        } else {
            SampleFormat::I32
        };
        return Ok((
            sample_format,
            StreamConfig {
                channels,
                sample_rate: CpalSampleRate(info.sample_rate.0.max(1)),
                buffer_size: BufferSize::Fixed(DEFAULT_EXCLUSIVE_PERIOD_FRAMES),
            },
        ));
    }

    let configs = device
        .supported_output_configs()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
    let desired_rate = CpalSampleRate(info.sample_rate.0.max(1));
    let desired_channels = info.channels.0.max(1);

    for range in configs {
        if range.channels() != desired_channels {
            continue;
        }
        if let Some(config) = range.try_with_sample_rate(desired_rate) {
            let sample_format = config.sample_format();
            return Ok((sample_format, config.into()));
        }
    }

    let supported_config = device
        .default_output_config()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
    let sample_format = supported_config.sample_format();
    Ok((sample_format, supported_config.into()))
}

#[cfg(windows)]
fn spawn_wasapi_exclusive_render_worker(
    selected_device_id: Option<String>,
    device_name: String,
    config: StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
    consumer: Consumer<QueuedSample>,
) -> Result<JoinHandle<Result<()>>> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let shared_for_worker = shared.clone();
    let worker = thread::spawn(move || {
        run_wasapi_exclusive_render_worker(
            selected_device_id,
            device_name,
            config,
            sample_format,
            shared_for_worker,
            consumer,
            ready_tx,
        )
    });

    // 等待 worker 完成 IAudioClient::Initialize + start_stream。
    // 慢速 DAC / 高采样率独占协商可能需要数秒，给 8 秒上限。
    match ready_rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Ok(())) => Ok(worker),
        Ok(Err(message)) => {
            shared.stopped.store(true, Ordering::Release);
            let _ = worker.join();
            Err(BackendError::ExclusiveModeUnavailable(message))
        }
        Err(_) => {
            // 超时：视为启动失败，让 worker 优雅退出
            shared.stopped.store(true, Ordering::Release);
            let _ = worker.join();
            Err(BackendError::ExclusiveModeUnavailable(
                "WASAPI exclusive stream init timed out".into(),
            ))
        }
    }
}

#[cfg(not(windows))]
fn spawn_wasapi_exclusive_render_worker(
    _selected_device_id: Option<String>,
    _device_name: String,
    _config: StreamConfig,
    _sample_format: SampleFormat,
    _shared: Arc<PlaybackShared>,
    _consumer: Consumer<QueuedSample>,
) -> Result<JoinHandle<Result<()>>> {
    Err(BackendError::ExclusiveModeUnavailable(
        "WASAPI exclusive output is only available on Windows".into(),
    ))
}

#[cfg(windows)]
fn run_wasapi_exclusive_render_worker(
    selected_device_id: Option<String>,
    device_name: String,
    config: StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
    mut consumer: Consumer<QueuedSample>,
    ready_tx: Sender<std::result::Result<(), String>>,
) -> Result<()> {
    use wasapi::{Direction, SampleType, StreamMode, WaveFormat};

    // 任何提前 return 都通过这个 helper 把失败原因送回主线程，
    // 避免 spawn_wasapi_exclusive_render_worker 在 recv_timeout 处永远等待。
    let signal_err = |tx: &Sender<std::result::Result<(), String>>, err: &BackendError| {
        let _ = tx.send(Err(err.to_string()));
    };

    let init_result: Result<(
        wasapi::AudioClient,
        wasapi::AudioRenderClient,
        u32,
        Duration,
    )> = (|| {
        wasapi::initialize_mta()
            .ok()
            .map_err(|err| BackendError::Internal(err.to_string()))?;

        let enumerator = wasapi::DeviceEnumerator::new()
            .map_err(|err| BackendError::Internal(err.to_string()))?;
        let device = if selected_device_id.is_some() {
            enumerator
                .get_device_collection(&Direction::Render)
                .and_then(|collection| collection.get_device_with_name(&device_name))
                .map_err(|err| BackendError::DeviceLost(err.to_string()))?
        } else {
            enumerator
                .get_default_device(&Direction::Render)
                .map_err(|err| BackendError::DeviceLost(err.to_string()))?
        };

        let mut audio_client = device
            .get_iaudioclient()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let sample_type = match sample_format {
            SampleFormat::I16 | SampleFormat::I32 => SampleType::Int,
            SampleFormat::F32 => SampleType::Float,
            other => {
                return Err(BackendError::UnsupportedFormat(format!(
                    "exclusive output sample format {other:?}"
                )));
            }
        };
        let valid_bits = match sample_format {
            SampleFormat::I16 => 16,
            SampleFormat::I32 => 24,
            SampleFormat::F32 => 32,
            _ => 32,
        };
        let store_bits = if sample_format == SampleFormat::I16 {
            16
        } else {
            32
        };
        let desired_format = WaveFormat::new(
            store_bits,
            valid_bits,
            &sample_type,
            config.sample_rate.0 as usize,
            usize::from(config.channels),
            None,
        );
        let desired_format = audio_client
            .is_supported_exclusive_with_quirks(&desired_format)
            .map_err(|err| exclusive_mode_unavailable(err.to_string()))?;
        let desired_period = wasapi::calculate_period_100ns(
            i64::from(DEFAULT_EXCLUSIVE_PERIOD_FRAMES),
            i64::from(desired_format.get_samplespersec()),
        );
        let period = audio_client
            .calculate_aligned_period_near(desired_period, Some(128), &desired_format)
            .unwrap_or(desired_period);
        let mode = StreamMode::PollingExclusive {
            period_hns: period,
            buffer_duration_hns: 16 * period,
        };

        audio_client
            .initialize_client(&desired_format, &Direction::Render, &mode)
            .or_else(|err| {
                let buffer_size = audio_client.get_buffer_size()?;
                let aligned_period = wasapi::calculate_period_100ns(
                    i64::from(buffer_size),
                    i64::from(desired_format.get_samplespersec()),
                );
                audio_client = device.get_iaudioclient()?;
                let mode = StreamMode::PollingExclusive {
                    period_hns: aligned_period,
                    buffer_duration_hns: 16 * aligned_period,
                };
                audio_client
                    .initialize_client(&desired_format, &Direction::Render, &mode)
                    .map_err(|_| err)
            })
            .map_err(|err| exclusive_mode_unavailable(err.to_string()))?;

        let render_client = audio_client
            .get_audiorenderclient()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let buffer_frames = audio_client
            .get_buffer_size()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let sleep_period = Duration::from_millis(
            (500 * u64::from(buffer_frames) / u64::from(config.sample_rate.0.max(1))).max(1),
        );
        audio_client
            .start_stream()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

        Ok((audio_client, render_client, buffer_frames, sleep_period))
    })();

    let (audio_client, render_client, _buffer_frames, sleep_period) = match init_result {
        Ok(parts) => {
            // 启动成功才通知主线程：避免 H-2 描述的「先 Started 后 Error」乱序
            let _ = ready_tx.send(Ok(()));
            parts
        }
        Err(err) => {
            signal_err(&ready_tx, &err);
            return Err(err);
        }
    };

    let mut observed_generation = shared.buffer_generation();

    while !shared.stopped.load(Ordering::Acquire) {
        let frames = audio_client
            .get_available_space_in_frames()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        if frames > 0 {
            let bytes = render_wasapi_output_bytes(
                frames as usize,
                sample_format,
                &shared,
                &mut consumer,
                &mut observed_generation,
            );
            render_client
                .write_to_device(frames as usize, &bytes, None)
                .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        }
        thread::sleep(sleep_period);
    }

    let _ = audio_client.stop_stream();
    Ok(())
}

fn exclusive_mode_unavailable(detail: String) -> BackendError {
    BackendError::ExclusiveModeUnavailable(detail)
}

fn render_wasapi_output_bytes(
    frames: usize,
    sample_format: SampleFormat,
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    observed_generation: &mut u32,
) -> Vec<u8> {
    let channels = shared.output_channels.max(1);
    let sample_count = frames * channels;
    let mut samples = vec![0.0_f32; sample_count];
    render_output(
        &mut samples,
        shared,
        consumer,
        observed_generation,
        |sample, value| *sample = value,
    );

    match sample_format {
        SampleFormat::I16 => {
            let mut bytes = Vec::with_capacity(sample_count * 2);
            for sample in samples {
                bytes.extend_from_slice(
                    &((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes(),
                );
            }
            bytes
        }
        SampleFormat::I32 => {
            let mut bytes = Vec::with_capacity(sample_count * 4);
            for sample in samples {
                let value = (sample.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
                bytes.extend_from_slice(&(value << 8).to_le_bytes());
            }
            bytes
        }
        SampleFormat::F32 => {
            let mut bytes = Vec::with_capacity(sample_count * 4);
            for sample in samples {
                bytes.extend_from_slice(&sample.clamp(-1.0, 1.0).to_le_bytes());
            }
            bytes
        }
        _ => Vec::new(),
    }
}

fn spawn_decode_worker(input: DecodeWorkerInput) -> JoinHandle<()> {
    thread::spawn(move || {
        let DecodeWorkerInput {
            decoder,
            path,
            track_id,
            info,
            shared,
            producer,
            event_bus,
            start_seconds,
        } = input;
        let result = run_decode_worker(
            decoder,
            &track_id,
            &info,
            &shared,
            producer,
            &event_bus,
            start_seconds,
        );
        if let Err(err) = result {
            shared.stopped.store(true, Ordering::Release);
            event_bus.publish(PlayerEvent::Error {
                message: format!("{}: {err}", path.display()),
            });
            event_bus.publish(PlayerEvent::PlaybackStopped);
            return;
        }

        if !shared.stopped.load(Ordering::Acquire) {
            shared.stopped.store(true, Ordering::Release);
            event_bus.publish(PlayerEvent::Progress {
                track_id: track_id.clone(),
                seconds: info.duration_seconds,
                total: info.duration_seconds,
            });
            event_bus.publish(PlayerEvent::PlaybackEnded {
                track_id: track_id.clone(),
            });
            debug!("finished playback: {track_id}");
        }
    })
}

fn run_decode_worker(
    mut decoder: Box<dyn Decoder>,
    track_id: &str,
    info: &StreamInfo,
    shared: &Arc<PlaybackShared>,
    mut producer: Producer<QueuedSample>,
    event_bus: &EventBus,
    start_seconds: f64,
) -> Result<()> {
    if start_seconds > 0.0 {
        decoder
            .seek(start_seconds)
            .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
    }

    let input_rate = info.sample_rate.0.max(1);
    let input_channels = usize::from(info.channels.0).max(1);
    // 持久化 sinc 重采样器：跨包保留 history，
    // 消除"每包独立计算 → 包边界 click"的旧 bug，并复用一份内存。
    let mut resampler =
        StatefulSincResampler::new(input_rate, shared.output_rate, shared.output_channels)
            .map_err(|err| BackendError::Internal(err.to_string()))?;
    let visualizer =
        SimpleVisualizer::new(SPECTRUM_FFT_SIZE, SPECTRUM_BINS, shared.output_channels)
            .map_err(|err| BackendError::Internal(err.to_string()))?;
    let mut last_progress = Instant::now();
    let mut last_spectrum = Instant::now();
    let mut remap_scratch: Vec<f32> = Vec::new();
    let mut output_scratch: Vec<f32> = Vec::new();

    while !shared.stopped.load(Ordering::Acquire) {
        if let Some(seconds) = shared.seek_request.lock().take() {
            decoder
                .seek(seconds)
                .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
            // seek 后 resampler 内部 history 已属过去时间段，必须 reset
            resampler.reset();
        }

        if shared.paused.load(Ordering::Acquire) {
            publish_progress_if_due(
                track_id,
                shared,
                event_bus,
                info.duration_seconds,
                &mut last_progress,
            );
            thread::sleep(QUEUE_SLEEP);
            continue;
        }

        if producer.slots() == 0 {
            publish_progress_if_due(
                track_id,
                shared,
                event_bus,
                info.duration_seconds,
                &mut last_progress,
            );
            thread::sleep(QUEUE_SLEEP);
            continue;
        }

        let Some(packet) = decoder
            .next_packet()
            .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?
        else {
            break;
        };

        adapt_samples_into(
            &packet.samples,
            input_channels,
            shared.output_channels,
            &mut resampler,
            &mut remap_scratch,
            &mut output_scratch,
        )?;
        push_samples(
            &output_scratch,
            shared,
            event_bus,
            track_id,
            info.duration_seconds,
            &mut last_progress,
            &mut producer,
        );
        publish_spectrum_if_due(&visualizer, &output_scratch, event_bus, &mut last_spectrum)?;
    }

    // EOF 后 drain：等 ring 被 render 消费完才声明真正结束。
    // 注意：paused 时不 break（旧 bug #5：暂停 EOF 会把 ring 剩余样本吞掉），
    // 等用户 resume 或 stop 再前进；只有外部 stop 才退出。
    while !shared.stopped.load(Ordering::Acquire) && producer.slots() < shared.max_buffer_samples {
        publish_progress_if_due(
            track_id,
            shared,
            event_bus,
            info.duration_seconds,
            &mut last_progress,
        );
        thread::sleep(QUEUE_SLEEP);
    }

    Ok(())
}

fn publish_spectrum_if_due(
    visualizer: &SimpleVisualizer,
    samples: &[f32],
    event_bus: &EventBus,
    last_spectrum: &mut Instant,
) -> Result<()> {
    visualizer
        .push_samples(samples)
        .map_err(|err| BackendError::Internal(err.to_string()))?;
    if last_spectrum.elapsed() < SPECTRUM_INTERVAL {
        return Ok(());
    }

    *last_spectrum = Instant::now();
    if let Some(frame) = visualizer.latest_frame() {
        event_bus.publish(PlayerEvent::Spectrum { bins: frame.bins });
    }
    Ok(())
}

fn push_samples(
    samples: &[f32],
    shared: &Arc<PlaybackShared>,
    event_bus: &EventBus,
    track_id: &str,
    total_seconds: f64,
    last_progress: &mut Instant,
    producer: &mut Producer<QueuedSample>,
) {
    let mut offset = 0;
    while offset < samples.len() && !shared.stopped.load(Ordering::Acquire) {
        if let Some(seconds) = shared.seek_request.lock().take() {
            shared.frame_position.store(
                seconds_to_frames(seconds, shared.output_rate),
                Ordering::Relaxed,
            );
            return;
        }

        let generation = shared.buffer_generation();
        let count = producer.slots().min(samples.len() - offset);
        let mut written = 0;
        for sample in &samples[offset..offset + count] {
            if producer
                .push(QueuedSample {
                    generation,
                    value: *sample,
                })
                .is_err()
            {
                break;
            }
            written += 1;
        }

        offset += written;
        if written == 0 {
            publish_progress_if_due(track_id, shared, event_bus, total_seconds, last_progress);
            thread::sleep(QUEUE_SLEEP);
        }
    }
}

fn publish_progress_if_due(
    track_id: &str,
    shared: &PlaybackShared,
    event_bus: &EventBus,
    total_seconds: f64,
    last_progress: &mut Instant,
) {
    if last_progress.elapsed() < PROGRESS_INTERVAL {
        return;
    }

    *last_progress = Instant::now();
    event_bus.publish(PlayerEvent::Progress {
        track_id: track_id.to_string(),
        seconds: shared.progress_seconds().min(total_seconds.max(0.0)),
        total: total_seconds,
    });
}

fn render_output_f32(
    data: &mut [f32],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    observed_generation: &mut u32,
) {
    render_output(
        data,
        shared,
        consumer,
        observed_generation,
        |sample, value| *sample = value,
    );
}

fn render_output_f64(
    data: &mut [f64],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    observed_generation: &mut u32,
) {
    render_output(
        data,
        shared,
        consumer,
        observed_generation,
        |sample, value| *sample = f64::from(value),
    );
}

fn render_output_i16(
    data: &mut [i16],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    observed_generation: &mut u32,
) {
    render_output(
        data,
        shared,
        consumer,
        observed_generation,
        |sample, value| {
            *sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        },
    );
}

fn render_output_u16(
    data: &mut [u16],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    observed_generation: &mut u32,
) {
    render_output(
        data,
        shared,
        consumer,
        observed_generation,
        |sample, value| {
            *sample = ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
        },
    );
}

fn render_output<T>(
    data: &mut [T],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    observed_generation: &mut u32,
    mut write_sample: impl FnMut(&mut T, f32),
) {
    if shared.stopped.load(Ordering::Acquire) || shared.paused.load(Ordering::Acquire) {
        for sample in data {
            write_sample(sample, 0.0);
        }
        return;
    }

    let volume = shared.volume();
    let current_generation = shared.buffer_generation();
    if current_generation != *observed_generation {
        *observed_generation = current_generation;
    }

    let mut consumed = 0_usize;
    for sample in data.iter_mut() {
        let mut has_sample = false;
        let value = loop {
            match consumer.pop() {
                Ok(queued) if queued.generation == *observed_generation => {
                    has_sample = true;
                    break queued.value;
                }
                Ok(_) => continue,
                Err(_) => break 0.0,
            }
        };
        if has_sample {
            consumed += 1;
        }
        write_sample(sample, value * volume);
    }

    let frames = consumed / shared.output_channels.max(1);
    if frames > 0 {
        shared
            .frame_position
            .fetch_add(frames as u64, Ordering::Relaxed);
    }
}

/// 把单包样本走完：channel remap → 跨包 sinc 重采样 → 写入 output scratch。
/// 用持久化 `StatefulSincResampler` 避免每包独立计算（旧 #7 包边界 click）+ 减少分配。
/// sinc 失败时降级为无状态线性重采样（fallback path，仅保人间）。
fn adapt_samples_into(
    input: &[f32],
    input_channels: usize,
    output_channels: usize,
    resampler: &mut StatefulSincResampler,
    remap_scratch: &mut Vec<f32>,
    output_scratch: &mut Vec<f32>,
) -> Result<()> {
    let input_channels = input_channels.max(1);
    let output_channels = output_channels.max(1);
    output_scratch.clear();

    if input.is_empty() {
        return Ok(());
    }
    if !input.len().is_multiple_of(input_channels) {
        return Err(BackendError::Internal(format!(
            "decoder packet length {} not multiple of {input_channels} channels",
            input.len()
        )));
    }

    remap_scratch.clear();
    remap_channels_into(input, input_channels, output_channels, remap_scratch);

    if resampler.input_rate() == resampler.output_rate() {
        output_scratch.extend_from_slice(remap_scratch);
        return Ok(());
    }

    match resampler.process(remap_scratch, output_scratch) {
        Ok(()) => Ok(()),
        Err(sinc_err) => {
            output_scratch.clear();
            resample_interleaved_linear(
                remap_scratch,
                output_channels,
                resampler.input_rate().max(1),
                resampler.output_rate().max(1),
                output_scratch,
            )
            .map_err(|linear_err| {
                BackendError::Internal(format!(
                    "resampler failed ({} Hz -> {} Hz, {} ch): sinc={sinc_err}; linear={linear_err}",
                    resampler.input_rate(),
                    resampler.output_rate(),
                    output_channels
                ))
            })
        }
    }
}

fn remap_channels_into(
    input: &[f32],
    input_channels: usize,
    output_channels: usize,
    output: &mut Vec<f32>,
) {
    let input_frames = input.len() / input_channels;
    output.reserve(input_frames * output_channels);

    for frame in 0..input_frames {
        let offset = frame * input_channels;
        if output_channels == 1 {
            let sum: f32 = input[offset..offset + input_channels].iter().sum();
            output.push(sum / input_channels as f32);
            continue;
        }

        for channel in 0..output_channels {
            let mapped = if input_channels == 1 {
                0
            } else {
                channel.min(input_channels - 1)
            };
            output.push(input[offset + mapped]);
        }
    }
}

fn seconds_to_frames(seconds: f64, sample_rate: u32) -> u64 {
    (seconds.max(0.0) * f64::from(sample_rate.max(1))).round() as u64
}

fn map_build_stream_error(err: cpal::BuildStreamError) -> BackendError {
    BackendError::DeviceLost(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapt(input: &[f32], in_rate: u32, in_ch: usize, out_rate: u32, out_ch: usize) -> Vec<f32> {
        let mut resampler = StatefulSincResampler::new(in_rate.max(1), out_rate.max(1), out_ch.max(1))
            .expect("resampler");
        let mut remap = Vec::new();
        let mut out = Vec::new();
        adapt_samples_into(input, in_ch, out_ch, &mut resampler, &mut remap, &mut out)
            .expect("adapt");
        out
    }

    #[test]
    fn adapts_mono_to_stereo_without_resampling() {
        let output = adapt(&[0.25, -0.5], 44_100, 1, 44_100, 2);
        assert_eq!(output, vec![0.25, 0.25, -0.5, -0.5]);
    }

    #[test]
    fn adapts_stereo_to_mono_by_averaging_channels() {
        let output = adapt(&[0.25, 0.75, -0.5, 0.5], 44_100, 2, 44_100, 1);
        assert_eq!(output, vec![0.5, 0.0]);
    }

    #[test]
    fn resamples_to_target_rate() {
        let output = adapt(&[0.0, 1.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0], 4, 1, 2, 1);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output.iter().all(|sample| sample.abs() <= 1.0));
    }
}
