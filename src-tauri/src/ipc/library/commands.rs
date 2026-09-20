use super::prelude::*;
use crate::ipc::error::{IpcError, IpcResult};
use tauri::Emitter;

/// 曲目歌词写盘后广播:任务栏歌词条按 trackId 缓存曲目元数据,只在切歌时
/// 重拉,原本没歌词的曲目导入歌词后条会一直显示「暂无歌词稿」。所有写歌词
/// 的入口(本地导入 / 在线歌词)都在写完缓存后发这个事件,条收到即重拉。
pub(crate) const LYRICS_UPDATED_EVENT: &str = "seraph://track-lyrics-updated";

fn emit_lyrics_updated(app: &AppHandle, track_id: &str) {
    if let Err(err) = app.emit(LYRICS_UPDATED_EVENT, track_id) {
        tracing::warn!("emit {LYRICS_UPDATED_EVENT} failed: {err}");
    }
}

#[tauri::command]
pub async fn get_playlist(
    app: AppHandle,
    include_lyrics: Option<bool>,
) -> IpcResult<super::snapshot::PlaylistSnapshot> {
    let app_for_read = app.clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || read_library_snapshot(&app_for_read))
            .await
            .map_err(|err| {
                IpcError::new(
                    crate::ipc::error::IpcErrorCode::Internal,
                    format!("get_playlist task panicked: {err}"),
                )
            })??;
    // 曲库可立即使用；封面补扫和 GC 在独立阻塞任务里继续，完成后通知前端刷新摘要。
    static MAINTENANCE_STARTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if !MAINTENANCE_STARTED.swap(true, Ordering::AcqRel) {
        tauri::async_runtime::spawn_blocking(move || {
            if backfill_missing_covers(&app) {
                let _ = app.emit("seraph://library-updated", ());
            }
            gc_orphan_covers(&app);
        });
    }
    Ok(super::snapshot::PlaylistSnapshot {
        snapshot,
        include_lyrics: include_lyrics.unwrap_or(true),
    })
}

#[tauri::command]
pub async fn get_track_info(app: AppHandle, track_id: String) -> IpcResult<Option<ImportedTrack>> {
    tauri::async_runtime::spawn_blocking(move || {
        read_cached_track(&app, &track_id).map(|track| {
            track.map(|mut track| {
                // 排除规则只在回传前打标，不落缓存
                mark_hidden_document(&mut track.lyrics);
                track
            })
        })
    })
    .await
    .map_err(|err| IpcError::from(format!("get_track_info task panicked: {err}")))?
    .map_err(IpcError::from)
}

#[tauri::command]
pub async fn delete_track(app: AppHandle, track: DeleteTrackRequest) -> IpcResult<bool> {
    // H-1：读改写 + 可能的锁等待都是阻塞操作，放 spawn_blocking，避免占用主线程
    // （Tauri 同步命令在主线程执行，与 backfill 抢 LIBRARY_LOCK 时会冻结整个窗口）。
    tauri::async_runtime::spawn_blocking(move || delete_track_inner(&app, &track))
        .await
        .map_err(|err| {
            IpcError::new(
                crate::ipc::error::IpcErrorCode::Internal,
                format!("delete_track task panicked: {err}"),
            )
        })?
}

fn delete_track_inner(app: &AppHandle, track: &DeleteTrackRequest) -> IpcResult<bool> {
    let track_id = track.id.trim();
    let target_key = delete_track_request_key(track);
    if track_id.is_empty() && target_key.is_none() {
        return Err(IpcError::invalid_input("missing track identity"));
    }

    // 兼容旧单曲入口，但文件路径始终来自后端曲库，不信任请求中的路径。
    let _guard = LIBRARY_LOCK.lock();
    let tracks = read_cached_tracks_for_update(app)?;
    let ids = tracks
        .iter()
        .filter(|item| cached_track_matches_delete(item, track_id, target_key.as_deref()))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let result = super::deletion::delete_selected_tracks(tracks, &ids, |updated| {
        write_cached_tracks(app, updated)
    })?;
    if let Some(failure) = result.failures.first() {
        return Err(IpcError::from(failure.message.clone()));
    }
    Ok(!result.deleted_ids.is_empty())
}

