use super::prelude::*;

pub(crate) async fn import_bilibili_audio_inner(
    app: &AppHandle,
    client: &Client,
    input: &str,
    options: &BilibiliImportOptions,
    // 审2-S3：单曲导入传 true；收藏夹批量循环传 false——批量时逐首清理只
    // preserve 当前一首，缓存超限会把同批先导入的文件删掉，改由批量调用方在
    // 整批结束后统一清理一次并 preserve 全部成功导入的文件。
    enforce_cache_limit: bool,
) -> Result<ImportedTrack, String> {
    let bvid = resolve_bvid(client, input).await?;
    // 先确定 ffmpeg 是否可用：EAC3 / 杜比全景声流只能靠 ffmpeg 解码，
    // 缺少 ffmpeg 时必须在选流阶段就避开它们，否则会导入一个永远无法播放的文件。
    let ffmpeg_path = options
        .remux_with_ffmpeg
        .unwrap_or(true)
        .then(|| find_ffmpeg(app))
        .flatten();
    let resolved = resolve_audio(client, &bvid, options, ffmpeg_path.is_some()).await?;
    let cache_path = audio_cache_path(
        app,
        &resolved.video.bvid,
        resolved.video.cid,
        resolved.stream.output_extension(),
    )?;

    // P1-1：下载走专用 client（无总超时，仅连接/读空闲超时），
    // 避免大文件在 30 秒总超时下必然失败。
    let download_client = bilibili_download_client_for_app(app)?;
    // P2-4：EAC3 等必须 remux 的流，remux 失败时不允许 fallback 落盘。
    let must_remux = matches!(
        resolved.stream.kind,
        AudioKind::Dolby | AudioKind::DolbyAtmos
    );
    let final_path = ensure_audio_file(
        &download_client,
        &resolved.stream.audio_urls(),
        &cache_path,
        ffmpeg_path.as_deref(),
        must_remux,
    )
    .await?;
    if enforce_cache_limit {
        // 审2-S2：enforce_cache_limit 是同步磁盘遍历 + 删除，不能在 async
        // runtime 线程上直调；挪进 spawn_blocking，失败只 warn 不中断导入
        // （保持原 `let _ =` 的容错语义）。
        let app_for_cache = app.clone();
        let preserve = vec![final_path.clone()];
        match tauri::async_runtime::spawn_blocking(move || {
            enforce_cache_limit_preserving_many(&app_for_cache, &preserve)
        })
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => tracing::warn!("bilibili 导入后缓存清理失败: {err}"),
            Err(err) => tracing::warn!("bilibili 导入后缓存清理任务异常终止: {err}"),
        }
    }

    let track = track_from_resolved_audio(&resolved, &final_path, ffmpeg_path.is_some())?;
    // P1-3：合并曲库缓存是带锁的阻塞读改写，放进 spawn_blocking，
    // 避免在 async 上下文里持有 parking_lot 锁阻塞调度线程。
    let app_for_merge = app.clone();
    let imported = tauri::async_runtime::spawn_blocking(move || {
        merge_tracks_into_cache(&app_for_merge, &[track])
    })
    .await
    .map_err(|err| format!("曲库合并任务异常终止: {err}"))??;
    imported
        .into_iter()
        .next()
        .ok_or_else(|| "failed to import bilibili audio".to_string())
}

/// P0-1：仅允许 B 站官方域名，防止把请求（尤其是登录 Cookie）发往任意用户输入的 URL。
pub(crate) fn is_bilibili_host(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("b23.tv")
            || host.eq_ignore_ascii_case("acg.tv")
            || host.eq_ignore_ascii_case("bilibili.com")
            || host
                .to_ascii_lowercase()
                .strip_suffix(".bilibili.com")
                .is_some_and(|prefix| !prefix.is_empty())
    })
}

/// M-3：playurl 返回的 DASH 音频 URL 允许的 CDN host 后缀。
/// 官方播放地址落在 B 站自有 / 授权 CDN 域名族；对下载 client 而言这些请求会携带
/// 登录 Cookie（SESSDATA），必须限定在这些域内，避免响应里的第三方/畸形 URL 把凭据带走。
const BILIBILI_CDN_HOST_SUFFIXES: &[&str] = &[
    ".bilivideo.com",
    ".bilivideo.cn",
    ".hdslb.com",
    ".akamaized.net",
    ".bilibili.com",
];

/// M-3：校验 playurl 返回的音频下载 URL 是否可安全携带 Cookie 请求。
/// 要求：scheme 必须是 https（拒绝 http 明文，防被动监听截获会话）；
/// host 必须落在 [`BILIBILI_CDN_HOST_SUFFIXES`] 白名单内。
pub(crate) fn is_safe_bilibili_download_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    BILIBILI_CDN_HOST_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix) && host.len() > suffix.len())
        || host == "bilibili.com"
}

