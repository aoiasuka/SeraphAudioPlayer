use std::{
    collections::{hash_map::DefaultHasher, BTreeMap},
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::{
    header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE,
        ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
    },
    Client,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::{
    cache::{cache_dir, enforce_cache_limit_preserving},
    library::{merge_tracks_into_cache, ImportedTrack},
};

const VIEW_API: &str = "https://api.bilibili.com/x/web-interface/view";
const PLAY_URL_API: &str = "https://api.bilibili.com/x/player/playurl";
const NAV_API: &str = "https://api.bilibili.com/x/web-interface/nav";
const QR_GENERATE_API: &str =
    "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const QR_POLL_API: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
const FAV_RESOURCE_LIST_API: &str = "https://api.bilibili.com/x/v3/fav/resource/list";
const BILIBILI_REFERER: &str = "https://www.bilibili.com";
const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36";
const PLAY_URL_FNVAL: &str = "4048";
const FAV_PAGE_SIZE: usize = 20;
const FAV_MAX_ITEMS: usize = 200;
const MAX_AVATAR_BYTES: usize = 512 * 1024;

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

#[derive(Debug, Default, Serialize, Deserialize)]
struct BilibiliSession {
    cookies: BTreeMap<String, String>,
    saved_at: u64,
    username: Option<String>,
    mid: Option<u64>,
    face: Option<String>,
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

#[tauri::command]
pub async fn import_bilibili_audio(app: AppHandle, input: String) -> Result<ImportedTrack, String> {
    import_bilibili_audio_with_options(app, input, None).await
}

#[tauri::command]
pub async fn import_bilibili_audio_with_options(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
) -> Result<ImportedTrack, String> {
    let client = bilibili_client_for_app(&app)?;
    import_bilibili_audio_inner(&app, &client, &input, &options.unwrap_or_default()).await
}

#[tauri::command]
pub async fn import_bilibili_favorites(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
) -> Result<BilibiliBatchImportResult, String> {
    let media_id = extract_media_id(&input).ok_or_else(|| {
        "没有找到有效的 B 站收藏夹 media_id/fid，请粘贴收藏夹链接或数字 ID".to_string()
    })?;
    let options = options.unwrap_or_default();
    let client = bilibili_client_for_app(&app)?;
    let bvids = fetch_favorite_bvids(&client, &media_id, FAV_MAX_ITEMS).await?;
    if bvids.is_empty() {
        return Err("收藏夹里没有可导入的视频，或当前账号没有访问权限".into());
    }

    let mut tracks = Vec::new();
    let mut failed = Vec::new();
    for item in bvids {
        let bvid = item.bvid.clone().unwrap_or_default();
        let display_name = item.title.clone().unwrap_or_else(|| bvid.clone());
        match import_bilibili_audio_inner(&app, &client, &bvid, &options).await {
            Ok(track) => tracks.push(track),
            Err(reason) => failed.push(BilibiliImportFailure {
                input: display_name,
                reason,
            }),
        }
    }

    Ok(BilibiliBatchImportResult { tracks, failed })
}

#[tauri::command]
pub async fn bilibili_login_qrcode(app: AppHandle) -> Result<BilibiliLoginQrCode, String> {
    let client = bilibili_client_for_app(&app)?;
    let response = client
        .get(QR_GENERATE_API)
        .send()
        .await
        .map_err(|err| format!("无法请求 B 站二维码: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站二维码请求失败: {err}"))?;
    let api = parse_json_response::<ApiResponse<QrGenerateData>>(response, "bilibili qrcode")
        .await?
        .into_data("bilibili qrcode")?;

    Ok(BilibiliLoginQrCode {
        url: api.url,
        qrcode_key: api.qrcode_key,
    })
}

#[tauri::command]
pub async fn bilibili_poll_login(
    app: AppHandle,
    qrcode_key: String,
) -> Result<BilibiliLoginPollResult, String> {
    let qrcode_key = qrcode_key.trim();
    if qrcode_key.is_empty() {
        return Err("缺少二维码 key".into());
    }

    let client = bilibili_client_for_app(&app)?;
    let response = client
        .get(QR_POLL_API)
        .query(&[("qrcode_key", qrcode_key)])
        .send()
        .await
        .map_err(|err| format!("无法轮询 B 站登录状态: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站登录轮询失败: {err}"))?;
    let headers = response.headers().clone();
    let api = parse_json_response::<ApiResponse<QrPollData>>(response, "bilibili login poll")
        .await?
        .into_data("bilibili login poll")?;

    if api.code == 0 {
        let mut session = load_session(&app)?.unwrap_or_default();
        merge_set_cookie_headers(&headers, &mut session.cookies);
        session.saved_at = now_secs();
        save_session(&app, &session)?;

        let profile = bilibili_login_status(app.clone()).await?;
        return Ok(BilibiliLoginPollResult {
            code: api.code,
            message: api.message.unwrap_or_else(|| "登录成功".into()),
            url: api.url,
            logged_in: profile.logged_in,
            profile: Some(profile),
        });
    }

    Ok(BilibiliLoginPollResult {
        code: api.code,
        message: api.message.unwrap_or_else(|| login_poll_message(api.code)),
        url: api.url,
        logged_in: false,
        profile: None,
    })
}

#[tauri::command]
pub async fn bilibili_login_status(app: AppHandle) -> Result<BilibiliLoginStatus, String> {
    let Some(session) = load_session(&app)? else {
        return Ok(BilibiliLoginStatus {
            logged_in: false,
            username: None,
            mid: None,
            face: None,
        });
    };

    let client = bilibili_client_with_cookie(session.cookie_header().as_deref())?;
    let response = client
        .get(NAV_API)
        .send()
        .await
        .map_err(|err| format!("无法检查 B 站登录状态: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站登录状态请求失败: {err}"))?;
    let api = parse_json_response::<ApiResponse<NavData>>(response, "bilibili nav").await?;
    let data = match api.into_data("bilibili nav") {
        Ok(data) => data,
        Err(_) => {
            return Ok(BilibiliLoginStatus {
                logged_in: false,
                username: None,
                mid: None,
                face: None,
            })
        }
    };

    if data.is_login {
        let face = match data.face.as_deref() {
            Some(face) => resolve_avatar_data_url(&client, face)
                .await
                .or_else(|_| Ok::<_, String>(Some(normalize_url(face))))
                .ok()
                .flatten(),
            None => None,
        };
        let mut next_session = session;
        next_session.username = data.uname.clone();
        next_session.mid = data.mid;
        next_session.face = face.clone();
        next_session.saved_at = now_secs();
        save_session(&app, &next_session)?;
        return Ok(BilibiliLoginStatus {
            logged_in: true,
            username: data.uname,
            mid: data.mid,
            face,
        });
    }

    Ok(BilibiliLoginStatus {
        logged_in: false,
        username: None,
        mid: None,
        face: None,
    })
}

#[tauri::command]
pub fn bilibili_logout(app: AppHandle) -> Result<(), String> {
    let path = session_path(&app)?;
    if path.is_file() {
        fs::remove_file(&path)
            .map_err(|err| format!("无法删除 B 站登录会话 {}: {err}", path.display()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn bilibili_ffmpeg_status(app: AppHandle) -> Result<BilibiliFfmpegStatus, String> {
    let path = find_ffmpeg(&app);
    Ok(BilibiliFfmpegStatus {
        available: path.is_some(),
        path: path.map(|value| value.to_string_lossy().to_string()),
    })
}

async fn import_bilibili_audio_inner(
    app: &AppHandle,
    client: &Client,
    input: &str,
    options: &BilibiliImportOptions,
) -> Result<ImportedTrack, String> {
    let bvid = resolve_bvid(client, input).await?;
    let resolved = resolve_audio(client, &bvid, options).await?;
    let ffmpeg_path = options
        .remux_with_ffmpeg
        .unwrap_or(true)
        .then(|| find_ffmpeg(app))
        .flatten();
    let cache_path = audio_cache_path(
        app,
        &resolved.video.bvid,
        resolved.video.cid,
        resolved.stream.output_extension(ffmpeg_path.is_some()),
    )?;

    ensure_audio_file(
        client,
        &resolved.stream.audio_urls(),
        &cache_path,
        ffmpeg_path.as_deref(),
    )
    .await?;
    let _ = enforce_cache_limit_preserving(app, &cache_path);

    let track = track_from_resolved_audio(&resolved, &cache_path, ffmpeg_path.is_some())?;
    let imported = merge_tracks_into_cache(app, &[track])?;
    imported
        .into_iter()
        .next()
        .ok_or_else(|| "failed to import bilibili audio".to_string())
}

async fn resolve_bvid(client: &Client, input: &str) -> Result<String, String> {
    if let Some(bvid) = extract_bvid(input) {
        return Ok(bvid);
    }

    let trimmed = input.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("没有找到有效的 B 站 BV 号或视频链接".into());
    }

    let response = client
        .get(trimmed)
        .send()
        .await
        .map_err(|err| format!("无法打开 B 站链接: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站链接不可访问: {err}"))?;

    let final_url = response.url().to_string();
    if let Some(bvid) = extract_bvid(&final_url) {
        return Ok(bvid);
    }

    let body = response
        .text()
        .await
        .map_err(|err| format!("无法读取 B 站页面内容: {err}"))?;
    extract_bvid(&body).ok_or_else(|| "链接中没有找到可解析的 BV 号".into())
}

async fn resolve_audio(
    client: &Client,
    bvid: &str,
    options: &BilibiliImportOptions,
) -> Result<ResolvedAudio, String> {
    let video = fetch_video_data(client, bvid).await?;
    let stream = fetch_audio_stream(client, &video.bvid, video.cid, options).await?;
    Ok(ResolvedAudio { video, stream })
}

async fn fetch_video_data(client: &Client, bvid: &str) -> Result<VideoData, String> {
    let response = client
        .get(VIEW_API)
        .query(&[("bvid", bvid)])
        .send()
        .await
        .map_err(|err| format!("failed to fetch bilibili video info: {err}"))?
        .error_for_status()
        .map_err(|err| format!("bilibili video info request failed: {err}"))?;

    let api =
        parse_json_response::<ApiResponse<VideoData>>(response, "bilibili video info").await?;
    api.into_data("bilibili video info")
}

async fn fetch_audio_stream(
    client: &Client,
    bvid: &str,
    cid: i64,
    options: &BilibiliImportOptions,
) -> Result<AudioStream, String> {
    let response = client
        .get(PLAY_URL_API)
        .query(&[
            ("fnval", PLAY_URL_FNVAL),
            ("fnver", "0"),
            ("fourk", "1"),
            ("high_quality", "1"),
            ("platform", "pc"),
            ("bvid", bvid),
            ("cid", &cid.to_string()),
        ])
        .send()
        .await
        .map_err(|err| format!("failed to fetch bilibili play url: {err}"))?
        .error_for_status()
        .map_err(|err| format!("bilibili play url request failed: {err}"))?;

    let api =
        parse_json_response::<ApiResponse<PlayUrlData>>(response, "bilibili play url").await?;
    let play_url = api.into_data("bilibili play url")?;
    let dash = play_url
        .dash
        .ok_or_else(|| "bilibili response has no dash audio streams".to_string())?;

    select_audio_stream(
        dash,
        options.prefer_flac.unwrap_or(true),
        options.prefer_dolby_atmos.unwrap_or(true),
    )
}

fn select_audio_stream(
    dash: DashData,
    prefer_flac: bool,
    prefer_dolby_atmos: bool,
) -> Result<AudioStream, String> {
    let mut streams = Vec::new();

    if prefer_dolby_atmos {
        if let Some(dolby) = dash.dolby {
            let container_kind = dolby.kind;
            if let Some(audio) = dolby.audio {
                streams.extend(audio.into_iter().filter_map(|value| {
                    let kind = dolby_audio_kind(&value, container_kind);
                    audio_stream_from_value(value, kind)
                }));
            }
        }
    }

    if prefer_flac {
        if let Some(audio) = dash.flac.and_then(|flac| flac.audio) {
            if let Some(stream) = audio_stream_from_value(audio, AudioKind::Flac) {
                streams.push(stream);
            }
        }
    }

    if let Some(audio) = dash.audio {
        streams.extend(
            audio
                .into_iter()
                .filter_map(|value| audio_stream_from_value(value, AudioKind::Normal)),
        );
    }

    streams
        .into_iter()
        .max_by_key(|stream| (stream.kind_rank(), stream.bandwidth.unwrap_or_default()))
        .ok_or_else(|| "bilibili response has no usable audio stream".to_string())
}

fn dolby_audio_kind(value: &Value, container_kind: Option<u32>) -> AudioKind {
    if is_dolby_atmos_stream(value, container_kind) {
        AudioKind::DolbyAtmos
    } else {
        AudioKind::Dolby
    }
}

fn is_dolby_atmos_stream(value: &Value, container_kind: Option<u32>) -> bool {
    if container_kind.is_some_and(|kind| kind > 0) {
        return true;
    }

    let quality = value
        .get("id")
        .or_else(|| value.get("quality"))
        .or_else(|| value.get("audio_quality"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if matches!(quality, 30250 | 30255) {
        return true;
    }

    let mut haystack = String::new();
    for key in [
        "codecs",
        "desc",
        "description",
        "display_desc",
        "displayDesc",
        "quality_desc",
        "qualityDesc",
    ] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            haystack.push_str(text);
            haystack.push(' ');
        }
    }
    let haystack = haystack.to_ascii_lowercase();
    haystack.contains("atmos")
        || haystack.contains("dolby")
        || haystack.contains("eac3")
        || haystack.contains("ec-3")
        || haystack.contains("ac-4")
        || haystack.contains("杜比")
        || haystack.contains("全景声")
}

fn audio_stream_from_value(value: Value, kind: AudioKind) -> Option<AudioStream> {
    let base_url = value
        .get("baseUrl")
        .or_else(|| value.get("base_url"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if base_url.is_empty() {
        return None;
    }

    let mut backup_urls = Vec::new();
    for key in ["backupUrl", "backup_url"] {
        if let Some(urls) = value.get(key).and_then(Value::as_array) {
            for url in urls.iter().filter_map(Value::as_str) {
                let url = url.trim();
                if !url.is_empty() && !backup_urls.iter().any(|item| item == url) {
                    backup_urls.push(url.to_string());
                }
            }
        }
    }

    let bandwidth = value
        .get("bandwidth")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let codecs = value
        .get("codecs")
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(AudioStream {
        base_url,
        backup_urls,
        bandwidth,
        codecs,
        kind,
    })
}

async fn fetch_favorite_bvids(
    client: &Client,
    media_id: &str,
    max_items: usize,
) -> Result<Vec<FavMedia>, String> {
    let mut all = Vec::new();
    let mut page = 1usize;

    while all.len() < max_items {
        let response = client
            .get(FAV_RESOURCE_LIST_API)
            .query(&[
                ("media_id", media_id),
                ("pn", &page.to_string()),
                ("ps", &FAV_PAGE_SIZE.to_string()),
                ("platform", "web"),
            ])
            .send()
            .await
            .map_err(|err| format!("failed to fetch bilibili favorite list: {err}"))?
            .error_for_status()
            .map_err(|err| format!("bilibili favorite list request failed: {err}"))?;
        let api =
            parse_json_response::<ApiResponse<FavListData>>(response, "bilibili favorite list")
                .await?;
        let data = api.into_data("bilibili favorite list")?;
        let medias = data.medias.unwrap_or_default();
        let count_before = all.len();
        for media in medias {
            if media.bvid.as_deref().is_some_and(|value| !value.trim().is_empty()) {
                all.push(media);
                if all.len() >= max_items {
                    break;
                }
            }
        }

        if !data.has_more || all.len() == count_before {
            break;
        }
        page += 1;
    }

    Ok(all)
}

async fn parse_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, String> {
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("failed to read {label} response body: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| {
        let preview = String::from_utf8_lossy(&bytes);
        let preview = preview.chars().take(240).collect::<String>();
        format!("failed to parse {label} json: {err}; body preview: {preview}")
    })
}

async fn ensure_audio_file(
    client: &Client,
    audio_urls: &[String],
    path: &Path,
    ffmpeg_path: Option<&Path>,
) -> Result<(), String> {
    if path.is_file()
        && fs::metadata(path)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false)
    {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create bilibili cache dir: {err}"))?;
    }

    let mut last_error = None;
    for audio_url in audio_urls {
        match download_audio_bytes(client, audio_url).await {
            Ok(bytes) => {
                write_audio_file(path, &bytes, ffmpeg_path)?;
                return Ok(());
            }
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| "bilibili response has no audio download url".into()))
}

async fn download_audio_bytes(client: &Client, audio_url: &str) -> Result<Vec<u8>, String> {
    if audio_url.trim().is_empty() {
        return Err("empty bilibili audio url".into());
    }

    let bytes = client
        .get(audio_url)
        .send()
        .await
        .map_err(|err| format!("failed to download bilibili audio: {err}"))?
        .error_for_status()
        .map_err(|err| format!("bilibili audio download failed: {err}"))?
        .bytes()
        .await
        .map_err(|err| format!("failed to read bilibili audio bytes: {err}"))?;

    if bytes.is_empty() {
        return Err("downloaded bilibili audio is empty".into());
    }

    Ok(bytes.to_vec())
}

async fn resolve_avatar_data_url(client: &Client, url: &str) -> Result<Option<String>, String> {
    let url = normalize_url(url);
    if url.trim().is_empty() || url.starts_with("data:") {
        return Ok((!url.trim().is_empty()).then_some(url));
    }

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|err| format!("failed to download bilibili avatar: {err}"))?
        .error_for_status()
        .map_err(|err| format!("bilibili avatar download failed: {err}"))?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(avatar_mime_type)
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("failed to read bilibili avatar bytes: {err}"))?;
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return Ok(None);
    }

    Ok(Some(format!(
        "data:{};base64,{}",
        content_type,
        BASE64.encode(bytes)
    )))
}

fn avatar_mime_type(value: &str) -> Option<&'static str> {
    let value = value.split(';').next()?.trim().to_ascii_lowercase();
    match value.as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        _ => None,
    }
}