#[tauri::command]
pub async fn delete_tracks(
    app: AppHandle,
    track_ids: Vec<String>,
) -> IpcResult<DeleteTracksResult> {
    tauri::async_runtime::spawn_blocking(move || {
        // 一次持锁读改写，防止与导入/重缓存互相覆盖；整批只提交一次存储代次。
        let _guard = LIBRARY_LOCK.lock();
        let tracks = read_cached_tracks_for_update(&app)?;
        super::deletion::delete_selected_tracks(tracks, &track_ids, |updated| {
            write_cached_tracks(&app, updated)
        })
    })
    .await
    .map_err(|err| IpcError::from(format!("批量删除任务异常终止: {err}")))?
    .map_err(IpcError::from)
}

#[tauri::command]
pub fn list_devices() -> IpcResult<Vec<OutputDeviceInfo>> {
    let devices = list_output_devices().map_err(|err| IpcError::from(err.to_string()))?;
    Ok(devices
        .into_iter()
        .map(|device| OutputDeviceInfo {
            id: device.id,
            name: device.name,
            is_default: device.is_default,
            legacy_ids: device.legacy_ids,
        })
        .collect())
}

#[tauri::command]
pub async fn import_tracks(app: AppHandle, paths: Vec<String>) -> IpcResult<Vec<ImportedTrack>> {
    // L-18：文件遍历 + lofty 解析 + 可能的 ffprobe 子进程都是阻塞 IO，
    // 放到 spawn_blocking，避免占用 Tauri 命令调度线程、阻塞其它 IPC（含播放控制）。
    let tracks =
        tauri::async_runtime::spawn_blocking(move || -> Result<Vec<ImportedTrack>, String> {
            let mut tracks = Vec::new();
            let mut seen_files = HashSet::new();
            let mut visited_dirs = HashSet::new();
            // P3-11：子目录读失败只累积警告，不中止整批导入。
            let mut warnings = Vec::new();
            // 拿不到应用数据目录时封面提取降级跳过，不影响导入本身
            let covers_dir = covers_dir_path(&app).ok();

            for path in paths {
                collect_audio_files(
                    PathBuf::from(path),
                    &mut tracks,
                    &mut seen_files,
                    &mut visited_dirs,
                    0,
                    &mut warnings,
                    covers_dir.as_deref(),
                )?;
            }

            for warning in &warnings {
                tracing::warn!("import_tracks: {warning}");
            }
            if !warnings.is_empty() {
                let _ = app.emit(
                    "seraph://library-import-warning",
                    format!("已跳过 {} 项无法读取的文件或目录", warnings.len()),
                );
            }

            if !tracks.is_empty() {
                // P1-3 + P0-2：持锁读改写；缓存损坏时报错并备份，不再当空库覆盖。
                let _guard = LIBRARY_LOCK.lock();
                let cached = read_cached_tracks_for_update(&app)?;
                let merged = merge_cached_tracks(cached, &tracks);
                write_cached_tracks(&app, &merged)?;
                return Ok(imported_tracks_from_cache(&merged, &tracks));
            }

            Ok(tracks)
        })
        .await
        .map_err(|err| {
            IpcError::new(
                crate::ipc::error::IpcErrorCode::Internal,
                format!("import task panicked: {err}"),
            )
        })??;
    Ok(tracks)
}

#[tauri::command]
pub async fn save_track_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics_bytes: Vec<u8>,
    track_path: Option<String>,
    prefer_traditional: Option<bool>,
) -> IpcResult<LyricDocument> {
    // H-1：持锁 + 可能的 lofty/ffprobe 探测（未入库曲目）都是阻塞操作，放 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || {
        save_track_lyrics_inner(
            &app,
            &track_id,
            &lyrics_bytes,
            track_path.as_deref(),
            prefer_traditional.unwrap_or(false),
        )
    })
    .await
    .map_err(|err| {
        IpcError::new(
            crate::ipc::error::IpcErrorCode::Internal,
            format!("save_track_lyrics task panicked: {err}"),
        )
    })?
}

