//! 歌单 / 媒体库持久化层。
//!
//! 当前只定义类型与 trait，实际存储后端（JSON/SQLite/sled）由后续实现。

pub mod library;
pub mod track;

pub use library::{Library, LibraryError};
pub use track::{Playlist, PlaylistId, PlaylistKind};
