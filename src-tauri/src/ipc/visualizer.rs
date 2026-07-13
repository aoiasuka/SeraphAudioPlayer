//! 频谱可视化 IPC。
//!
//! 前端播放中以 ~30fps 轮询 [`get_spectrum_frame`]：
//! 从渲染线程的 [`SpectrumTap`](seraph_audio::SpectrumTap) drain 新样本，
//! 喂给 `seraph-visualizer` 的 FFT（在 IPC 线程计算，不占音频线程），
//! 返回 log 频率分箱后的柱状数据。

use parking_lot::Mutex;
use seraph_visualizer::{SimpleVisualizer, Visualizer};
use serde::Serialize;
use tauri::State;

use super::error::{IpcError, IpcResult};
use crate::state::AppState;

const FFT_SIZE: usize = 2048;
const BIN_COUNT: usize = 48;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumFrameDto {
    pub bins: Vec<f32>,
    pub peak_left: f32,
    pub peak_right: f32,
}

struct VisualizerSlot {
    channels: usize,
    visualizer: SimpleVisualizer,
}

/// FFT 状态常驻：声道数变化（切曲目/换设备）时重建。
static VISUALIZER_SLOT: Mutex<Option<VisualizerSlot>> = Mutex::new(None);

#[tauri::command]
pub fn get_spectrum_frame(state: State<'_, AppState>) -> IpcResult<Option<SpectrumFrameDto>> {
    let tap = state.audio.spectrum_tap();
    let mut samples = Vec::new();
    let channels = tap.drain(&mut samples).max(1);

    let mut guard = VISUALIZER_SLOT.lock();
    let rebuild = !matches!(guard.as_ref(), Some(slot) if slot.channels == channels);
    if rebuild {
        let visualizer = SimpleVisualizer::new(FFT_SIZE, BIN_COUNT, channels)
            .map_err(|err| IpcError::from(format!("visualizer init failed: {err}")))?;
        *guard = Some(VisualizerSlot {
            channels,
            visualizer,
        });
    }
    let slot = guard.as_mut().expect("slot populated above");

    if !samples.is_empty() {
        let _ = slot.visualizer.push_samples(&samples);
    }

    Ok(slot
        .visualizer
        .latest_frame()
        .map(|frame| SpectrumFrameDto {
            bins: frame.bins,
            peak_left: frame.peak_left,
            peak_right: frame.peak_right,
        }))
}