fn write_audio_file(
    path: &Path,
    bytes: &[u8],
    ffmpeg_path: Option<&Path>,
) -> Result<(), String> {
    let temp_path = temp_download_path(path);
    fs::write(&temp_path, bytes)
        .map_err(|err| format!("failed to write bilibili temp file: {err}"))?;

    if let Some(ffmpeg_path) = ffmpeg_path {
        match remux_audio(ffmpeg_path, &temp_path, path) {
            Ok(()) => {
                let _ = fs::remove_file(&temp_path);
                return Ok(());
            }
            Err(_) => {
                let _ = fs::remove_file(path);
            }
        }
    }

    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to finalize bilibili audio file: {err}"))?;
    Ok(())
}

fn remux_audio(ffmpeg_path: &Path, input: &Path, output: &Path) -> Result<(), String> {
    let result = Command::new(ffmpeg_path)
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input)
        .arg("-vn")
        .arg("-c:a")
        .arg("copy")
        .arg(output)
        .output()
        .map_err(|err| format!("failed to start ffmpeg: {err}"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("ffmpeg remux failed: {stderr}"));
    }

    if !output.is_file() || fs::metadata(output).map(|meta| meta.len() == 0).unwrap_or(true) {
        return Err("ffmpeg did not create a usable output file".into());
    }

    Ok(())
}

