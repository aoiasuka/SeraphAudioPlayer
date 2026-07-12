use super::prelude::*;

pub(crate) const MAX_IMPORT_RECURSION_DEPTH: usize = 64;

/// P1-3：曲库缓存"读全量 → 内存合并 → 覆盖写"序列的模块级互斥锁。
/// 所有读改写路径（导入、删除、歌词写入、缓存缺失标记）必须持有它，
/// 防止并发命令互相覆盖丢更新。parking_lot Mutex 只在同步块内持有，
/// async 路径需先进 spawn_blocking 再调用。
pub(crate) static LIBRARY_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// 内存常驻曲库：首次访问从磁盘加载后常驻，读命令（get_playlist / get_track_info）
/// 直接克隆内存快照，不再每次 IPC 全量读盘 + 解析 JSON。写路径落盘成功后同步刷新。
/// 歌词在内存中仍内联于 ImportedTrack（前端契约不变），仅磁盘层拆分为边车文件。
static LIBRARY_MEMORY: parking_lot::RwLock<Option<Vec<ImportedTrack>>> =
    parking_lot::RwLock::new(None);

pub(crate) fn collect_audio_files(
    path: PathBuf,
    tracks: &mut Vec<ImportedTrack>,
    seen_files: &mut HashSet<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
    warnings: &mut Vec<String>,
    covers_dir: Option<&Path>,
) -> Result<(), String> {
    if path.is_dir() {
        // L-14：symlink / Windows junction 可能指向祖先目录导致无限递归栈溢出。
        // 用 canonicalize 后的真实路径去重 + 递归深度上限双重防护。
        if depth >= MAX_IMPORT_RECURSION_DEPTH {
            return Ok(());
        }
        let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !visited_dirs.insert(real) {
            return Ok(());
        }

        // P3-11：单个子目录读失败（如无权限）只记警告并跳过，不中止整批导入。
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(format!("无法读取目录 {}: {err}", path.display()));
                return Ok(());
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!("读取目录项失败 {}: {err}", path.display()));
                    continue;
                }
            };
            collect_audio_files(
                entry.path(),
                tracks,
                seen_files,
                visited_dirs,
                depth + 1,
                warnings,
                covers_dir,
            )?;
        }
        return Ok(());
    }

    if path.is_file() && is_audio_file(&path) {
        let key = import_dedupe_key(&path);
        if seen_files.insert(key) {
            tracks.push(track_from_path(&path, covers_dir)?);
        }
    }

    Ok(())
}

pub(crate) fn read_cached_tracks(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    // 读命令：命中内存直接克隆；未加载则加载一次并常驻。
    if let Some(tracks) = LIBRARY_MEMORY.read().as_ref() {
        return Ok(tracks.clone());
    }
    let loaded = load_tracks_from_disk(app)?;
    let mut guard = LIBRARY_MEMORY.write();
    // 双检：加载期间可能已有写路径填充内存，避免覆盖更新的快照
    if guard.is_none() {
        *guard = Some(loaded);
    }
    Ok(guard.as_ref().cloned().unwrap_or_default())
}

/// 从磁盘加载曲库主文件并合并歌词边车，附加 enrich（补 B 站 source_url）。
/// 主文件不存在视为空库；损坏则返回错误（读命令降级处理）。
fn load_tracks_from_disk(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    let path = library_cache_path(app)?;
    let tracks = read_tracks_from_file(&path)?;
    let lyrics_by_id = read_lyrics_sidecar(app)?;
    // 边车有则覆盖；否则保留主文件内联歌词（兼容旧格式未迁移的库）
    let merged = merge_lyrics_from_storage(tracks, &lyrics_by_id);
    Ok(merged.into_iter().map(enrich_cached_track).collect())
}

pub(crate) fn read_tracks_from_file(path: &Path) -> Result<Vec<ImportedTrack>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read library cache {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse library cache {}: {err}", path.display()))
}

/// 读取歌词边车文件（track_id → 歌词行）。不存在或损坏都返回空表——
/// 歌词丢失可从主文件内联或重新匹配恢复，不阻断曲库读取。
fn read_lyrics_sidecar(app: &AppHandle) -> Result<HashMap<String, Vec<LyricLine>>, String> {
    let path = library_lyrics_path(app)?;
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    match fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(_) => Ok(HashMap::new()),
    }
}

