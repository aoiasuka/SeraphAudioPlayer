use super::prelude::*;
use super::snapshot::LibrarySnapshot;
use super::storage::{LibraryStorage, LoadFailureMemo};
use std::sync::Arc;

pub(crate) const MAX_IMPORT_RECURSION_DEPTH: usize = 64;

/// P1-3：曲库缓存"读全量 → 内存合并 → 覆盖写"序列的模块级互斥锁。
/// 所有读改写路径（导入、删除、歌词写入、缓存缺失标记）必须持有它，
/// 防止并发命令互相覆盖丢更新。parking_lot Mutex 只在同步块内持有，
/// async 路径需先进 spawn_blocking 再调用。
pub(crate) static LIBRARY_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// 读侧只在取 Arc 时持锁；单曲查询通过快照索引定位，不复制其他曲目和歌词。
static LIBRARY_MEMORY: parking_lot::RwLock<Option<Arc<LibrarySnapshot>>> =
    parking_lot::RwLock::new(None);

/// STO-01：曲库无法恢复时内存快照一直是空的，每条命令都会走到冷读。
/// 存储文件没变就复用上一次的失败结果，不再重读磁盘（见 `storage::LoadFailureMemo`）。
static LIBRARY_LOAD_FAILURE: LoadFailureMemo = LoadFailureMemo::new();

/// 一次导入的遍历状态：扫出的曲目、警告、去重集合，以及按目录缓存的歌词 sidecar 索引
/// （LIB-02：索引跟着这一次导入走，导入结束即丢弃，不会读到过期的目录内容）。
pub(crate) struct ImportScan<'a> {
    pub(crate) tracks: Vec<ImportedTrack>,
    /// P3-11：单个子目录或文件读失败只累积警告，不中止整批导入。
    pub(crate) warnings: Vec<String>,
    seen_files: HashSet<String>,
    visited_dirs: HashSet<PathBuf>,
    covers_dir: Option<&'a Path>,
    sidecars: SidecarLyricsIndex,
}

impl<'a> ImportScan<'a> {
    /// `covers_dir` 为空时不提取内嵌封面（拿不到应用数据目录时的降级）。
    pub(crate) fn new(covers_dir: Option<&'a Path>) -> Self {
        Self {
            tracks: Vec::new(),
            warnings: Vec::new(),
            seen_files: HashSet::new(),
            visited_dirs: HashSet::new(),
            covers_dir,
            sidecars: SidecarLyricsIndex::default(),
        }
    }

    /// 收集一个文件，或递归收集一个目录下的全部音频文件。
    pub(crate) fn collect(&mut self, path: PathBuf) {
        self.collect_at(path, 0);
    }

    fn collect_at(&mut self, path: PathBuf, depth: usize) {
        if path.is_dir() {
            // L-14：symlink / Windows junction 可能指向祖先目录导致无限递归栈溢出。
            // 用 canonicalize 后的真实路径去重 + 递归深度上限双重防护。
            if depth >= MAX_IMPORT_RECURSION_DEPTH {
                return;
            }
            let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !self.visited_dirs.insert(real) {
                return;
            }

            // P3-11：单个子目录读失败（如无权限）只记警告并跳过，不中止整批导入。
            let entries = match fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(err) => {
                    self.warnings
                        .push(format!("无法读取目录 {}: {err}", path.display()));
                    return;
                }
            };

            for entry in entries {
                match entry {
                    Ok(entry) => self.collect_at(entry.path(), depth + 1),
                    Err(err) => self
                        .warnings
                        .push(format!("读取目录项失败 {}: {err}", path.display())),
                }
            }
            return;
        }

        if path.is_file() && is_audio_file(&path) {
            let key = import_dedupe_key(&path);
            if self.seen_files.insert(key) {
                let sidecar_lyrics = self
                    .sidecars
                    .find(&path)
                    .and_then(|sidecar| read_sidecar_lyrics(&sidecar));
                match track_from_path_with_lyrics(&path, self.covers_dir, sidecar_lyrics) {
                    Ok(track) => self.tracks.push(track),
                    Err(error) => self
                        .warnings
                        .push(format!("无法导入 {}：{error}", path.display())),
                }
            }
        }
    }

    /// 这一次导入为找歌词 sidecar 枚举过多少个目录（诊断日志用）。
    pub(crate) fn sidecar_dirs_scanned(&self) -> usize {
        self.sidecars.dirs.len()
    }
}

pub(crate) fn read_cached_track(
    app: &AppHandle,
    track_id: &str,
) -> Result<Option<ImportedTrack>, String> {
    Ok(read_library_snapshot(app)?.get(track_id).cloned())
}

pub(crate) fn read_library_snapshot(app: &AppHandle) -> Result<Arc<LibrarySnapshot>, String> {
    if let Some(snapshot) = LIBRARY_MEMORY.read().as_ref() {
        return Ok(Arc::clone(snapshot));
    }
    // 冷读取与写入/恢复/快照清理串行化，不能在读两个文件之间删掉旧版本。
    let _guard = LIBRARY_LOCK.lock();
    read_library_snapshot_for_update(app)
}

