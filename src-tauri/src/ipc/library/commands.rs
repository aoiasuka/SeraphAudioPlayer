#[tauri::command]
pub fn get_playlist(app: AppHandle) -> Result<Vec<ImportedTrack>, String> {
    read_cached_tracks(&app)
}

#[tauri::command]
pub fn get_track_info(app: AppHandle, track_id: String) -> Result<Option<ImportedTrack>, String> {
    Ok(read_cached_tracks(&app)?
        .into_iter()
        .find(|track| track.id == track_id))
}

#[tauri::command]
pub fn delete_track(app: AppHandle, track: DeleteTrackRequest) -> Result<bool, String> {
    let track_id = track.id.trim();
    let target_key = delete_track_request_key(&track);
    if track_id.is_empty() && target_key.is_none() {
        return Err("missing track identity".into());
    }

    let tracks = read_cached_tracks(&app)?;
    let (updated, removed) = remove_cached_track(tracks, track_id, target_key.as_deref());
    if removed {
        write_cached_tracks(&app, &updated)?;
    }

    Ok(removed)
}

#[tauri::command]
pub fn list_devices() -> Result<Vec<OutputDeviceInfo>, String> {
    let devices = list_output_devices().map_err(|err| err.to_string())?;
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
pub async fn import_tracks(app: AppHandle, paths: Vec<String>) -> Result<Vec<ImportedTrack>, String> {
    // L-18：文件遍历 + lofty 解析 + 可能的 ffprobe 子进程都是阻塞 IO，
    // 放到 spawn_blocking，避免占用 Tauri 命令调度线程、阻塞其它 IPC（含播放控制）。
    tauri::async_runtime::spawn_blocking(move || {
        let mut tracks = Vec::new();
        let mut seen_files = HashSet::new();
        let mut visited_dirs = HashSet::new();

        for path in paths {
            collect_audio_files(
                PathBuf::from(path),
                &mut tracks,
                &mut seen_files,
                &mut visited_dirs,
                0,
            )?;
        }

        if !tracks.is_empty() {
            let cached = read_cached_tracks(&app).unwrap_or_default();
            let merged = merge_cached_tracks(cached, &tracks);
            write_cached_tracks(&app, &merged)?;
            return Ok(imported_tracks_from_cache(&merged, &tracks));
        }

        Ok(tracks)
    })
    .await
    .map_err(|err| format!("import task panicked: {err}"))?
}

#[tauri::command]
pub fn save_track_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics_bytes: Vec<u8>,
    track_path: Option<String>,
) -> Result<Vec<LyricLine>, String> {
    if track_id.trim().is_empty() {
        return Err("missing track id".into());
    }

    if lyrics_bytes.is_empty() {
        return Err("lyrics file is empty".into());
    }

    // 后端独立校验大小：前端虽然限制了 2MB，但 IPC 可被绕过；
    // 设为 4MB 留余量，同时阻挡明显异常输入。
    const MAX_LYRICS_BYTES: usize = 4 * 1024 * 1024;
    if lyrics_bytes.len() > MAX_LYRICS_BYTES {
        return Err(format!(
            "lyrics file too large: {} bytes (limit {})",
            lyrics_bytes.len(),
            MAX_LYRICS_BYTES
        ));
    }

    let lyrics = parse_lyrics_bytes(&lyrics_bytes);
    if lyrics.is_empty() {
        return Err("lyrics file has no usable text".into());
    }

    let mut tracks = read_cached_tracks(&app)?;
    apply_track_lyrics(
        &mut tracks,
        &track_id,
        lyrics.clone(),
        track_path.as_deref(),
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
) -> Result<Vec<OnlineLyricsCandidate>, String> {
    let query = online_lyrics_query(&title, &artist);
    if query.is_empty() {
        return Err("missing track title".into());
    }

    let client = online_lyrics_client()?;
    let candidates = fetch_online_lyrics_from_sources(&client, &query, duration).await;
    if candidates.is_empty() {
        return Err("online lyrics not found".into());
    }

    Ok(candidates)
}

#[tauri::command]
pub fn apply_online_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics: Vec<LyricLine>,
    track_path: Option<String>,
) -> Result<Vec<LyricLine>, String> {
    if track_id.trim().is_empty() {
        return Err("missing track id".into());
    }
    if lyrics.is_empty() {
        return Err("lyrics file has no usable text".into());
    }

    let mut tracks = read_cached_tracks(&app)?;
    apply_track_lyrics(
        &mut tracks,
        &track_id,
        lyrics.clone(),
        track_path.as_deref(),
    )?;
    write_cached_tracks(&app, &tracks)?;

    Ok(lyrics)
}
