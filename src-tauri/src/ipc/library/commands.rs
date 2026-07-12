use super::prelude::*;
use crate::ipc::error::{IpcError, IpcResult};

#[tauri::command]
pub async fn get_playlist(app: AppHandle) -> IpcResult<Vec<ImportedTrack>> {
    // 封面补扫含 lofty 标签解析（阻塞 IO），与读缓存一并放 spawn_blocking，
    // 避免旧曲库首次补扫时卡住 IPC 调度线程。
    let tracks = tauri::async_runtime::spawn_blocking(move || {
        backfill_missing_covers(&app);
        gc_orphan_covers(&app);
        read_cached_tracks(&app)
    })
    .await
    .map_err(|err| {
        IpcError::new(
            crate::ipc::error::IpcErrorCode::Internal,
            format!("get_playlist task panicked: {err}"),
        )
    })??;
    Ok(tracks)
}

#[tauri::command]
pub fn get_track_info(app: AppHandle, track_id: String) -> IpcResult<Option<ImportedTrack>> {
    Ok(read_cached_tracks(&app)?
        .into_iter()
        .find(|track| track.id == track_id))
}

#[tauri::command]
pub fn delete_track(app: AppHandle, track: DeleteTrackRequest) -> IpcResult<bool> {
    let track_id = track.id.trim();
    let target_key = delete_track_request_key(&track);
    if track_id.is_empty() && target_key.is_none() {
        return Err(IpcError::invalid_input("missing track identity"));
    }

    // P1-3：读改写序列全程持锁，防止与并发导入互相覆盖。
    let _guard = LIBRARY_LOCK.lock();
    let tracks = read_cached_tracks_for_update(&app)?;
    let (updated, removed) = remove_cached_track(tracks, track_id, target_key.as_deref());
    if removed {
        write_cached_tracks(&app, &updated)?;
    }

    Ok(removed)
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
pub fn save_track_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics_bytes: Vec<u8>,
    track_path: Option<String>,
) -> IpcResult<Vec<LyricLine>> {
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

    let lyrics = parse_lyrics_bytes(&lyrics_bytes);
    if lyrics.is_empty() {
        return Err(IpcError::invalid_input("lyrics file has no usable text"));
    }

    // P1-3：读改写序列全程持锁，防止与并发导入互相覆盖。
    let _guard = LIBRARY_LOCK.lock();
    let mut tracks = read_cached_tracks_for_update(&app)?;
    apply_track_lyrics(
        &mut tracks,
        &track_id,
        lyrics.clone(),
        track_path.as_deref(),
        covers_dir_path(&app).ok().as_deref(),
    )?;
    write_cached_tracks(&app, &tracks)?;

    Ok(lyrics)
}

#[tauri::command]
pub async fn fetch_online_lyrics(
    _track_id: String,
    title: String,
    artist: String,
    duration: u64,
) -> IpcResult<Vec<OnlineLyricsCandidate>> {
    let query = online_lyrics_query(&title, &artist);
    if query.is_empty() {
        return Err(IpcError::invalid_input("missing track title"));
    }

    let client = online_lyrics_client().map_err(IpcError::network)?;
    let candidates = fetch_online_lyrics_from_sources(&client, &query, duration).await;
    if candidates.is_empty() {
        return Err(IpcError::not_found("online lyrics not found"));
    }

    Ok(candidates)
}

#[tauri::command]
pub fn apply_online_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics: Vec<LyricLine>,
    track_path: Option<String>,
) -> IpcResult<Vec<LyricLine>> {
    if track_id.trim().is_empty() {
        return Err(IpcError::invalid_input("missing track id"));
    }
    if lyrics.is_empty() {
        return Err(IpcError::invalid_input("lyrics file has no usable text"));
    }

    // P1-3：读改写序列全程持锁，防止与并发导入互相覆盖。
    let _guard = LIBRARY_LOCK.lock();
    let mut tracks = read_cached_tracks_for_update(&app)?;
    apply_track_lyrics(
        &mut tracks,
        &track_id,
        lyrics.clone(),
        track_path.as_deref(),
        covers_dir_path(&app).ok().as_deref(),
    )?;
    write_cached_tracks(&app, &tracks)?;

    Ok(lyrics)
}