fn track_from_resolved_audio(
    resolved: &ResolvedAudio,
    path: &Path,
    remuxed: bool,
) -> Result<ImportedTrack, String> {
    let metadata =
        fs::metadata(path).map_err(|err| format!("failed to read cached audio metadata: {err}"))?;
    let mut hasher = DefaultHasher::new();
    format!("{}:{}", resolved.video.bvid, resolved.video.cid).hash(&mut hasher);
    let hash = hasher.finish();
    let bitrate = resolved.stream.bandwidth.map(|value| value / 1000);
    let (glow1, glow2) = color_pair(hash);
    let format = resolved.stream.format_label().to_string();

    Ok(ImportedTrack {
        id: format!(
            "bilibili-{}-{}",
            resolved.video.bvid.to_ascii_lowercase(),
            resolved.video.cid
        ),
        title: resolved.video.title.clone(),
        artist: resolved.video.owner.name.clone(),
        album: "Bilibili".into(),
        album_year: None,
        cover: resolved.video.pic.clone().unwrap_or_default(),
        format: format.clone(),
        bitdepth: format_bilibili_quality(&format, bitrate, &resolved.stream.kind, remuxed),
        sample_rate: "Unknown".into(),
        bitrate: format_bitrate(bitrate),
        channels: "Stereo".into(),
        size: format_file_size(metadata.len()),
        path: path.to_string_lossy().to_string(),
        source_url: Some(format!(
            "https://www.bilibili.com/video/{}",
            resolved.video.bvid
        )),
        source_id: Some(resolved.video.bvid.clone()),
        cache_missing: false,
        duration: resolved.video.duration,
        glow_color: glow1.clone(),
        glow1,
        glow2,
        lyrics: Vec::new(),
    })
}

