//! WASAPI Exclusive backend trait adapter.
//!
//! The main player engine owns the high-level decode/resample/playback session. This adapter is the
//! lower-level [`AudioBackend`] implementation for callers that already have interleaved `f32` PCM
//! and want to submit it directly to a WASAPI exclusive stream.

use crate::backend::{AudioBackend, BackendError, Result};
use crate::device::{resolve_output_device_id, AudioDevice, ShareMode};
#[cfg(windows)]
use crate::engine::{quantize_i16_tpdf, quantize_i24, quantize_i32, TpdfDither};
use seraph_core::types::{BitDepth, Channels, SampleRate};
#[cfg(windows)]
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_EXCLUSIVE_PERIOD_FRAMES: u32 = 512;
const BUFFER_SECONDS: usize = 2;
/// F-1（同型）：暂停/恢复增益斜坡时长。
#[cfg(windows)]
const GAIN_RAMP_SECONDS: f32 = 0.005;

pub struct WasapiExclusive {
    current_device: Option<AudioDevice>,
    current_format: Option<(SampleRate, BitDepth, Channels)>,
    stream: Option<WasapiStream>,
}

struct WasapiStream {
    shared: Arc<WasapiShared>,
    tx: SyncSender<Vec<f32>>,
    worker: Option<JoinHandle<Result<()>>>,
}

struct WasapiShared {
    paused: AtomicBool,
    stopped: AtomicBool,
    channels: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasapiSampleFormat {
    I16,
    I24In32,
    I32,
}

impl WasapiExclusive {
    pub fn new() -> Self {
        Self {
            current_device: None,
            current_format: None,
            stream: None,
        }
    }
}

impl Default for WasapiExclusive {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for WasapiExclusive {
    fn list_devices(&self) -> Result<Vec<AudioDevice>> {
        crate::device::list_output_devices()
    }

    fn open(
        &mut self,
        device: &AudioDevice,
        share_mode: ShareMode,
        sample_rate: SampleRate,
        bit_depth: BitDepth,
        channels: Channels,
    ) -> Result<()> {
        if share_mode != ShareMode::Exclusive {
            return Err(BackendError::ExclusiveModeUnavailable(
                "WasapiExclusive only supports exclusive share mode".into(),
            ));
        }
        if sample_rate.0 == 0 || channels.0 == 0 {
            return Err(BackendError::UnsupportedFormat(
                "sample rate and channels must be non-zero".into(),
            ));
        }

        self.close()?;

        let format = WasapiSampleFormat::from_bit_depth(bit_depth);
        let shared = Arc::new(WasapiShared {
            paused: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            channels: usize::from(channels.0),
        });
        let queue_chunks = (sample_rate.0 as usize / DEFAULT_EXCLUSIVE_PERIOD_FRAMES as usize)
            .saturating_mul(BUFFER_SECONDS)
            .max(4);
        let (tx, rx) = mpsc::sync_channel(queue_chunks);
        let device_id = resolve_output_device_id(&device.id)?;
        let worker = spawn_wasapi_submit_worker(
            device_id.clone(),
            sample_rate,
            bit_depth,
            channels,
            format,
            shared.clone(),
            rx,
        )?;

        let mut current_device = device.clone();
        current_device.id = device_id;
        self.current_device = Some(current_device);
        self.current_format = Some((sample_rate, bit_depth, channels));
        self.stream = Some(WasapiStream {
            shared,
            tx,
            worker: Some(worker),
        });
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            stream.shared.stopped.store(true, Ordering::Release);
            if let Some(worker) = stream.worker.take() {
                match worker.join() {
                    Ok(result) => result?,
                    Err(_) => {
                        return Err(BackendError::Internal(
                            "WASAPI render worker panicked".into(),
                        ));
                    }
                }
            }
        }
        self.current_device = None;
        self.current_format = None;
        Ok(())
    }

    fn play(&mut self) -> Result<()> {
        let Some(stream) = self.stream.as_ref() else {
            return Err(BackendError::Internal("WASAPI stream is not open".into()));
        };
        stream.shared.paused.store(false, Ordering::Release);
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        let Some(stream) = self.stream.as_ref() else {
            return Ok(());
        };
        stream.shared.paused.store(true, Ordering::Release);
        Ok(())
    }

    fn submit(&mut self, samples: &[f32]) -> Result<usize> {
        let Some(stream) = self.stream.as_mut() else {
            return Err(BackendError::Internal("WASAPI stream is not open".into()));
        };
        if stream.shared.stopped.load(Ordering::Acquire) {
            return Err(BackendError::DeviceLost(
                "WASAPI stream is no longer running".into(),
            ));
        }

        match stream.tx.try_send(samples.to_vec()) {
            Ok(()) => Ok(samples.len()),
            Err(TrySendError::Full(_)) => Ok(0),
            Err(TrySendError::Disconnected(_)) => Err(BackendError::DeviceLost(
                "WASAPI render worker is not available".into(),
            )),
        }
    }