/// 曲库里是否有哪首曲目的文件路径就是 `path`。**不碰文件系统**：只比较归一化后的字符串
/// （ASCII 大小写、正反斜杠），所以 `path` 是网络共享路径时也不会因为这次查询连出去。
/// 曲库读不出来按「没有」处理。SEC3-01 用它决定一个网络路径能不能交给资源管理器。
pub(crate) fn library_references_path(app: &AppHandle, path: &Path) -> bool {
    read_library_snapshot(app).is_ok_and(|snapshot| tracks_reference_path(&snapshot.tracks, path))
}

pub(crate) fn tracks_reference_path(tracks: &[ImportedTrack], path: &Path) -> bool {
    let wanted = normalize_cover_key(&path.to_string_lossy());
    tracks
        .iter()
        .any(|track| normalize_cover_key(&track.path) == wanted)
}

/// 曲库里已有曲目所在的网络主机（小写）。同样不碰文件系统，只看路径的写法。
/// PL-01 用它判断清单里的网络共享条目指向的是不是用户此前自己导入过的主机；
/// 曲库读不出来按「没有」处理（结果是多跳过，不是多放行）。
pub(crate) fn library_network_servers(app: &AppHandle) -> HashSet<String> {
    read_library_snapshot(app)
        .map(|snapshot| network_servers_of(&snapshot.tracks))
        .unwrap_or_default()
}

pub(crate) fn network_servers_of(tracks: &[ImportedTrack]) -> HashSet<String> {
    use crate::ipc::path_guard::{path_location, PathLocation};
    tracks
        .iter()
        .filter_map(|track| match path_location(Path::new(&track.path)) {
            PathLocation::Network { server, .. } if !server.is_empty() => Some(server),
            _ => None,
        })
        .collect()
}

fn read_library_snapshot_for_update(app: &AppHandle) -> Result<Arc<LibrarySnapshot>, String> {
    if let Some(snapshot) = LIBRARY_MEMORY.read().as_ref() {
        return Ok(Arc::clone(snapshot));
    }
    let loaded = Arc::new(LibrarySnapshot::new(load_tracks_from_disk(app)?));
    let mut guard = LIBRARY_MEMORY.write();
    // 双检：加载期间可能已有写路径填充内存，避免覆盖更新的快照
    Ok(Arc::clone(guard.get_or_insert(loaded)))
}

/// 从磁盘加载曲库主文件并合并歌词边车，附加 enrich（补 B 站 source_url）。
/// 主文件不存在视为空库；损坏则返回错误（读命令降级处理）。
fn load_tracks_from_disk(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    let path = library_cache_path(app)?;
    let storage = LibraryStorage::new(path.parent().expect("library cache has parent"));
    let tracks = LIBRARY_LOAD_FAILURE.load(&storage)?;
    Ok(tracks.into_iter().map(enrich_cached_track).collect())
}

/// P0-2：读改写路径专用读取。持锁调用，返回内存快照（已含歌词与 enrich）。
/// 内存未加载时从磁盘加载；磁盘损坏时把坏文件备份为 `.corrupt` 并显式报错——
/// 绝不能把损坏缓存当空库，否则随后的覆盖写会把用户整个曲库静默清空。
pub(crate) fn read_cached_tracks_for_update(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    read_library_snapshot_for_update(app).map(|snapshot| snapshot.tracks.clone())
}

/// 提交曲库并刷新内存快照。**接收所有权**（BE-01）：落盘成功后这份数据直接成为新的内存快照，
/// 不再 `to_vec()` 深拷贝一遍全库（含歌词）。调用方写完还要用其中某条记录的，
/// 在调用前把那一条克隆出来。
pub(crate) fn write_cached_tracks(
    app: &AppHandle,
    tracks: Vec<ImportedTrack>,
) -> Result<(), String> {
    let path = library_cache_path(app)?;
    let previous = LIBRARY_MEMORY.read().clone();
    let stats = LibraryStorage::new(path.parent().expect("library cache has parent")).save(
        &tracks,
        previous.as_ref().map(|snapshot| snapshot.tracks.as_slice()),
    )?;
    tracing::debug!(
        metadata_bytes = stats.metadata_bytes,
        lyrics_bytes = stats.lyrics_bytes,
        "曲库快照提交"
    );
    if stats.metadata_bytes == 0 && stats.lyrics_bytes == 0 && previous.is_some() {
        return Ok(());
    }

    // 落盘成功后刷新内存快照（存完整含歌词版本，读命令零成本命中）
    *LIBRARY_MEMORY.write() = Some(Arc::new(LibrarySnapshot::new(tracks)));
    Ok(())
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
    let imported = imported_tracks_from_cache(&merged, tracks);
    write_cached_tracks(app, merged)?;
    Ok(imported)
}

