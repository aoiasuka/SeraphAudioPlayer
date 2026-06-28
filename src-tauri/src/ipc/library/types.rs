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
struct ProviderLyricLine {
    start_ms: u64,
    text: String,
}

#[derive(Debug, Default)]
struct ParsedAudioMetadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_year: Option<String>,
    duration: Option<u64>,
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    bit_depth: Option<u8>,
    channels: Option<u8>,
    lyrics: Vec<LyricLine>,
}

#[derive(Debug, Default)]
struct FilenameMetadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}