    fn current_device(&self) -> Option<&AudioDevice> {
        self.current_device.as_ref()
    }

    fn current_format(&self) -> Option<(SampleRate, BitDepth, Channels)> {
        self.current_format
    }
}

impl Drop for WasapiExclusive {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl WasapiSampleFormat {
    fn from_bit_depth(bit_depth: BitDepth) -> Self {
        match bit_depth.0 {
            0..=16 => Self::I16,
            17..=24 => Self::I24In32,
            _ => Self::I32,
        }
    }

    fn valid_bits(self, requested: BitDepth) -> usize {
        match self {
            Self::I16 => 16,
            Self::I24In32 => requested.0.clamp(17, 24) as usize,
            Self::I32 => 32,
        }
    }

    fn store_bits(self) -> usize {
        match self {
            Self::I16 => 16,
            Self::I24In32 | Self::I32 => 32,
        }
    }
}

#[cfg(windows)]
fn spawn_wasapi_submit_worker(
    device_id: String,
    sample_rate: SampleRate,
    bit_depth: BitDepth,
    channels: Channels,
    format: WasapiSampleFormat,
    shared: Arc<WasapiShared>,
    rx: Receiver<Vec<f32>>,
) -> Result<JoinHandle<Result<()>>> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let shared_for_worker = shared.clone();
    let worker = thread::spawn(move || {
        run_wasapi_submit_worker(WasapiSubmitWorkerConfig {
            device_id,
            sample_rate,
            bit_depth,
            channels,
            format,
            shared: shared_for_worker,
            rx,
            ready_tx,
        })
    });

    match ready_rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Ok(())) => Ok(worker),
        Ok(Err(message)) => {
            shared.stopped.store(true, Ordering::Release);
            let _ = worker.join();
            Err(BackendError::ExclusiveModeUnavailable(message))
        }
        Err(_) => {
            // F-4（同型）：初始化超时不 join——worker 可能卡死在驱动调用内部，
            // stopped 已置位，worker 返回后会自行退出；泄漏 detached 线程好过调用方永久阻塞。
            shared.stopped.store(true, Ordering::Release);
            drop(worker);
            Err(BackendError::ExclusiveModeUnavailable(
                "WASAPI exclusive stream init timed out".into(),
            ))
        }
    }
}

#[cfg(not(windows))]
fn spawn_wasapi_submit_worker(
    _device_id: String,
    _sample_rate: SampleRate,
    _bit_depth: BitDepth,
    _channels: Channels,
    _format: WasapiSampleFormat,
    _shared: Arc<WasapiShared>,
    _rx: Receiver<Vec<f32>>,
) -> Result<JoinHandle<Result<()>>> {
    Err(BackendError::ExclusiveModeUnavailable(
        "WASAPI exclusive output is only available on Windows".into(),
    ))
}

#[cfg(windows)]
struct WasapiSubmitWorkerConfig {
    device_id: String,
    sample_rate: SampleRate,
    bit_depth: BitDepth,
    channels: Channels,
    format: WasapiSampleFormat,
    shared: Arc<WasapiShared>,
    rx: Receiver<Vec<f32>>,
    ready_tx: Sender<std::result::Result<(), String>>,
}