fn save_track_lyrics_inner(
    app: &AppHandle,
    track_id: &str,
    lyrics_bytes: &[u8],
    track_path: Option<&str>,
    prefer_traditional: bool,
) -> IpcResult<LyricDocument> {
    if track_id.trim().is_empty() {
        return Err(IpcError::invalid_input("missing track id"));
    }

    if lyrics_bytes.is_empty() {
        return Err(IpcError::invalid_input("lyrics file is empty"));
    }

    // 后端独立校验大小：前端虽然限制了 2MB，但 IPC 可被绕过；
    // 设为 4MB 留余量，同时阻挡明显异常输入。
    const MAX_LYRICS_BYTES: usize = 4 * 1024 * 1024;
    if lyrics_bytes.len() > MAX_LYRICS_BYTES {
        return Err(IpcError::invalid_input(format!(
            "lyrics file too large: {} bytes (limit {})",
            lyrics_bytes.len(),
            MAX_LYRICS_BYTES
        )));
    }

    let lines = parse_lyrics_bytes(lyrics_bytes);
    if lines.is_empty() {
        return Err(IpcError::invalid_input("lyrics file has no usable text"));
    }
    // 用户手动导入 = 固定选择
    let mut source = LyricSource::of(LyricSourceKind::Manual);
    source.pinned = true;
    let mut lyrics = LyricDocument::from_lines(lines, source);
    if prefer_traditional {
        lyrics = lyrics_to_traditional(lyrics);
    }

    // P1-3：读改写序列全程持锁，防止与并发导入互相覆盖。
    let _guard = LIBRARY_LOCK.lock();
    let mut tracks = read_cached_tracks_for_update(app)?;
    apply_track_lyrics(
        &mut tracks,
        track_id,
        lyrics,
        track_path,
        covers_dir_path(app).ok().as_deref(),
    )?;
    let stored = stored_lyrics(&tracks, track_id);
    write_cached_tracks(app, &tracks)?;
    emit_lyrics_updated(app, track_id);

    Ok(marked(stored))
}