fn audio_cache_path(
    app: &AppHandle,
    bvid: &str,
    cid: i64,
    extension: &str,
) -> Result<PathBuf, String> {
    Ok(cache_dir(app)?.join(format!(
        "{}-{cid}.{}",
        sanitize_file_component(bvid),
        extension.trim_start_matches('.')
    )))
}

fn temp_download_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bilibili-audio");
    path.with_file_name(format!("{file_name}.download"))
}

fn bilibili_client_for_app(app: &AppHandle) -> Result<Client, String> {
    let cookie = load_session(app)?.and_then(|session| session.cookie_header());
    bilibili_client_with_cookie(cookie.as_deref())
}

fn bilibili_client_with_cookie(cookie: Option<&str>) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_zstd()
        .no_deflate()
        .default_headers(bilibili_headers(cookie)?)
        .build()
        .map_err(|err| format!("failed to create http client: {err}"))
}

fn bilibili_headers(cookie: Option<&str>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, header_value(USER_AGENT_VALUE)?);
    headers.insert(REFERER, header_value(BILIBILI_REFERER)?);
    headers.insert(ORIGIN, header_value(BILIBILI_REFERER)?);
    headers.insert(ACCEPT, header_value("*/*")?);
    headers.insert(ACCEPT_ENCODING, header_value("identity")?);
    headers.insert(
        ACCEPT_LANGUAGE,
        header_value("zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")?,
    );
    if let Some(cookie) = cookie.map(str::trim).filter(|value| !value.is_empty()) {
        headers.insert(COOKIE, header_value(cookie)?);
    }
    Ok(headers)
}