/// P0-2：读改写路径专用读取。持锁调用，返回内存快照（已含歌词与 enrich）。
/// 内存未加载时从磁盘加载；磁盘损坏时把坏文件备份为 `.corrupt` 并显式报错——
/// 绝不能把损坏缓存当空库，否则随后的覆盖写会把用户整个曲库静默清空。
pub(crate) fn read_cached_tracks_for_update(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    if let Some(tracks) = LIBRARY_MEMORY.read().as_ref() {
        return Ok(tracks.clone());
    }
    match load_tracks_from_disk(app) {
        Ok(tracks) => {
            *LIBRARY_MEMORY.write() = Some(tracks.clone());
            Ok(tracks)
        }
        Err(err) => {
            let backup = backup_corrupt_file(&library_cache_path(app)?);
            Err(format!(
                "曲库缓存损坏，已中止写入以免覆盖数据（坏文件已备份到 {}）: {err}",
                backup.display()
            ))
        }
    }
}

/// 把损坏的 JSON 文件备份为 `<原名>.corrupt`（复制而非移动，保留现场）。
pub(crate) fn backup_corrupt_file(path: &Path) -> PathBuf {
    let backup = PathBuf::from(format!("{}.corrupt", path.display()));
    let _ = fs::copy(path, &backup);
    backup
}

pub(crate) fn write_cached_tracks(app: &AppHandle, tracks: &[ImportedTrack]) -> Result<(), String> {
    let path = library_cache_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create library cache dir {}: {err}",
                parent.display()
            )
        })?;
    }

    // 磁盘层拆分：主文件存曲目元数据（歌词字段清空），歌词单独存边车文件。
    // 大歌词不再随每次曲库写入反复序列化，主文件体积与写入成本显著下降。
    let (stripped, lyrics_by_id) = split_lyrics_for_storage(tracks);

    let main_bytes = serde_json::to_vec_pretty(&stripped)
        .map_err(|err| format!("failed to serialize library cache: {err}"))?;
    // P0-2：temp+rename 原子写，避免写一半崩溃/断电截断 JSON 丢曲库。
    write_json_atomic(&path, &main_bytes)?;

    let lyrics_path = library_lyrics_path(app)?;
    let lyrics_bytes = serde_json::to_vec(&lyrics_by_id)
        .map_err(|err| format!("failed to serialize lyrics sidecar: {err}"))?;
    write_json_atomic(&lyrics_path, &lyrics_bytes)?;

    // 落盘成功后刷新内存快照（存完整含歌词版本，读命令零成本命中）
    *LIBRARY_MEMORY.write() = Some(tracks.to_vec());
    Ok(())
}

/// 把曲目拆成「歌词清空的主记录」+「track_id → 歌词」映射，供磁盘分离存储。
/// 纯函数，便于测试往返一致性。
pub(crate) fn split_lyrics_for_storage(
    tracks: &[ImportedTrack],
) -> (Vec<ImportedTrack>, HashMap<String, Vec<LyricLine>>) {
    let mut lyrics_by_id = HashMap::new();
    let stripped = tracks
        .iter()
        .map(|track| {
            if !track.lyrics.is_empty() {
                lyrics_by_id.insert(track.id.clone(), track.lyrics.clone());
            }
            ImportedTrack {
                lyrics: Vec::new(),
                ..track.clone()
            }
        })
        .collect();
    (stripped, lyrics_by_id)
}

/// 把歌词边车按 track_id 合并回主记录（读盘时用）。
pub(crate) fn merge_lyrics_from_storage(
    mut tracks: Vec<ImportedTrack>,
    lyrics_by_id: &HashMap<String, Vec<LyricLine>>,
) -> Vec<ImportedTrack> {
    for track in &mut tracks {
        if let Some(lyrics) = lyrics_by_id.get(&track.id) {
            track.lyrics = lyrics.clone();
        }
    }
    tracks
}

/// 原子写 JSON：先写同目录临时文件再 rename（Windows 同卷 rename 原子）。
pub(crate) fn write_json_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&tmp, bytes)
        .map_err(|err| format!("failed to write temp file {}: {err}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        format!("failed to replace {}: {err}", path.display())
    })
}

pub(crate) fn merge_tracks_into_cache(
    app: &AppHandle,
    tracks: &[ImportedTrack],
) -> Result<Vec<ImportedTrack>, String> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }

    let _guard = LIBRARY_LOCK.lock();
    let cached = read_cached_tracks_for_update(app)?;
    let merged = merge_cached_tracks(cached, tracks);
    write_cached_tracks(app, &merged)?;
    Ok(imported_tracks_from_cache(&merged, tracks))
}

