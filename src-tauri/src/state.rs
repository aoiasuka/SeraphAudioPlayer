use parking_lot::RwLock;
use seraph_audio::PlaybackController;
use seraph_core::{EventBus, PlayerState};
use std::sync::Arc;

/// Tauri 全局应用状态。
///
/// 真正实现音频功能后，这里会持有 `AudioEngine` / `Library` 等的句柄；
/// 当前只暴露事件总线和状态机的占位。
pub struct AppState {
    pub event_bus: EventBus,
    pub player_state: Arc<RwLock<PlayerState>>,
    pub audio: PlaybackController,
}

impl AppState {
    pub fn new() -> Self {
        let event_bus = EventBus::new();
        Self {
            audio: PlaybackController::new(event_bus.clone()),
            event_bus,
            player_state: Arc::new(RwLock::new(PlayerState::Stopped)),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