pub(crate) async fn resolve_bvid(_client: &Client, input: &str) -> Result<String, String> {
    if let Some(bvid) = extract_bvid(input) {
        return Ok(bvid);
    }

    let trimmed = input.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("没有找到有效的 B 站 BV 号或视频链接".into());
    }

    let url = reqwest::Url::parse(trimmed).map_err(|_| "无效的 B 站链接".to_string())?;
    if !is_bilibili_host(&url) {
        return Err("仅支持 B 站链接（b23.tv / acg.tv / bilibili.com）".into());
    }

    // P0-1：解析短链/网页一律使用无 Cookie 的裸 client，
    // 避免 SESSDATA 等登录凭据随 default_headers 发往非预期主机。
    let bare_client = bilibili_client_with_cookie(None)?;
    let response = bare_client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("无法打开 B 站链接: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站链接不可访问: {err}"))?;

    let final_url = response.url().to_string();
    if let Some(bvid) = extract_bvid(&final_url) {
        return Ok(bvid);
    }

    // 限制 HTML 抓取大小：避免恶意服务器/重定向到大文件导致内存爆炸。
    let body = read_bytes_capped(response, MAX_HTML_BYTES)
        .await
        .map_err(|err| format!("无法读取 B 站页面内容: {err}"))?;
    let body_str = String::from_utf8_lossy(&body);
    extract_bvid(&body_str).ok_or_else(|| "链接中没有找到可解析的 BV 号".into())
}

pub(crate) async fn resolve_audio(
    client: &Client,
    bvid: &str,
    options: &BilibiliImportOptions,
    ffmpeg_available: bool,
) -> Result<ResolvedAudio, String> {
    let video = fetch_video_data(client, bvid).await?;
    let stream =
        fetch_audio_stream(client, &video.bvid, video.cid, options, ffmpeg_available).await?;
    Ok(ResolvedAudio { video, stream })
}

pub(crate) async fn fetch_video_data(client: &Client, bvid: &str) -> Result<VideoData, String> {
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

pub(crate) async fn fetch_audio_stream(
    client: &Client,
    bvid: &str,
    cid: i64,
    options: &BilibiliImportOptions,
    ffmpeg_available: bool,
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
        ffmpeg_available,
    )
}