pub(crate) fn mark_tracks_cache_missing_by_paths(
    app: &AppHandle,
    removed_paths: &[PathBuf],
) -> Result<(), String> {
    if removed_paths.is_empty() {
        return Ok(());
    }

    let removed = removed_paths
        .iter()
        .map(|path| import_dedupe_key(path))
        .collect::<HashSet<_>>();
    let _guard = LIBRARY_LOCK.lock();
    let tracks = read_cached_tracks_for_update(app)?;
    let updated = tracks
        .into_iter()
        .map(|mut track| {
            // 缓存清理传来的是 *实际磁盘文件路径*。
            // 之前用 `import_track_key(&track)` 比对，但对于 Bilibili 曲目
            // 它返回的是 `source-id:bv...` 而非文件路径，永远无法命中。
            // 改用 track.path 直接归一化，确保物理路径匹配。
            let track_key = import_dedupe_key(Path::new(&track.path));
            if removed.contains(&track_key) && track.source_url.is_some() {
                track.cache_missing = true;
                track.size = "0 MB".into();
            }
            track
        })
        .collect::<Vec<_>>();
    write_cached_tracks(app, &updated)
}

pub(crate) fn library_cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("library-cache.json"))
}

/// 歌词边车文件路径：与主曲库文件同目录，track_id → 歌词行的 JSON 映射。
pub(crate) fn library_lyrics_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("library-lyrics.json"))
}

pub(crate) fn merge_cached_tracks(
    mut cached: Vec<ImportedTrack>,
    imported: &[ImportedTrack],
) -> Vec<ImportedTrack> {
    let mut index_by_key = HashMap::with_capacity(cached.len());
    for (index, track) in cached.iter().enumerate() {
        index_by_key.insert(import_track_key(track), index);
    }

    for track in imported {
        let key = import_track_key(track);
        if let Some(index) = index_by_key.get(&key).copied() {
            cached[index] = merge_imported_track(&cached[index], track);
        } else {
            index_by_key.insert(key, cached.len());
            cached.push(track.clone());
        }
    }

    dedupe_cached_tracks(cached)
}

pub(crate) fn remove_cached_track(
    tracks: Vec<ImportedTrack>,
    track_id: &str,
    target_key: Option<&str>,
) -> (Vec<ImportedTrack>, bool) {
    let before = tracks.len();
    let updated = tracks
        .into_iter()
        .filter(|track| {
            let id_matches = !track_id.is_empty() && track.id == track_id;
            let key_matches = target_key.is_some_and(|key| import_track_key(track) == key);
            !(id_matches || key_matches)
        })
        .collect::<Vec<_>>();

    let removed = updated.len() != before;
    (updated, removed)
}

pub(crate) fn dedupe_cached_tracks(tracks: Vec<ImportedTrack>) -> Vec<ImportedTrack> {
    let mut output: Vec<ImportedTrack> = Vec::new();
    let mut index_by_key: HashMap<String, usize> = HashMap::new();

    for track in tracks {
        let key = import_track_key(&track);
        if let Some(index) = index_by_key.get(&key).copied() {
            let existing = &output[index];
            output[index] = if existing.cache_missing && !track.cache_missing {
                merge_imported_track(existing, &track)
            } else {
                merge_imported_track(&track, existing)
            };
        } else {
            index_by_key.insert(key, output.len());
            output.push(track);
        }
    }

    output
}

pub(crate) fn merge_imported_track(
    cached: &ImportedTrack,
    imported: &ImportedTrack,
) -> ImportedTrack {
    let mut merged = imported.clone();
    if merged.lyrics.is_empty() && !cached.lyrics.is_empty() {
        merged.lyrics = cached.lyrics.clone();
    }
    merged
}

pub(crate) fn enrich_cached_track(mut track: ImportedTrack) -> ImportedTrack {
    if track.source_url.is_none() && track.id.starts_with("bilibili-") {
        if let Some(source_id) = track
            .source_id
            .clone()
            .or_else(|| bilibili_source_id_from_path(&track.path))
        {
            track.source_url = Some(format!("https://www.bilibili.com/video/{source_id}"));
            track.source_id = Some(source_id);
        }
    }
    track
}

pub(crate) fn bilibili_source_id_from_path(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?.trim();
    let (source_id, _) = stem.rsplit_once('-')?;
    if source_id.len() >= 12 && source_id.get(..2)?.eq_ignore_ascii_case("BV") {
        Some(source_id.to_string())
    } else {
        None
    }
}

