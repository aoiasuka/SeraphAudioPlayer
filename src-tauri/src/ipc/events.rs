//! 事件桥：把 [`PlayerEvent`](seraph_core::PlayerEvent) 转 Tauri `emit`。
//!
//! 启动时调用 [`wire_event_bus`]，会在独立线程订阅 EventBus，
//! 任何 publish 都转发到前端 `seraph://event` 频道。

use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;

pub const FRONTEND_EVENT: &str = "seraph://event";

pub fn wire_event_bus(app: AppHandle) {
    let state = app.state::<AppState>();
    let rx = state.event_bus.subscribe();
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if let Err(err) = app.emit(FRONTEND_EVENT, &event) {
                warn!("failed to emit player event: {err}");
            }
        }
    });
}