fn header_value(value: &str) -> Result<HeaderValue, String> {
    HeaderValue::from_str(value).map_err(|err| format!("invalid http header: {err}"))
}

fn session_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("bilibili-session.json"))
}

fn load_session(app: &AppHandle) -> Result<Option<BilibiliSession>, String> {
    let path = session_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }

    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read bilibili session {}: {err}", path.display()))?;
    let session = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse bilibili session {}: {err}", path.display()))?;
    Ok(Some(session))
}

fn save_session(app: &AppHandle, session: &BilibiliSession) -> Result<(), String> {
    let path = session_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create bilibili session dir: {err}"))?;
    }
    let bytes = serde_json::to_vec_pretty(session)
        .map_err(|err| format!("failed to serialize bilibili session: {err}"))?;
    fs::write(&path, bytes)
        .map_err(|err| format!("failed to write bilibili session {}: {err}", path.display()))
}

fn merge_set_cookie_headers(headers: &HeaderMap, cookies: &mut BTreeMap<String, String>) {
    for value in headers.get_all(SET_COOKIE).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let Some((name, cookie_value)) = value.split(';').next().and_then(|part| part.split_once('='))
        else {
            continue;
        };
        let name = name.trim();
        let cookie_value = cookie_value.trim();
        if !name.is_empty() && !cookie_value.is_empty() {
            cookies.insert(name.to_string(), cookie_value.to_string());
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn extract_bvid(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    trimmed
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .find(|part| {
            let part = part.as_bytes();
            part.len() >= 12
                && part[0].eq_ignore_ascii_case(&b'B')
                && part[1].eq_ignore_ascii_case(&b'V')
        })
        .map(|value| value.to_string())
}

fn extract_media_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) && !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }

    if let Some(start) = trimmed.find("/ml") {
        let value = trimmed[start + 3..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if !value.is_empty() {
            return Some(value);
        }
    }

    for key in ["media_id", "fid"] {
        if let Some(value) = query_value(trimmed, key) {
            return Some(value);
        }
    }

    None
}

