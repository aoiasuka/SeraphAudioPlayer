use seraph_core::types::Track;
use serde::{Deserialize, Serialize};

pub type PlaylistId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaylistKind {
    Liked,
    Recent,
    User,
    Album,
    Artist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub kind: PlaylistKind,
    pub tracks: Vec<Track>,
}