pub(crate) fn imported_tracks_from_cache(
    cached: &[ImportedTrack],
    imported: &[ImportedTrack],
) -> Vec<ImportedTrack> {
    let cached_by_key = cached
        .iter()
        .map(|track| (import_track_key(track), track.clone()))
        .collect::<HashMap<_, _>>();

    imported
        .iter()
        .map(|track| {
            cached_by_key
                .get(&import_track_key(track))
                .cloned()
                .unwrap_or_else(|| track.clone())
        })
        .collect()
}

pub(crate) fn delete_track_request_key(track: &DeleteTrackRequest) -> Option<String> {
    if let Some(source_id) = track
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("source-id:{}", source_id.to_ascii_lowercase()));
    }

    if let Some(source_url) = track
        .source_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("source-url:{}", source_url.to_ascii_lowercase()));
    }

    let path = track.path.trim();
    if path.is_empty() {
        None
    } else {
        Some(import_dedupe_key(Path::new(path)))
    }
}

pub(crate) fn apply_track_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    lyrics: Vec<LyricLine>,
    track_path: Option<&str>,
    covers_dir: Option<&Path>,
) -> Result<(), String> {
    let index = ensure_track_for_lyrics(tracks, track_id, track_path, covers_dir)?;
    let track = &mut tracks[index];
    track.lyrics = lyrics;
    Ok(())
}

pub(crate) fn ensure_track_for_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    track_path: Option<&str>,
    covers_dir: Option<&Path>,
) -> Result<usize, String> {
    if let Some(index) = tracks.iter().position(|track| track.id == track_id) {
        return Ok(index);
    }

    let Some(track_path) = track_path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Err("track was not found in the library cache".into());
    };

    let path = PathBuf::from(track_path);
    if !path.is_file() {
        return Err(
            "track is not in the library cache and the audio file is unavailable; re-import the audio file first"
                .into(),
        );
    }

    // 歌词保存时曲目不在缓存里才走到这里重建条目（罕见），封面同样在此提取
    let mut track = track_from_path(&path, covers_dir)?;
    track.id = track_id.to_string();
    tracks.push(track);
    Ok(tracks.len() - 1)
}

pub(crate) fn import_track_key(track: &ImportedTrack) -> String {
    if let Some(source_id) = track
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("source-id:{}", source_id.to_ascii_lowercase());
    }
    if let Some(source_url) = track
        .source_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("source-url:{}", source_url.to_ascii_lowercase());
    }
    import_dedupe_key(Path::new(&track.path))
}

pub(crate) fn import_dedupe_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase()
}

pub(crate) fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some()
        || audio_format_from_magic(path).is_some()
}

#[cfg(test)]
pub(crate) fn audio_format_label(path: &Path) -> String {
    audio_format_from_magic(path)
        .or_else(|| {
            path.extension()
                .and_then(|value| value.to_str())
                .and_then(audio_format_from_extension)
        })
        .unwrap_or("AUDIO")
        .to_string()
}

pub(crate) fn audio_format_from_magic(path: &Path) -> Option<&'static str> {
    let mut file = fs::File::open(path).ok()?;
    let mut header = [0_u8; 64];
    let read = file.read(&mut header).ok()?;
    audio_format_from_header(&header[..read])
}

pub(crate) fn audio_format_from_header(header: &[u8]) -> Option<&'static str> {
    if header.starts_with(b"DSD ") {
        return Some("DSF");
    }
    if header.len() >= 16 && &header[0..4] == b"FRM8" && &header[12..16] == b"DSD " {
        return Some("DFF");
    }
    if header.starts_with(b"fLaC") {
        return Some("FLAC");
    }
    if header.len() >= 12 && &header[0..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        return Some("WAV");
    }
    if header.len() >= 12
        && &header[0..4] == b"FORM"
        && (&header[8..12] == b"AIFF" || &header[8..12] == b"AIFC")
    {
        return Some("AIFF");
    }
    if header.starts_with(b"ID3") || is_mpeg_audio_header(header) {
        return Some("MP3");
    }
    if header.starts_with(b"OggS") {
        return Some(if header.windows(8).any(|window| window == b"OpusHead") {
            "OPUS"
        } else {
            "OGG"
        });
    }
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        return Some(if header.windows(4).any(|window| window == b"alac") {
            "ALAC"
        } else {
            "M4A"
        });
    }
    if is_adts_header(header) {
        return Some("AAC");
    }

    None
}

pub(crate) fn audio_format_from_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "aac" => Some("AAC"),
        "aif" | "aiff" => Some("AIFF"),
        "alac" => Some("ALAC"),
        "dff" => Some("DFF"),
        "dsf" => Some("DSF"),
        "flac" => Some("FLAC"),
        "m4a" => Some("M4A"),
        "mp3" => Some("MP3"),
        "ogg" => Some("OGG"),
        "opus" => Some("OPUS"),
        "wav" => Some("WAV"),
        "wma" => Some("WMA"),
        _ => None,
    }
}

