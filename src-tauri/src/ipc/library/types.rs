use super::prelude::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    #[serde(rename = "legacyIds")]
    pub legacy_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTracksResult {
    pub deleted_ids: Vec<String>,
    /// 实际移除的音频文件数，不包含完成标记或原本已丢失的文件。
    pub deleted_files: usize,
    pub failures: Vec<DeleteTrackFailure>,
}

#[derive(Debug, Serialize)]
pub struct DeleteTrackFailure {
    pub id: String,
    pub title: String,
    pub message: String,
}

/// 逐字歌词的一个音节/单词（TTML `<span begin end>`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricWord {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// 歌词行。`time`/`text` 是所有来源的最小公倍数；其余字段只有逐字来源
/// （AMLL TTML）会填，序列化时 None 一律省略，旧曲库缓存（只有 time/text）
/// 反序列化时走 `serde(default)`，格式向前向后都兼容。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
    /// 行结束时间（秒）；LRC 类来源没有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    /// 逐字时间轴；为空/None 时前端按整行处理。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<LyricWord>>,
    /// 译文（TTML `ttm:role="x-translation"`）。LRC 类来源的译文仍以
    /// 相邻同时间戳行表示，前端两种形态都渲染。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    /// 音译/罗马音（TTML `ttm:role="x-roman"`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roman: Option<String>,
}

impl LyricLine {
    pub fn new(time: f64, text: impl Into<String>) -> Self {
        Self {
            time,
            text: text.into(),
            ..Self::default()
        }
    }
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
    /// 该候选在 AMLL TTML DB 里的查找键（如 `ncm-lyrics/123`），只在后端
    /// 三源搜索 → TTML 二次查找之间传递，不出 IPC。
    #[serde(skip)]
    pub ttml_lookup_keys: Vec<String>,
}

/// 前端「歌词设置」里影响在线获取的选项（`fetch_online_lyrics` 的 options 参数）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OnlineLyricsOptions {
    /// "auto" | "netease" | "kugou" | "qq"
    pub source_priority: String,
    pub prefer_traditional: bool,
    pub ttml_enabled: bool,
    /// 地址模板（`{dir}`/`{id}`/`%s` 占位；无占位符视为 base URL）
    pub ttml_db_url: String,
    /// true = 自定义地址模式（任意公网 https，禁重定向）；false = 预设镜像白名单
    pub ttml_db_custom: bool,
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