/// 写库后回读该曲目的文档（含并入的查找键），作为命令返回值。
fn stored_lyrics(tracks: &[ImportedTrack], track_id: &str) -> LyricDocument {
    tracks
        .iter()
        .find(|track| track.id == track_id)
        .map(|track| track.lyrics.clone())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn fetch_online_lyrics(
    _track_id: String,
    title: String,
    artist: String,
    duration: u64,
    options: Option<OnlineLyricsOptions>,
) -> IpcResult<Vec<OnlineLyricsCandidate>> {
    let query = online_lyrics_query(&title, &artist);
    if query.is_empty() {
        return Err(IpcError::invalid_input("missing track title"));
    }
    let options = options.unwrap_or_default();

    let client = online_lyrics_client().map_err(IpcError::network)?;
    let fetch = fetch_online_lyrics_from_sources(
        &client,
        &title,
        &artist,
        duration,
        LyricsSourcePriority::parse(&options.source_priority),
    )
    .await;

    let mut candidates = fetch.candidates;
    // TTML 二次查找：曲目已知的查找键（本地歌词文件名里的网易云 ID）排最前直取，
    // 再用网易云/QQ 候选的平台 ID 试取；命中一律 splice 到候选最前
    if options.ttml_enabled {
        let mut ttml_hit = false;
        if let Some(template) = normalize_amll_db_url(&options.ttml_db_url, options.ttml_db_custom)
        {
            let mut seeds = Vec::with_capacity(candidates.len() + 1);
            if !options.lookup_keys.is_empty() {
                seeds.push(OnlineLyricsCandidate {
                    id: "lookup-seed".into(),
                    source: String::new(),
                    title: title.clone(),
                    artist: artist.clone(),
                    album: None,
                    duration: (duration > 0).then_some(duration),
                    lyrics: LyricDocument::EMPTY,
                    ttml_lookup_keys: options.lookup_keys.clone(),
                });
            }
            seeds.extend(candidates.iter().cloned());
            let ttml = fetch_amll_ttml_candidates(&template, options.ttml_db_custom, &seeds).await;
            if !ttml.is_empty() {
                ttml_hit = true;
                candidates.splice(0..0, ttml);
            }
        } else {
            tracing::warn!("AMLL TTML DB 地址不在白名单内，已跳过 TTML 查找");
        }
        // DB 直取一无所获时再问官方索引（固定走 api.amll.dev，与用户 DB 地址模式无关）；
        // 只有高置信度匹配才会返回，任何失败都只记 debug、不影响三源结果
        if !ttml_hit {
            if let Some(candidate) =
                fetch_amll_api_candidate(&title, &artist, (duration > 0).then_some(duration)).await
            {
                candidates.insert(0, candidate);
            }
        }
    }

    if candidates.is_empty() {
        // M-12：有源在搜索阶段就失败（断网/接口异常）时不能谎报“未找到”，
        // 让前端拿到 network 错误码给出可行动的提示。
        if fetch.failed_sources > 0 {
            return Err(IpcError::network(
                "在线歌词源访问失败，请检查网络连接后重试",
            ));
        }
        return Err(IpcError::not_found("online lyrics not found"));
    }

    for candidate in &mut candidates {
        if options.prefer_traditional {
            candidate.lyrics = lyrics_to_traditional(std::mem::take(&mut candidate.lyrics));
        }
        mark_hidden_document(&mut candidate.lyrics);
    }

    Ok(candidates)
}

/// 歌词排除规则同步到后端并广播刷新（主窗口与任务栏都按事件重拉当前曲目歌词）。
#[tauri::command]
pub fn set_lyrics_exclude_rules(
    app: AppHandle,
    rules: Vec<LyricsExcludeRule>,
) -> IpcResult<Vec<LyricsExcludeRuleStatus>> {
    if rules.len() > MAX_EXCLUDE_RULES {
        return Err(IpcError::invalid_input(format!(
            "排除规则最多 {MAX_EXCLUDE_RULES} 条"
        )));
    }
    let statuses = replace_rules(&rules);
    if let Err(err) = app.emit(LYRICS_RULES_UPDATED_EVENT, ()) {
        tracing::warn!("emit {LYRICS_RULES_UPDATED_EVENT} failed: {err}");
    }
    Ok(statuses)
}

/// 编辑弹窗逐条校验（不改当前生效规则）。
#[tauri::command]
pub fn validate_lyrics_exclude_rules(
    rules: Vec<LyricsExcludeRule>,
) -> IpcResult<Vec<LyricsExcludeRuleStatus>> {
    Ok(validate_rules(&rules))
}

/// 在用户指定的歌词目录里按「艺术家 - 曲名」匹配歌词文件，命中即写入曲库并返回。
/// 文件名括号里的网易云 ID 记为 `lyrics_lookup_keys`，供 AMLL TTML 直取。
#[tauri::command]
pub async fn find_local_lyrics(
    app: AppHandle,
    track_id: String,
    track_path: Option<String>,
    title: String,
    artist: String,
    folder: String,
    prefer_traditional: Option<bool>,
) -> IpcResult<Option<LocalLyricsMatch>> {
    tauri::async_runtime::spawn_blocking(move || {
        find_local_lyrics_inner(
            &app,
            &track_id,
            track_path.as_deref(),
            &title,
            &artist,
            &folder,
            prefer_traditional.unwrap_or(false),
        )
    })
    .await
    .map_err(|err| {
        IpcError::new(
            crate::ipc::error::IpcErrorCode::Internal,
            format!("find_local_lyrics task panicked: {err}"),
        )
    })?
}

fn find_local_lyrics_inner(
    app: &AppHandle,
    track_id: &str,
    track_path: Option<&str>,
    title: &str,
    artist: &str,
    folder: &str,
    prefer_traditional: bool,
) -> IpcResult<Option<LocalLyricsMatch>> {
    if track_id.trim().is_empty() {
        return Err(IpcError::invalid_input("missing track id"));
    }
    let folder = validate_lyrics_folder(folder).map_err(IpcError::invalid_input)?;
    let Some((path, lookup_keys)) = find_in_folder(&folder, title, artist) else {
        return Ok(None);
    };
    let Some(mut lyrics) = read_local_lyrics(&path) else {
        return Ok(None);
    };
    if prefer_traditional {
        lyrics = lyrics_to_traditional(lyrics);
    }
    // 文件名括号里的平台 ID 记进文档来源，下次在线匹配直取 AMLL
    lyrics.source = std::mem::take(&mut lyrics.source).with_lookup_keys(lookup_keys.clone());

    let _guard = LIBRARY_LOCK.lock();
    let mut tracks = read_cached_tracks_for_update(app)?;
    apply_track_lyrics(
        &mut tracks,
        track_id,
        lyrics,
        track_path,
        covers_dir_path(app).ok().as_deref(),
    )?;
    let stored = stored_lyrics(&tracks, track_id);
    write_cached_tracks(app, &tracks)?;
    emit_lyrics_updated(app, track_id);

    Ok(Some(LocalLyricsMatch {
        path: path.to_string_lossy().into_owned(),
        lyrics: marked(stored),
        lookup_keys,
    }))
}

#[tauri::command]
pub async fn apply_online_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics: LyricDocument,
    track_path: Option<String>,
    lookup_keys: Option<Vec<String>>,
) -> IpcResult<LyricDocument> {
    // H-1：持锁 + 可能的 lofty/ffprobe 探测都是阻塞操作，放 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || {
        apply_online_lyrics_inner(
            &app,
            &track_id,
            lyrics,
            track_path.as_deref(),
            lookup_keys.unwrap_or_default(),
        )
    })
    .await
    .map_err(|err| {
        IpcError::new(
            crate::ipc::error::IpcErrorCode::Internal,
            format!("apply_online_lyrics task panicked: {err}"),
        )
    })?
}