pub(crate) fn is_mpeg_audio_header(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && (header[1] & 0xe0) == 0xe0 && (header[1] & 0x06) != 0
}

pub(crate) fn is_adts_header(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && (header[1] & 0xf0) == 0xf0
}

pub(crate) fn is_dsd_format(format: &str) -> bool {
    format.eq_ignore_ascii_case("DSF") || format.eq_ignore_ascii_case("DFF")
}

pub(crate) fn track_from_path(
    path: &Path,
    covers_dir: Option<&Path>,
) -> Result<ImportedTrack, String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("failed to read file metadata {}: {err}", path.display()))?;
    let path_string = path.to_string_lossy().to_string();
    let mut hasher = DefaultHasher::new();
    path_string.to_ascii_lowercase().hash(&mut hasher);
    let hash = hasher.finish();
    // L-2: 一次性探测 magic（带最多 64 字节读取），后续 format/is_dsd 沿用结果。
    let magic_format = audio_format_from_magic(path);
    let ext_format = path
        .extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension);
    let format = magic_format.or(ext_format).unwrap_or("AUDIO").to_string();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Unknown Track");
    let is_dsd = magic_format
        .map(is_dsd_format)
        .unwrap_or_else(|| ext_format.is_some_and(is_dsd_format));
    let audio_metadata = parse_audio_metadata_with_dsd_hint(path, is_dsd);
    let lyrics = external_lrc_lyrics(path).unwrap_or_else(|| audio_metadata.lyrics.clone());
    let filename_metadata = parse_filename_metadata(stem);
    let title = audio_metadata
        .title
        .or(filename_metadata.title)
        .unwrap_or_else(|| clean_metadata_text(stem).unwrap_or_else(|| "Unknown Track".into()));
    let artist = audio_metadata
        .artist
        .or(filename_metadata.artist)
        .unwrap_or_else(|| "Unknown Artist".into());
    let album = audio_metadata
        .album
        .or(filename_metadata.album)
        .unwrap_or_else(|| "Local Files".into());
    let size = format_file_size(metadata.len());
    let (glow1, glow2) = color_pair(hash);
    // 内嵌封面按内容哈希落盘到 covers 目录（同专辑多曲共用一张图），
    // cover 存文件绝对路径，前端经 asset 协议加载；无封面保持空串。
    let cover = audio_metadata
        .cover
        .as_ref()
        .zip(covers_dir)
        .and_then(|(art, dir)| save_cover_art(dir, art))
        .unwrap_or_default();

    Ok(ImportedTrack {
        id: format!("local-{hash:016x}"),
        title,
        artist,
        album,
        album_year: audio_metadata.album_year,
        cover,
        format: format.clone(),
        bitdepth: format_audio_quality(
            &format,
            audio_metadata.bit_depth,
            audio_metadata.sample_rate,
        ),
        sample_rate: format_sample_rate(&format, audio_metadata.sample_rate),
        bitrate: format_bitrate(audio_metadata.bitrate),
        channels: format_channels(audio_metadata.channels),
        size,
        path: path_string,
        source_url: None,
        source_id: None,
        cache_missing: false,
        duration: audio_metadata.duration.unwrap_or(0),
        glow_color: glow1.clone(),
        glow1,
        glow2,
        lyrics,
    })
}

pub(crate) fn parse_audio_metadata_with_dsd_hint(
    path: &Path,
    is_dsd_hint: bool,
) -> ParsedAudioMetadata {
    let Ok(tagged_file) = lofty::read_from_path(path) else {
        let mut parsed = ParsedAudioMetadata::default();
        enrich_with_decoder_probe_dsd(path, &mut parsed, is_dsd_hint);
        return parsed;
    };

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs();

    let mut parsed = ParsedAudioMetadata {
        duration: (duration > 0).then_some(duration),
        bitrate: properties
            .audio_bitrate()
            .or_else(|| properties.overall_bitrate()),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth(),
        channels: properties.channels(),
        ..ParsedAudioMetadata::default()
    };

    if let Some(tag) = tag {
        parsed.title = tag
            .title()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.artist = tag
            .artist()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album = tag
            .album()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album_year = tag
            .date()
            .and_then(|value| (value.year > 0).then(|| value.year.to_string()));
    }

    parsed.lyrics = lyrics_from_tags(tagged_file.tags());
    parsed.cover = cover_art_from_tags(tagged_file.tags());
    enrich_with_decoder_probe_dsd(path, &mut parsed, is_dsd_hint);

    parsed
}

