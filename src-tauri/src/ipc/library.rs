//! Library IPC handlers.
//!
//! 原先用 `include!` 把子文件拼成单一巨型模块，rust-analyzer 无法正常
//! 索引且可见性没有边界；现改为真模块树，跨模块共享项收敛到 [`prelude`]。

mod commands;
mod dsd_tags;
mod lyrics;
mod media_library;
mod metadata;
mod online_covers;
mod online_lyrics;
mod prelude;
#[cfg(test)]
mod tests;
mod types;
mod wav_id3;

// 兄弟 ipc 模块（cache/bilibili）沿用 `super::library::xxx` 路径
pub use commands::*;
// playlist_io 复用歌词模块的编码探测链（UTF-16 → UTF-8 → GBK）解码 .m3u
pub(crate) use lyrics::decode_lyric_bytes;
pub(crate) use media_library::{mark_tracks_cache_missing_by_paths, merge_tracks_into_cache};
pub use online_covers::*;
pub use types::ImportedTrack;