/// 回写曲目的 AMLL 查找键上限（一个候选正常只有 1~2 个平台 ID）。
const MAX_APPLIED_LOOKUP_KEYS: usize = 8;

/// 前端回传的查找键先过字符集校验（`dir/id`，与 `lookup_url` 同口径），非法丢弃、去重、封顶。
fn sanitize_lookup_keys(keys: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in keys {
        let key = key.trim();
        if is_valid_lookup_key(key) && !out.iter().any(|k| k == key) {
            out.push(key.to_string());
            if out.len() >= MAX_APPLIED_LOOKUP_KEYS {
                break;
            }
        }
    }
    out
}

fn apply_online_lyrics_inner(
    app: &AppHandle,
    track_id: &str,
    mut lyrics: LyricDocument,
    track_path: Option<&str>,
    lookup_keys: Vec<String>,
) -> IpcResult<LyricDocument> {
    if track_id.trim().is_empty() {
        return Err(IpcError::invalid_input("missing track id"));
    }
    if lyrics.is_empty() {
        return Err(IpcError::invalid_input("lyrics file has no usable text"));
    }
    // 前端传回的文档来源自候选（已带 provider / 查找键）；显式传入的查找键（AMLL 命中的候选
    // 携带的）经清洗后并入；用户明确应用 = 固定选择。hidden 是显示层标记，不落盘。
    let lookup_keys = sanitize_lookup_keys(lookup_keys);
    lyrics.source = std::mem::take(&mut lyrics.source).with_lookup_keys(lookup_keys);
    lyrics.source.pinned = true;
    lyrics.source.lookup_keys =
        sanitize_lookup_keys(std::mem::take(&mut lyrics.source.lookup_keys));
    for line in &mut lyrics.lines {
        line.hidden = false;
    }
    let lyrics = LyricDocument::from_lines(lyrics.lines, lyrics.source);

    // P1-3：读改写序列全程持锁，防止与并发导入互相覆盖。
    let _guard = LIBRARY_LOCK.lock();
    let mut tracks = read_cached_tracks_for_update(app)?;
    apply_track_lyrics(
        &mut tracks,
        track_id,
        lyrics,
        track_path,
        covers_dir_path(app).ok().as_deref(),
    )?;
    let stored = stored_lyrics(&tracks, track_id);
    write_cached_tracks(app, &tracks)?;
    emit_lyrics_updated(app, track_id);

    Ok(marked(stored))
}

