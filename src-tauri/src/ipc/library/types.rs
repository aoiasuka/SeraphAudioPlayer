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
    /// 歌词文档（含来源、AMLL 查找键）。列表摘要（`includeLyrics=false`）不带此字段，
    /// 反序列化缺省为空文档。
    #[serde(default)]
    pub lyrics: LyricDocument,
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

/// 逐字歌词的一个音节 / 单词。时间一律**毫秒整数**（2026-09-20 模型重构起）。
/// `end_ms` 为 None = 终点未知（增强型 LRC 末音节没有结束标签等），由 `infer_line_ends`
/// 用下一句起点补齐并在行上打 `end_inferred`；前端缺省用行终点兜底。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricWord {
    pub start_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    pub text: String,
    /// 逐字音译（QQ `roma` / KRC `type 0` 有；目前只随行级 `roman` 一起存，尚未渲染）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roman: Option<String>,
}

impl LyricWord {
    pub fn new(start_ms: u64, end_ms: Option<u64>, text: impl Into<String>) -> Self {
        Self {
            start_ms,
            end_ms,
            text: text.into(),
            roman: None,
        }
    }
}

/// 译文 / 音译这类**副轨文本**：可带自己的逐字时间轴与语言标记。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricText {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<LyricWord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

impl LyricText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            words: None,
            lang: None,
        }
    }
}

/// 行的角色：主唱 / 和声（TTML `x-bg`）/ 制作信息（作词作曲等，`from_lines` 按模式识别）。
/// 目前只存不渲染（方案 B2 再消费）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricRole {
    #[default]
    Main,
    Background,
    Credit,
}

impl LyricRole {
    fn is_main(&self) -> bool {
        *self == Self::Main
    }
}

/// 歌词行。`start_ms` / `text` 是所有来源的最小公倍数；其余字段按来源可选，序列化时
/// 缺省值一律省略。译文统一进 `translations`（TTML 的 `x-translation`、KRC `[language:]`、
/// 网易云 tlyric / ytlrc、QQ trans），来源不明的双语 LRC（相邻同时间戳行）仍是两条独立
/// 的 Main 行，前端按同起点分组兜底渲染。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub start_ms: u64,
    /// 行结束时间；None = 来源没给（普通 LRC）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    /// `end_ms` 是解析后处理用下一句起点推导出来的（导出时不写这个终点标签）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub end_inferred: bool,
    pub text: String,
    /// 逐字时间轴；为空 / None 时前端按整行处理。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<LyricWord>>,
    /// 译文，可多轨（网易云 ytlrc 与 tlyric 并存时只取一份，预留多轨）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translations: Vec<LyricText>,
    /// 音译 / 罗马音。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roman: Option<LyricText>,
    #[serde(default, skip_serializing_if = "LyricRole::is_main")]
    pub role: LyricRole,
    /// TTML `ttm:agent`（对唱 v1 / v2），只存不渲染。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// 被「歌词排除规则」命中（Rust 侧 regex/关键词匹配）。**只在返回给前端前打标**，
    /// 不落曲库缓存；前端按此标记隐藏而不自己匹配。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// 秒（f64）→ 毫秒整数，负数与 NaN 归零。解析器内部拿到秒时统一经此换算。
pub fn seconds_to_ms(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 1000.0).round() as u64
}

impl LyricLine {
    pub fn new(start_ms: u64, text: impl Into<String>) -> Self {
        Self {
            start_ms,
            text: text.into(),
            ..Self::default()
        }
    }

    /// 第一份译文文本（测试与调试的常用取法）。
    #[cfg(test)]
    pub fn translation_text(&self) -> Option<&str> {
        self.translations.first().map(|text| text.text.as_str())
    }

    pub fn roman_text(&self) -> Option<&str> {
        self.roman.as_ref().map(|text| text.text.as_str())
    }
}

/// 歌词来源的种类。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricSourceKind {
    #[default]
    Unknown,
    /// 音频标签内嵌
    Embedded,
    /// 同名 sidecar 文件
    Sidecar,
    /// 本地歌词目录匹配
    Folder,
    /// 用户手动导入文件
    Manual,
    /// 三源在线匹配（provider = netease / kugou / qq）
    Online,
    /// AMLL TTML（DB 直取或索引 API）
    Ttml,
    /// 旧格式曲库迁移而来（来源不详）
    Legacy,
}

