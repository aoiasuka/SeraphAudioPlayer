use crate::{
    backend::{BackendError, Result},
    device::output_device_by_id,
};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, Stream, StreamConfig,
};
use parking_lot::Mutex;
use seraph_core::{EventBus, PlayerEvent};
use seraph_decoder::{open_decoder, probe_stream_info, StreamInfo};
use seraph_dsp::resample_interleaved_linear;
use seraph_visualizer::{SimpleVisualizer, Visualizer};
use std::{
    collections::VecDeque,
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
}

struct PlaybackSession {
    path: PathBuf,
    track_id: String,
    duration_seconds: f64,
    shared: Arc<PlaybackShared>,
    worker: Option<JoinHandle<()>>,
    _stream: Stream,
}

struct PlaybackShared {
    queue: Mutex<VecDeque<f32>>,
    seek_request: Mutex<Option<f64>>,
    paused: AtomicBool,
    stopped: AtomicBool,
    frame_position: AtomicU64,
    volume_bits: AtomicU32,
    output_rate: u32,
    output_channels: usize,
    max_buffer_samples: usize,
}

impl PlaybackEngine {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            session: None,
            volume: 0.7,
            selected_device_id: None,
        }
    }

    pub fn play_file(&mut self, path: PathBuf, track_id: String, start_seconds: f64) -> Result<()> {
        if self.session.as_ref().is_some_and(|session| {
            session.path == path
                && session.track_id == track_id
                && !session.shared.stopped.load(Ordering::Acquire)
        }) {
            if start_seconds > 0.0 {
                self.seek(start_seconds)?;
            }
            return self.resume();
        }

        let info = probe_stream_info(&path)
            .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
        let duration_seconds = info.duration_seconds;
        self.stop_session();

        let device = self.output_device()?;
        let supported_config = device
            .default_output_config()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let sample_format = supported_config.sample_format();
        let config: StreamConfig = supported_config.into();
        let output_rate = config.sample_rate.0;
        let output_channels = usize::from(config.channels).max(1);
        let shared = Arc::new(PlaybackShared::new(
            output_rate,
            output_channels,
            self.volume,
        ));
        shared.frame_position.store(
            seconds_to_frames(start_seconds, output_rate),
            Ordering::Relaxed,
        );

        let stream = build_output_stream(&device, &config, sample_format, shared.clone())?;
        stream
            .play()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

        let worker = spawn_decode_worker(
            path.clone(),
            track_id.clone(),
            info,
            shared.clone(),
            self.event_bus.clone(),
            start_seconds,
        );

        debug!(
            "started playback: {} @ {} Hz / {} ch",
            path.display(),
            output_rate,
            output_channels
        );
        self.session = Some(PlaybackSession {
            path,
            track_id: track_id.clone(),
            duration_seconds,
            shared,
            worker: Some(worker),
            _stream: stream,
        });
        self.event_bus.publish(PlayerEvent::TrackChanged {
            track_id: track_id.clone(),
        });
        self.event_bus
            .publish(PlayerEvent::PlaybackStarted { track_id });
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
        session.shared.queue.lock().clear();
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
        session.shared.queue.lock().clear();
        if let Some(worker) = session.worker.take() {
            let _ = worker.join();
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
            queue: Mutex::new(VecDeque::with_capacity(max_buffer_samples)),
            seek_request: Mutex::new(None),
            paused: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            frame_position: AtomicU64::new(0),
            volume_bits: AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()),
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

    fn progress_seconds(&self) -> f64 {
        self.frame_position.load(Ordering::Relaxed) as f64 / self.output_rate.max(1) as f64
    }
}