/// 从标签中选内嵌封面：优先 CoverFront，否则取第一张非空图片。
pub(crate) fn cover_art_from_tags(tags: &[Tag]) -> Option<CoverArt> {
    let mut chosen = None;
    for picture in tags.iter().flat_map(|tag| tag.pictures()) {
        if picture.data().is_empty() {
            continue;
        }
        if picture.pic_type() == PictureType::CoverFront {
            chosen = Some(picture);
            break;
        }
        if chosen.is_none() {
            chosen = Some(picture);
        }
    }
    let picture = chosen?;
    let ext = cover_image_extension(picture.mime_type(), picture.data())?;
    Some(CoverArt {
        data: picture.data().to_vec(),
        ext,
    })
}

/// 由 MIME 或图片魔数推断扩展名；识别不了的类型不落盘（返回 None）。
pub(crate) fn cover_image_extension(mime: Option<&MimeType>, data: &[u8]) -> Option<&'static str> {
    match mime {
        Some(MimeType::Jpeg) => return Some("jpg"),
        Some(MimeType::Png) => return Some("png"),
        Some(MimeType::Bmp) => return Some("bmp"),
        Some(MimeType::Gif) => return Some("gif"),
        Some(MimeType::Tiff) => return Some("tiff"),
        _ => {}
    }
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if data.starts_with(b"GIF8") {
        Some("gif")
    } else if data.starts_with(b"BM") {
        Some("bmp")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// 封面落盘序号：并发导入时保证临时文件名互不相同。
pub(crate) static COVER_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 封面按内容哈希写入 covers 目录并返回绝对路径；已存在同内容文件直接复用。
pub(crate) fn save_cover_art(covers_dir: &Path, art: &CoverArt) -> Option<String> {
    let mut hasher = DefaultHasher::new();
    art.data.hash(&mut hasher);
    let content_hash = hasher.finish();
    let target = covers_dir.join(format!("{content_hash:016x}.{}", art.ext));

    if !target.is_file() {
        fs::create_dir_all(covers_dir).ok()?;
        // 临时文件 + rename 原子落盘；rename 失败但目标已存在说明并发写入者已完成
        let seq = COVER_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = covers_dir.join(format!(".cover-{content_hash:016x}-{seq}.tmp"));
        fs::write(&tmp, &art.data).ok()?;
        if fs::rename(&tmp, &target).is_err() {
            let _ = fs::remove_file(&tmp);
            if !target.is_file() {
                return None;
            }
        }
    }

    Some(target.to_string_lossy().to_string())
}

pub(crate) fn covers_dir_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("covers"))
}

/// 只读标签提取封面并落盘（启动补扫用，跳过 ffprobe 等重探测）。
pub(crate) fn extract_embedded_cover(path: &Path, covers_dir: &Path) -> Option<String> {
    let tagged_file = lofty::read_from_path(path).ok()?;
    let art = cover_art_from_tags(tagged_file.tags())?;
    save_cover_art(covers_dir, &art)
}

/// 旧版曲库缓存里的本地曲目没有封面（当时不提取）。启动时一次性补扫
/// cover 为空且文件仍存在的本地曲目，完成后写标记文件，后续启动零成本；
/// 新导入的曲目在导入时即提取封面，不依赖本流程。
pub(crate) fn backfill_missing_covers(app: &AppHandle) {
    let Ok(covers_dir) = covers_dir_path(app) else {
        return;
    };
    let marker = covers_dir.join(".cover-backfill-v1");
    if marker.is_file() {
        return;
    }

    let _guard = LIBRARY_LOCK.lock();
    let Ok(mut tracks) = read_cached_tracks_for_update(app) else {
        // 缓存损坏时不写标记，等缓存恢复后下次启动重试
        return;
    };

    let mut changed = false;
    for track in &mut tracks {
        // B 站曲目封面来自视频封面 URL，这里只补本地文件
        if !track.cover.is_empty() || track.source_url.is_some() {
            continue;
        }
        let path = Path::new(&track.path);
        if !path.is_file() {
            continue;
        }
        if let Some(cover) = extract_embedded_cover(path, &covers_dir) {
            track.cover = cover;
            changed = true;
        }
    }

    if changed && write_cached_tracks(app, &tracks).is_err() {
        // 写失败不落标记，下次启动重试
        return;
    }
    if fs::create_dir_all(&covers_dir).is_ok() {
        let _ = fs::write(&marker, b"v1");
    }
}

