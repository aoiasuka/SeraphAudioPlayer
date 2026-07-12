//! Library IPC handlers.
//!
//! 原先用 `include!` 把子文件拼成单一巨型模块，rust-analyzer 无法正常
//! 索引且可见性没有边界；现改为真模块树，跨模块共享项收敛到 [`prelude`]。

mod commands;
mod lyrics;
mod media_library;
mod metadata;
mod online_covers;
mod online_lyrics;
mod prelude;
#[cfg(test)]
mod tests;
mod types;

// 兄弟 ipc 模块（cache/bilibili）沿用 `super::library::xxx` 路径
pub use commands::*;
pub(crate) use media_library::{mark_tracks_cache_missing_by_paths, merge_tracks_into_cache};
pub use online_covers::*;
pub use types::ImportedTrack;