fn build_output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    shared: Arc<PlaybackShared>,
) -> Result<Stream> {
    let err_fn = |err| warn!("audio output stream error: {err}");
    match sample_format {
        SampleFormat::F32 => device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| render_output_f32(data, &shared),
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::I16 => device
            .build_output_stream(
                config,
                move |data: &mut [i16], _| render_output_i16(data, &shared),
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::U16 => device
            .build_output_stream(
                config,
                move |data: &mut [u16], _| render_output_u16(data, &shared),
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        SampleFormat::F64 => device
            .build_output_stream(
                config,
                move |data: &mut [f64], _| render_output_f64(data, &shared),
                err_fn,
                None,
            )
            .map_err(map_build_stream_error),
        other => Err(BackendError::UnsupportedFormat(format!(
            "output sample format {other:?}"
        ))),
    }
}

fn spawn_decode_worker(
    path: PathBuf,
    track_id: String,
    info: StreamInfo,
    shared: Arc<PlaybackShared>,
    event_bus: EventBus,
    start_seconds: f64,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let result = run_decode_worker(
            &path,
            &track_id,
            &info,
            &shared,
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
    path: &PathBuf,
    track_id: &str,
    info: &StreamInfo,
    shared: &Arc<PlaybackShared>,
    event_bus: &EventBus,
    start_seconds: f64,
) -> Result<()> {
    let mut decoder =
        open_decoder(path).map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
    if start_seconds > 0.0 {
        decoder
            .seek(start_seconds)
            .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
    }

    let input_rate = info.sample_rate.0.max(1);
    let input_channels = usize::from(info.channels.0).max(1);
    let visualizer =
        SimpleVisualizer::new(SPECTRUM_FFT_SIZE, SPECTRUM_BINS, shared.output_channels)
            .map_err(|err| BackendError::Internal(err.to_string()))?;
    let mut last_progress = Instant::now();
    let mut last_spectrum = Instant::now();

    while !shared.stopped.load(Ordering::Acquire) {
        if let Some(seconds) = shared.seek_request.lock().take() {
            decoder
                .seek(seconds)
                .map_err(|err| BackendError::UnsupportedFormat(err.to_string()))?;
            shared.queue.lock().clear();
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

        if shared.queue.lock().len() >= shared.max_buffer_samples {
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

        let samples = adapt_samples(
            &packet.samples,
            input_rate,
            input_channels,
            shared.output_rate,
            shared.output_channels,
        );
        push_samples(
            &samples,
            shared,
            event_bus,
            track_id,
            info.duration_seconds,
            &mut last_progress,
        );
        publish_spectrum_if_due(&visualizer, &samples, event_bus, &mut last_spectrum)?;
    }

    while !shared.stopped.load(Ordering::Acquire) && !shared.queue.lock().is_empty() {
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
) {
    let mut offset = 0;
    while offset < samples.len() && !shared.stopped.load(Ordering::Acquire) {
        if let Some(seconds) = shared.seek_request.lock().take() {
            shared.frame_position.store(
                seconds_to_frames(seconds, shared.output_rate),
                Ordering::Relaxed,
            );
            shared.queue.lock().clear();
            return;
        }

        let written = {
            let mut queue = shared.queue.lock();
            let available = shared.max_buffer_samples.saturating_sub(queue.len());
            let count = available.min(samples.len() - offset);
            queue.extend(samples[offset..offset + count].iter().copied());
            count
        };

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

fn render_output_f32(data: &mut [f32], shared: &PlaybackShared) {
    render_output(data, shared, |sample, value| *sample = value);
}

fn render_output_f64(data: &mut [f64], shared: &PlaybackShared) {
    render_output(data, shared, |sample, value| *sample = f64::from(value));
}

fn render_output_i16(data: &mut [i16], shared: &PlaybackShared) {
    render_output(data, shared, |sample, value| {
        *sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
    });
}

fn render_output_u16(data: &mut [u16], shared: &PlaybackShared) {
    render_output(data, shared, |sample, value| {
        *sample = ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
    });
}

fn render_output<T>(
    data: &mut [T],
    shared: &PlaybackShared,
    mut write_sample: impl FnMut(&mut T, f32),
) {
    if shared.stopped.load(Ordering::Acquire) || shared.paused.load(Ordering::Acquire) {
        for sample in data {
            write_sample(sample, 0.0);
        }
        return;
    }

    let volume = shared.volume();
    let mut consumed = 0_usize;
    {
        let mut queue = shared.queue.lock();
        for sample in data.iter_mut() {
            let Some(value) = queue.pop_front() else {
                write_sample(sample, 0.0);
                continue;
            };
            consumed += 1;
            let value = value * volume;
            write_sample(sample, value);
        }
    }

    let frames = consumed / shared.output_channels.max(1);
    if frames > 0 {
        shared
            .frame_position
            .fetch_add(frames as u64, Ordering::Relaxed);
    }
}

fn adapt_samples(
    input: &[f32],
    input_rate: u32,
    input_channels: usize,
    output_rate: u32,
    output_channels: usize,
) -> Vec<f32> {
    let input_channels = input_channels.max(1);
    let output_channels = output_channels.max(1);
    if input.is_empty() {
        return Vec::new();
    }

    let input_frames = input.len() / input_channels;
    if input_frames == 0 {
        return Vec::new();
    }

    let remapped = remap_channels(input, input_channels, output_channels);
    if input_rate == output_rate {
        return remapped;
    }

    let mut output = Vec::new();
    match resample_interleaved_linear(
        &remapped,
        output_channels,
        input_rate.max(1),
        output_rate.max(1),
        &mut output,
    ) {
        Ok(()) => output,
        Err(_) => remapped,
    }
}

fn remap_channels(input: &[f32], input_channels: usize, output_channels: usize) -> Vec<f32> {
    let input_frames = input.len() / input_channels;
    let mut output = Vec::with_capacity(input_frames * output_channels);

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

    output
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

    #[test]
    fn adapts_mono_to_stereo_without_resampling() {
        let output = adapt_samples(&[0.25, -0.5], 44_100, 1, 44_100, 2);
        assert_eq!(output, vec![0.25, 0.25, -0.5, -0.5]);
    }

    #[test]
    fn adapts_stereo_to_mono_by_averaging_channels() {
        let output = adapt_samples(&[0.25, 0.75, -0.5, 0.5], 44_100, 2, 44_100, 1);
        assert_eq!(output, vec![0.5, 0.0]);
    }

    #[test]
    fn resamples_to_target_rate() {
        let output = adapt_samples(&[0.0, 1.0, 0.0, -1.0], 4, 1, 2, 1);
        assert_eq!(output.len(), 2);
        assert!((output[0] - 0.0).abs() < 0.001);
        assert!((output[1] - 0.0).abs() < 0.001);
    }
}
