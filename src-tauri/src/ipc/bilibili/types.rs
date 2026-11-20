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
    /// BILI-05：分 P 列表。接口没给、或格式出乎意料时为空（退回上面的首个 cid），
    /// 不让这一个字段拖垮整条导入。
    #[serde(default, deserialize_with = "lenient_video_pages")]
    pub(crate) pages: Vec<VideoPage>,
}

/// 视频信息接口里的一个分 P（原始数据，规整见 `pages.rs`）。
#[derive(Debug, Clone, Default)]
pub(crate) struct VideoPage {
    pub(crate) cid: i64,
    /// 分 P 序号，从 1 起；缺失为 0
    pub(crate) page: u32,
    /// 分 P 标题
    pub(crate) part: String,
    /// 这一 P 的时长（秒）；缺失为 0
    pub(crate) duration: u64,
}

fn lenient_video_pages<'de, D>(deserializer: D) -> Result<Vec<VideoPage>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| VideoPage {
                    cid: item.get("cid").and_then(Value::as_i64).unwrap_or_default(),
                    page: item
                        .get("page")
                        .and_then(Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or_default(),
                    part: item
                        .get("part")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    duration: item
                        .get("duration")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default())
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

pub(crate) struct ResolvedAudio<'a> {
    pub(crate) video: &'a VideoData,
    /// BILI-05：要导入的那个分 P
    pub(crate) page: &'a SelectedPage,
    pub(crate) stream: AudioStream,
}

/// BILI-05：输入没有指定分 P 的多 P 视频怎么处理。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MultiPageMode {
    /// 只导入第 1 P（默认，与分 P 功能出现之前一致）
    #[default]
    First,
    /// 导入全部分 P
    All,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliImportOptions {
    pub(crate) prefer_flac: Option<bool>,
    pub(crate) prefer_dolby_atmos: Option<bool>,
    pub(crate) remux_with_ffmpeg: Option<bool>,
    pub(crate) existing_track_id: Option<String>,
    /// BILI-05：输入没有指定分 P 时的处理方式。带 `?p=N` 的输入只导入那一 P，不看这个值；
    /// 重新缓存 / 重新加载永远只出一首，同样不看。
    pub(crate) multi_page: Option<MultiPageMode>,
}

/// BILI-05：视频概况，前端据此决定要不要问「只导入首 P 还是全部」。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliVideoInfo {
    pub(crate) bvid: String,
    pub(crate) title: String,
    /// 输入里指定的分 P（`?p=N`）；没有指定为 null
    pub(crate) requested_page: Option<u32>,
    pub(crate) pages: Vec<BilibiliVideoPageInfo>,
    /// 一次全量导入最多多少个分 P；分 P 数超过它时全量导入会被拒绝，前端据此提前说明
    pub(crate) full_import_limit: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliVideoPageInfo {
    pub(crate) page: u32,
    pub(crate) title: String,
    pub(crate) duration: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliLoginQrCode {
    pub(crate) url: String,
    pub(crate) qrcode_key: String,
    pub(crate) data_url: String,
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
    /// 用户中途取消（已导入部分照常返回）
    pub(crate) cancelled: bool,
}

/// 收藏夹批量导入进度事件（`seraph://bilibili-batch`）payload。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliBatchProgress {
    pub(crate) task_id: String,
    pub(crate) current: usize,
    pub(crate) total: usize,
    pub(crate) title: String,
    pub(crate) ok: bool,
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