/// 重缓存和删除共享曲库锁，校验与落盘不可分开，否则删除后仍可能重新插入。
pub(crate) fn replace_track_in_cache(
    app: &AppHandle,
    track_id: &str,
    imported: &ImportedTrack,
) -> Result<ImportedTrack, String> {
    let _guard = LIBRARY_LOCK.lock();
    let mut cached = read_cached_tracks_for_update(app)?;
    let updated = replace_cached_track(&mut cached, track_id, imported)?;
    write_cached_tracks(app, cached)?;
    Ok(updated)
}

pub(crate) fn replace_cached_track(
    cached: &mut [ImportedTrack],
    track_id: &str,
    imported: &ImportedTrack,
) -> Result<ImportedTrack, String> {
    let existing = cached
        .iter_mut()
        .find(|track| track.id == track_id)
        .ok_or_else(|| "曲目已被移除，已取消重新缓存".to_string())?;
    if import_track_key(existing) != import_track_key(imported) {
        return Err("重新缓存的曲目来源不匹配".into());
    }
    let mut updated = merge_imported_track(existing, imported);
    updated.id.clone_from(&existing.id);
    *existing = updated.clone();
    Ok(updated)
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
    write_cached_tracks(app, updated)
}

pub(crate) fn library_cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("library-cache.json"))
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