/// covers 目录孤儿封面 GC：删除不再被任何曲目引用的封面文件（曲目删除后
/// 内容哈希共享的封面会残留）。
/// - 持 LIBRARY_LOCK 取最新曲库快照，避免与并发导入的读改写竞态；
/// - 1 小时宽限期：导入流程先落盘封面、后持锁入库，刚写入尚未入库的
///   新封面不会被误删；
/// - 每个进程生命周期只跑一次（启动后首次 get_playlist 触发）。
pub(crate) fn gc_orphan_covers(app: &AppHandle) {
    use std::sync::atomic::AtomicBool;
    static GC_DONE: AtomicBool = AtomicBool::new(false);
    if GC_DONE.swap(true, Ordering::SeqCst) {
        return;
    }

    let Ok(covers_dir) = covers_dir_path(app) else {
        return;
    };
    if !covers_dir.is_dir() {
        return;
    }

    let _guard = LIBRARY_LOCK.lock();
    let Ok(tracks) = read_cached_tracks_for_update(app) else {
        return;
    };
    let referenced: HashSet<String> = tracks
        .iter()
        .filter(|track| !track.cover.is_empty() && !track.cover.starts_with("http"))
        .map(|track| normalize_cover_key(&track.cover))
        .collect();

    let Ok(entries) = fs::read_dir(&covers_dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // 标记文件（.cover-backfill-v1 等点前缀且非 .tmp 残留）保留
        if name.starts_with('.') && !name.ends_with(".tmp") {
            continue;
        }
        if referenced.contains(&normalize_cover_key(&path.to_string_lossy())) {
            continue;
        }
        let recently_modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|age| age < Duration::from_secs(3600))
            .unwrap_or(true);
        if recently_modified {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!("cover GC: removed {removed} orphan cover file(s)");
    }
}

/// 封面路径归一化（大小写与分隔符不敏感），用于引用集合比较。
pub(crate) fn normalize_cover_key(path: &str) -> String {
    path.to_ascii_lowercase().replace('/', "\\")
}

pub(crate) fn enrich_with_decoder_probe_dsd(
    path: &Path,
    parsed: &mut ParsedAudioMetadata,
    is_dsd_hint: bool,
) {
    if parsed.duration.is_some()
        && parsed.bit_depth.is_some()
        && parsed.sample_rate.is_some()
        && parsed.channels.is_some()
        && !is_dsd_hint
    {
        return;
    }

    let Ok(info) = probe_stream_info(path) else {
        return;
    };

    if parsed.duration.is_none() && info.duration_seconds > 0.0 {
        parsed.duration = Some(info.duration_seconds.round() as u64);
    }
    if parsed.bit_depth.is_none() && info.bit_depth.0 <= u8::MAX as u16 {
        parsed.bit_depth = Some(info.bit_depth.0 as u8);
    }
    if parsed.sample_rate.is_none() && info.sample_rate.0 > 0 {
        parsed.sample_rate = Some(info.sample_rate.0);
    }
    if parsed.channels.is_none() && info.channels.0 <= u8::MAX as u16 {
        parsed.channels = Some(info.channels.0 as u8);
    }
}

#[cfg(test)]
pub(crate) fn parse_audio_metadata(path: &Path) -> ParsedAudioMetadata {
    let Ok(tagged_file) = lofty::read_from_path(path) else {
        let mut parsed = ParsedAudioMetadata::default();
        enrich_with_decoder_probe(path, &mut parsed);
        return parsed;
    };

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs();

    let mut parsed = ParsedAudioMetadata {
        duration: (duration > 0).then_some(duration),
        bitrate: properties
            .audio_bitrate()
            .or_else(|| properties.overall_bitrate()),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth(),
        channels: properties.channels(),
        ..ParsedAudioMetadata::default()
    };

    if let Some(tag) = tag {
        parsed.title = tag
            .title()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.artist = tag
            .artist()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album = tag
            .album()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album_year = tag
            .date()
            .and_then(|value| (value.year > 0).then(|| value.year.to_string()));
    }

    parsed.lyrics = lyrics_from_tags(tagged_file.tags());
    enrich_with_decoder_probe(path, &mut parsed);

    parsed
}