fn query_value(input: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let start = input.find(&marker)? + marker.len();
    let value = input[start..]
        .split(|ch| ch == '&' || ch == '#' || ch == '?' || ch == '/')
        .next()?
        .trim();
    if value.chars().all(|ch| ch.is_ascii_digit()) && !value.is_empty() {
        Some(value.to_string())
    } else {
        None
    }
}

fn normalize_url(value: &str) -> String {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = value.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        value.to_string()
    }
}

fn sanitize_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "bilibili-audio".into()
    } else {
        trimmed
    }
}

fn format_from_codec(codec: &str) -> &'static str {
    let codec = codec.to_ascii_lowercase();
    if codec.contains("flac") {
        "FLAC"
    } else if codec.contains("ec-3") || codec.contains("eac3") || codec.contains("ac-4") {
        "EAC3"
    } else if codec.contains("mp4a") {
        "M4A"
    } else if codec.contains("opus") {
        "OPUS"
    } else {
        "AUDIO"
    }
}

fn format_bilibili_quality(
    format: &str,
    bitrate: Option<u32>,
    kind: &AudioKind,
    remuxed: bool,
) -> String {
    let quality = match kind {
        AudioKind::DolbyAtmos => "Bilibili Dolby Atmos",
        AudioKind::Flac => {
            if remuxed {
                "Bilibili FLAC"
            } else {
                "Bilibili FLAC stream"
            }
        }
        AudioKind::Dolby => "Bilibili Dolby",
        AudioKind::Normal => "Bilibili",
    };

    match bitrate {
        Some(value) if value > 0 => format!("{format} {quality} / {value} kbps"),
        _ => format!("{format} {quality}"),
    }
}

