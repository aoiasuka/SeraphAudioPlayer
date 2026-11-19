//! 可视化 IPC 仅更新需求并读取后台完成帧。FFT 和声学分析由唯一的 tap 读线程处理。
//!
//! VIZ-01：
//! - 三个命令都是 async。同步命令在主线程执行，分析页开着时每秒 30 次的取帧
//!   （取锁 + 拷贝一帧）会排进窗口消息循环；现在跑在异步运行时的工作线程上。
//!   命令体只持一把短锁、做一次内存拷贝，不含阻塞 IO，不需要再挪进阻塞线程池。
//! - 帧以二进制返回（`tauri::ipc::Response`，布局见 `visualizer_wire`），前端用
//!   类型化数组直接读；还没有帧时返回空字节串。失败仍是结构化的 `IpcError`。

use super::error::IpcResult;
use crate::state::AppState;
use crate::visualizer_service::{AnalysisDemand, FrameRequest};
use tauri::{ipc::Response, State};

#[tauri::command]
pub async fn get_spectrum_frame(
    state: State<'_, AppState>,
    request: Option<FrameRequest>,
) -> IpcResult<Response> {
    state
        .visualizer
        .spectrum_bytes(request.unwrap_or_default())
        .map(Response::new)
}

#[tauri::command]
pub async fn get_analysis_frame(
    state: State<'_, AppState>,
    request: Option<FrameRequest>,
    demand: Option<AnalysisDemand>,
) -> IpcResult<Response> {
    state
        .visualizer
        .analysis_bytes(request.unwrap_or_default(), demand.unwrap_or_default())
        .map(Response::new)
}

#[tauri::command]
pub async fn reset_analysis_meters(state: State<'_, AppState>) -> IpcResult<()> {
    state.visualizer.reset();
    Ok(())
}
