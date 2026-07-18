//! 可视化 / 声学分析 IPC。
//!
//! 渲染线程只把最终输出样本旁路进 [`SpectrumTap`](seraph_audio::SpectrumTap)；
//! 这里是唯一的读侧。侧栏频谱（`get_spectrum_frame`）与声学分析页
//! （`get_analysis_frame`）都经同一个 [`pump`] drain——谁先轮询谁触发计算，
//! 两个消费者共享结果，不会互相抢走 tap 里的样本。
//! FFT 与响度分析都在 IPC 线程算，不占音频线程。

use parking_lot::Mutex;
use seraph_audio::TapMeta;
use seraph_visualizer::{AnalysisEngine, SimpleVisualizer, Visualizer};
use serde::Serialize;
use tauri::State;

use super::error::{IpcError, IpcResult};
use crate::state::AppState;

/// 侧栏小频谱：48 柱够用，FFT 2048 延迟低。
const SIDEBAR_FFT_SIZE: usize = 2048;
const SIDEBAR_BIN_COUNT: usize = 48;
/// 分析页频谱：96 频点（1/12 倍频程量级），FFT 4096 换更好的低频分辨率。
const ANALYSIS_FFT_SIZE: usize = 4096;
const ANALYSIS_BIN_COUNT: usize = 96;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumFrameDto {
    pub bins: Vec<f32>,
    pub peak_left: f32,
    pub peak_right: f32,
}

/// 声学分析页一帧：频谱 + 电平 + 响度 + 立体声。`None` 序列化为 null（数据不足）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisFrameDto {
    /// 96 频点，0..1（-72dB 地板线性映射，与侧栏频谱同口径）
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
    /// 交错 L,R 抽取样本对（声场散点）
    pub scatter: Vec<f32>,
    pub sample_rate: u32,
}

struct AnalysisHub {
    channels: usize,
    sample_rate: u32,
    sidebar: SimpleVisualizer,
    analysis_fft: SimpleVisualizer,
    analysis: AnalysisEngine,
}

impl AnalysisHub {
    fn new(channels: usize, sample_rate: u32) -> IpcResult<Self> {
        let sidebar = SimpleVisualizer::new(SIDEBAR_FFT_SIZE, SIDEBAR_BIN_COUNT, channels)
            .map_err(|err| IpcError::from(format!("visualizer init failed: {err}")))?;
        let analysis_fft = SimpleVisualizer::new(ANALYSIS_FFT_SIZE, ANALYSIS_BIN_COUNT, channels)
            .map_err(|err| IpcError::from(format!("visualizer init failed: {err}")))?;
        Ok(Self {
            channels,
            sample_rate,
            sidebar,
            analysis_fft,
            analysis: AnalysisEngine::new(sample_rate, channels),
        })
    }
}

/// 常驻分析状态：声道数 / 采样率变化（切曲目、换设备）时重建。
static HUB: Mutex<Option<AnalysisHub>> = Mutex::new(None);

/// drain tap 一次，喂给全部消费者。返回持有 hub 的锁守卫。
fn pump(state: &State<'_, AppState>) -> IpcResult<parking_lot::MutexGuard<'static, Option<AnalysisHub>>> {
    let tap = state.audio.spectrum_tap();
    let mut samples = Vec::new();
    let TapMeta {
        channels,
        sample_rate,
    } = tap.drain(&mut samples);
    let channels = channels.max(1);
    let sample_rate = sample_rate.max(8_000);

    let mut guard = HUB.lock();
    let rebuild = !matches!(
        guard.as_ref(),
        Some(hub) if hub.channels == channels && hub.sample_rate == sample_rate
    );
    if rebuild {
        *guard = Some(AnalysisHub::new(channels, sample_rate)?);
    }

    if !samples.is_empty() {
        let hub = guard.as_mut().expect("hub populated above");
        let _ = hub.sidebar.push_samples(&samples);
        let _ = hub.analysis_fft.push_samples(&samples);
        hub.analysis.push(&samples);
    }
    Ok(guard)
}

#[tauri::command]
pub fn get_spectrum_frame(state: State<'_, AppState>) -> IpcResult<Option<SpectrumFrameDto>> {
    let guard = pump(&state)?;
    let hub = guard.as_ref().expect("hub populated by pump");
    Ok(hub.sidebar.latest_frame().map(|frame| SpectrumFrameDto {
        bins: frame.bins,
        peak_left: frame.peak_left,
        peak_right: frame.peak_right,
    }))
}

#[tauri::command]
pub fn get_analysis_frame(state: State<'_, AppState>) -> IpcResult<Option<AnalysisFrameDto>> {
    let guard = pump(&state)?;
    let hub = guard.as_ref().expect("hub populated by pump");
    let snapshot = hub.analysis.snapshot();
    let spectrum = hub
        .analysis_fft
        .latest_frame()
        .map(|frame| frame.bins)
        .unwrap_or_default();

    Ok(Some(AnalysisFrameDto {
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
        sample_rate: hub.sample_rate,
    }))
}

/// 换曲目时清空积分响度 / LRA / 真峰会话最大值（前端在曲目切换时调用）。
#[tauri::command]
pub fn reset_analysis_meters() {
    if let Some(hub) = HUB.lock().as_mut() {
        hub.analysis.reset_session();
    }
}