/// 来源与身份：哪个平台、哪个 ID、有哪些 AMLL 查找键、是否用户手动固定。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricSource {
    pub kind: LyricSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_track_id: Option<String>,
    /// AMLL TTML DB 查找键（如 `ncm-lyrics/65923804`）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lookup_keys: Vec<String>,
    /// 用户手动导入 / 明确应用 = 固定选择，自动流程不得替换（B2 消费）。
    pub pinned: bool,
    /// 在线获取的 Unix 秒。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<u64>,
}

impl LyricSource {
    pub fn of(kind: LyricSourceKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    pub fn online(provider: &str, provider_track_id: impl Into<String>) -> Self {
        Self {
            kind: LyricSourceKind::Online,
            provider: Some(provider.to_string()),
            provider_track_id: Some(provider_track_id.into()),
            fetched_at: unix_now(),
            ..Self::default()
        }
    }

    pub fn ttml(provider_track_id: impl Into<String>) -> Self {
        Self {
            kind: LyricSourceKind::Ttml,
            provider: Some("amll".into()),
            provider_track_id: Some(provider_track_id.into()),
            fetched_at: unix_now(),
            ..Self::default()
        }
    }

    pub fn with_lookup_keys(mut self, keys: Vec<String>) -> Self {
        for key in keys {
            if !self.lookup_keys.contains(&key) {
                self.lookup_keys.push(key);
            }
        }
        self
    }
}

fn unix_now() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// 同步粒度：纯文本合成的假时间轴 / 逐行 / 逐字。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricSync {
    #[default]
    None,
    Line,
    Word,
}

/// 当前歌词文档格式版本。
pub const LYRIC_DOCUMENT_SCHEMA: u32 = 2;

/// 一首歌的歌词文档：行 + 来源 + 同步粒度。曲库里每首曲目存一份；IPC 也整份传。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricDocument {
    pub schema: u32,
    pub source: LyricSource,
    pub sync: LyricSync,
    /// 解析时已折进各行时间的 `[offset:]`（毫秒，记录以便日后「忽略文件 offset」重算）。
    pub offset_ms: i32,
    pub lines: Vec<LyricLine>,
}

impl Default for LyricDocument {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// 空文档（曲目没有歌词）。
pub static EMPTY_LYRICS: LyricDocument = LyricDocument::EMPTY;

impl LyricDocument {
    pub const EMPTY: LyricDocument = LyricDocument {
        schema: LYRIC_DOCUMENT_SCHEMA,
        source: LyricSource {
            kind: LyricSourceKind::Unknown,
            provider: None,
            provider_track_id: None,
            lookup_keys: Vec::new(),
            pinned: false,
            fetched_at: None,
        },
        sync: LyricSync::None,
        offset_ms: 0,
        lines: Vec::new(),
    };

    /// 唯一的组装入口：解析器产出的行（已排序去重）+ 来源 → 文档。这里统一判定同步粒度
    /// 与制作信息行角色；空行列表得到空文档但保留来源。
    pub fn from_lines(mut lines: Vec<LyricLine>, source: LyricSource) -> Self {
        let sync = if lines.iter().any(|line| line.words.is_some()) {
            LyricSync::Word
        } else if lines.is_empty() || lyric_lines_are_unsynced(&lines) {
            LyricSync::None
        } else {
            LyricSync::Line
        };
        for line in &mut lines {
            if line.role == LyricRole::Main && is_credit_line(&line.text) {
                line.role = LyricRole::Credit;
            }
        }
        Self {
            schema: LYRIC_DOCUMENT_SCHEMA,
            source,
            sync,
            offset_ms: 0,
            lines,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

/// 「纯文本合成」的假时间轴：`parse_lyrics_text` 对无时间戳歌词按 `index * 4s` 铺时间，
/// 全部行恰为 4 秒等差、且没有 words/end 即视为未同步。单行 `[00:00.00]` 真 LRC 与单行
/// 纯文本无法区分，一律按未同步算。
pub fn lyric_lines_are_unsynced(lines: &[LyricLine]) -> bool {
    !lines.is_empty()
        && lines.iter().enumerate().all(|(index, line)| {
            line.start_ms == index as u64 * 4000 && line.words.is_none() && line.end_ms.is_none()
        })
}

/// 制作信息行（与前端「制作信息预设」排除规则同一模式）。
pub fn is_credit_line(text: &str) -> bool {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::RegexBuilder::new(
            r"^(作词|作曲|编曲|制作人|混音|母带|监制|出品|录音|和声|吉他|贝斯|鼓|键盘|弦乐|发行|OP|SP|词|曲)\s*[:：]",
        )
        .case_insensitive(true)
        .build()
        .expect("valid credits regex")
    });
    pattern.is_match(text.trim())
}

// ---------------------------------------------------------------------------
// 旧格式（2026-09-20 之前，秒制、译文两种形态）兼容读取
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LegacyLyricWord {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Deserialize)]
struct LegacyLyricLine {
    time: f64,
    text: String,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    words: Option<Vec<LegacyLyricWord>>,
    #[serde(default)]
    translation: Option<String>,
    #[serde(default)]
    roman: Option<String>,
}

impl From<LegacyLyricLine> for LyricLine {
    fn from(legacy: LegacyLyricLine) -> Self {
        let words = legacy.words.map(|words| {
            words
                .into_iter()
                .map(|word| {
                    let start_ms = seconds_to_ms(word.start);
                    let end_ms = seconds_to_ms(word.end);
                    // 旧格式用零时长占位「终点未知」
                    LyricWord::new(start_ms, (end_ms > start_ms).then_some(end_ms), word.text)
                })
                .collect::<Vec<_>>()
        });
        LyricLine {
            start_ms: seconds_to_ms(legacy.time),
            end_ms: legacy.end.map(seconds_to_ms),
            end_inferred: false,
            text: legacy.text,
            words,
            translations: legacy
                .translation
                .filter(|text| !text.trim().is_empty())
                .map(|text| vec![LyricText::new(text)])
                .unwrap_or_default(),
            roman: legacy
                .roman
                .filter(|text| !text.trim().is_empty())
                .map(LyricText::new),
            role: LyricRole::Main,
            agent: None,
            hidden: false,
        }
    }
}

/// 文档的新格式表示（与 `LyricDocument` 同形，字段缺省容错），供反序列化用。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LyricDocumentRepr {
    schema: u32,
    source: LyricSource,
    sync: LyricSync,
    offset_ms: i32,
    lines: Vec<LyricLine>,
}

