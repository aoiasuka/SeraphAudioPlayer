//! 可视化 IPC 仅更新需求并读取后台完成帧。FFT 和声学分析由唯一的 tap 读线程处理。

use super::error::IpcResult;
use crate::state::AppState;
use crate::visualizer_service::{AnalysisDemand, AnalysisFrameDto, FrameRequest, SpectrumFrameDto};
use tauri::State;

#[tauri::command]
pub fn get_spectrum_frame(
    state: State<'_, AppState>,
    request: Option<FrameRequest>,
) -> IpcResult<Option<SpectrumFrameDto>> {
    state.visualizer.spectrum(request.unwrap_or_default())
}

#[tauri::command]
pub fn get_analysis_frame(
    state: State<'_, AppState>,
    request: Option<FrameRequest>,
    demand: Option<AnalysisDemand>,
) -> IpcResult<Option<AnalysisFrameDto>> {
    state
        .visualizer
        .analysis(request.unwrap_or_default(), demand.unwrap_or_default())
}

#[tauri::command]
pub fn reset_analysis_meters(state: State<'_, AppState>) {
    state.visualizer.reset();
}
