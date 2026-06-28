const MAX_IMPORT_RECURSION_DEPTH: usize = 64;

fn collect_audio_files(
    path: PathBuf,
    tracks: &mut Vec<ImportedTrack>,
    seen_files: &mut HashSet<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
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

        let entries = fs::read_dir(&path)
            .map_err(|err| format!("failed to read directory {}: {err}", path.display()))?;

        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            collect_audio_files(entry.path(), tracks, seen_files, visited_dirs, depth + 1)?;
        }
        return Ok(());
    }

    if path.is_file() && is_audio_file(&path) {
        let key = import_dedupe_key(&path);
        if seen_files.insert(key) {
            tracks.push(track_from_path(&path)?);
        }
    }

    Ok(())
}

fn read_cached_tracks(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    let path = library_cache_path(app)?;
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read library cache {}: {err}", path.display()))?;
    let tracks: Vec<ImportedTrack> = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse library cache {}: {err}", path.display()))?;
    Ok(tracks.into_iter().map(enrich_cached_track).collect())
}

fn write_cached_tracks(app: &AppHandle, tracks: &[ImportedTrack]) -> Result<(), String> {
    let path = library_cache_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create library cache dir {}: {err}",
                parent.display()
            )
        })?;
    }
    let bytes = serde_json::to_vec_pretty(tracks)
        .map_err(|err| format!("failed to serialize library cache: {err}"))?;
    fs::write(&path, bytes)
        .map_err(|err| format!("failed to write library cache {}: {err}", path.display()))
}

pub(super) fn merge_tracks_into_cache(
    app: &AppHandle,
    tracks: &[ImportedTrack],
) -> Result<Vec<ImportedTrack>, String> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }

    let cached = read_cached_tracks(app).unwrap_or_default();
    let merged = merge_cached_tracks(cached, tracks);
    write_cached_tracks(app, &merged)?;
    Ok(imported_tracks_from_cache(&merged, tracks))
}

pub(super) fn mark_tracks_cache_missing_by_paths(
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
    let tracks = read_cached_tracks(app).unwrap_or_default();
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

fn library_cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("library-cache.json"))
}

fn merge_cached_tracks(
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

fn remove_cached_track(
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

fn dedupe_cached_tracks(tracks: Vec<ImportedTrack>) -> Vec<ImportedTrack> {
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

fn merge_imported_track(cached: &ImportedTrack, imported: &ImportedTrack) -> ImportedTrack {
    let mut merged = imported.clone();
    if merged.lyrics.is_empty() && !cached.lyrics.is_empty() {
        merged.lyrics = cached.lyrics.clone();
    }
    merged
}

fn enrich_cached_track(mut track: ImportedTrack) -> ImportedTrack {
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

fn bilibili_source_id_from_path(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?.trim();
    let (source_id, _) = stem.rsplit_once('-')?;
    if source_id.len() >= 12 && source_id.get(..2)?.eq_ignore_ascii_case("BV") {
        Some(source_id.to_string())
    } else {
        None
    }
}

fn imported_tracks_from_cache(
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

fn delete_track_request_key(track: &DeleteTrackRequest) -> Option<String> {
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

fn apply_track_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    lyrics: Vec<LyricLine>,
    track_path: Option<&str>,
) -> Result<(), String> {
    let index = ensure_track_for_lyrics(tracks, track_id, track_path)?;
    let track = &mut tracks[index];
    track.lyrics = lyrics;
    Ok(())
}

fn ensure_track_for_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    track_path: Option<&str>,
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

    let mut track = track_from_path(&path)?;
    track.id = track_id.to_string();
    tracks.push(track);
    Ok(tracks.len() - 1)
}

fn import_track_key(track: &ImportedTrack) -> String {
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

fn import_dedupe_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase()
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some()
        || audio_format_from_magic(path).is_some()
}

#[cfg(test)]
fn audio_format_label(path: &Path) -> String {
    audio_format_from_magic(path)
        .or_else(|| {
            path.extension()
                .and_then(|value| value.to_str())
                .and_then(audio_format_from_extension)
        })
        .unwrap_or("AUDIO")
        .to_string()
}

fn audio_format_from_magic(path: &Path) -> Option<&'static str> {
    let mut file = fs::File::open(path).ok()?;
    let mut header = [0_u8; 64];
    let read = file.read(&mut header).ok()?;
    audio_format_from_header(&header[..read])
}

fn audio_format_from_header(header: &[u8]) -> Option<&'static str> {
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

fn audio_format_from_extension(extension: &str) -> Option<&'static str> {
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

fn is_mpeg_audio_header(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && (header[1] & 0xe0) == 0xe0 && (header[1] & 0x06) != 0
}

fn is_adts_header(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && (header[1] & 0xf0) == 0xf0
}

fn is_dsd_format(format: &str) -> bool {
    format.eq_ignore_ascii_case("DSF") || format.eq_ignore_ascii_case("DFF")
}

fn track_from_path(path: &Path) -> Result<ImportedTrack, String> {
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
    let format = magic_format
        .or(ext_format)
        .unwrap_or("AUDIO")
        .to_string();
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

    Ok(ImportedTrack {
        id: format!("local-{hash:016x}"),
        title,
        artist,
        album,
        album_year: audio_metadata.album_year,
        cover: String::new(),
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

fn parse_audio_metadata_with_dsd_hint(path: &Path, is_dsd_hint: bool) -> ParsedAudioMetadata {
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
    enrich_with_decoder_probe_dsd(path, &mut parsed, is_dsd_hint);

    parsed
}

fn enrich_with_decoder_probe_dsd(
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
fn parse_audio_metadata(path: &Path) -> ParsedAudioMetadata {
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
fn enrich_with_decoder_probe(path: &Path, parsed: &mut ParsedAudioMetadata) {
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
fn write_test_dsf(path: &Path) {
    use std::io::Write;

    let channels = 2_u32;
    let dsd_rate = 2_822_400_u32;
    let sample_count = dsd_rate as u64;
    let block_size_per_channel = 8_u32;
    let data_len = channels as u64 * block_size_per_channel as u64;
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
    file.write_all(&[0xff; 8]).unwrap();
    file.write_all(&[0x00; 8]).unwrap();
}

#[cfg(test)]
fn temp_audio_path(prefix: &str, extension: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
}

#[cfg(test)]
fn is_dsd_file(path: &Path) -> bool {
    if let Some(format) = audio_format_from_magic(path) {
        return is_dsd_format(format);
    }

    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some_and(is_dsd_format)
}

fn external_lrc_lyrics(path: &Path) -> Option<Vec<LyricLine>> {
    let lyrics_path = find_lyrics_file(path)?;
    let bytes = fs::read(lyrics_path).ok()?;
    let lyrics = parse_lyrics_bytes(&bytes);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn find_lyrics_file(path: &Path) -> Option<PathBuf> {
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