pub(crate) fn select_audio_stream(
    dash: DashData,
    prefer_flac: bool,
    prefer_dolby_atmos: bool,
    ffmpeg_available: bool,
) -> Result<AudioStream, String> {
    let mut streams = Vec::new();

    // 杜比 / 全景声（EAC3）流 Symphonia 无法原生解码，必须依赖 ffmpeg fallback。
    // 没有 ffmpeg 时即便用户勾选了 prefer_dolby_atmos 也跳过，避免下载一个永远播不了的文件。
    if prefer_dolby_atmos && ffmpeg_available {
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
        .ok_or_else(|| {
            if ffmpeg_available {
                "bilibili response has no usable audio stream".to_string()
            } else {
                "未找到可直接播放的音频流（仅有杜比/全景声 EAC3 流，需要安装 ffmpeg 才能解码）"
                    .to_string()
            }
        })
}

pub(crate) fn dolby_audio_kind(value: &Value, container_kind: Option<u32>) -> AudioKind {
    if is_dolby_atmos_stream(value, container_kind) {
        AudioKind::DolbyAtmos
    } else {
        AudioKind::Dolby
    }
}

pub(crate) fn is_dolby_atmos_stream(value: &Value, container_kind: Option<u32>) -> bool {
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

pub(crate) fn audio_stream_from_value(value: Value, kind: AudioKind) -> Option<AudioStream> {
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

pub(crate) async fn fetch_favorite_bvids(
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
            if media
                .bvid
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
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

pub(crate) async fn parse_json_response<T: DeserializeOwned>(
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

/// P2-1：进程内进行中下载任务表，拒绝对同一目标文件的并发下载，
/// 防止交错写入产出损坏缓存又被 sentinel 标记为有效。
pub(crate) static DOWNLOADS_IN_FLIGHT: parking_lot::Mutex<BTreeSet<PathBuf>> =
    parking_lot::Mutex::new(BTreeSet::new());

pub(crate) struct DownloadSlot(PathBuf);

impl Drop for DownloadSlot {
    fn drop(&mut self) {
        DOWNLOADS_IN_FLIGHT.lock().remove(&self.0);
    }
}

pub(crate) fn acquire_download_slot(path: &Path) -> Result<DownloadSlot, String> {
    let key = path.to_path_buf();
    let mut in_flight = DOWNLOADS_IN_FLIGHT.lock();
    if !in_flight.insert(key.clone()) {
        return Err(format!("该音频正在下载中，请稍候再试: {}", path.display()));
    }
    Ok(DownloadSlot(key))
}

pub(crate) async fn ensure_audio_file(
    client: &Client,
    audio_urls: &[String],
    path: &Path,
    ffmpeg_path: Option<&Path>,
    must_remux: bool,
) -> Result<PathBuf, String> {
    // 同时存在 path 和 path.ok sentinel 才认为缓存有效；
    // 否则是上一次 remux/写入半途崩溃留下的不完整文件，需要重下。
    let sentinel = ok_sentinel_path(path);
    if cached_audio_file_is_valid(path) {
        return Ok(path.to_path_buf());
    }

    // 审2-S8：上次 remux 失败 fallback 落的 `.m4a` 也参与缓存命中检查，
    // 否则同 BV 重复导入时只查原扩展名路径，会绕过 fallback 缓存重新下载。
    let remux_fallback_path = path.with_extension("m4a");
    if remux_fallback_path != path && cached_audio_file_is_valid(&remux_fallback_path) {
        return Ok(remux_fallback_path);
    }

    let _slot = acquire_download_slot(path)?;

    // 清理可能残留的不完整文件 + 旧 sentinel
    let _ = fs::remove_file(&sentinel);
    if path.is_file() {
        let _ = fs::remove_file(path);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create bilibili cache dir: {err}"))?;
    }

    let mut last_error = None;
    for audio_url in audio_urls {
        let temp_path = temp_download_path(path);
        match download_audio_to_file(client, audio_url, &temp_path).await {
            Ok(()) => match finalize_audio_file(&temp_path, path, ffmpeg_path, must_remux) {
                Ok(final_path) => {
                    // 写一个零字节 sentinel 标记缓存完整可用；
                    // 下次启动只要看到 path 存在但 sentinel 不存在，就视为坏缓存重下。
                    let _ = fs::write(ok_sentinel_path(&final_path), b"");
                    return Ok(final_path);
                }
                Err(err) => {
                    let _ = fs::remove_file(&temp_path);
                    last_error = Some(err);
                }
            },
            Err(err) => {
                let _ = fs::remove_file(&temp_path);
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "bilibili response has no audio download url".into()))
}

pub(crate) fn ok_sentinel_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bilibili-audio");
    path.with_file_name(format!(".{file_name}.ok"))
}

/// 审2-S8：缓存有效性判定收敛到一处——文件存在且非空、且对应 ok sentinel
/// 存在，供原扩展名路径与 remux fallback `.m4a` 路径共用。
pub(crate) fn cached_audio_file_is_valid(path: &Path) -> bool {
    path.is_file()
        && ok_sentinel_path(path).is_file()
        && fs::metadata(path)
            .map(|meta| meta.len() > 0)
            .unwrap_or(false)
}

pub(crate) async fn download_audio_to_file(
    client: &Client,
    audio_url: &str,
    temp_path: &Path,
) -> Result<(), String> {
    if audio_url.trim().is_empty() {
        return Err("empty bilibili audio url".into());
    }

    // M-3：下载 client 携带登录 Cookie。playurl 响应里的 baseUrl/backupUrl 属外部输入，
    // 必须先校验为 https + B 站系 CDN 域名，拒绝把 SESSDATA 发往第三方/明文地址。
    if !is_safe_bilibili_download_url(audio_url) {
        return Err(format!(
            "拒绝从非 B 站 CDN 或非 https 地址下载音频: {audio_url}"
        ));
    }

    let mut response = client
        .get(audio_url)
        .send()
        .await
        .map_err(|err| format!("failed to download bilibili audio: {err}"))?
        .error_for_status()
        .map_err(|err| format!("bilibili audio download failed: {err}"))?;

    // 提前检查 Content-Length；超出上限直接拒绝，避免下载到一半才发现。
    if let Some(content_length) = response.content_length() {
        if content_length > MAX_AUDIO_DOWNLOAD_BYTES {
            return Err(format!(
                "bilibili audio too large: declared {} bytes (limit {})",
                content_length, MAX_AUDIO_DOWNLOAD_BYTES
            ));
        }
    }

    // L-13：流式写入临时文件，避免把整段 FLAC（数百 MB）整块驻留内存再二次复制。
    let mut file = fs::File::create(temp_path)
        .map_err(|err| format!("failed to create bilibili temp file: {err}"))?;
    let mut written: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("network read error: {err}"))?
    {
        written = written.saturating_add(chunk.len() as u64);
        if written > MAX_AUDIO_DOWNLOAD_BYTES {
            return Err(format!(
                "bilibili audio exceeded {MAX_AUDIO_DOWNLOAD_BYTES} bytes; aborted"
            ));
        }
        file.write_all(&chunk)
            .map_err(|err| format!("failed to write bilibili temp file: {err}"))?;
    }

    if written == 0 {
        return Err("downloaded bilibili audio is empty".into());
    }
    file.flush()
        .map_err(|err| format!("failed to flush bilibili temp file: {err}"))?;
    Ok(())
}

/// 增量读取响应体，超出上限即截断并报错；
/// 避免恶意服务器或异常重定向把进程内存撑爆。
pub(crate) async fn read_bytes_capped(
    mut response: reqwest::Response,
    cap: u64,
) -> std::result::Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("network read error: {err}"))?
    {
        if (buf.len() as u64).saturating_add(chunk.len() as u64) > cap {
            return Err(format!("response body exceeded {cap} bytes; aborted"));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// P2-8：头像 data URL 按源 URL 内存缓存，避免前端周期性查询登录状态时
/// 每次都重新下载头像并 base64 编码。
pub(crate) static AVATAR_DATA_URL_CACHE: parking_lot::Mutex<BTreeMap<String, String>> =
    parking_lot::Mutex::new(BTreeMap::new());
pub(crate) const MAX_AVATAR_CACHE_ENTRIES: usize = 8;

pub(crate) async fn resolve_avatar_data_url(
    client: &Client,
    url: &str,
) -> Result<Option<String>, String> {
    let url = normalize_url(url);
    if url.trim().is_empty() || url.starts_with("data:") {
        return Ok((!url.trim().is_empty()).then_some(url));
    }

    if let Some(cached) = AVATAR_DATA_URL_CACHE.lock().get(&url).cloned() {
        return Ok(Some(cached));
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
    // L-12：增量读取并在超限时立即中止，避免重定向到大文件时白白吃内存。
    let bytes = match read_bytes_capped(response, MAX_AVATAR_BYTES as u64).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    if bytes.is_empty() {
        return Ok(None);
    }

    let data_url = format!("data:{};base64,{}", content_type, BASE64.encode(bytes));
    {
        let mut cache = AVATAR_DATA_URL_CACHE.lock();
        if cache.len() >= MAX_AVATAR_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(url, data_url.clone());
    }
    Ok(Some(data_url))
}

pub(crate) fn avatar_mime_type(value: &str) -> Option<&'static str> {
    let value = value.split(';').next()?.trim().to_ascii_lowercase();
    match value.as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        _ => None,
    }
}

pub(crate) fn finalize_audio_file(
    temp_path: &Path,
    path: &Path,
    ffmpeg_path: Option<&Path>,
    must_remux: bool,
) -> Result<PathBuf, String> {
    // temp_path 已由 download_audio_to_file 流式写好；这里只做 remux 或就地改名。
    if let Some(ffmpeg_path) = ffmpeg_path {
        match remux_audio(ffmpeg_path, temp_path, path) {
            Ok(()) => {
                let _ = fs::remove_file(temp_path);
                return Ok(path.to_path_buf());
            }
            Err(err) => {
                let _ = fs::remove_file(path);
                // P2-4：EAC3 等必须 remux 的流不允许把原始 fMP4 字节冠以
                // .eac3 落盘——Symphonia 无法解码，会永久缓存一个不可播文件。
                if must_remux {
                    return Err(format!("音频流必须经 ffmpeg 重封装，但重封装失败: {err}"));
                }
            }
        }
    }

    // P2-4：remux 失败（或无 ffmpeg）时按实际容器（fMP4）落 .m4a 扩展名，
    // 而不是沿用 .flac 等可能与内容不符的扩展名。
    let fallback_path = if ffmpeg_path.is_some() {
        path.with_extension("m4a")
    } else {
        path.to_path_buf()
    };
    fs::rename(temp_path, &fallback_path)
        .map_err(|err| format!("failed to finalize bilibili audio file: {err}"))?;
    Ok(fallback_path)
}

pub(crate) fn remux_audio(ffmpeg_path: &Path, input: &Path, output: &Path) -> Result<(), String> {
    let mut command = Command::new(ffmpeg_path);
    hide_console_window(&mut command);
    let result = command
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

    if !output.is_file()
        || fs::metadata(output)
            .map(|meta| meta.len() == 0)
            .unwrap_or(true)
    {
        return Err("ffmpeg did not create a usable output file".into());
    }

    Ok(())
}

pub(crate) fn track_from_resolved_audio(
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

pub(crate) fn audio_cache_path(
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

/// P2-1：临时文件名带进程内计数器 + 时间纳秒的唯一后缀，
/// 保证并发下载即便命中同一目标也不会互相截断/交错写。
pub(crate) fn temp_download_path(path: &Path) -> PathBuf {
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bilibili-audio");
    let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!("{file_name}.{nanos}-{counter}.download"))
}
