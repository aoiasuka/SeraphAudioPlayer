use super::prelude::*;

#[derive(Debug, Deserialize)]
pub(crate) struct ApiResponse<T> {
    pub(crate) code: i32,
    #[serde(alias = "msg")]
    pub(crate) message: Option<String>,
    pub(crate) data: Option<T>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct VideoData {
    pub(crate) bvid: String,
    pub(crate) title: String,
    pub(crate) cid: i64,
    pub(crate) duration: u64,
    pub(crate) pic: Option<String>,
    pub(crate) owner: OwnerData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OwnerData {
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlayUrlData {
    pub(crate) dash: Option<DashData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DashData {
    pub(crate) audio: Option<Vec<Value>>,
    pub(crate) flac: Option<FlacData>,
    pub(crate) dolby: Option<DolbyData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FlacData {
    pub(crate) audio: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DolbyData {
    pub(crate) audio: Option<Vec<Value>>,
    #[serde(rename = "type")]
    pub(crate) kind: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) enum AudioKind {
    DolbyAtmos,
    Flac,
    Dolby,
    Normal,
}

#[derive(Debug, Clone)]
pub(crate) struct AudioStream {
    pub(crate) base_url: String,
    pub(crate) backup_urls: Vec<String>,
    pub(crate) bandwidth: Option<u32>,
    pub(crate) codecs: Option<String>,
    pub(crate) kind: AudioKind,
}

pub(crate) struct ResolvedAudio {
    pub(crate) video: VideoData,
    pub(crate) stream: AudioStream,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliImportOptions {
    pub(crate) prefer_flac: Option<bool>,
    pub(crate) prefer_dolby_atmos: Option<bool>,
    pub(crate) remux_with_ffmpeg: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginQrCode {
    pub(crate) url: String,
    pub(crate) qrcode_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginPollResult {
    pub(crate) code: i32,
    pub(crate) message: String,
    pub(crate) url: Option<String>,
    pub(crate) logged_in: bool,
    pub(crate) profile: Option<BilibiliLoginStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginStatus {
    pub(crate) logged_in: bool,
    pub(crate) username: Option<String>,
    pub(crate) mid: Option<u64>,
    pub(crate) face: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliFfmpegStatus {
    pub(crate) available: bool,
    pub(crate) path: Option<String>,
}

/// ffmpeg 下载/安装进度，通过 [`FFMPEG_DOWNLOAD_EVENT`] 推送给前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FfmpegDownloadProgress {
    /// "download" | "extract" | "done" | "error"
    pub(crate) stage: &'static str,
    pub(crate) downloaded: u64,
    pub(crate) total: u64,
    /// 0.0 - 100.0；total 未知时为 -1。
    pub(crate) percent: f64,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliBatchImportResult {
    pub(crate) tracks: Vec<ImportedTrack>,
    pub(crate) failed: Vec<BilibiliImportFailure>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliImportFailure {
    pub(crate) input: String,
    pub(crate) reason: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QrGenerateData {
    pub(crate) url: String,
    #[serde(rename = "qrcode_key")]
    pub(crate) qrcode_key: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QrPollData {
    pub(crate) code: i32,
    pub(crate) message: Option<String>,
    pub(crate) url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NavData {
    #[serde(rename = "isLogin")]
    pub(crate) is_login: bool,
    pub(crate) uname: Option<String>,
    pub(crate) mid: Option<u64>,
    pub(crate) face: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BilibiliSession {
    #[serde(default)]
    pub(crate) cookies: BTreeMap<String, String>,
    #[serde(default)]
    /// Set-Cookie 解析出的过期时间（Unix 秒）。
    /// 没有 expires/max-age 信息的 cookie 不会出现在这个映射里——按 session cookie 处理（永远不过期）。
    pub(crate) cookie_expires: BTreeMap<String, u64>,
    #[serde(default)]
    pub(crate) has_secure_cookies: bool,
    pub(crate) saved_at: u64,
    pub(crate) username: Option<String>,
    pub(crate) mid: Option<u64>,
    pub(crate) face: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct BilibiliSessionFile<'a> {
    pub(crate) saved_at: u64,
    pub(crate) username: &'a Option<String>,
    pub(crate) mid: Option<u64>,
    pub(crate) face: &'a Option<String>,
    pub(crate) has_secure_cookies: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cookies: Option<&'a BTreeMap<String, String>>,
    // L-11:持久化 cookie 过期时间，重启后不再把已过期 cookie 当永不过期的 session cookie。
    pub(crate) cookie_expires: &'a BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FavListData {
    pub(crate) medias: Option<Vec<FavMedia>>,
    #[serde(default)]
    pub(crate) has_more: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FavMedia {
    pub(crate) bvid: Option<String>,
    pub(crate) title: Option<String>,
}
