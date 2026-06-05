use serde::{Deserialize, Serialize};

/// 完整播放状态机。
///
/// 状态机仅在 Rust 侧维护，React 仅作为 UI 投影层；
/// 这避免双向状态同步带来的 race condition。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerState {
    #[default]
    Stopped,
    Loading,
    Buffering,
    Playing,
    Paused,
    Seeking,
    Transitioning,
    DeviceLost,
}