impl Default for LyricDocumentRepr {
    fn default() -> Self {
        Self {
            schema: LYRIC_DOCUMENT_SCHEMA,
            source: LyricSource::default(),
            sync: LyricSync::None,
            offset_ms: 0,
            lines: Vec::new(),
        }
    }
}

/// 反序列化同时接受：新格式对象；旧格式（v0.6.1 及以前）的行数组——迁移时来源标 `Legacy`、
/// 秒制换成毫秒、零时长音节终点改为未知、`translation` / `roman` 字符串升成结构。
impl<'de> Deserialize<'de> for LyricDocument {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Legacy(Vec<LegacyLyricLine>),
            Modern(LyricDocumentRepr),
        }
        Ok(match Either::deserialize(deserializer)? {
            Either::Legacy(lines) => LyricDocument::from_legacy_lines(lines),
            Either::Modern(repr) => LyricDocument {
                schema: LYRIC_DOCUMENT_SCHEMA.max(repr.schema),
                source: repr.source,
                sync: repr.sync,
                offset_ms: repr.offset_ms,
                lines: repr.lines,
            },
        })
    }
}

impl LyricDocument {
    fn from_legacy_lines(lines: Vec<LegacyLyricLine>) -> Self {
        if lines.is_empty() {
            return Self::EMPTY;
        }
        let lines = lines.into_iter().map(LyricLine::from).collect();
        Self::from_lines(lines, LyricSource::of(LyricSourceKind::Legacy))
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
    pub lyrics: LyricDocument,
    /// 该候选在 AMLL TTML DB 里的查找键（如 `ncm-lyrics/123`）。后端三源搜索 →
    /// TTML 二次查找之间传递；AMLL 命中的候选把键带出 IPC（`lookupKeys`），
    /// 前端应用时回传 `apply_online_lyrics` 回写进曲目 `lyrics_lookup_keys`。
    #[serde(rename = "lookupKeys", default, skip_serializing_if = "Vec::is_empty")]
    pub ttml_lookup_keys: Vec<String>,
}

/// `test_amll_ttml_db`（歌词设置页「测试连接」）的结果。`kind` 是机器可读分类：
/// invalid_url / unreachable / not_found / http_error / html / invalid_xml /
/// unsupported_ttml / ok；`message` 是给用户看的中文说明。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmllTestResult {
    pub ok: bool,
    pub kind: String,
    pub message: String,
    pub url: String,
    pub lines: usize,
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
    /// 曲目已知的 AMLL 查找键（`ImportedTrack.lyrics_lookup_keys`），有则直取 TTML
    pub lookup_keys: Vec<String>,
}

/// 歌词排除规则（与前端 `LyricsExcludeRule` 同形）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsExcludeRule {
    pub id: String,
    /// "keyword" | "regex"
    pub kind: String,
    pub pattern: String,
}

/// 单条规则的校验结果（`set_lyrics_exclude_rules` / `validate_lyrics_exclude_rule` 返回）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsExcludeRuleStatus {
    pub id: String,
    pub error: Option<String>,
}

