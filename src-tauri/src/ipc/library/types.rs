use super::prelude::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    #[serde(rename = "legacyIds")]
    pub legacy_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_year: Option<String>,
    pub cover: String,
    pub format: String,
    pub bitdepth: String,
    pub sample_rate: String,
    pub bitrate: String,
    pub channels: String,
    pub size: String,
    pub path: String,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub cache_missing: bool,
    pub duration: u64,
    pub glow_color: String,
    pub glow1: String,
    pub glow2: String,
    pub lyrics: Vec<LyricLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTrackRequest {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineLyricsCandidate {
    pub id: String,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<u64>,
    pub lyrics: Vec<LyricLine>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderLyricLine {
    pub(crate) start_ms: u64,
    pub(crate) text: String,
}

#[derive(Debug, Default)]
pub(crate) struct ParsedAudioMetadata {
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) album_year: Option<String>,
    pub(crate) duration: Option<u64>,
    pub(crate) bitrate: Option<u32>,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) bit_depth: Option<u8>,
    pub(crate) channels: Option<u8>,
    pub(crate) lyrics: Vec<LyricLine>,
    pub(crate) cover: Option<CoverArt>,
}

/// 内嵌封面原始图片数据 + 由 MIME/魔数推断出的扩展名（落盘 covers 目录时用）
#[derive(Debug)]
pub(crate) struct CoverArt {
    pub(crate) data: Vec<u8>,
    pub(crate) ext: &'static str,
}

#[derive(Debug, Default)]
pub(crate) struct FilenameMetadata {
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
}
