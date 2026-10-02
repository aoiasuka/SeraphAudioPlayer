use serde::{Deserialize, Serialize};

/// 播放状态（Rust 侧维护，React 只做投影）。
///
/// 只保留实际会进入的状态：此前定义的 Loading / Buffering / Seeking / Transitioning
/// 从未被构造（ARCH-02 清理）。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerState {
    #[default]
    Stopped,
    Playing,
    Paused,
    /// 输出设备丢失（拔出 / 被其它应用独占）后停在这里，直到用户重新起播或换设备
    DeviceLost,
}