/// `find_local_lyrics` 的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLyricsMatch {
    pub path: String,
    pub lyrics: LyricDocument,
    pub lookup_keys: Vec<String>,
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

#[cfg(test)]
mod lyric_document_tests {
    use super::*;

    #[test]
    fn legacy_line_array_migrates_into_document() {
        // v0.6.1 及以前落盘的形态：秒制、零时长占位、translation / roman 字符串
        let legacy = r#"[
            {"time":18.459,"text":"我的天","end":23.097,"words":[{"start":18.459,"end":18.814,"text":"我"},{"start":18.814,"end":18.814,"text":"的天"}],"translation":"my sky","roman":"wo de tian"},
            {"time":26.712,"text":"作词：某人"},
            {"time":30.0,"text":"普通行","hidden":true}
        ]"#;
        let doc: LyricDocument = serde_json::from_str(legacy).expect("legacy");
        assert_eq!(doc.schema, LYRIC_DOCUMENT_SCHEMA);
        assert_eq!(doc.source.kind, LyricSourceKind::Legacy);
        assert_eq!(doc.sync, LyricSync::Word);
        assert_eq!(doc.lines.len(), 3);
        let first = &doc.lines[0];
        assert_eq!((first.start_ms, first.end_ms), (18_459, Some(23_097)));
        let words = first.words.as_ref().unwrap();
        assert_eq!((words[0].start_ms, words[0].end_ms), (18_459, Some(18_814)));
        // 零时长占位 → 终点未知
        assert_eq!((words[1].start_ms, words[1].end_ms), (18_814, None));
        assert_eq!(first.translation_text(), Some("my sky"));
        assert_eq!(first.roman_text(), Some("wo de tian"));
        assert_eq!(doc.lines[1].role, LyricRole::Credit);
        assert_eq!(doc.lines[2].role, LyricRole::Main);
        // hidden 不落盘、不迁移
        assert!(!doc.lines[2].hidden);

        let empty: LyricDocument = serde_json::from_str("[]").expect("empty legacy");
        assert_eq!(empty, LyricDocument::EMPTY);
    }

    #[test]
    fn modern_document_round_trips_and_tolerates_missing_fields() {
        let mut doc = LyricDocument::from_lines(
            vec![LyricLine::new(1000, "a"), LyricLine::new(5000, "b")],
            LyricSource::online("netease", "123").with_lookup_keys(vec!["ncm-lyrics/123".into()]),
        );
        doc.lines[0].end_ms = Some(5000);
        doc.lines[0].end_inferred = true;
        doc.lines[0].translations.push(LyricText::new("甲"));
        assert_eq!(doc.sync, LyricSync::Line);
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(json["schema"], 2);
        assert_eq!(json["source"]["kind"], "online");
        assert_eq!(json["source"]["lookupKeys"][0], "ncm-lyrics/123");
        assert_eq!(json["lines"][0]["startMs"], 1000);
        assert_eq!(json["lines"][0]["endInferred"], true);
        assert!(json["lines"][1].get("endInferred").is_none(), "缺省值省略");
        assert!(json["lines"][1].get("role").is_none());
        let back: LyricDocument = serde_json::from_value(json).unwrap();
        assert_eq!(back, doc);

        // 只有 lines 的精简对象也能读
        let minimal: LyricDocument =
            serde_json::from_str(r#"{"lines":[{"startMs":10,"text":"x"}]}"#).unwrap();
        assert_eq!(minimal.lines[0].start_ms, 10);
        assert_eq!(minimal.source.kind, LyricSourceKind::Unknown);
    }

    #[test]
    fn sync_detection_and_credit_role() {
        let plain = LyricDocument::from_lines(
            vec![LyricLine::new(0, "a"), LyricLine::new(4000, "b")],
            LyricSource::default(),
        );
        assert_eq!(plain.sync, LyricSync::None);
        let mut word = LyricLine::new(0, "词：某人");
        word.words = Some(vec![LyricWord::new(0, Some(500), "词：某人")]);
        let doc = LyricDocument::from_lines(vec![word], LyricSource::default());
        assert_eq!(doc.sync, LyricSync::Word);
        assert_eq!(doc.lines[0].role, LyricRole::Credit);
        assert!(is_credit_line("作曲 : 周杰伦"));
        assert!(!is_credit_line("我的天空"));
        assert_eq!(seconds_to_ms(1.0005), 1001);
        assert_eq!(seconds_to_ms(-1.0), 0);
        assert_eq!(seconds_to_ms(f64::NAN), 0);
    }
}