#[cfg(test)]
pub(crate) fn enrich_with_decoder_probe(path: &Path, parsed: &mut ParsedAudioMetadata) {
    if parsed.duration.is_some()
        && parsed.bit_depth.is_some()
        && parsed.sample_rate.is_some()
        && parsed.channels.is_some()
        && !is_dsd_file(path)
    {
        return;
    }

    let Ok(info) = probe_stream_info(path) else {
        return;
    };

    if parsed.duration.is_none() && info.duration_seconds > 0.0 {
        parsed.duration = Some(info.duration_seconds.round() as u64);
    }
    if parsed.bit_depth.is_none() && info.bit_depth.0 <= u8::MAX as u16 {
        parsed.bit_depth = Some(info.bit_depth.0 as u8);
    }
    if parsed.sample_rate.is_none() && info.sample_rate.0 > 0 {
        parsed.sample_rate = Some(info.sample_rate.0);
    }
    if parsed.channels.is_none() && info.channels.0 <= u8::MAX as u16 {
        parsed.channels = Some(info.channels.0 as u8);
    }
}

#[cfg(test)]
pub(crate) fn write_test_dsf(path: &Path) {
    use std::io::Write;

    let channels = 2_u32;
    let dsd_rate = 2_822_400_u32;
    let sample_count = dsd_rate as u64;
    let block_size_per_channel = 8_u32;
    // 审2：数据体必须与 sample_count 自洽（每声道 sample_count/8 字节）——
    // 解码器现在会把声明时长与文件真实大小交叉校验，虚大的 sample_count
    // 不再被信任（此前夹具只写 16 字节数据却声称 1 秒时长）。
    let bytes_per_channel = sample_count / 8;
    let data_len = channels as u64 * bytes_per_channel;
    let file_size = 28_u64 + 52 + 12 + data_len;

    let mut file = fs::File::create(path).expect("create dsf");
    file.write_all(b"DSD ").unwrap();
    file.write_all(&28_u64.to_le_bytes()).unwrap();
    file.write_all(&file_size.to_le_bytes()).unwrap();
    file.write_all(&0_u64.to_le_bytes()).unwrap();

    file.write_all(b"fmt ").unwrap();
    file.write_all(&52_u64.to_le_bytes()).unwrap();
    file.write_all(&1_u32.to_le_bytes()).unwrap();
    file.write_all(&0_u32.to_le_bytes()).unwrap();
    file.write_all(&2_u32.to_le_bytes()).unwrap();
    file.write_all(&channels.to_le_bytes()).unwrap();
    file.write_all(&dsd_rate.to_le_bytes()).unwrap();
    file.write_all(&1_u32.to_le_bytes()).unwrap();
    file.write_all(&sample_count.to_le_bytes()).unwrap();
    file.write_all(&block_size_per_channel.to_le_bytes())
        .unwrap();
    file.write_all(&0_u32.to_le_bytes()).unwrap();

    file.write_all(b"data").unwrap();
    file.write_all(&(12_u64 + data_len).to_le_bytes()).unwrap();
    // 0x69 = 01101001，DSD 静音位型
    let body = vec![0x69_u8; data_len as usize];
    file.write_all(&body).unwrap();
}

#[cfg(test)]
pub(crate) fn temp_audio_path(prefix: &str, extension: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
}

#[cfg(test)]
pub(crate) fn is_dsd_file(path: &Path) -> bool {
    if let Some(format) = audio_format_from_magic(path) {
        return is_dsd_format(format);
    }

    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some_and(is_dsd_format)
}

pub(crate) fn external_lrc_lyrics(path: &Path) -> Option<Vec<LyricLine>> {
    let lyrics_path = find_lyrics_file(path)?;
    let bytes = fs::read(lyrics_path).ok()?;
    let lyrics = parse_lyrics_bytes(&bytes);
    (!lyrics.is_empty()).then_some(lyrics)
}

pub(crate) fn find_lyrics_file(path: &Path) -> Option<PathBuf> {
    for extension in ["lrc", "qrc", "krc", "yrc"] {
        let exact = path.with_extension(extension);
        if exact.is_file() {
            return Some(exact);
        }
    }

    let expected_stem = path.file_stem()?.to_string_lossy().to_lowercase();
    let parent = path.parent()?;
    let entries = fs::read_dir(parent).ok()?;

    for entry in entries.flatten() {
        let candidate = entry.path();
        let is_lyrics = candidate
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| {
                ["lrc", "qrc", "krc", "yrc"]
                    .iter()
                    .any(|lyrics_ext| ext.eq_ignore_ascii_case(lyrics_ext))
            });
        let same_stem = candidate
            .file_stem()
            .map(|value| value.to_string_lossy().to_lowercase() == expected_stem)
            .unwrap_or(false);

        if is_lyrics && same_stem {
            return Some(candidate);
        }
    }

    None
}
