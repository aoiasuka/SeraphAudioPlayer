//! 事件桥：把 [`PlayerEvent`](seraph_core::PlayerEvent) 转 Tauri `emit`。
//!
//! 启动时调用 [`wire_event_bus`]，会在独立线程订阅 EventBus，
//! 任何 publish 都转发到前端 `seraph://event` 频道。

use crate::state::AppState;
use seraph_core::{PlayerEvent, PlayerState};
use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;

pub const FRONTEND_EVENT: &str = "seraph://event";

pub fn wire_event_bus(app: AppHandle) {
    let state = app.state::<AppState>();
    let rx = state.event_bus.subscribe();
    std::thread::spawn(move || {
        let mut last_diagnostics = std::time::Instant::now();
        let mut previous_diagnostics = Default::default();
        while let Ok(event) = rx.recv() {
            let state = app.state::<AppState>();
            if last_diagnostics.elapsed() >= std::time::Duration::from_secs(5) {
                let diagnostics = state.audio.spectrum_tap().diagnostics();
                if diagnostics != previous_diagnostics {
                    tracing::info!(?diagnostics, "音频累计计数（缺样含起播、跳转和曲尾）");
                    previous_diagnostics = diagnostics;
                }
                last_diagnostics = std::time::Instant::now();
            }
            let should_emit = match &event {
                PlayerEvent::PlaybackStarted { track_id } => {
                    state.set_current_track(track_id);
                    *state.player_state.write() = PlayerState::Playing;
                    true
                }
                PlayerEvent::PlaybackResumed => {
                    *state.player_state.write() = PlayerState::Playing;
                    true
                }
                PlayerEvent::PlaybackPaused => {
                    *state.player_state.write() = PlayerState::Paused;
                    true
                }
                PlayerEvent::PlaybackStopped => {
                    *state.player_state.write() = PlayerState::Stopped;
                    true
                }
                PlayerEvent::PlaybackEnded { track_id } => {
                    if let Err(err) = state.handle_playback_ended(track_id) {
                        warn!("failed to advance after playback ended: {err}");
                    }
                    false
                }
                PlayerEvent::TrackChanged { track_id } => {
                    state.set_current_track(track_id);
                    true
                }
                PlayerEvent::Error { message } | PlayerEvent::SeekFailed { message, .. } => {
                    tracing::warn!(message, "播放操作失败");
                    true
                }
                _ => true,
            };

            if !should_emit {
                continue;
            }
            if let Err(err) = app.emit(FRONTEND_EVENT, &event) {
                warn!("failed to emit player event: {err}");
            }
        }
    });
}
