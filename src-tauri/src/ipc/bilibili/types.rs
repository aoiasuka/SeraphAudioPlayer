#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    code: i32,
    #[serde(alias = "msg")]
    message: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct VideoData {
    bvid: String,
    title: String,
    cid: i64,
    duration: u64,
    pic: Option<String>,
    owner: OwnerData,
}

#[derive(Debug, Deserialize)]
struct OwnerData {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PlayUrlData {
    dash: Option<DashData>,
}

#[derive(Debug, Deserialize)]
struct DashData {
    audio: Option<Vec<Value>>,
    flac: Option<FlacData>,
    dolby: Option<DolbyData>,
}

#[derive(Debug, Deserialize)]
struct FlacData {
    audio: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct DolbyData {
    audio: Option<Vec<Value>>,
    #[serde(rename = "type")]
    kind: Option<u32>,
}

#[derive(Debug, Clone)]
enum AudioKind {
    DolbyAtmos,
    Flac,
    Dolby,
    Normal,
}

#[derive(Debug, Clone)]
struct AudioStream {
    base_url: String,
    backup_urls: Vec<String>,
    bandwidth: Option<u32>,
    codecs: Option<String>,
    kind: AudioKind,
}

struct ResolvedAudio {
    video: VideoData,
    stream: AudioStream,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliImportOptions {
    prefer_flac: Option<bool>,
    prefer_dolby_atmos: Option<bool>,
    remux_with_ffmpeg: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginQrCode {
    url: String,
    qrcode_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginPollResult {
    code: i32,
    message: String,
    url: Option<String>,
    logged_in: bool,
    profile: Option<BilibiliLoginStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginStatus {
    logged_in: bool,
    username: Option<String>,
    mid: Option<u64>,
    face: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliFfmpegStatus {
    available: bool,
    path: Option<String>,
}

/// ffmpeg 下载/安装进度，通过 [`FFMPEG_DOWNLOAD_EVENT`] 推送给前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FfmpegDownloadProgress {
    /// "download" | "extract" | "done" | "error"
    stage: &'static str,
    downloaded: u64,
    total: u64,
    /// 0.0 - 100.0；total 未知时为 -1。
    percent: f64,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliBatchImportResult {
    tracks: Vec<ImportedTrack>,
    failed: Vec<BilibiliImportFailure>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliImportFailure {
    input: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct QrGenerateData {
    url: String,
    #[serde(rename = "qrcode_key")]
    qrcode_key: String,
}

#[derive(Debug, Deserialize)]
struct QrPollData {
    code: i32,
    message: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NavData {
    #[serde(rename = "isLogin")]
    is_login: bool,
    uname: Option<String>,
    mid: Option<u64>,
    face: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BilibiliSession {
    #[serde(default)]
    cookies: BTreeMap<String, String>,
    #[serde(default)]
    /// Set-Cookie 解析出的过期时间（Unix 秒）。
    /// 没有 expires/max-age 信息的 cookie 不会出现在这个映射里——按 session cookie 处理（永远不过期）。
    cookie_expires: BTreeMap<String, u64>,
    #[serde(default)]
    has_secure_cookies: bool,
    saved_at: u64,
    username: Option<String>,
    mid: Option<u64>,
    face: Option<String>,
}

#[derive(Serialize)]
struct BilibiliSessionFile<'a> {
    saved_at: u64,
    username: &'a Option<String>,
    mid: Option<u64>,
    face: &'a Option<String>,
    has_secure_cookies: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    cookies: Option<&'a BTreeMap<String, String>>,
    // L-11：持久化 cookie 过期时间，重启后不再把已过期 cookie 当永不过期的 session cookie。
    cookie_expires: &'a BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct FavListData {
    medias: Option<Vec<FavMedia>>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct FavMedia {
    bvid: Option<String>,
    title: Option<String>,
}