#[cfg(windows)]
fn run_wasapi_submit_worker(config: WasapiSubmitWorkerConfig) -> Result<()> {
    use wasapi::{Direction, SampleType, StreamMode, WaveFormat};

    let WasapiSubmitWorkerConfig {
        device_id,
        sample_rate,
        bit_depth,
        channels,
        format,
        shared,
        rx,
        ready_tx,
    } = config;

    let init_result: Result<(wasapi::AudioClient, wasapi::AudioRenderClient, Duration)> = (|| {
        wasapi::initialize_mta()
            .ok()
            .map_err(|err| BackendError::Internal(err.to_string()))?;

        let enumerator = wasapi::DeviceEnumerator::new()
            .map_err(|err| BackendError::Internal(err.to_string()))?;
        let device = enumerator
            .get_device(&device_id)
            .map_err(|_| BackendError::DeviceNotFound)?;

        let mut audio_client = device
            .get_iaudioclient()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let wave_format = WaveFormat::new(
            format.store_bits(),
            format.valid_bits(bit_depth),
            &SampleType::Int,
            sample_rate.0 as usize,
            usize::from(channels.0),
            None,
        );
        let wave_format = audio_client
            .is_supported_exclusive_with_quirks(&wave_format)
            .map_err(|err| BackendError::ExclusiveModeUnavailable(err.to_string()))?;
        let desired_period = wasapi::calculate_period_100ns(
            i64::from(DEFAULT_EXCLUSIVE_PERIOD_FRAMES),
            i64::from(wave_format.get_samplespersec()),
        );
        let period = audio_client
            .calculate_aligned_period_near(desired_period, Some(128), &wave_format)
            .unwrap_or(desired_period);
        let mode = StreamMode::PollingExclusive {
            period_hns: period,
            buffer_duration_hns: 16 * period,
        };

        audio_client
            .initialize_client(&wave_format, &Direction::Render, &mode)
            .map_err(|err| BackendError::ExclusiveModeUnavailable(err.to_string()))?;
        let render_client = audio_client
            .get_audiorenderclient()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let buffer_frames = audio_client
            .get_buffer_size()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let sleep_period = Duration::from_millis(
            (500 * u64::from(buffer_frames) / u64::from(sample_rate.0.max(1))).max(1),
        );
        audio_client
            .start_stream()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

        Ok((audio_client, render_client, sleep_period))
    })();

    let (audio_client, render_client, sleep_period) = match init_result {
        Ok(parts) => {
            let _ = ready_tx.send(Ok(()));
            parts
        }
        Err(err) => {
            let _ = ready_tx.send(Err(err.to_string()));
            return Err(err);
        }
    };

    let mut pending = VecDeque::new();
    // F-1（同型）：暂停/恢复的增益斜坡状态（worker 本地，无锁）。
    let mut gain = 0.0_f32;
    let gain_step = 1.0 / (GAIN_RAMP_SECONDS * sample_rate.0.max(1) as f32);
    let mut dither = TpdfDither::default();
    // F-13（同型）：scratch buffer 复用。
    let mut samples_scratch: Vec<f32> = Vec::new();
    let mut bytes_scratch: Vec<u8> = Vec::new();
    while !shared.stopped.load(Ordering::Acquire) {
        let frames = audio_client
            .get_available_space_in_frames()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        if frames > 0 {
            while let Ok(chunk) = rx.try_recv() {
                pending.extend(chunk);
            }
            render_submit_buffer(
                frames as usize,
                format,
                &shared,
                &mut pending,
                &mut gain,
                gain_step,
                &mut dither,
                &mut samples_scratch,
                &mut bytes_scratch,
            );
            render_client
                .write_to_device(frames as usize, &bytes_scratch, None)
                .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        }
        thread::sleep(sleep_period);
    }

    let _ = audio_client.stop_stream();
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn render_submit_buffer(
    frames: usize,
    format: WasapiSampleFormat,
    shared: &WasapiShared,
    pending: &mut VecDeque<f32>,
    gain: &mut f32,
    gain_step: f32,
    dither: &mut TpdfDither,
    samples: &mut Vec<f32>,
    bytes: &mut Vec<u8>,
) {
    let sample_count = frames * shared.channels.max(1);
    let paused = shared.paused.load(Ordering::Acquire);
    let target = if paused { 0.0 } else { 1.0 };
    samples.clear();
    samples.reserve(sample_count);
    for _ in 0..sample_count {
        // F-1（同型）：先 ramp 后静音——暂停后继续以递减增益消费 pending，
        // ramp 到 0 才输出纯静音；恢复时从 0 ramp-in，消除硬切爆音。
        *gain += (target - *gain).clamp(-gain_step, gain_step);
        let value = if paused && *gain <= 0.0 {
            0.0
        } else {
            pending.pop_front().unwrap_or(0.0) * *gain
        };
        samples.push(value.clamp(-1.0, 1.0));
    }

    bytes.clear();
    match format {
        WasapiSampleFormat::I16 => {
            bytes.reserve(sample_count * 2);
            for &sample in samples.iter() {
                // F-14（同型）：×32768 + round + clamp，叠加 TPDF dither。
                bytes.extend_from_slice(&quantize_i16_tpdf(sample, dither).to_le_bytes());
            }
        }
        WasapiSampleFormat::I24In32 => {
            bytes.reserve(sample_count * 4);
            for &sample in samples.iter() {
                bytes.extend_from_slice(&(quantize_i24(sample) << 8).to_le_bytes());
            }
        }
        WasapiSampleFormat::I32 => {
            bytes.reserve(sample_count * 4);
            for &sample in samples.iter() {
                bytes.extend_from_slice(&quantize_i32(sample).to_le_bytes());
            }
        }
    }
}