pub(crate) fn cached_track_matches_delete(
    track: &ImportedTrack,
    track_id: &str,
    target_key: Option<&str>,
) -> bool {
    (!track_id.is_empty() && track.id == track_id)
        || target_key.is_some_and(|key| import_track_key(track) == key)
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
    // 重新导入是自动流程：本次扫出的 sidecar / 内嵌歌词不得覆盖用户固定的选择；
    // 本次没扫到歌词时也保留已缓存的一份。
    if !lyrics_replaceable_by_auto(&cached.lyrics) || merged.lyrics.is_empty() {
        merged.lyrics = cached.lyrics.clone();
    }
    // M-4：重新导入同一文件时，若本次提取不到封面（内嵌封面缺失/covers 目录暂不可写/
    // 在线匹配的封面本就不在文件里），保留已缓存的封面引用。否则封面引用被清空后，
    // gc_orphan_covers 会把封面文件当孤儿物理删除，造成用户成果静默丢失。
    if merged.cover.is_empty() && !cached.cover.is_empty() {
        merged.cover = cached.cover.clone();
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
    // 按引用建索引，只克隆命中的条目（此前把全库连歌词克隆进了 HashMap）
    let cached_by_key = cached
        .iter()
        .map(|track| (import_track_key(track), track))
        .collect::<HashMap<_, _>>();

    imported
        .iter()
        .map(|track| {
            cached_by_key
                .get(&import_track_key(track))
                .map(|cached| (*cached).clone())
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

/// 自动流程（重新导入、歌词目录匹配）能否替换曲目现有歌词：空文档随便换；用户手动导入 /
/// 明确应用的（`source.pinned`）不许。`find_local_lyrics` 与 `merge_imported_track` 共用。
pub(crate) fn lyrics_replaceable_by_auto(current: &LyricDocument) -> bool {
    current.is_empty() || !current.source.pinned
}

/// 写入歌词文档。曲目原有的 AMLL 查找键（来自更早的本地文件名 / 候选）并入新文档的来源，
/// 不因换一份歌词而丢失。
pub(crate) fn apply_track_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    mut lyrics: LyricDocument,
    track_path: Option<&str>,
    covers_dir: Option<&Path>,
) -> Result<(), String> {
    let index = ensure_track_for_lyrics(tracks, track_id, track_path, covers_dir)?;
    let track = &mut tracks[index];
    let previous_keys = std::mem::take(&mut track.lyrics.source.lookup_keys);
    lyrics.source = std::mem::take(&mut lyrics.source).with_lookup_keys(previous_keys);
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
    let extension = path.extension().and_then(|value| value.to_str());
    if extension.and_then(audio_format_from_extension).is_some() {
        return true;
    }
    // LIB-03：扩展名明摆着不是音频的文件不再打开嗅探。导入一个混着封面图、cue、log、
    // 视频的文件夹时，此前每个这样的文件都多一次打开和读取。
    if extension.is_some_and(is_known_non_audio_extension) {
        return false;
    }
    audio_format_from_magic(path).is_some()
}

/// 音乐文件夹里常见、而且肯定不是音频的扩展名（LIB-03）。命中即跳过文件头嗅探。
///
/// 表里的类型本来就嗅探不出音频格式，跳过它们只省掉一次打开和读取；唯一会变的是被误判的
/// 文件不再混进曲库——UTF-16 文本以 `FF FE` 开头，恰好像 MPEG 帧同步字，EAC 的抓轨日志
/// （`.log`）就是这种编码。
///
/// **故意不在表里的**：MP4 家族（`mp4` / `m4v` / `mov` / `3gp` / `m4b`）以 `ftyp` 开头，
/// 一直按 M4A 导入（只有音轨的 MP4、有声书）；`mpg` / `mpeg` / `mp2` / `mpa` 可能是 MPEG
/// 音频基本流；`bin` / `dat` 之类含义不定。它们和没有扩展名的文件一样照旧嗅探。
pub(crate) fn is_known_non_audio_extension(extension: &str) -> bool {
    const IMAGES: &[&str] = &[
        "jpg", "jpeg", "jpe", "png", "gif", "bmp", "webp", "tif", "tiff", "ico", "svg", "heic",
        "avif", "psd",
    ];
    // 文本、清单、歌词、字幕、校验与日志
    const TEXT: &[&str] = &[
        "txt", "log", "cue", "nfo", "md", "json", "xml", "htm", "html", "pdf", "doc", "docx",
        "rtf", "ini", "db", "m3u", "m3u8", "pls", "lrc", "ttml", "qrc", "krc", "yrc", "srt", "ass",
        "ssa", "sfv", "md5", "ffp", "accurip", "toc", "url", "lnk",
    ];
    // 视频（MP4 家族与 MPEG 基本流除外，见上）
    const VIDEO: &[&str] = &[
        "mkv", "avi", "wmv", "flv", "webm", "ts", "m2ts", "vob", "rm", "rmvb",
    ];
    // 压缩包、镜像与可执行文件
    const ARCHIVES: &[&str] = &[
        "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "exe", "dll", "msi", "bat", "cmd",
    ];
    [IMAGES, TEXT, VIDEO, ARCHIVES]
        .into_iter()
        .flatten()
        .any(|known| extension.eq_ignore_ascii_case(known))
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

/// MPEG 音频帧头（MP1 / MP2 / MP3）。LIB-03：此前只看 11 位同步字和「层不是保留值」，
/// 任何以 `FF Ex` / `FF Fx` 开头的文件都会被当成 MP3。现在：
/// - 4 字节帧头里的保留值全部排除：版本 `01`、层 `00`、比特率索引 `1111`、采样率索引 `11`、
///   预加重 `10`。自由格式（比特率索引 `0000`）也不认——实际文件里几乎见不到，而以
///   `FF Ex 0x` 开头的非音频二进制却不少。
/// - `FF FE` 开头的一律不认：它是 UTF-16 LE 文本的 BOM（Windows 上的 `.reg`、EAC 抓轨日志、
///   不少 cue 都是这种编码），而作为帧头只能是「MPEG-1 Layer I 带 CRC」，后面跟着的文字
///   又常常凑成一个字段全部合法的帧头。两者撞上时按文本处理。
///
/// 扩展名本身是音频的文件不走嗅探，不受这些规则影响。
pub(crate) fn is_mpeg_audio_header(header: &[u8]) -> bool {
    let [0xff, second, third, fourth, ..] = *header else {
        return false;
    };
    if second & 0xe0 != 0xe0 || second == 0xfe {
        return false;
    }
    let version = (second >> 3) & 0b11;
    let layer = (second >> 1) & 0b11;
    let bitrate_index = third >> 4;
    let sample_rate_index = (third >> 2) & 0b11;
    let emphasis = fourth & 0b11;
    version != 0b01
        && layer != 0b00
        && bitrate_index != 0b1111
        && bitrate_index != 0b0000
        && sample_rate_index != 0b11
        && emphasis != 0b10
}

/// AAC 的 ADTS 帧头：12 位同步字、层固定 `00`、采样率索引 0–12（13 / 14 保留，15 禁用）。
/// 层必须是 `00` 这一条不能省——MPEG 帧头因为别的保留值被拒之后会落到这里，
/// 只看同步字的话又会被当成 AAC 放进来。
pub(crate) fn is_adts_header(header: &[u8]) -> bool {
    let [0xff, second, third, ..] = *header else {
        return false;
    };
    second & 0xf6 == 0xf0 && (third >> 2) & 0x0f <= 12
}

pub(crate) fn is_dsd_format(format: &str) -> bool {
    format.eq_ignore_ascii_case("DSF") || format.eq_ignore_ascii_case("DFF")
}

pub(crate) fn track_from_path(
    path: &Path,
    covers_dir: Option<&Path>,
) -> Result<ImportedTrack, String> {
    track_from_path_with_lyrics(path, covers_dir, external_lrc_lyrics(path))
}

/// 本地曲目 ID 里的那段哈希：文件路径（ASCII 小写）的稳定哈希，同时决定配色。
///
/// LIB-01：算法固定在 `seraph_core::stable_hash`（与此前的 `DefaultHasher` 逐位相同，
/// 已有曲库的 ID 不变）。收藏、歌单、歌词都按这个 ID 关联，**别换算法、别改小写规则**——
/// 回归闸是 `local_track_ids_are_pinned`。
pub(crate) fn local_track_hash(path: &str) -> u64 {
    seraph_core::stable_hash::hash_str(&path.to_ascii_lowercase())
}

/// 本地曲目的 ID：`local-` 加 16 位十六进制的 [`local_track_hash`]。
pub(crate) fn local_track_id(path: &str) -> String {
    format!("local-{:016x}", local_track_hash(path))
}

/// `sidecar_lyrics` 是调用方已经找好并读出的同名歌词文件（没有就是 `None`）。批量导入经
/// [`SidecarLyricsIndex`] 查找，每个目录只枚举一次（LIB-02）；单首走 [`track_from_path`]。
fn track_from_path_with_lyrics(
    path: &Path,
    covers_dir: Option<&Path>,
    sidecar_lyrics: Option<LyricDocument>,
) -> Result<ImportedTrack, String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("failed to read file metadata {}: {err}", path.display()))?;
    let path_string = path.to_string_lossy().to_string();
    let hash = local_track_hash(&path_string);
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
    // 同名 sidecar 优先于标签内嵌歌词；两者都是文件类来源，`[offset:]` 已记进文档
    let lyrics = sidecar_lyrics.unwrap_or_else(|| audio_metadata.lyrics.clone());
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
        id: local_track_id(&path_string),
        title,
        artist,
        album,
        album_year: audio_metadata.album_year,
        track_number: audio_metadata.track_number,
        disc_number: audio_metadata.disc_number,
        album_artist: audio_metadata.album_artist,
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
        // v0.5.1：lofty 不支持 DSD 容器（.dsf/.dff 直接读取失败）——标题/
        // 艺术家/专辑/封面改由 dsd_tags 手动定位内嵌 ID3v2 解析，DSD 曲目
        // 不再落到「文件名当标题 + 无封面」。
        if let Some(tags) = dsd_tags_from_path(path) {
            parsed.title = tags.title;
            parsed.artist = tags.artist;
            parsed.album = tags.album;
            parsed.album_year = tags.year;
            parsed.track_number = tags.track_number;
            parsed.disc_number = tags.disc_number;
            parsed.album_artist = tags.album_artist;
            parsed.cover = tags.cover;
            parsed.lyrics = tags.lyrics;
        }
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
        (parsed.track_number, parsed.disc_number, parsed.album_artist) =
            album_position_from_tag(tag);
    }

    parsed.lyrics = lyrics_from_tags(tagged_file.tags());
    parsed.cover = cover_art_from_tags(tagged_file.tags());
    // v0.5.2：部分 WAV 的 ID3v2.4 帧大小错写成普通 u32（规范要求 syncsafe），
    // lofty 按规范解读导致 APIC 封面截断（显示半张图/空白）；封面缺失或
    // 疑似截断时从 id3 chunk 手动重提取。
    if tagged_file.file_type() == FileType::Wav {
        let current = parsed.cover.take();
        parsed.cover = wav_cover_fallback(path, current);
    }
    enrich_with_decoder_probe_dsd(path, &mut parsed, is_dsd_hint);

    parsed
}

/// S-07：内嵌封面体积上限（与 WAV/DSD 兜底解析的 32 MB 口径一致）。
/// lofty 解析出的 APIC 数据直接进内存并落盘，恶意音频可内嵌超大「封面」
/// 撑爆内存/磁盘，超限一律按无封面处理。
pub(crate) const MAX_EMBEDDED_COVER_BYTES: usize = 32 * 1024 * 1024;

/// 从标签中选内嵌封面：优先 CoverFront，否则取第一张非空图片。
pub(crate) fn cover_art_from_tags(tags: &[Tag]) -> Option<CoverArt> {
    let mut chosen = None;
    for picture in tags.iter().flat_map(|tag| tag.pictures()) {
        if picture.data().is_empty() || picture.data().len() > MAX_EMBEDDED_COVER_BYTES {
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
    // S-07：落盘入口统一体积校验（在线封面 4 MB 上限之外的兜底防线）。
    if art.data.is_empty() || art.data.len() > MAX_EMBEDDED_COVER_BYTES {
        return None;
    }
    // LIB-01：文件名是内容的稳定哈希（算法固定，换工具链不会让已有封面变成孤儿）
    let content_hash = seraph_core::stable_hash::hash_bytes(&art.data);
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

/// （音轨号, 碟号, 专辑艺术家）
pub(crate) type AlbumPosition = (Option<u32>, Option<u32>, Option<String>);

/// 音轨号、碟号、专辑艺术家（LIB5-01）。0 视为没有。
fn album_position_from_tag(tag: &Tag) -> AlbumPosition {
    (
        tag.track().filter(|value| *value > 0),
        tag.disk().filter(|value| *value > 0),
        tag.get_string(ItemKey::AlbumArtist)
            .and_then(clean_metadata_text),
    )
}

/// 只读标签取音轨号、碟号、专辑艺术家（存量补读用，不做 ffprobe 等重探测）。
pub(crate) fn read_album_position(path: &Path) -> AlbumPosition {
    match lofty::read_from_path(path) {
        Ok(tagged_file) => tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag())
            .map(album_position_from_tag)
            .unwrap_or_default(),
        // lofty 不支持 DSD 容器，改走内嵌 ID3v2
        Err(_) => dsd_tags_from_path(path)
            .map(|tags| (tags.track_number, tags.disc_number, tags.album_artist))
            .unwrap_or_default(),
    }
}

/// LIB5-01：v0.6.3 及更早导入的本地曲目没有音轨号、碟号、专辑艺术家。启动时一次性
/// 补读这三项都为空的本地曲目，完成后写标记文件；结构与 `backfill_missing_covers` 相同
/// （锁外读标签，持锁只做读改写，以并发写为准）。返回是否改动了曲库。
pub(crate) fn backfill_album_positions(app: &AppHandle) -> bool {
    let Ok(covers_dir) = covers_dir_path(app) else {
        return false;
    };
    let marker = covers_dir.join(".album-position-backfill-v1");
    if marker.is_file() {
        return false;
    }
    let missing = |track: &ImportedTrack| {
        track.source_url.is_none()
            && track.track_number.is_none()
            && track.disc_number.is_none()
            && track.album_artist.is_none()
    };
    let candidates: Vec<String> = {
        let Ok(snapshot) = read_library_snapshot(app) else {
            // 缓存损坏时不写标记，等缓存恢复后下次启动重试
            return false;
        };
        snapshot
            .tracks
            .iter()
            .filter(|track| missing(track))
            .map(|track| track.path.clone())
            .collect()
    };

    let mut found: HashMap<String, AlbumPosition> = HashMap::new();
    for path_string in &candidates {
        let path = Path::new(path_string);
        if !path.is_file() {
            continue;
        }
        let position = read_album_position(path);
        if position != (None, None, None) {
            found.insert(path_string.clone(), position);
        }
    }

    let mut changed = false;
    if !found.is_empty() {
        let _guard = LIBRARY_LOCK.lock();
        let Ok(mut tracks) = read_cached_tracks_for_update(app) else {
            return false;
        };
        for track in &mut tracks {
            if !missing(track) {
                continue;
            }
            let Some((track_number, disc_number, album_artist)) = found.get(&track.path) else {
                continue;
            };
            track.track_number = *track_number;
            track.disc_number = *disc_number;
            track.album_artist = album_artist.clone();
            changed = true;
        }
        if changed && write_cached_tracks(app, tracks).is_err() {
            // 写失败不落标记，下次启动重试
            return false;
        }
    }

    if fs::create_dir_all(&covers_dir).is_ok() {
        let _ = fs::write(&marker, b"v1");
    }
    changed
}

/// 只读标签提取封面并落盘（启动补扫用，跳过 ffprobe 等重探测）。
pub(crate) fn extract_embedded_cover(path: &Path, covers_dir: &Path) -> Option<String> {
    let art = match lofty::read_from_path(path) {
        Ok(tagged_file) => {
            let cover = cover_art_from_tags(tagged_file.tags());
            // v0.5.2：WAV 封面截断兜底（u32 帧大小病灶），与导入路径同语义
            if tagged_file.file_type() == FileType::Wav {
                wav_cover_fallback(path, cover)
            } else {
                cover
            }
        }
        // v0.5.1：lofty 不支持 DSD 容器，改走内嵌 ID3v2 手动提取
        Err(_) => dsd_tags_from_path(path).and_then(|tags| tags.cover),
    }?;
    save_cover_art(covers_dir, &art)
}

/// 旧版曲库缓存里的本地曲目没有封面（当时不提取）。启动时一次性补扫
/// cover 为空或落盘封面为截断图的本地曲目，完成后写标记文件，后续启动
/// 零成本；新导入的曲目在导入时即提取封面，不依赖本流程。
pub(crate) fn backfill_missing_covers(app: &AppHandle) -> bool {
    let Ok(covers_dir) = covers_dir_path(app) else {
        return false;
    };
    // v0.5.2 标记升级 v2 → v3：WAV 的 ID3v2.4 u32 帧大小病灶修复前，
    // 截断的半张封面已落盘且 cover 非空，需要再补扫一次重新提取。
    let marker = covers_dir.join(".cover-backfill-v3");
    if marker.is_file() {
        return false;
    }

    // H-1：封面提取含 lofty 全标签解析（每首数十毫秒，大曲库累计到分钟级），
    // 绝不能持 LIBRARY_LOCK 期间做。这里先短暂持锁取一份快照，随即释放锁，
    // 在锁外完成候选过滤（含截断检测的文件读取）与所有重 IO；最后再短暂持锁读改写落盘。
    // 期间若有并发导入改动了曲库，以并发写为准：只回填「候选仍存在且封面未被并发更新」的曲目。
    let snapshot: Vec<(String, String)> = {
        let Ok(snapshot) = read_library_snapshot(app) else {
            // 缓存损坏时不写标记，等缓存恢复后下次启动重试
            return false;
        };
        snapshot
            .tracks
            .iter()
            .filter(|track| track.source_url.is_none())
            .map(|track| (track.path.clone(), track.cover.clone()))
            .collect()
    };

    // 锁外过滤候选：缺封面，或落盘封面是修复前留下的截断图
    let candidates: Vec<(String, String)> = snapshot
        .into_iter()
        .filter(|(_, cover)| cover.is_empty() || cover_file_looks_truncated(cover))
        .collect();

    if candidates.is_empty() {
        if fs::create_dir_all(&covers_dir).is_ok() {
            let _ = fs::write(&marker, b"v3");
        }
        return false;
    }

    // 锁外做重 IO：逐候选提取封面，得到 path -> cover 映射。
    let mut extracted: HashMap<String, String> = HashMap::new();
    for (path_string, _) in &candidates {
        let path = Path::new(path_string);
        if !path.is_file() {
            continue;
        }
        if let Some(cover) = extract_embedded_cover(path, &covers_dir) {
            extracted.insert(path_string.clone(), cover);
        }
    }

    let mut changed = false;
    if !extracted.is_empty() {
        let snapshot_cover: HashMap<&String, &String> = candidates
            .iter()
            .map(|(path, cover)| (path, cover))
            .collect();
        // 重新持锁读改写：封面与快照一致（未被并发导入更新）才回填。
        let _guard = LIBRARY_LOCK.lock();
        let Ok(mut tracks) = read_cached_tracks_for_update(app) else {
            return false;
        };
        for track in &mut tracks {
            if track.source_url.is_some() {
                continue;
            }
            let Some(cover) = extracted.get(&track.path) else {
                continue;
            };
            let Some(old_cover) = snapshot_cover.get(&track.path) else {
                continue;
            };
            if track.cover != **old_cover || track.cover == *cover {
                continue;
            }
            track.cover = cover.clone();
            changed = true;
        }
        if changed && write_cached_tracks(app, tracks).is_err() {
            // 写失败不落标记，下次启动重试
            return false;
        }
    }

    if fs::create_dir_all(&covers_dir).is_ok() {
        let _ = fs::write(&marker, b"v3");
    }
    changed
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

    let Ok(mut snapshot) = read_library_snapshot(app) else {
        return;
    };
    let cover_references = |snapshot: &LibrarySnapshot| {
        snapshot
            .tracks
            .iter()
            .filter(|track| !track.cover.is_empty() && !track.cover.starts_with("http"))
            .map(|track| normalize_cover_key(&track.cover))
            .collect::<HashSet<String>>()
    };
    let mut referenced = cover_references(&snapshot);

    let Ok(entries) = fs::read_dir(&covers_dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let mut candidates = Vec::new();
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
        candidates.push(path);
    }
    // 目录遍历在锁外；每批最多 32 个候选，删除前复核最新引用和宽限期。
    let mut removed = 0usize;
    for batch in candidates.chunks(32) {
        let _guard = LIBRARY_LOCK.lock();
        let Ok(latest) = read_library_snapshot_for_update(app) else {
            return;
        };
        if !Arc::ptr_eq(&snapshot, &latest) {
            referenced = cover_references(&latest);
            snapshot = latest;
        }
        for path in batch {
            if referenced.contains(&normalize_cover_key(&path.to_string_lossy())) {
                continue;
            }
            let old_enough = fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= Duration::from_secs(3600));
            if old_enough && fs::remove_file(path).is_ok() {
                removed += 1;
            }
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

/// F-06：外部歌词文件大小上限。与 `save_track_lyrics` 的 IPC 口径（4 MB）一致——
/// 原先这里是裸 `fs::read`，共享目录里放个 GB 级 `.lrc` 就能在导入时把内存吃满。
const MAX_EXTERNAL_LYRICS_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn external_lrc_lyrics(path: &Path) -> Option<LyricDocument> {
    read_sidecar_lyrics(&find_lyrics_file(path)?)
}

/// 读出并解析一个歌词 sidecar 文件；超过大小上限、读不了或解析不出歌词行都当没有。
pub(crate) fn read_sidecar_lyrics(lyrics_path: &Path) -> Option<LyricDocument> {
    // 先看 metadata 再读，避免为了判大小把整个文件读进来
    if fs::metadata(lyrics_path)
        .map(|meta| meta.len() > MAX_EXTERNAL_LYRICS_BYTES)
        .unwrap_or(true)
    {
        return None;
    }
    let bytes = fs::read(lyrics_path).ok()?;
    // `.ttml` sidecar 走 TTML 解析（逐字），其余仍按字节内容嗅探；LRC 的 `[offset:]` 记进文档
    let ext = lyrics_path.extension().and_then(|value| value.to_str());
    let (lyrics, offset_ms) = parse_lyrics_file_bytes_with_offset(ext, &bytes);
    (!lyrics.is_empty()).then(|| {
        LyricDocument::from_lines(lyrics, LyricSource::of(LyricSourceKind::Sidecar))
            .with_offset(offset_ms)
    })
}

/// 同名歌词 sidecar 的扩展名，按优先级排列：`.ttml` 逐字优先于行级格式。
const SIDECAR_LYRIC_EXTENSIONS: [&str; 5] = ["ttml", "lrc", "qrc", "krc", "yrc"];

/// 每个 sidecar 扩展名一位，下标同 [`SIDECAR_LYRIC_EXTENSIONS`]。
type SidecarExtensionSet = [bool; SIDECAR_LYRIC_EXTENSIONS.len()];

fn sidecar_rank(extension: &str) -> Option<usize> {
    SIDECAR_LYRIC_EXTENSIONS
        .iter()
        .position(|lyrics_ext| extension.eq_ignore_ascii_case(lyrics_ext))
}

/// 单首曲目的同名歌词查找：先按优先级精确检查（Windows 文件系统大小写不敏感，这一步就能
/// 匹配到大小写不同的文件名），都不存在才枚举所在目录做大小写不敏感兜底。
/// 批量导入不要逐首调它——没有歌词的曲目每首都会枚举一遍目录，改用 [`SidecarLyricsIndex`]。
pub(crate) fn find_lyrics_file(path: &Path) -> Option<PathBuf> {
    exact_sidecar(path, &[true; SIDECAR_LYRIC_EXTENSIONS.len()])
        .or_else(|| SidecarDir::scan(path.parent()?).case_insensitive(path))
}

/// 按优先级检查 `<stem>.<ext>` 是否存在，只查 `extensions` 里标记的扩展名。
fn exact_sidecar(path: &Path, extensions: &SidecarExtensionSet) -> Option<PathBuf> {
    SIDECAR_LYRIC_EXTENSIONS
        .iter()
        .zip(extensions)
        .filter(|(_, wanted)| **wanted)
        .map(|(extension, _)| path.with_extension(extension))
        .find(|exact| exact.is_file())
}

/// 一个目录里的歌词 sidecar 候选。
struct SidecarDir {
    /// 目录里出现过哪些 sidecar 扩展名。一个都没有时，这个目录里的曲目不可能有同名歌词。
    present: SidecarExtensionSet,
    /// 小写 stem → 优先级最高的候选（同优先级取先枚举到的）。
    by_stem: HashMap<String, (usize, PathBuf)>,
}

impl SidecarDir {
    fn scan(dir: &Path) -> Self {
        let mut index = Self {
            present: [false; SIDECAR_LYRIC_EXTENSIONS.len()],
            by_stem: HashMap::new(),
        };
        let Ok(entries) = fs::read_dir(dir) else {
            // 枚举不了就无从断言「这里没有歌词文件」：精确检查照做，兜底没有候选
            index.present = [true; SIDECAR_LYRIC_EXTENSIONS.len()];
            return index;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                // 漏读了目录项，同样不能据此跳过精确检查
                index.present = [true; SIDECAR_LYRIC_EXTENSIONS.len()];
                continue;
            };
            let candidate = entry.path();
            let Some(rank) = candidate
                .extension()
                .and_then(|value| value.to_str())
                .and_then(sidecar_rank)
            else {
                continue;
            };
            index.present[rank] = true;
            let Some(stem) = candidate.file_stem() else {
                continue;
            };
            let stem = stem.to_string_lossy().to_lowercase();
            // 同 stem 多个候选时按扩展名优先级取，不受目录枚举顺序影响
            match index.by_stem.get(&stem) {
                Some((current, _)) if *current <= rank => {}
                _ => {
                    index.by_stem.insert(stem, (rank, candidate));
                }
            }
        }
        index
    }

    /// 大小写不敏感兜底：stem 小写后相同的候选。
    fn case_insensitive(&self, path: &Path) -> Option<PathBuf> {
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        self.by_stem
            .get(&stem)
            .map(|(_, candidate)| candidate.clone())
    }
}

/// 同名歌词 sidecar 的按目录索引（LIB-02）。此前导入时每首曲目都调一次 [`find_lyrics_file`]，
/// 没有歌词文件的曲目每首都把所在目录枚举一遍——5000 首的文件夹要枚举 5000 次、2500 万个
/// 目录项。现在一次导入里每个目录只枚举一次：
/// - 目录里一个歌词扩展名的文件都没有（最常见）→ 直接判定没有，连精确检查都省掉；
/// - 否则只对目录里出现过的扩展名做精确检查，再用同一份索引做大小写不敏感兜底。
///
/// 结果与逐首调用 [`find_lyrics_file`] 相同。索引只反映枚举那一刻的目录内容，
/// 所以只在一次导入内使用，不要跨命令长期持有。
#[derive(Default)]
pub(crate) struct SidecarLyricsIndex {
    dirs: HashMap<PathBuf, SidecarDir>,
}

impl SidecarLyricsIndex {
    pub(crate) fn find(&mut self, path: &Path) -> Option<PathBuf> {
        let parent = path.parent()?;
        if !self.dirs.contains_key(parent) {
            self.dirs
                .insert(parent.to_path_buf(), SidecarDir::scan(parent));
        }
        let dir = self.dirs.get(parent)?;
        if !dir.present.contains(&true) {
            return None;
        }
        exact_sidecar(path, &dir.present).or_else(|| dir.case_insensitive(path))
    }
}
