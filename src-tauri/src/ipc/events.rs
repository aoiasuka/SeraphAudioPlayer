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
    // REL-02：自动续播要打开下一首（play_track 同步等引擎回执，最长 30 s），不能在这条
    // 唯一的事件分发线程上做——期间进度、错误事件全部积压，前端看起来卡住。
    // 交给专用线程串行处理（单线程保持曲末事件的先后顺序）。
    let (advance_tx, advance_rx) = std::sync::mpsc::sync_channel::<String>(8);
    let advance_app = app.clone();
    let advance = std::thread::Builder::new()
        .name("seraph-auto-advance".into())
        .spawn(move || {
            while let Ok(track_id) = advance_rx.recv() {
                let state = advance_app.state::<AppState>();
                if let Err(err) = state.handle_playback_ended(&track_id) {
                    warn!("failed to advance after playback ended: {err}");
                }
            }
        });
    if let Err(err) = advance {
        warn!("failed to spawn auto-advance thread: {err}");
    }
    let dispatcher = std::thread::Builder::new()
        .name("seraph-event-bridge".into())
        .spawn(move || {
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
                        // 设备丢失后紧跟一条 Stopped；保留 DeviceLost 让快照能区分「停了」与「设备没了」
                        let mut player_state = state.player_state.write();
                        if *player_state != PlayerState::DeviceLost {
                            *player_state = PlayerState::Stopped;
                        }
                        true
                    }
                    PlayerEvent::DeviceLost { reason } => {
                        tracing::warn!(reason, "输出设备丢失");
                        *state.player_state.write() = PlayerState::DeviceLost;
                        true
                    }
                    PlayerEvent::PlaybackEnded { track_id } => {
                        if advance_tx.try_send(track_id.clone()).is_err() {
                            warn!("auto-advance queue is unavailable; playback will stop");
                            *state.player_state.write() = PlayerState::Stopped;
                            state.event_bus.publish(PlayerEvent::PlaybackStopped);
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
    if let Err(err) = dispatcher {
        warn!("failed to spawn player event bridge: {err}");
    }
}
