//! 唯一的 tap 读侧。IPC 续租需求并读取已完成帧，FFT / 声学分析在专用线程执行。

use crate::ipc::error::{IpcError, IpcResult};
use parking_lot::{Condvar, Mutex};
use seraph_audio::{SpectrumTap, TapMeta};
use seraph_visualizer::{AnalysisEngine, AnalysisFeatures, SimpleVisualizer, Visualizer};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const DEMAND_LEASE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRequest {
    pub session_id: String,
    pub request_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AnalysisDemand {
    pub spectrum: bool,
    pub loudness: bool,
    pub levels: bool,
    pub field: bool,
    pub scope: bool,
}

impl Default for AnalysisDemand {
    fn default() -> Self {
        Self {
            spectrum: true,
            loudness: true,
            levels: true,
            field: true,
            scope: true,
        }
    }
}

impl AnalysisDemand {
    fn features(self) -> AnalysisFeatures {
        AnalysisFeatures {
            loudness: self.loudness,
            levels: self.levels,
            stereo: self.field,
            waveform: self.scope,
        }
    }

    pub fn is_empty(self) -> bool {
        !self.spectrum && !self.loudness && !self.levels && !self.field && !self.scope
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Demand {
    Sidebar,
    Analysis(AnalysisDemand),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumFrameDto {
    pub bins: Vec<f32>,
    pub peak_left: f32,
    pub peak_right: f32,
    pub request_id: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisFrameDto {
    pub spectrum: Vec<f32>,
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
    pub momentary_lufs: Option<f32>,
    pub short_term_lufs: Option<f32>,
    pub integrated_lufs: Option<f32>,
    pub loudness_range_lu: Option<f32>,
    pub true_peak_db: Option<f32>,
    pub true_peak_max_db: Option<f32>,
    pub correlation: f32,
    pub scatter: Vec<f32>,
    pub waveform: Vec<i16>,
    pub sample_rate: u32,
    pub request_id: String,
    pub sequence: u64,
}

#[derive(Default)]
struct Frames {
    spectrum: Option<SpectrumFrameDto>,
    analysis: Option<AnalysisFrameDto>,
}

#[derive(Clone)]
struct Subscription {
    request: FrameRequest,
    demand: Demand,
    renewed_at: Instant,
    revision: u64,
}

#[derive(Default)]
struct Control {
    started: bool,
    stopped: bool,
    revision: u64,
    reset: u64,
    subscription: Option<Subscription>,
    frames: Frames,
    error: Option<IpcError>,
}

#[derive(Default)]
struct Shared {
    control: Mutex<Control>,
    wake: Condvar,
}

pub(crate) struct VisualizerService {
    tap: Arc<SpectrumTap>,
    shared: Arc<Shared>,
}

impl VisualizerService {
    pub(crate) fn new(tap: Arc<SpectrumTap>) -> Self {
        Self {
            tap,
            shared: Arc::new(Shared::default()),
        }
    }

    fn touch(
        &self,
        request: FrameRequest,
        demand: Demand,
    ) -> IpcResult<parking_lot::MutexGuard<'_, Control>> {
        if request.session_id.len() > 256 || request.request_id.len() > 256 {
            return Err(IpcError::invalid_input("分析会话标识过长"));
        }
        let mut control = self.shared.control.lock();
        if !control.started {
            let shared = Arc::clone(&self.shared);
            let tap = Arc::clone(&self.tap);
            thread::Builder::new()
                .name("seraph-analysis".into())
                .spawn(move || run_worker(shared, tap))
                .map_err(|err| IpcError::from(format!("无法启动分析线程：{err}")))?;
            control.started = true;
        }
        let same = control
            .subscription
            .as_ref()
            .is_some_and(|previous| previous.request == request && previous.demand == demand);
        if same {
            control
                .subscription
                .as_mut()
                .expect("subscription exists")
                .renewed_at = Instant::now();
        } else {
            control.revision += 1;
            control.subscription = Some(Subscription {
                request,
                demand,
                renewed_at: Instant::now(),
                revision: control.revision,
            });
            control.frames = Frames::default();
            control.error = None;
        }
        self.shared.wake.notify_one();
        Ok(control)
    }

    pub(crate) fn spectrum(&self, request: FrameRequest) -> IpcResult<Option<SpectrumFrameDto>> {
        let control = self.touch(request, Demand::Sidebar)?;
        if let Some(error) = &control.error {
            return Err(error.clone());
        }
        Ok(control.frames.spectrum.clone())
    }

    pub(crate) fn analysis(
        &self,
        request: FrameRequest,
        demand: AnalysisDemand,
    ) -> IpcResult<Option<AnalysisFrameDto>> {
        if demand.is_empty() {
            return Ok(None);
        }
        let control = self.touch(request, Demand::Analysis(demand))?;
        if let Some(error) = &control.error {
            return Err(error.clone());
        }
        Ok(control.frames.analysis.clone())
    }

    pub(crate) fn reset(&self) {
        let mut control = self.shared.control.lock();
        control.reset += 1;
        control.frames = Frames::default();
        self.shared.wake.notify_one();
    }
}

impl Drop for VisualizerService {
    fn drop(&mut self) {
        self.shared.control.lock().stopped = true;
        self.shared.wake.notify_one();
    }
}

struct AnalysisHub {
    meta: TapMeta,
    sidebar: Option<SimpleVisualizer>,
    spectrum: Option<SimpleVisualizer>,
    analysis: AnalysisEngine,
}

impl AnalysisHub {
    fn new(meta: TapMeta) -> Self {
        Self {
            meta,
            sidebar: None,
            spectrum: None,
            analysis: AnalysisEngine::new(meta.sample_rate, meta.channels),
        }
    }

    fn reset_continuity(&mut self) {
        if let Some(fft) = &self.sidebar {
            fft.reset();
        }
        if let Some(fft) = &self.spectrum {
            fft.reset();
        }
        self.analysis.reset_continuity();
    }

    fn compute(
        &mut self,
        samples: &[f32],
        subscription: &Subscription,
        sequence: u64,
    ) -> IpcResult<Frames> {
        let mut frames = Frames::default();
        if samples.is_empty() {
            return Ok(frames);
        }
        let request_id = subscription.request.request_id.clone();
        match subscription.demand {
            Demand::Sidebar => {
                let fft = get_fft(&mut self.sidebar, 2048, 48, self.meta)?;
                fft.push_samples(samples)
                    .map_err(|err| IpcError::from(err.to_string()))?;
                frames.spectrum = fft.latest_frame().map(|frame| SpectrumFrameDto {
                    bins: frame.bins,
                    peak_left: frame.peak_left,
                    peak_right: frame.peak_right,
                    request_id,
                    sequence,
                });
            }
            Demand::Analysis(demand) => {
                let spectrum = if demand.spectrum {
                    let fft = get_fft(&mut self.spectrum, 4096, 96, self.meta)?;
                    fft.push_samples(samples)
                        .map_err(|err| IpcError::from(err.to_string()))?;
                    fft.latest_frame()
                        .map(|frame| frame.bins)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let features = demand.features();
                self.analysis.push_with_features(samples, features);
                let snapshot = self.analysis.snapshot_with_features(features);
                frames.analysis = Some(AnalysisFrameDto {
                    spectrum,
                    peak_left: snapshot.peak_left,
                    peak_right: snapshot.peak_right,
                    rms_left: snapshot.rms_left,
                    rms_right: snapshot.rms_right,
                    momentary_lufs: snapshot.momentary_lufs,
                    short_term_lufs: snapshot.short_term_lufs,
                    integrated_lufs: snapshot.integrated_lufs,
                    loudness_range_lu: snapshot.loudness_range_lu,
                    true_peak_db: snapshot.true_peak_db,
                    true_peak_max_db: snapshot.true_peak_max_db,
                    correlation: snapshot.correlation,
                    scatter: snapshot.scatter,
                    waveform: snapshot.waveform,
                    sample_rate: self.meta.sample_rate,
                    request_id,
                    sequence,
                });
            }
        }
        Ok(frames)
    }
}

fn get_fft(
    slot: &mut Option<SimpleVisualizer>,
    size: usize,
    bins: usize,
    meta: TapMeta,
) -> IpcResult<&SimpleVisualizer> {
    if slot.is_none() {
        *slot = Some(
            SimpleVisualizer::new(size, bins, meta.channels, meta.sample_rate)
                .map_err(|err| IpcError::from(format!("频谱初始化失败：{err}")))?,
        );
    }
    Ok(slot.as_ref().expect("FFT initialized"))
}

fn run_worker(shared: Arc<Shared>, tap: Arc<SpectrumTap>) {
    let mut hub: Option<AnalysisHub> = None;
    let mut samples = Vec::with_capacity(64 * 1024);
    let mut previous: Option<Subscription> = None;
    let mut last_reset = 0;
    let mut last_frame = Instant::now() - FRAME_INTERVAL;
    let mut sequence = 0;
    let mut idle = true;
    loop {
        let (subscription, reset) = {
            let mut control = shared.control.lock();
            loop {
                if control.stopped {
                    return;
                }
                if let Some(subscription) = &control.subscription {
                    if subscription.renewed_at.elapsed() < DEMAND_LEASE {
                        let remaining = FRAME_INTERVAL.saturating_sub(last_frame.elapsed());
                        if remaining.is_zero() {
                            break (subscription.clone(), control.reset);
                        }
                        shared.wake.wait_for(&mut control, remaining);
                        continue;
                    }
                }
                idle = true;
                shared.wake.wait(&mut control);
            }
        };
        let changed = previous
            .as_ref()
            .is_none_or(|old| old.revision != subscription.revision);
        let session_changed = previous
            .as_ref()
            .is_none_or(|old| old.request.session_id != subscription.request.session_id);
        samples.clear();
        let mut meta = tap.drain(&mut samples);
        meta.channels = meta.channels.max(1);
        meta.sample_rate = meta.sample_rate.max(8_000);
        if hub.as_ref().is_none_or(|hub| hub.meta != meta) {
            hub = Some(AnalysisHub::new(meta));
        }
        let current = hub.as_mut().expect("hub initialized");
        if session_changed || reset != last_reset {
            current.analysis.reset_session();
        }
        if idle || changed || reset != last_reset {
            current.reset_continuity();
            // 旧曲目、隐藏期间和上一订阅积压的数据不进入新一轮分析。
            samples.clear();
        }
        idle = false;
        last_reset = reset;
        sequence += 1;
        let result = current.compute(&samples, &subscription, sequence);
        last_frame = Instant::now();
        previous = Some(subscription.clone());
        let mut control = shared.control.lock();
        // IPC 在计算期间可能切换需求/曲目；旧结果绝不发布到新请求。
        if control.reset == reset
            && control
                .subscription
                .as_ref()
                .is_some_and(|active| active.revision == subscription.revision)
        {
            match result {
                Ok(frames) => {
                    if frames.spectrum.is_some() {
                        control.frames.spectrum = frames.spectrum;
                    }
                    if frames.analysis.is_some() {
                        control.frames.analysis = frames.analysis;
                    }
                    control.error = None;
                }
                Err(error) => {
                    control.error = Some(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_never_initializes_analysis_fft_or_loudness() {
        let mut hub = AnalysisHub::new(TapMeta {
            channels: 2,
            sample_rate: 48_000,
        });
        let subscription = Subscription {
            request: FrameRequest::default(),
            demand: Demand::Sidebar,
            renewed_at: Instant::now(),
            revision: 1,
        };
        let frames = hub.compute(&vec![0.2; 96_000], &subscription, 1).unwrap();
        assert!(frames.spectrum.is_some());
        assert!(hub.spectrum.is_none());
        assert!(hub.analysis.snapshot().integrated_lufs.is_none());
    }

    #[test]
    fn scope_only_does_not_compute_fft_loudness_or_scatter() {
        let mut hub = AnalysisHub::new(TapMeta {
            channels: 2,
            sample_rate: 48_000,
        });
        let demand = AnalysisDemand {
            spectrum: false,
            loudness: false,
            levels: false,
            field: false,
            scope: true,
        };
        let subscription = Subscription {
            request: FrameRequest {
                session_id: "track-a".into(),
                request_id: "request-a".into(),
            },
            demand: Demand::Analysis(demand),
            renewed_at: Instant::now(),
            revision: 1,
        };
        let frame = hub
            .compute(&vec![0.2; 96_000], &subscription, 7)
            .unwrap()
            .analysis
            .unwrap();
        assert!(!frame.waveform.is_empty());
        assert!(frame.scatter.is_empty() && frame.spectrum.is_empty());
        assert!(frame.integrated_lufs.is_none() && frame.true_peak_db.is_none());
        assert_eq!(frame.request_id, "request-a");
        assert_eq!(frame.sequence, 7);
        assert!(hub.sidebar.is_none() && hub.spectrum.is_none());
    }
}
