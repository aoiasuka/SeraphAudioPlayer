use crate::track::{Playlist, PlaylistId};
use seraph_core::types::{Track, TrackId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("library not implemented yet")]
    NotImplemented,
    #[error("playlist not found: {0}")]
    PlaylistNotFound(PlaylistId),
    #[error("track not found: {0}")]
    TrackNotFound(TrackId),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

/// 媒体库 trait。
pub trait Library: Send + Sync {
    fn list_playlists(&self) -> Result<Vec<Playlist>, LibraryError>;
    fn get_playlist(&self, id: &PlaylistId) -> Result<Playlist, LibraryError>;
    fn get_track(&self, id: &TrackId) -> Result<Track, LibraryError>;
    fn add_track(&mut self, track: Track) -> Result<(), LibraryError>;
}