fn format_bitrate(bitrate: Option<u32>) -> String {
    match bitrate {
        Some(value) if value > 0 => format!("{value} kbps"),
        _ => "Unknown".into(),
    }
}

fn format_file_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    format!("{mb:.1} MB")
}

fn color_pair(hash: u64) -> (String, String) {
    const PAIRS: [(&str, &str); 6] = [
        ("#67e8f9", "#a5b4fc"),
        ("#7dd3fc", "#f0abfc"),
        ("#5eead4", "#93c5fd"),
        ("#f9a8d4", "#86efac"),
        ("#fde68a", "#67e8f9"),
        ("#c4b5fd", "#fda4af"),
    ];
    let pair = PAIRS[(hash as usize) % PAIRS.len()];
    (pair.0.into(), pair.1.into())
}

fn find_ffmpeg(app: &AppHandle) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut candidates = Vec::new();

    if let Ok(app_dir) = app.path().app_data_dir() {
        candidates.push(app_dir.join("ffmpeg").join(exe_name));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(exe_name));
            candidates.push(dir.join("ffmpeg").join(exe_name));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|dir| dir.join(exe_name)));
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn login_poll_message(code: i32) -> String {
    match code {
        86101 => "等待扫码".into(),
        86090 => "已扫码，等待手机确认".into(),
        86038 => "二维码已过期".into(),
        _ => format!("登录状态码 {code}"),
    }
}

