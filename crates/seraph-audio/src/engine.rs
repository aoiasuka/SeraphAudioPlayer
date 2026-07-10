use crate::{
    backend::{BackendError, Result},
    device::{output_device_by_id, resolve_output_device_id},
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
const DEFAULT_EXCLUSIVE_PERIOD_FRAMES: u32 = 512;
/// F-1：增益斜坡时长，暂停/恢复/seek/音量变化在 ~5ms 内平滑过渡，消除爆音。
const GAIN_RAMP_SECONDS: f32 = 0.005;
/// F-2：can_resume 路径中，请求位置与内部精确位置之差小于该阈值时跳过 seek。
const RESUME_SEEK_THRESHOLD_SECONDS: f64 = 1.0;
/// F-3：seek 上界钳制时距曲尾保留的余量（秒）。
const SEEK_END_MARGIN_SECONDS: f64 = 0.1;
/// F-15：EOF 后向重采样器喂入的零填充帧数（≥ sinc radius=16，冲出尾部残留）。
const RESAMPLER_FLUSH_FRAMES: usize = 32;

#[derive(Clone)]
pub struct PlaybackController {
    tx: Sender<PlaybackRequest>,
}

struct PlaybackRequest {
    command: PlaybackCommand,
    reply: Sender<Result<()>>,
}

enum PlaybackCommand {
    PlayFile {
        path: PathBuf,
        track_id: String,
        /// F-16：`None` = 续播（不改变位置）；`Some(s)` = 指定位置播放（含 `Some(0.0)` 从头播）。
        start_seconds: Option<f64>,
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
        let (tx, rx) = mpsc::channel::<PlaybackRequest>();
        thread::spawn(move || {
            let mut engine = PlaybackEngine::new(event_bus.clone());
            while let Ok(request) = rx.recv() {
                let result = match request.command {
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

                if let Err(err) = &result {
                    event_bus.publish(PlayerEvent::Error {
                        message: err.to_string(),
                    });
                    event_bus.publish(PlayerEvent::PlaybackStopped);
                }

                let _ = request.reply.send(result);
            }
        });

        Self { tx }
    }

    pub fn play_file(&self, path: PathBuf, track_id: String, start_seconds: f64) -> Result<()> {
        self.play_file_at(path, track_id, Some(start_seconds))
    }

    /// F-16：显式区分「续播」（`None`）与「从指定位置播放」（`Some`，含 `Some(0.0)` 从头播）。
    pub fn play_file_at(
        &self,
        path: PathBuf,
        track_id: String,
        start_seconds: Option<f64>,
    ) -> Result<()> {
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
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(PlaybackRequest { command, reply })
            .map_err(|_| BackendError::Internal("audio thread is not available".into()))?;
        rx.recv()
            .map_err(|_| BackendError::Internal("audio thread did not return a result".into()))?
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

    pub fn play_file(
        &mut self,
        path: PathBuf,
        track_id: String,
        start_seconds: Option<f64>,
    ) -> Result<()> {
        // F-19-1：ASIO 未实现，检查放在最前——避免先 stop 掉当前曲目再报 NotImplemented。
        if self.driver == OutputDriver::Asio {
            return Err(BackendError::NotImplemented);
        }

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
            // F-16：None = 续播，不动位置；Some(s) = 指定位置（含 0.0 从头播）。
            // F-2：请求位置与内部精确位置足够接近时跳过 seek，
            // 避免暂停→恢复因前端 250ms 粒度快照回跳并丢弃整个 3 秒缓冲。
            let mut pending_seek = None;
            if let Some(seconds) = start_seconds {
                let session = self.session.as_ref().expect("can_resume implies session");
                let seconds = clamp_seek_seconds(seconds, session.duration_seconds);
                let current = session.shared.progress_seconds();
                if (seconds - current).abs() >= RESUME_SEEK_THRESHOLD_SECONDS {
                    pending_seek = Some(seconds);
                }
            }
            if let Some(seconds) = pending_seek {
                self.seek(seconds)?;
            }
            return self.resume();
        }

        // 先停旧 session，再 open 解码器：
        // 即使 open 失败也保证旧的播放真的停了（否则前端 UI 已切歌但实际仍在播旧曲）。
        // F-6：失败分支不再手工 publish Error/PlaybackStopped，统一交给 controller 循环，
        // 避免同一失败被双重上报。
        self.stop_session();

        let decoder =
            open_decoder(&path).map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
        let info = decoder
            .info()
            .cloned()
            .ok_or_else(|| BackendError::Internal("decoder opened without stream info".into()))?;
        let duration_seconds = info.duration_seconds;
        // F-3：起播位置钳制到 [0, duration-0.1]，避免 seek 到曲尾之后导致解码失败/瞬间跳曲。
        let start_seconds = clamp_seek_seconds(start_seconds.unwrap_or(0.0), duration_seconds);

        let device = self.output_device()?;
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
                let endpoint_id = self
                    .selected_device_id
                    .as_deref()
                    .map(resolve_output_device_id)
                    .transpose()?;
                let worker = spawn_wasapi_exclusive_render_worker(
                    endpoint_id,
                    config.clone(),
                    sample_format,
                    shared.clone(),
                    consumer,
                    self.event_bus.clone(),
                )?;
                (None, Some(worker))
            }
            OutputDriver::Shared => {
                let stream = build_output_stream(
                    &device,
                    &config,
                    sample_format,
                    shared.clone(),
                    consumer,
                    self.event_bus.clone(),
                )?;
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
        self.play_file(path, track_id, Some(seconds))?;
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

        // F-3：钳制到 [0, duration-0.1]，避免 seek 到曲尾之后触发 decoder 失败或瞬间跳曲。
        let seconds = clamp_seek_seconds(seconds, session.duration_seconds);
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
        let device_id = resolve_output_device_id(&device_id)?;
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
        self.play_file(path, track_id, Some(seconds))?;
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

/// 运行期渲染失败统一上报：置 stopped + 发 Error/PlaybackStopped（只发一次），
/// 让解码线程退出，避免设备丢失（拔出/被独占抢占）后 UI 永久假死（H-2）。
/// F-18：设备丢失额外发布 `PlayerEvent::DeviceLost`，让前端能区分「文件坏了」与「设备拔了」。
fn report_render_failure(event_bus: &EventBus, shared: &PlaybackShared, err: &BackendError) {
    if !shared.stopped.swap(true, Ordering::AcqRel) {
        if let BackendError::DeviceLost(reason) = err {
            event_bus.publish(PlayerEvent::DeviceLost {
                reason: reason.clone(),
            });
        }
        event_bus.publish(PlayerEvent::Error {
            message: err.to_string(),
        });
        event_bus.publish(PlayerEvent::PlaybackStopped);
    }
}

fn build_output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
    mut consumer: Consumer<QueuedSample>,
    event_bus: EventBus,
) -> Result<Stream> {
    let err_shared = shared.clone();
    let err_fn = move |err: cpal::StreamError| {
        warn!("audio output stream error: {err}");
        report_render_failure(
            &event_bus,
            &err_shared,
            &BackendError::DeviceLost(err.to_string()),
        );
    };
    let mut state = RenderState::new(shared.buffer_generation());
    let mut dither = TpdfDither::default();
    match sample_format {
        SampleFormat::F32 => device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    render_output_f32(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::I16 => device
            .build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    render_output_i16(data, &shared, &mut consumer, &mut state, &mut dither)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::U16 => device
            .build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    render_output_u16(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::F64 => device
            .build_output_stream(
                config,
                move |data: &mut [f64], _| {
                    render_output_f64(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::I8 => device
            .build_output_stream(
                config,
                move |data: &mut [i8], _| {
                    render_output_i8(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::U8 => device
            .build_output_stream(
                config,
                move |data: &mut [u8], _| {
                    render_output_u8(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::I32 => device
            .build_output_stream(
                config,
                move |data: &mut [i32], _| {
                    render_output_i32(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::U32 => device
            .build_output_stream(
                config,
                move |data: &mut [u32], _| {
                    render_output_u32(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::I64 => device
            .build_output_stream(
                config,
                move |data: &mut [i64], _| {
                    render_output_i64(data, &shared, &mut consumer, &mut state)
                },
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::U64 => device
            .build_output_stream(
                config,
                move |data: &mut [u64], _| {
                    render_output_u64(data, &shared, &mut consumer, &mut state)
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
        // F-8：位深未知（0/缺省，典型为有损格式的全精度 float 解码输出）时选 I32（24-in-32），
        // 避免被硬量化到 16bit 丢掉动态余量；已知 ≤16bit 才用 I16。
        let sample_format = if info.bit_depth.0 == 0 || info.bit_depth.0 > 16 {
            SampleFormat::I32
        } else {
            SampleFormat::I16
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

    // 收集「声道匹配 + 能匹配目标采样率」的候选,只保留引擎能渲染的采样格式,
    // 再按格式质量优先级(F32 > I32 > … > U8)挑最优。这样即使设备把 U8 之类的
    // 低质量格式排在前面(常见于某些 Hi-Res 采样率只有低位深配置覆盖的情况),
    // 也不会被误选,从而避免 build_output_stream 报 "unsupported format: ... U8"。
    let best = configs
        .filter(|range| range.channels() == desired_channels)
        .filter_map(|range| range.try_with_sample_rate(desired_rate))
        .filter(|config| is_engine_output_format(config.sample_format()))
        .min_by_key(|config| output_format_priority(config.sample_format()));
    if let Some(config) = best {
        let sample_format = config.sample_format();
        return Ok((sample_format, config.into()));
    }

    // 没有「精确采样率 + 受支持格式」的组合(例如设备不支持该 Hi-Res 采样率):
    // 退回设备默认输出配置(通常为 F32),由解码线程重采样到设备采样率后再输出。
    let supported_config = device
        .default_output_config()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
    let sample_format = supported_config.sample_format();
    Ok((sample_format, supported_config.into()))
}

/// `build_output_stream` 能够渲染的输出采样格式。
fn is_engine_output_format(format: SampleFormat) -> bool {
    output_format_priority(format) != u8::MAX
}

/// 输出采样格式的选用优先级,数值越小越优先(F32 与引擎内部 f32 一致,优先级最高)。
/// 引擎无法渲染的格式返回 `u8::MAX`,既表示「不支持」也使其排到最后。
fn output_format_priority(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::U32 => 2,
        SampleFormat::I16 => 3,
        SampleFormat::U16 => 4,
        SampleFormat::F64 => 5,
        SampleFormat::I64 => 6,
        SampleFormat::U64 => 7,
        SampleFormat::I8 => 8,
        SampleFormat::U8 => 9,
        _ => u8::MAX,
    }
}

#[cfg(windows)]
fn spawn_wasapi_exclusive_render_worker(
    endpoint_id: Option<String>,
    config: StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
    consumer: Consumer<QueuedSample>,
    event_bus: EventBus,
) -> Result<JoinHandle<Result<()>>> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let shared_for_worker = shared.clone();
    let worker = thread::spawn(move || {
        run_wasapi_exclusive_render_worker(
            endpoint_id,
            config,
            sample_format,
            shared_for_worker,
            consumer,
            ready_tx,
            event_bus,
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
            // 超时：视为启动失败。F-4：不 join——worker 很可能正卡死在驱动调用内部，
            // 无限期 join 会让引擎线程永久阻塞（所有播放命令失效）。stopped 已置位，
            // worker 一旦从驱动调用返回会自行退出并 stop_stream；泄漏 detached 线程
            // 好过整个引擎死锁。
            shared.stopped.store(true, Ordering::Release);
            drop(worker);
            Err(BackendError::ExclusiveModeUnavailable(
                "WASAPI exclusive stream init timed out".into(),
            ))
        }
    }
}

#[cfg(not(windows))]
fn spawn_wasapi_exclusive_render_worker(
    _endpoint_id: Option<String>,
    _config: StreamConfig,
    _sample_format: SampleFormat,
    _shared: Arc<PlaybackShared>,
    _consumer: Consumer<QueuedSample>,
    _event_bus: EventBus,
) -> Result<JoinHandle<Result<()>>> {
    Err(BackendError::ExclusiveModeUnavailable(
        "WASAPI exclusive output is only available on Windows".into(),
    ))
}

#[cfg(windows)]
fn run_wasapi_exclusive_render_worker(
    endpoint_id: Option<String>,
    config: StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
    mut consumer: Consumer<QueuedSample>,
    ready_tx: Sender<std::result::Result<(), String>>,
    event_bus: EventBus,
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
        let device = match endpoint_id.as_deref() {
            Some(id) => enumerator
                .get_device(id)
                .map_err(|_| BackendError::DeviceNotFound)?,
            None => enumerator
                .get_default_device(&Direction::Render)
                .map_err(|err| BackendError::DeviceLost(err.to_string()))?,
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

    let mut state = RenderState::new(shared.buffer_generation());
    let mut dither = TpdfDither::default();
    // F-13：scratch buffer 跨迭代复用，避免每 ~90ms 重建两个大 Vec。
    let mut samples_scratch: Vec<f32> = Vec::new();
    let mut bytes_scratch: Vec<u8> = Vec::new();

    while !shared.stopped.load(Ordering::Acquire) {
        let frames = match audio_client.get_available_space_in_frames() {
            Ok(frames) => frames,
            Err(err) => {
                // H-2：运行期设备丢失（拔出/被独占抢占），通知前端并退出。
                let err = BackendError::DeviceLost(err.to_string());
                report_render_failure(&event_bus, &shared, &err);
                return Err(err);
            }
        };
        if frames > 0 {
            render_wasapi_output_bytes(
                frames as usize,
                sample_format,
                &shared,
                &mut consumer,
                &mut state,
                &mut dither,
                &mut samples_scratch,
                &mut bytes_scratch,
            );
            if let Err(err) = render_client.write_to_device(frames as usize, &bytes_scratch, None) {
                let err = BackendError::DeviceLost(err.to_string());
                report_render_failure(&event_bus, &shared, &err);
                return Err(err);
            }
        }
        thread::sleep(sleep_period);
    }

    let _ = audio_client.stop_stream();
    Ok(())
}

fn exclusive_mode_unavailable(detail: String) -> BackendError {
    BackendError::ExclusiveModeUnavailable(detail)
}

#[allow(clippy::too_many_arguments)]
fn render_wasapi_output_bytes(
    frames: usize,
    sample_format: SampleFormat,
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
    dither: &mut TpdfDither,
    samples: &mut Vec<f32>,
    bytes: &mut Vec<u8>,
) {
    let channels = shared.output_channels.max(1);
    let sample_count = frames * channels;
    samples.clear();
    samples.resize(sample_count, 0.0);
    render_output(samples, shared, consumer, state, |sample, value| {
        *sample = value
    });

    bytes.clear();
    match sample_format {
        SampleFormat::I16 => {
            bytes.reserve(sample_count * 2);
            for &sample in samples.iter() {
                // F-14：×32768 + clamp + round，16bit 输出叠加 TPDF dither。
                bytes.extend_from_slice(&quantize_i16_tpdf(sample, dither).to_le_bytes());
            }
        }
        SampleFormat::I32 => {
            bytes.reserve(sample_count * 4);
            for &sample in samples.iter() {
                // 24-in-32：有效 24bit 左移到高位。
                bytes.extend_from_slice(&(quantize_i24(sample) << 8).to_le_bytes());
            }
        }
        SampleFormat::F32 => {
            bytes.reserve(sample_count * 4);
            for &sample in samples.iter() {
                bytes.extend_from_slice(&sample.clamp(-1.0, 1.0).to_le_bytes());
            }
        }
        _ => {}
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
        // F-5：解码栈（symphonia/ffmpeg 等）对畸形文件可能 panic；catch_unwind 兜底，
        // panic 时同 Err 分支处理，避免 stopped 不置位导致 UI 永久停在「播放中」假状态。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_decode_worker(
                decoder,
                &track_id,
                &info,
                &shared,
                producer,
                &event_bus,
                start_seconds,
            )
        }))
        .unwrap_or_else(|_| Err(BackendError::Internal("decode worker panicked".into())));
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
        // F-3：seek 失败降级为忽略（从头播放）+ warning，而非终止整曲。
        if let Err(err) = decoder.seek(start_seconds) {
            warn!("initial seek to {start_seconds:.3}s failed, starting from 0: {err}");
            shared.frame_position.store(0, Ordering::Relaxed);
        }
    }

    let input_rate = info.sample_rate.0.max(1);
    let input_channels = usize::from(info.channels.0).max(1);
    // 持久化 sinc 重采样器：跨包保留 history，
    // 消除"每包独立计算 → 包边界 click"的旧 bug，并复用一份内存。
    let mut resampler =
        StatefulSincResampler::new(input_rate, shared.output_rate, shared.output_channels)
            .map_err(|err| BackendError::Internal(err.to_string()))?;
    let mut progress = ProgressTracker::new();
    let mut remap_scratch: Vec<f32> = Vec::new();
    let mut output_scratch: Vec<f32> = Vec::new();

    // 外层 session 循环：解码到 EOF 后进入 drain；drain 期间若收到 seek 请求，
    // 回主循环重新 seek 并继续解码（H-3：避免曲尾回拖被误判为播放结束）。
    'session: loop {
        // ---- 主解码循环 ----
        loop {
            if shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }

            if let Some(seconds) = shared.seek_request.lock().take() {
                match decoder.seek(seconds) {
                    // seek 后 resampler 内部 history 已属过去时间段，必须 reset
                    Ok(()) => resampler.reset(),
                    // F-3：seek 失败降级为忽略该次 seek + warning，而非终止整曲。
                    Err(err) => warn!("seek to {seconds:.3}s failed, ignoring: {err}"),
                }
            }

            if shared.paused.load(Ordering::Acquire) {
                publish_progress_if_due(
                    track_id,
                    shared,
                    event_bus,
                    info.duration_seconds,
                    &mut progress,
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
                    &mut progress,
                );
                thread::sleep(QUEUE_SLEEP);
                continue;
            }

            let Some(packet) = decoder
                .next_packet()
                .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?
            else {
                break; // EOF → 进入 drain
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
                &mut progress,
                &mut producer,
            );
        }

        // F-15：EOF 后向重采样器喂 ≥radius 帧零，把尾部残留的 radius 输入帧冲出来，
        // 否则重采样路径每曲结尾丢约 16 帧（gapless 场景累积可闻）。
        if resampler.input_rate() != resampler.output_rate() {
            let flush_input = vec![0.0_f32; RESAMPLER_FLUSH_FRAMES * input_channels];
            if adapt_samples_into(
                &flush_input,
                input_channels,
                shared.output_channels,
                &mut resampler,
                &mut remap_scratch,
                &mut output_scratch,
            )
            .is_ok()
            {
                push_samples(
                    &output_scratch,
                    shared,
                    event_bus,
                    track_id,
                    info.duration_seconds,
                    &mut progress,
                    &mut producer,
                );
            }
        }

        // ---- EOF 后 drain：等 ring 被 render 消费完才声明真正结束 ----
        // 注意：paused 时不 break（旧 bug #5：暂停 EOF 会把 ring 剩余样本吞掉），
        // 等用户 resume 或 stop 再前进。drain 期间命中 seek 请求则回主循环（H-3）。
        loop {
            if shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            if shared.seek_request.lock().is_some() {
                // 留给主循环统一 take + decoder.seek + resampler.reset
                continue 'session;
            }
            if producer.slots() >= shared.max_buffer_samples {
                return Ok(());
            }
            publish_progress_if_due(
                track_id,
                shared,
                event_bus,
                info.duration_seconds,
                &mut progress,
            );
            thread::sleep(QUEUE_SLEEP);
        }
    }
}

fn push_samples(
    samples: &[f32],
    shared: &Arc<PlaybackShared>,
    event_bus: &EventBus,
    track_id: &str,
    total_seconds: f64,
    progress: &mut ProgressTracker,
    producer: &mut Producer<QueuedSample>,
) {
    let mut offset = 0;
    while offset < samples.len() && !shared.stopped.load(Ordering::Acquire) {
        // H-1：检测到 seek 请求立即让出本包，但**不消费**请求——
        // 交由主解码循环统一执行 decoder.seek + resampler.reset。
        // frame_position 已在 engine.seek() 中更新，这里无需重设。
        if shared.seek_request.lock().is_some() {
            return;
        }

        let generation = shared.buffer_generation();
        let count = producer.slots().min(samples.len() - offset);
        // F-13：write_chunk_uninit 批量提交，替代逐样本 push（每次一组原子操作）。
        let written = match producer.write_chunk_uninit(count) {
            Ok(chunk) => chunk.fill_from_iter(
                samples[offset..offset + count]
                    .iter()
                    .map(|&value| QueuedSample { generation, value }),
            ),
            Err(_) => 0,
        };

        offset += written;
        if written == 0 {
            publish_progress_if_due(track_id, shared, event_bus, total_seconds, progress);
            thread::sleep(QUEUE_SLEEP);
        }
    }
}

/// F-19-2：进度事件去重——除按时间间隔节流外，frame_position 未变化（典型为暂停）时
/// 不再重复发布内容完全相同的 Progress 事件。
struct ProgressTracker {
    last_publish: Instant,
    last_frames: u64,
}

impl ProgressTracker {
    fn new() -> Self {
        Self {
            last_publish: Instant::now(),
            last_frames: u64::MAX,
        }
    }
}

fn publish_progress_if_due(
    track_id: &str,
    shared: &PlaybackShared,
    event_bus: &EventBus,
    total_seconds: f64,
    tracker: &mut ProgressTracker,
) {
    if tracker.last_publish.elapsed() < PROGRESS_INTERVAL {
        return;
    }

    let frames = shared.frame_position.load(Ordering::Relaxed);
    if frames == tracker.last_frames {
        return;
    }

    tracker.last_publish = Instant::now();
    tracker.last_frames = frames;
    let progress = shared.progress_seconds();
    event_bus.publish(PlayerEvent::Progress {
        track_id: track_id.to_string(),
        // M-7：仅在已知总时长(>0)时才钳制，否则透传真实进度，
        // 避免 duration 探测失败的曲目进度永远停在 0:00。
        seconds: if total_seconds > 0.0 {
            progress.min(total_seconds)
        } else {
            progress
        },
        total: total_seconds,
    });
}

/// 回调本地渲染状态（F-1/F-11）：不含锁与分配，随输出流生存。
struct RenderState {
    observed_generation: u32,
    /// F-1：当前增益，每样本向目标增益（volume 或 0）以固定步长逼近。
    gain: f32,
}

impl RenderState {
    fn new(observed_generation: u32) -> Self {
        Self {
            observed_generation,
            gain: 0.0, // 起播从 0 ramp-in
        }
    }
}

/// F-14：TPDF dither 噪声源（两个 LCG 均匀噪声之差 → 三角分布，±1 LSB）。
#[derive(Clone)]
pub(crate) struct TpdfDither {
    rng: u32,
}

impl Default for TpdfDither {
    fn default() -> Self {
        Self { rng: 0x1234_5678 }
    }
}

impl TpdfDither {
    fn next_uniform(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / 16_777_216.0 // [0, 1)
    }

    /// 三角分布噪声，范围 (-1, 1)（单位：目标格式的 LSB）。
    pub(crate) fn next_tpdf(&mut self) -> f32 {
        self.next_uniform() - self.next_uniform()
    }
}

/// F-1：每样本增益斜坡步长（~5ms 内走完全程 0→1）。
fn gain_ramp_step(output_rate: u32) -> f32 {
    1.0 / (GAIN_RAMP_SECONDS * output_rate.max(1) as f32)
}

/// F-1：当前增益向目标增益逼近一步（纯函数，可测）。
fn ramp_gain(current: f32, target: f32, step: f32) -> f32 {
    current + (target - current).clamp(-step, step)
}

/// F-11：wrapping 代际比较——`generation` 等于或新于 `observed` 时为 true。
fn generation_is_current(generation: u32, observed: u32) -> bool {
    generation.wrapping_sub(observed) < u32::MAX / 2
}

/// F-11：用 read_chunk 批量丢弃 ring 头部的陈旧代样本，
/// 替代回调内逐样本 pop 清扫（seek 后最多 3 秒 × fs × ch 次原子操作）。
/// 返回是否丢弃过样本（用于触发新代数据 ramp-in）。
fn discard_stale_samples(consumer: &mut Consumer<QueuedSample>, observed: u32) -> bool {
    let mut discarded = false;
    loop {
        let slots = consumer.slots();
        if slots == 0 {
            return discarded;
        }
        let Ok(chunk) = consumer.read_chunk(slots) else {
            return discarded;
        };
        let (first, second) = chunk.as_slices();
        let mut stale = 0_usize;
        for queued in first.iter().chain(second.iter()) {
            if generation_is_current(queued.generation, observed) {
                break;
            }
            stale += 1;
        }
        if stale == 0 {
            return discarded;
        }
        discarded = true;
        let done = stale < slots;
        chunk.commit(stale);
        if done {
            return discarded;
        }
    }
}

// ---- F-14：统一整数量化（×2^(bits-1) + round + clamp；unsigned 静音点为半量程）----

/// 无 dither 版 i16 量化（基准实现，dither 版在其上叠加 TPDF 噪声；单测用）。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn quantize_i16(value: f32) -> i16 {
    (value * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

/// 16bit 输出叠加 TPDF dither（HiFi：消除量化失真的信号相关性）。
pub(crate) fn quantize_i16_tpdf(value: f32, dither: &mut TpdfDither) -> i16 {
    (value * 32_768.0 + dither.next_tpdf())
        .round()
        .clamp(-32_768.0, 32_767.0) as i16
}

fn quantize_u16(value: f32) -> u16 {
    (value * 32_768.0 + 32_768.0).round().clamp(0.0, 65_535.0) as u16
}

fn quantize_i8(value: f32) -> i8 {
    (value * 128.0).round().clamp(-128.0, 127.0) as i8
}

fn quantize_u8(value: f32) -> u8 {
    (value * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8
}

pub(crate) fn quantize_i32(value: f32) -> i32 {
    // f64 → i32 的 `as` 转换饱和，天然处理 +2^31 溢出。
    (f64::from(value) * 2_147_483_648.0).round() as i32
}

fn quantize_u32(value: f32) -> u32 {
    (f64::from(value) * 2_147_483_648.0 + 2_147_483_648.0).round() as u32
}

fn quantize_i64(value: f32) -> i64 {
    (f64::from(value) * 9.223_372_036_854_776e18) as i64
}

fn quantize_u64(value: f32) -> u64 {
    (f64::from(value) * 9.223_372_036_854_776e18 + 9.223_372_036_854_776e18) as u64
}

/// 24-in-32 有效位量化（独占模式 I32 容器）。
pub(crate) fn quantize_i24(value: f32) -> i32 {
    (value * 8_388_608.0)
        .round()
        .clamp(-8_388_608.0, 8_388_607.0) as i32
}

fn render_output_f32(
    data: &mut [f32],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = value
    });
}

fn render_output_f64(
    data: &mut [f64],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = f64::from(value)
    });
}

fn render_output_i16(
    data: &mut [i16],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
    dither: &mut TpdfDither,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_i16_tpdf(value, dither);
    });
}

fn render_output_u16(
    data: &mut [u16],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_u16(value.clamp(-1.0, 1.0));
    });
}

fn render_output_i8(
    data: &mut [i8],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_i8(value.clamp(-1.0, 1.0));
    });
}

fn render_output_u8(
    data: &mut [u8],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_u8(value.clamp(-1.0, 1.0));
    });
}

fn render_output_i32(
    data: &mut [i32],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_i32(value.clamp(-1.0, 1.0));
    });
}

fn render_output_u32(
    data: &mut [u32],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_u32(value.clamp(-1.0, 1.0));
    });
}

fn render_output_i64(
    data: &mut [i64],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_i64(value.clamp(-1.0, 1.0));
    });
}

fn render_output_u64(
    data: &mut [u64],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
) {
    render_output(data, shared, consumer, state, |sample, value| {
        *sample = quantize_u64(value.clamp(-1.0, 1.0));
    });
}

/// 实时音频回调核心：无锁、无分配、无 IO。
/// F-1：每样本增益斜坡——暂停/停止先 ramp 到 0 再静音（期间继续消费样本），
/// 恢复/起播/seek 后从 0 ramp-in，音量变化按步长逼近，消除硬切爆音与 zipper noise。
fn render_output<T>(
    data: &mut [T],
    shared: &PlaybackShared,
    consumer: &mut Consumer<QueuedSample>,
    state: &mut RenderState,
    mut write_sample: impl FnMut(&mut T, f32),
) {
    let silenced = shared.stopped.load(Ordering::Acquire) || shared.paused.load(Ordering::Acquire);
    // 已经 ramp 到 0：纯静音快路径，不消费样本（暂停期间保住缓冲与进度）。
    if silenced && state.gain <= 0.0 {
        for sample in data {
            write_sample(sample, 0.0);
        }
        return;
    }

    let target = if silenced { 0.0 } else { shared.volume() };
    let step = gain_ramp_step(shared.output_rate);

    let current_generation = shared.buffer_generation();
    if current_generation != state.observed_generation {
        state.observed_generation = current_generation;
    }
    // F-11：seek 后的陈旧代样本批量丢弃；有丢弃说明发生了硬切，新数据从 0 ramp-in。
    if discard_stale_samples(consumer, state.observed_generation) {
        state.gain = 0.0;
    }

    let mut consumed = 0_usize;
    for sample in data.iter_mut() {
        state.gain = ramp_gain(state.gain, target, step);
        if silenced && state.gain <= 0.0 {
            // ramp-out 完成，剩余样本输出静音且不再消费。
            write_sample(sample, 0.0);
            continue;
        }
        let mut has_sample = false;
        let value = loop {
            match consumer.pop() {
                Ok(queued) => {
                    // F-11：接受「等于或新于」observed 的代（wrapping 比较）并同步 observed，
                    // 避免回调期间 seek 落地后误丢新代样本。
                    if generation_is_current(queued.generation, state.observed_generation) {
                        if queued.generation != state.observed_generation {
                            state.observed_generation = queued.generation;
                            // 代际切换 = 波形不连续，新段从 0 ramp-in。
                            state.gain = 0.0;
                        }
                        has_sample = true;
                        break queued.value;
                    }
                    continue; // 旧代样本，丢弃
                }
                Err(_) => break 0.0,
            }
        };
        if has_sample {
            consumed += 1;
        }
        write_sample(sample, value * state.gain);
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

/// F-17：多声道→立体声下混系数（ITU-R BS.775 风格）。
/// 假定标准 WAVE 声道序：FL FR FC LFE BL BR [SL SR]。
/// 返回 (left_coef, right_coef)。LFE 不参与下混。
fn stereo_downmix_coefficient(channel: usize) -> (f32, f32) {
    const C: f32 = std::f32::consts::FRAC_1_SQRT_2; // 0.7071
    match channel {
        0 => (1.0, 0.0),               // FL
        1 => (0.0, 1.0),               // FR
        2 => (C, C),                   // FC（人声）
        3 => (0.0, 0.0),               // LFE
        ch if ch % 2 == 0 => (C, 0.0), // BL/SL
        _ => (0.0, C),                 // BR/SR
    }
}

/// F-17：单帧多声道 → 立体声下混（含归一化防削波）。
/// 5.1 时 L = (FL + 0.7071·FC + 0.7071·BL) / (1 + 2×0.7071)，R 对称。
fn downmix_frame_to_stereo(frame: &[f32]) -> (f32, f32) {
    let mut left = 0.0_f32;
    let mut right = 0.0_f32;
    let mut left_sum = 0.0_f32;
    let mut right_sum = 0.0_f32;
    for (channel, &sample) in frame.iter().enumerate() {
        let (cl, cr) = stereo_downmix_coefficient(channel);
        left += cl * sample;
        right += cr * sample;
        left_sum += cl;
        right_sum += cr;
    }
    let norm = 1.0 / left_sum.max(right_sum).max(1.0);
    (left * norm, right * norm)
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
        let frame_samples = &input[offset..offset + input_channels];

        if output_channels == 1 {
            let sum: f32 = frame_samples.iter().sum();
            output.push(sum / input_channels as f32);
            continue;
        }

        // F-17：≥3 声道下混到立体声用 ITU 系数，保住中置（人声）与环绕内容。
        if output_channels == 2 && input_channels >= 3 {
            let (left, right) = downmix_frame_to_stereo(frame_samples);
            output.push(left);
            output.push(right);
            continue;
        }

        for channel in 0..output_channels {
            let value = if input_channels == 1 {
                // 单声道广播到所有输出声道
                frame_samples[0]
            } else if channel < input_channels {
                frame_samples[channel]
            } else {
                // F-17：上混时附加声道填 0，而非复制最后一个输入声道
                0.0
            };
            output.push(value);
        }
    }
}

/// F-3：seek/起播位置钳制到 [0, duration - 0.1]（duration 已知时），
/// 避免 seek 到曲尾之后导致 decoder 失败或「点播放却瞬间跳曲」。
fn clamp_seek_seconds(seconds: f64, duration_seconds: f64) -> f64 {
    let seconds = seconds.max(0.0);
    if duration_seconds > 0.0 {
        seconds.min((duration_seconds - SEEK_END_MARGIN_SECONDS).max(0.0))
    } else {
        seconds
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
        let mut resampler =
            StatefulSincResampler::new(in_rate.max(1), out_rate.max(1), out_ch.max(1))
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

    #[test]
    fn playback_controller_returns_audio_thread_errors() {
        let controller = PlaybackController::new(EventBus::new());
        let missing = std::env::temp_dir().join("seraph-audio-missing-fixture.flac");
        let _ = std::fs::remove_file(&missing);

        let err = controller
            .play_file(missing, "missing-track".into(), 0.0)
            .expect_err("missing file should fail in the audio thread");

        assert!(err.to_string().contains("unsupported format"));
    }

    // ---- F-17：下混 ----

    #[test]
    fn downmix_5_1_to_stereo_uses_itu_coefficients() {
        const C: f32 = std::f32::consts::FRAC_1_SQRT_2;
        // FL FR FC LFE BL BR
        let frame = [1.0, 0.0, 1.0, 1.0, 1.0, 0.0];
        let (l, r) = downmix_frame_to_stereo(&frame);
        let norm = 1.0 / (1.0 + 2.0 * C);
        assert!((l - (1.0 + C + C) * norm).abs() < 1e-6);
        // 右声道只应收到 FC 的 0.7071（FR/BR 为 0；LFE 不参与）
        assert!((r - C * norm).abs() < 1e-6);
    }

    #[test]
    fn downmix_preserves_center_channel() {
        // 只有中置有信号（人声场景），下混后左右均应保留能量
        let frame = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let (l, r) = downmix_frame_to_stereo(&frame);
        assert!(l > 0.2 && (l - r).abs() < 1e-6);
    }

    #[test]
    fn downmix_full_scale_does_not_clip() {
        let frame = [1.0_f32; 6];
        let (l, r) = downmix_frame_to_stereo(&frame);
        assert!(l <= 1.0 + 1e-6 && r <= 1.0 + 1e-6);
    }

    #[test]
    fn upmix_stereo_fills_extra_channels_with_silence() {
        let mut out = Vec::new();
        remap_channels_into(&[0.3, -0.3], 2, 6, &mut out);
        assert_eq!(out, vec![0.3, -0.3, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn remap_5_1_to_stereo_goes_through_downmix() {
        let mut out = Vec::new();
        remap_channels_into(&[0.0, 0.5, 0.0, 0.9, 0.0, 0.0], 6, 2, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[0].abs() < 1e-6, "左声道不应包含 FR/LFE");
        assert!(out[1] > 0.0, "FR 应保留在右声道");
    }

    // ---- F-14：量化 ----

    #[test]
    fn quantize_i16_rounds_and_clamps() {
        assert_eq!(quantize_i16(0.0), 0);
        assert_eq!(quantize_i16(1.0), 32_767); // +1.0 被 clamp
        assert_eq!(quantize_i16(-1.0), -32_768);
        assert_eq!(quantize_i16(2.0), 32_767);
        assert_eq!(quantize_i16(-2.0), -32_768);
        // round 而非截尾：0.6 LSB → 1
        assert_eq!(quantize_i16(0.6 / 32_768.0), 1);
        assert_eq!(quantize_i16(-0.6 / 32_768.0), -1);
    }

    #[test]
    fn quantize_unsigned_silence_is_half_scale() {
        assert_eq!(quantize_u16(0.0), 32_768);
        assert_eq!(quantize_u8(0.0), 128);
        assert_eq!(quantize_u16(-1.0), 0);
        assert_eq!(quantize_u16(1.0), 65_535);
        assert_eq!(quantize_u8(1.0), 255);
    }

    #[test]
    fn quantize_i24_and_i32_full_scale() {
        assert_eq!(quantize_i24(0.0), 0);
        assert_eq!(quantize_i24(-1.0), -8_388_608);
        assert_eq!(quantize_i24(1.0), 8_388_607);
        assert_eq!(quantize_i32(-1.0), i32::MIN);
        assert_eq!(quantize_i32(1.0), i32::MAX);
    }

    #[test]
    fn tpdf_dither_stays_within_one_lsb() {
        let mut dither = TpdfDither::default();
        for _ in 0..10_000 {
            let noise = dither.next_tpdf();
            assert!(noise > -1.0 && noise < 1.0);
        }
    }

    // ---- F-1：增益斜坡 ----

    #[test]
    fn ramp_gain_reaches_target_within_window() {
        let rate = 48_000_u32;
        let step = gain_ramp_step(rate);
        let mut gain = 0.0_f32;
        let samples_needed = (GAIN_RAMP_SECONDS * rate as f32).ceil() as usize + 1;
        for _ in 0..samples_needed {
            gain = ramp_gain(gain, 1.0, step);
        }
        assert!((gain - 1.0).abs() < 1e-6);
        // ramp-out 回到精确 0（静音快路径依赖 gain <= 0.0）
        for _ in 0..samples_needed {
            gain = ramp_gain(gain, 0.0, step);
        }
        assert_eq!(gain, 0.0);
    }

    #[test]
    fn ramp_gain_moves_monotonically() {
        let step = gain_ramp_step(44_100);
        let g1 = ramp_gain(0.0, 0.7, step);
        let g2 = ramp_gain(g1, 0.7, step);
        assert!(g1 > 0.0 && g2 > g1 && g2 <= 0.7);
    }

    // ---- F-11：代际比较 ----

    #[test]
    fn generation_comparison_accepts_current_and_newer() {
        assert!(generation_is_current(5, 5));
        assert!(generation_is_current(6, 5), "更新代必须被接受");
        assert!(!generation_is_current(4, 5), "旧代必须被丢弃");
        // wrapping 边界
        assert!(generation_is_current(0, u32::MAX));
        assert!(!generation_is_current(u32::MAX, 0));
    }

    // ---- F-3：seek 钳制 ----

    #[test]
    fn clamp_seek_respects_duration_margin() {
        assert_eq!(clamp_seek_seconds(-5.0, 100.0), 0.0);
        assert_eq!(clamp_seek_seconds(50.0, 100.0), 50.0);
        assert!((clamp_seek_seconds(100.0, 100.0) - 99.9).abs() < 1e-9);
        assert!((clamp_seek_seconds(500.0, 100.0) - 99.9).abs() < 1e-9);
        // duration 未知时透传
        assert_eq!(clamp_seek_seconds(42.0, 0.0), 42.0);
        // 极短曲目不会钳成负数
        assert_eq!(clamp_seek_seconds(1.0, 0.05), 0.0);
    }
}
