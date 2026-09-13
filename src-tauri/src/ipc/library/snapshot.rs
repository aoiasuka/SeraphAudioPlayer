use super::types::ImportedTrack;
use serde::{ser::SerializeSeq, Serialize, Serializer};
use std::collections::HashMap;
use std::sync::Arc;

/// 顺序列表与 ID 索引属于同一个不可变快照，读者无需持锁克隆整库。
pub(crate) struct LibrarySnapshot {
    pub(crate) tracks: Vec<ImportedTrack>,
    index_by_id: HashMap<String, usize>,
}

/// 在 IPC 序列化时借用 Arc 内的曲目，默认兼容完整格式；主窗口可请求无歌词摘要。
pub(crate) struct PlaylistSnapshot {
    pub(crate) snapshot: Arc<LibrarySnapshot>,
    pub(crate) include_lyrics: bool,
}

impl Serialize for PlaylistSnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.snapshot.tracks.len()))?;
        for track in &self.snapshot.tracks {
            if self.include_lyrics {
                sequence.serialize_element(track)?;
            } else {
                sequence.serialize_element(&super::storage::TrackMetadata::from(track))?;
            }
        }
        sequence.end()
    }
}

impl LibrarySnapshot {
    pub(crate) fn new(tracks: Vec<ImportedTrack>) -> Self {
        let mut index_by_id = HashMap::with_capacity(tracks.len());
        for (index, track) in tracks.iter().enumerate() {
            // 兼容旧缓存的重复 ID：与原先 iter().find() 一样取第一项。
            index_by_id.entry(track.id.clone()).or_insert(index);
        }
        Self {
            tracks,
            index_by_id,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&ImportedTrack> {
        self.index_by_id.get(id).map(|index| &self.tracks[*index])
    }
}
