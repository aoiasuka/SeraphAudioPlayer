use crate::types::TrackId;
use serde::{Deserialize, Serialize};

/// 全局播放器事件。
///
/// 由音频引擎线程产生，通过 [`crate::EventBus`] 广播给所有订阅者，
/// 最终由 Tauri 层桥接成 `app.emit_all(...)` 推给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerEvent {
    PlaybackStarted { track_id: TrackId },
    PlaybackPaused,
    PlaybackResumed,
    PlaybackStopped,
    PlaybackEnded { track_id: TrackId },
    TrackChanged { track_id: TrackId },
    Progress {
        track_id: TrackId,
        seconds: f64,
        total: f64,
    },
    BufferingStart,
    BufferingEnd,
    DeviceLost { reason: String },
    DeviceRecovered { device_name: String },
    VolumeChanged { volume: f32 },
    Error { message: String },
    Spectrum { bins: Vec<f32> },
}