impl<T> ApiResponse<T> {
    fn into_data(self, label: &str) -> Result<T, String> {
        if self.code != 0 {
            return Err(format!(
                "{label} request failed: {}",
                self.message
                    .unwrap_or_else(|| format!("code {}", self.code))
            ));
        }

        self.data
            .ok_or_else(|| format!("{label} response has no data"))
    }
}

impl AudioStream {
    fn audio_urls(&self) -> Vec<String> {
        let mut urls = Vec::with_capacity(self.backup_urls.len() + 1);
        if !self.base_url.trim().is_empty() {
            urls.push(self.base_url.clone());
        }

        for url in &self.backup_urls {
            if !url.trim().is_empty() && !urls.iter().any(|item| item == url) {
                urls.push(url.clone());
            }
        }

        urls
    }

    fn kind_rank(&self) -> u8 {
        match self.kind {
            AudioKind::DolbyAtmos => 4,
            AudioKind::Flac => 3,
            AudioKind::Dolby => 2,
            AudioKind::Normal => 1,
        }
    }

    fn format_label(&self) -> &'static str {
        match self.kind {
            AudioKind::DolbyAtmos | AudioKind::Dolby => "EAC3",
            AudioKind::Flac => "FLAC",
            _ => self
                .codecs
                .as_deref()
                .map(format_from_codec)
                .unwrap_or("M4A"),
        }
    }

    fn output_extension(&self, remuxed: bool) -> &'static str {
        match self.format_label() {
            "FLAC" if remuxed => "flac",
            "OPUS" if remuxed => "opus",
            "EAC3" if remuxed => "eac3",
            _ => "m4a",
        }
    }
}

impl BilibiliSession {
    fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }

        Some(
            self.cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_bvid, extract_media_id};

    #[test]
    fn extracts_bvid_from_plain_text() {
        assert_eq!(
            extract_bvid("BV1xx411c7mD").as_deref(),
            Some("BV1xx411c7mD")
        );
    }

    #[test]
    fn extracts_bvid_from_url() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD/?spm_id_from=333")
                .as_deref(),
            Some("BV1xx411c7mD")
        );
    }

    #[test]
    fn extracts_media_id_from_favorite_url() {
        assert_eq!(
            extract_media_id("https://space.bilibili.com/1/favlist?fid=123456&ftype=create")
                .as_deref(),
            Some("123456")
        );
        assert_eq!(
            extract_media_id("https://www.bilibili.com/medialist/detail/ml987654")
                .as_deref(),
            Some("987654")
        );
        assert_eq!(extract_media_id("123").as_deref(), Some("123"));
    }
}