/// 歌词设置页「测试连接」：对模板真发一次样例 GET，返回分类结果（不改任何状态）。
#[tauri::command]
pub async fn test_amll_ttml_db(
    template: String,
    custom: bool,
    sample_key: Option<String>,
) -> IpcResult<AmllTestResult> {
    Ok(run_amll_ttml_db_test(&template, custom, sample_key.as_deref()).await)
}

/// 把曲库里该曲目的歌词导出为 LRC（增强型 / 逐字 / 逐行）。
///
/// 主窗口专用（F-03 白名单不放行）。路径经 `validate_export_path`（F-02：绝对路径、
/// `.lrc` 扩展名、拒 `..`）。导出的是曲库原始歌词，不受排除规则影响——排除是显示层概念。
/// 返回写出的行数。
#[tauri::command]
pub async fn export_track_lyrics(
    app: AppHandle,
    track_id: String,
    path: String,
    format: LrcExportFormat,
    options: Option<LrcExportOptions>,
) -> IpcResult<usize> {
    tauri::async_runtime::spawn_blocking(move || {
        export_track_lyrics_inner(&app, &track_id, &path, format, &options.unwrap_or_default())
    })
    .await
    .map_err(|err| {
        IpcError::new(
            crate::ipc::error::IpcErrorCode::Internal,
            format!("export_track_lyrics task panicked: {err}"),
        )
    })?
}

fn export_track_lyrics_inner(
    app: &AppHandle,
    track_id: &str,
    path: &str,
    format: LrcExportFormat,
    options: &LrcExportOptions,
) -> IpcResult<usize> {
    if track_id.trim().is_empty() {
        return Err(IpcError::invalid_input("missing track id"));
    }
    let target = super::super::path_guard::validate_export_path(path, &["lrc"])?;

    let track = read_cached_track(app, track_id)
        .map_err(IpcError::from)?
        .ok_or_else(|| IpcError::not_found("track was not found"))?;
    if track.lyrics.is_empty() {
        return Err(IpcError::invalid_input("track has no lyrics"));
    }

    let tool = format!("Seraph Audio Player {}", env!("CARGO_PKG_VERSION"));
    let meta = LrcExportMeta {
        title: &track.title,
        artist: &track.artist,
        album: &track.album,
        tool: &tool,
    };
    let content = lyrics_to_lrc(&track.lyrics.lines, format, options, &meta);
    let written = content
        .lines()
        .filter(|line| line.starts_with('[') && line.as_bytes().get(3) == Some(&b':'))
        .count();
    fs::write(&target, content.as_bytes()).map_err(|err| {
        IpcError::new(
            crate::ipc::error::IpcErrorCode::Io,
            format!("failed to write lyrics file: {err}"),
        )
    })?;
    Ok(written)
}

#[cfg(test)]
mod lookup_key_tests {
    use super::sanitize_lookup_keys;

    #[test]
    fn sanitize_lookup_keys_filters_dedups_and_caps() {
        let keys = vec![
            " ncm-lyrics/1 ".to_string(),
            "ncm-lyrics/1".to_string(),
            "ncm-lyrics/../x".to_string(),
            "bare".to_string(),
            "qq-lyrics/2".to_string(),
        ];
        assert_eq!(
            sanitize_lookup_keys(keys),
            vec!["ncm-lyrics/1".to_string(), "qq-lyrics/2".to_string()]
        );
        let many = (0..20).map(|i| format!("ncm-lyrics/{i}")).collect();
        assert_eq!(sanitize_lookup_keys(many).len(), 8);
    }
}
