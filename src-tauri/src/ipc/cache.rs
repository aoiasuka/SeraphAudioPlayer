pub(crate) mod operations;
#[cfg(test)]
mod tests;

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::error::IpcResult;
use super::library::mark_tracks_cache_missing_by_paths;

const SETTINGS_FILE: &str = "cache-settings.json";
const DEFAULT_CACHE_DIR_NAME: &str = "bilibili-cache";
const PROGRAM_DATA_DIR_NAME: &str = "Seraph Audio Player";
const CACHE_MARKER_FILE: &str = ".seraph-cache";
const DEFAULT_MAX_SIZE_MB: u64 = 5 * 1024;
const CLEANUP_THRESHOLD_PERCENT: u64 = 90;
const CLEANUP_TARGET_PERCENT: u64 = 75;
/// CACHE-03：最多记这么多个旧缓存目录。超出时丢掉最早的（记日志），
/// 不因此拦住用户换目录。
const MAX_PREVIOUS_CACHE_DIRS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSettings {
    pub cache_dir: String,
    pub max_size_mb: u64,
    pub auto_cleanup: bool,
    /// CACHE-03：换过缓存目录后，仍留有已下载文件的旧目录。新下载只落到
    /// `cache_dir`；这些目录里的文件不搬动（曲库照旧引用、照常播放），但继续
    /// 计入用量、参与配额清理与手动清理。只由换目录的流程维护，设置页不能直接改。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_dirs: Vec<String>,
}

/// CACHE-03：一个旧缓存目录当前的用量。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousCacheDirStatus {
    pub path: String,
    pub used_bytes: u64,
    pub file_count: usize,
    /// 目录当前管不到（磁盘没接、目录或缓存标记不在了）时的说明；
    /// 此时它不计入用量，其中的文件也不会被清理。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStatus {
    pub settings: CacheSettings,
    pub used_bytes: u64,
    pub used_mb: f64,
    pub max_bytes: u64,
    pub usage_percent: f64,
    pub file_count: usize,
    /// CACHE-01：已保存的缓存目录所在磁盘当前不可用时给出原因；此时用量全为 0，
    /// 设置本身原样保留，盘接回后自动恢复。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// CACHE-03：各旧缓存目录的用量明细。上面的用量与文件数已经把它们算在内。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub previous_dirs: Vec<PreviousCacheDirStatus>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCacheSettings {
    pub cache_dir: Option<String>,
    pub max_size_mb: Option<u64>,
    pub auto_cleanup: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCleanupResult {
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub used_bytes: u64,
    pub removed_paths: Vec<String>,
    /// P2-2：删除失败的文件不再中止整体流程，错误在这里汇总返回。
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn get_cache_status(app: AppHandle) -> IpcResult<CacheStatus> {
    // H-1：目录扫描是阻塞 IO，放 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || Ok(cache_status(&app)?))
        .await
        .map_err(|err| {
            crate::ipc::error::IpcError::new(
                crate::ipc::error::IpcErrorCode::Internal,
                format!("get_cache_status task panicked: {err}"),
            )
        })?
}

#[tauri::command]
pub async fn update_cache_settings(
    app: AppHandle,
    settings: UpdateCacheSettings,
) -> IpcResult<CacheStatus> {
    // H-1：校验 + 落盘 + enforce（缓存扫描/删除）都是阻塞 IO，放 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || update_cache_settings_inner(&app, settings))
        .await
        .map_err(|err| {
            crate::ipc::error::IpcError::new(
                crate::ipc::error::IpcErrorCode::Internal,
                format!("update_cache_settings task panicked: {err}"),
            )
        })?
}

fn update_cache_settings_inner(
    app: &AppHandle,
    settings: UpdateCacheSettings,
) -> IpcResult<CacheStatus> {
    let cleanup = operations::OPERATIONS.acquire_cleanup()?;
    // CACHE-01：这里读未经目录校验的已保存值——原目录所在磁盘不可用时，
    // 用户仍要能把目录改到别处；最终生效的目录在下面统一校验。
    let mut current = read_saved_cache_settings(app)?;

    if let Some(cache_dir) = settings.cache_dir {
        let cache_dir = cache_dir.trim();
        if !cache_dir.is_empty() {
            let path = PathBuf::from(cache_dir);
            // CACHE-03：先查与已有缓存目录的包含关系——下面的安全校验会建目录、
            // 写标记，被拒绝的目录不该留下这些痕迹。
            reject_overlapping_cache_dir(&current, &path)?;
            // 用户主动设置目录时执行严格校验：非空且无 marker → 拒绝
            ensure_cache_dir_safe(&path)?;
            // CACHE-03：旧目录里已下载的文件继续受管，不因换目录而被遗忘。
            switch_cache_dir(&mut current, &path);
        }
    }

    if let Some(max_size_mb) = settings.max_size_mb {
        current.max_size_mb = max_size_mb.clamp(128, 1024 * 1024);
    }

    if let Some(auto_cleanup) = settings.auto_cleanup {
        current.auto_cleanup = auto_cleanup;
    }

    // 没改目录而原目录所在磁盘不可用：给出可读的原因，不保存半截改动。
    if let SavedDirState::Unreachable(reason) = saved_dir_state(Path::new(&current.cache_dir)) {
        return Err(reason.into());
    }
    ensure_cache_dir(Path::new(&current.cache_dir))?;
    save_cache_settings(app, &current)?;
    enforce_cache_limit_with_lease(app, &[], &cleanup)?;
    Ok(cache_status(app)?)
}

#[tauri::command]
pub async fn clear_cache(app: AppHandle) -> IpcResult<CacheCleanupResult> {
    // H-1：目录扫描 + 逐文件删除 + 曲库读改写都是阻塞 IO，放 spawn_blocking，
    // 避免占用主线程（同步命令在主线程执行会冻结窗口）。
    tauri::async_runtime::spawn_blocking(move || clear_cache_inner(&app))
        .await
        .map_err(|err| {
            crate::ipc::error::IpcError::new(
                crate::ipc::error::IpcErrorCode::Internal,
                format!("clear_cache task panicked: {err}"),
            )
        })?
}

fn clear_cache_inner(app: &AppHandle) -> IpcResult<CacheCleanupResult> {
    let cleanup = operations::OPERATIONS.acquire_cleanup()?;
    let mut settings = load_cache_settings(app)?;
    let result = cleanup_cache_files(
        Path::new(&settings.cache_dir),
        &previous_cache_dirs(&settings),
        &with_now_playing(&[]),
        CleanupMode::Clear,
        &cleanup,
    )?;
    mark_cleanup_result(app, &result)?;
    // CACHE-03：清空之后已经没有文件的旧目录不必再记着。
    forget_emptied_previous_dirs(app, &mut settings);
    Ok(result)
}

/// 把已经没有东西可管的旧缓存目录从设置里去掉。文件已经删完，保存失败只记日志，
/// 不让清理因此报错；下次换目录或清理时还会再剔除一次。
fn forget_emptied_previous_dirs(app: &AppHandle, settings: &mut CacheSettings) {
    let before = settings.previous_dirs.len();
    settings
        .previous_dirs
        .retain(|dir| previous_dir_still_relevant(Path::new(dir)));
    if settings.previous_dirs.len() == before {
        return;
    }
    if let Err(err) = save_cache_settings(app, settings) {
        tracing::warn!("failed to save pruned previous cache dirs: {err}");
    }
}

/// CACHE-02：最近一次起播的文件。清理（配额与手动）都保留它——走 ffmpeg 的曲目
/// 在下一次 seek 时要重新打开文件，播放中被删就会失败。
static NOW_PLAYING: parking_lot::Mutex<Option<PathBuf>> = parking_lot::Mutex::new(None);

/// CACHE-02：起播时调用。记下当前播放的文件，并把受管缓存文件的修改时间刷新为
/// 当前时间——配额清理按修改时间升序淘汰，刷新后淘汰顺序就是「最久未播放」，
/// 而不是「最早下载」。不是受管缓存文件（本地音乐）时不触碰文件。
pub(crate) fn note_playback_started(path: &Path) {
    *NOW_PLAYING.lock() = Some(path.to_path_buf());
    if let Err(err) = touch_managed_cache_file(path) {
        tracing::debug!("cache touch skipped for {}: {err}", path.display());
    }
}

fn now_playing_path() -> Option<PathBuf> {
    NOW_PLAYING.lock().clone()
}

/// 只刷新「父目录带缓存标记」的受管音频文件；链接和重解析点一律不碰。
fn touch_managed_cache_file(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || !is_managed_cache_file(path) {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    match cache_delete_metadata(&parent.join(CACHE_MARKER_FILE))? {
        Some(marker) if marker.is_file() => {}
        _ => return Ok(()),
    }
    let Some(metadata) = cache_delete_metadata(path)? else {
        return Ok(());
    };
    reject_cache_link(path, &metadata)?;
    if !metadata.is_file() {
        return Ok(());
    }
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(SystemTime::now()))
        .map_err(|err| err.to_string())
}

/// 调用方给的保留列表 + 正在播放的文件。
fn with_now_playing(preserve_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut preserve = preserve_paths.to_vec();
    preserve.extend(now_playing_path());
    preserve
}

pub(crate) fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let settings = load_cache_settings(app)?;
    let path = PathBuf::from(settings.cache_dir);
    ensure_cache_dir(&path)?;
    // 有下载时不扫临时文件；租约覆盖整个清扫，避免检查后又启动新下载。
    if let Ok(cleanup) = operations::OPERATIONS.acquire_cleanup() {
        let _ = sweep_orphan_temp_files(&path, &cleanup);
    }
    Ok(path)
}

/// 审2-S3：多路径 preserve 版本——收藏夹批量导入整批结束后统一清理一次，
/// preserve 本批全部成功导入的文件，避免逐首清理把同批先导入的文件删掉。
/// 单曲导入也走这里（传单元素切片）。
pub(crate) fn enforce_cache_limit_preserving_many(
    app: &AppHandle,
    preserve: &[PathBuf],
) -> Result<CacheCleanupResult, String> {
    enforce_cache_limit_inner(app, preserve)
}

fn enforce_cache_limit_inner(
    app: &AppHandle,
    preserve_paths: &[PathBuf],
) -> Result<CacheCleanupResult, String> {
    let cleanup = operations::OPERATIONS.acquire_cleanup()?;
    enforce_cache_limit_with_lease(app, preserve_paths, &cleanup)
}

fn enforce_cache_limit_with_lease(
    app: &AppHandle,
    preserve_paths: &[PathBuf],
    cleanup: &operations::CacheCleanupSlot<'_>,
) -> Result<CacheCleanupResult, String> {
    let settings = load_cache_settings(app)?;
    let previous = previous_cache_dirs(&settings);
    let mode = if settings.auto_cleanup && settings.max_size_mb > 0 {
        CleanupMode::Quota(settings.max_size_mb.saturating_mul(1024 * 1024))
    } else {
        CleanupMode::Measure
    };
    sweep_orphan_temp_files(Path::new(&settings.cache_dir), cleanup)?;
    for dir in &previous {
        // 旧目录当前管不到（盘没接、标记没了）就跳过，不影响当前目录的清理
        let _ = sweep_orphan_temp_files(dir, cleanup);
    }
    let result = cleanup_cache_files(
        Path::new(&settings.cache_dir),
        &previous,
        &with_now_playing(preserve_paths),
        mode,
        cleanup,
    )?;
    mark_cleanup_result(app, &result)?;
    Ok(result)
}

fn mark_cleanup_result(app: &AppHandle, result: &CacheCleanupResult) -> Result<(), String> {
    let paths = result
        .removed_paths
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    mark_tracks_cache_missing_by_paths(app, &paths)
}

enum CleanupMode {
    Clear,
    Quota(u64),
    Measure,
}

/// 与 AppHandle 无关的清理内核，使用隔离目录做回归；租约必须覆盖删除和曲库标记提交。
///
/// `primary` 是当前缓存目录，必须是受管目录，否则整体报错。`previous` 是 CACHE-03
/// 的旧缓存目录：当前管不到的（盘没接、目录或标记不在了）直接跳过，其中的文件
/// 既不计入用量也不删除。配额淘汰在所有目录的文件里统一按「最久未播放」排序。
fn cleanup_cache_files(
    primary: &Path,
    previous: &[PathBuf],
    preserve_paths: &[PathBuf],
    mode: CleanupMode,
    _cleanup: &operations::CacheCleanupSlot<'_>,
) -> Result<CacheCleanupResult, String> {
    require_managed_cache_dir(primary)?;
    let mut entries = collect_cache_files(primary)?;
    for dir in previous {
        match scan_previous_cache_dir(dir) {
            Ok(files) => entries.extend(files),
            Err(reason) => {
                tracing::debug!("previous cache dir {} skipped: {reason}", dir.display())
            }
        }
    }
    let mut used_bytes = entries
        .iter()
        .fold(0_u64, |total, entry| total.saturating_add(entry.size));
    let target = match mode {
        CleanupMode::Clear => 0,
        CleanupMode::Quota(maximum)
            if used_bytes >= maximum.saturating_mul(CLEANUP_THRESHOLD_PERCENT) / 100 =>
        {
            maximum.saturating_mul(CLEANUP_TARGET_PERCENT) / 100
        }
        CleanupMode::Quota(_) | CleanupMode::Measure => used_bytes,
    };
    if !matches!(mode, CleanupMode::Clear) && target >= used_bytes {
        return Ok(CacheCleanupResult {
            removed_files: 0,
            removed_bytes: 0,
            used_bytes,
            removed_paths: Vec::new(),
            errors: Vec::new(),
        });
    }
    entries.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    let preserve_keys = preserve_paths
        .iter()
        .map(|path| normalized_path_key(path))
        .collect::<HashSet<_>>();
    let mut removed_paths = Vec::new();
    let mut removed_bytes = 0_u64;
    let mut errors = Vec::new();
    for entry in entries {
        // 全量清理也删除零字节临时文件；配额模式达到目标就停止。
        if !matches!(mode, CleanupMode::Clear) && used_bytes <= target {
            break;
        }
        if preserve_keys.contains(&normalized_path_key(&entry.path)) {
            continue;
        }
        let removed = (|| {
            if let Some(metadata) = cache_delete_metadata(&entry.path)? {
                reject_cache_link(&entry.path, &metadata)?;
                if !metadata.is_file() {
                    return Err("缓存条目不再是普通文件".to_string());
                }
            }
            // 完成标记无法移除时不动音频，避免把残留标记错误复用于下一次下载。
            remove_ok_sentinel(&entry.path)?;
            remove_cache_file_if_present(&entry.path)
        })();
        match removed {
            Ok(_) => {
                used_bytes = used_bytes.saturating_sub(entry.size);
                removed_bytes = removed_bytes.saturating_add(entry.size);
                removed_paths.push(entry.path.to_string_lossy().to_string());
            }
            Err(err) => errors.push(format!("{}: {err}", entry.path.display())),
        }
    }
    Ok(CacheCleanupResult {
        removed_files: removed_paths.len(),
        removed_bytes,
        used_bytes,
        removed_paths,
        errors,
    })
}

/// P1-2：删除缓存音频时同步清掉对应的 `.{name}.ok` sentinel，
/// 避免零字节 sentinel 永久残留。
fn remove_ok_sentinel(path: &Path) -> Result<(), String> {
    if let Some(file_name) = path.file_name().and_then(|value| value.to_str()) {
        let sentinel = path.with_file_name(format!(".{file_name}.ok"));
        if let Some(metadata) = cache_delete_metadata(&sentinel)? {
            reject_cache_link(&sentinel, &metadata)?;
            if !metadata.is_file() {
                return Err("缓存完成标记不是普通文件".into());
            }
        }
        remove_cache_file_if_present(&sentinel)?;
    }
    Ok(())
}

/// 单曲删除仅处理已入库的精确路径，支持仍有缓存标记的历史目录。
/// 不创建目录/标记，不递归删除，也不跟随符号链接或 Windows 重解析点。
pub(crate) fn delete_streaming_cache_file(path: &Path) -> Result<bool, String> {
    use std::path::Component;

    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("缓存文件路径无效，已保留曲目记录".into());
    }
    let is_audio = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "m4a" | "flac" | "opus" | "aac" | "mp3" | "eac3"
            )
        });
    if !is_audio {
        return Err("该文件不是受支持的流媒体音频缓存，已保留曲目记录".into());
    }

    // 必须检查整个祖先链；缓存目录自身有标记也不能豁免上级 junction。
    for ancestor in path.ancestors() {
        if let Some(metadata) = cache_delete_metadata(ancestor)? {
            reject_cache_link(ancestor, &metadata)?;
            if ancestor != path && !metadata.is_dir() {
                return Err("缓存文件的上级路径不是目录".into());
            }
        }
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "缓存文件名无效".to_string())?;
    #[cfg(windows)]
    if path
        .components()
        .any(|part| matches!(part, Component::Normal(name) if name.to_string_lossy().contains(':')))
    {
        return Err("不允许删除备用数据流路径".into());
    }
    let sentinel = path.with_file_name(format!(".{file_name}.ok"));
    let audio_metadata = cache_delete_metadata(path)?;
    let sentinel_metadata = cache_delete_metadata(&sentinel)?;
    for (target, metadata) in [
        (path, &audio_metadata),
        (sentinel.as_path(), &sentinel_metadata),
    ] {
        if let Some(metadata) = metadata {
            reject_cache_link(target, metadata)?;
            if !metadata.is_file() {
                return Err(format!("拒绝删除非普通文件：{}", target.display()));
            }
        }
    }
    // 两个文件都已不存在时，只清理过期记录，不必重建已经移除的缓存目录。
    if audio_metadata.is_none() && sentinel_metadata.is_none() {
        return Ok(false);
    }

    let mut managed = false;
    for directory in path.parent().into_iter().flat_map(Path::ancestors) {
        let marker = directory.join(CACHE_MARKER_FILE);
        if let Some(metadata) = cache_delete_metadata(&marker)? {
            reject_cache_link(&marker, &metadata)?;
            validate_cache_dir(directory)?;
            if !metadata.is_file() {
                return Err("缓存目录标记不是普通文件".into());
            }
            require_managed_cache_dir(directory)?;
            managed = true;
            break;
        }
    }
    if !managed {
        return Err("文件不在 Seraph 管理的缓存目录中，已保留文件和曲目记录".into());
    }

    // 先删除完成标记；标记无法清理时不触碰音频。
    remove_cache_file_if_present(&sentinel)?;
    remove_cache_file_if_present(path)
}

fn cache_delete_metadata(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("无法检查缓存文件 {}：{err}", path.display())),
    }
}

fn reject_cache_link(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    let is_link = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let is_link = {
        use std::os::windows::fs::MetadataExt;
        is_link || metadata.file_attributes() & 0x400 != 0
    };
    if is_link {
        return Err(format!(
            "拒绝通过链接或重解析点删除文件：{}",
            path.display()
        ));
    }
    Ok(())
}

fn remove_cache_file_if_present(path: &Path) -> Result<bool, String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(format!("删除缓存文件失败 {}：{err}", path.display())),
    }
}

fn normalized_path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase()
}

/// 目录的比较键：规范化路径、统一分隔符与大小写、去掉末尾分隔符。
/// 目录本身可能还不存在（刚选的新目录）——规范化最深的那个已存在的祖先，
/// 再接上余下的部分；整条路径都解析不了（盘没接）时按原样比较。
fn dir_key(path: &Path) -> String {
    let mut base = path;
    let mut tail = Vec::new();
    let mut full = loop {
        if let Ok(resolved) = base.canonicalize() {
            break resolved;
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                base = parent;
            }
            _ => {
                tail.clear();
                break path.to_path_buf();
            }
        }
    };
    full.extend(tail.iter().rev());
    let text = full.to_string_lossy().replace('/', "\\").to_lowercase();
    text.strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .trim_end_matches('\\')
        .to_string()
}

/// `child` 是否位于 `parent` 之内（两者都是 [`dir_key`]；相同不算）。
fn dir_key_inside(child: &str, parent: &str) -> bool {
    child.len() > parent.len()
        && child.starts_with(parent)
        && child.as_bytes()[parent.len()] == b'\\'
}

/// 两个目录是否重叠：同一个目录，或一个在另一个里面。
/// 重叠的目录一起扫描时，同一个文件会被数两遍、删两遍。
fn cache_dirs_overlap(a: &Path, b: &Path) -> bool {
    let (a, b) = (dir_key(a), dir_key(b));
    a == b || dir_key_inside(&a, &b) || dir_key_inside(&b, &a)
}

/// CACHE-03：实际参与统计与清理的旧缓存目录。
///
/// 与当前目录重叠、或与排在前面的条目重叠的一律跳过。正常的换目录流程不会
/// 写出这样的清单（见 [`reject_overlapping_cache_dir`]），这里防的是手改过的
/// 设置文件和「不安全目录退回默认目录」这类绕过它的路径。
fn previous_cache_dirs(settings: &CacheSettings) -> Vec<PathBuf> {
    let primary = Path::new(&settings.cache_dir);
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in settings.previous_dirs.iter().take(MAX_PREVIOUS_CACHE_DIRS) {
        let path = PathBuf::from(entry);
        if cache_dirs_overlap(&path, primary)
            || dirs.iter().any(|kept| cache_dirs_overlap(kept, &path))
        {
            continue;
        }
        dirs.push(path);
    }
    dirs
}

/// CACHE-03：读出一个旧缓存目录里的受管文件。目录当前管不到时返回原因——
/// 调用方一律跳过它：不计入用量，更不删除其中的文件。校验与当前目录同一套
/// （本机绝对路径、祖先链无链接、缓存标记是普通文件），不因为是旧目录而放宽。
fn scan_previous_cache_dir(path: &Path) -> Result<Vec<CacheFile>, String> {
    if cache_volume_unreachable(path) {
        return Err("所在磁盘当前不可用，接回后继续计入用量与清理".into());
    }
    if require_managed_cache_dir(path).is_err() {
        return Err(
            "目录当前不受管理（已删除、缺少缓存标记或路径不安全），其中的文件不会被清理".into(),
        );
    }
    collect_cache_files(path)
}

fn previous_dir_status(path: &Path) -> PreviousCacheDirStatus {
    let (used_bytes, file_count, unavailable_reason) = match scan_previous_cache_dir(path) {
        Ok(files) => (
            files
                .iter()
                .fold(0_u64, |total, file| total.saturating_add(file.size)),
            files.len(),
            None,
        ),
        Err(reason) => (0, 0, Some(reason)),
    };
    PreviousCacheDirStatus {
        path: path.to_string_lossy().to_string(),
        used_bytes,
        file_count,
        unavailable_reason,
    }
}

/// 旧缓存目录是否还要留在清单里。
/// - 所在磁盘没接：留着，接回后继续管理；
/// - 确定没有东西可管了（目录没了、标记没了、里面已经没有缓存文件）：剔除；
/// - 卷在但一时读不了（权限、IO 错误）：留着，不因为一次读失败就忘掉它。
fn previous_dir_still_relevant(path: &Path) -> bool {
    if cache_volume_unreachable(path) {
        return true;
    }
    let missing = |target: &Path| matches!(fs::symlink_metadata(target), Err(err) if err.kind() == std::io::ErrorKind::NotFound);
    if missing(path) || missing(&path.join(CACHE_MARKER_FILE)) {
        return false;
    }
    scan_previous_cache_dir(path).map_or(true, |files| !files.is_empty())
}

/// CACHE-03：新目录不能与当前或以前的缓存目录互相包含（换回以前用过的同一个
/// 目录是允许的）。必须在 [`ensure_cache_dir_safe`] 之前调用。
fn reject_overlapping_cache_dir(settings: &CacheSettings, new_dir: &Path) -> Result<(), String> {
    let new_key = dir_key(new_dir);
    let known = std::iter::once(settings.cache_dir.as_str())
        .chain(settings.previous_dirs.iter().map(String::as_str));
    for existing in known {
        let key = dir_key(Path::new(existing));
        if dir_key_inside(&new_key, &key) || dir_key_inside(&key, &new_key) {
            return Err(format!(
                "新的缓存目录不能位于已有缓存目录 {existing} 之内，也不能包含它；请另选一个互不包含的目录"
            ));
        }
    }
    Ok(())
}

/// CACHE-03：把缓存目录换成 `new_dir`。原目录记进 `previous_dirs`（换回以前
/// 用过的目录则把它移出清单），并顺手剔除已经没有文件可管的条目。
/// 只是同一个目录换了写法时，仅更新写法。
fn switch_cache_dir(settings: &mut CacheSettings, new_dir: &Path) {
    let new_key = dir_key(new_dir);
    let old_dir = std::mem::replace(
        &mut settings.cache_dir,
        new_dir.to_string_lossy().to_string(),
    );
    if dir_key(Path::new(&old_dir)) == new_key {
        return;
    }
    let mut previous = std::mem::take(&mut settings.previous_dirs);
    previous.push(old_dir);
    let mut seen = HashSet::new();
    previous.retain(|entry| {
        let path = Path::new(entry);
        let key = dir_key(path);
        key != new_key && seen.insert(key) && previous_dir_still_relevant(path)
    });
    if previous.len() > MAX_PREVIOUS_CACHE_DIRS {
        let excess = previous.len() - MAX_PREVIOUS_CACHE_DIRS;
        for dropped in previous.drain(..excess) {
            tracing::warn!(
                "previous cache dir {dropped} is no longer managed: more than {MAX_PREVIOUS_CACHE_DIRS} retired cache directories"
            );
        }
    }
    settings.previous_dirs = previous;
}

fn cache_status(app: &AppHandle) -> Result<CacheStatus, String> {
    let saved = read_saved_cache_settings(app)?;
    if let SavedDirState::Unreachable(reason) = saved_dir_state(Path::new(&saved.cache_dir)) {
        let max_bytes = saved.max_size_mb.saturating_mul(1024 * 1024);
        return Ok(CacheStatus {
            settings: saved,
            used_bytes: 0,
            used_mb: 0.0,
            max_bytes,
            usage_percent: 0.0,
            file_count: 0,
            unavailable_reason: Some(reason),
            previous_dirs: Vec::new(),
        });
    }
    let settings = load_cache_settings(app)?;
    let cache_dir = PathBuf::from(&settings.cache_dir);
    ensure_cache_dir(&cache_dir)?;
    // 空闲查看缓存时也清扫旧临时文件，不依赖正持有来源租约的下载入口。
    if let Ok(cleanup) = operations::OPERATIONS.acquire_cleanup() {
        let _ = sweep_orphan_temp_files(&cache_dir, &cleanup);
    }
    let entries = collect_cache_files(&cache_dir)?;
    let mut used_bytes = entries
        .iter()
        .fold(0_u64, |total, entry| total.saturating_add(entry.size));
    let mut file_count = entries.len();
    // CACHE-03：旧缓存目录里的文件仍被曲库引用、占着磁盘，一并计入。
    let previous_dirs = previous_cache_dirs(&settings)
        .iter()
        .map(|dir| previous_dir_status(dir))
        .collect::<Vec<_>>();
    for dir in &previous_dirs {
        used_bytes = used_bytes.saturating_add(dir.used_bytes);
        file_count += dir.file_count;
    }
    let max_bytes = settings.max_size_mb.saturating_mul(1024 * 1024);
    let usage_percent = if max_bytes == 0 {
        0.0
    } else {
        used_bytes as f64 / max_bytes as f64 * 100.0
    };

    Ok(CacheStatus {
        settings,
        used_bytes,
        used_mb: used_bytes as f64 / 1024.0 / 1024.0,
        max_bytes,
        usage_percent,
        file_count,
        unavailable_reason: None,
        previous_dirs,
    })
}

/// CACHE-01：已保存缓存目录的三种状态。
#[derive(Debug, PartialEq, Eq)]
enum SavedDirState {
    Usable,
    /// 目录所在磁盘当前不存在或未就绪（移动硬盘没接、盘符没挂载）。
    /// 属于暂时状况：报错，但**不**改写已保存的设置。
    Unreachable(String),
    /// 目录本身不安全（链接、伪造标记、非空且无标记等）：退回默认目录。
    Unsafe(String),
}

/// 路径所在卷的根目录取不到元数据即视为「暂时不可达」。
/// 只看卷根：卷在而目录有问题的情况一律按不安全处理，不放宽。
fn cache_volume_unreachable(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    match path.ancestors().last() {
        Some(root) if root != path => fs::metadata(root).is_err(),
        _ => false,
    }
}

fn saved_dir_state(path: &Path) -> SavedDirState {
    if cache_volume_unreachable(path) {
        return SavedDirState::Unreachable(format!(
            "缓存目录 {} 所在的磁盘当前不可用；设置已保留，接回磁盘后恢复，也可以在设置里改用其他目录",
            path.display()
        ));
    }
    match ensure_cache_dir_safe(path) {
        Ok(()) => SavedDirState::Usable,
        Err(err) => SavedDirState::Unsafe(err),
    }
}

/// 读取并校验缓存设置。目录暂时不可达时返回错误（缓存相关功能本次不可用），
/// 不会悄悄把下载改落到系统盘，也不改写设置文件。
fn load_cache_settings(app: &AppHandle) -> Result<CacheSettings, String> {
    let mut settings = read_saved_cache_settings(app)?;
    match saved_dir_state(Path::new(&settings.cache_dir)) {
        SavedDirState::Usable => return Ok(settings),
        SavedDirState::Unreachable(reason) => return Err(reason),
        SavedDirState::Unsafe(reason) => {
            tracing::warn!(
                "cache dir {} rejected, falling back to default: {reason}",
                settings.cache_dir
            );
            settings.cache_dir = default_cache_dir(app)?.to_string_lossy().to_string();
            // CACHE-03：默认目录若曾作为旧目录记着，现在它是当前目录了，不再重复登记。
            // 被判为不安全的原目录不记进旧目录清单——它本来就不该由我们清理。
            let default_key = dir_key(Path::new(&settings.cache_dir));
            settings
                .previous_dirs
                .retain(|dir| dir_key(Path::new(dir)) != default_key);
            save_cache_settings(app, &settings)?;
        }
    }
    ensure_cache_dir_safe(Path::new(&settings.cache_dir))?;
    Ok(settings)
}

/// 只读出已保存的值（含损坏回退与数值钳位），不校验目录。
fn read_saved_cache_settings(app: &AppHandle) -> Result<CacheSettings, String> {
    let path = settings_path(app)?;
    if !path.is_file() {
        let settings = default_cache_settings(app)?;
        ensure_cache_dir(Path::new(&settings.cache_dir))?;
        save_cache_settings(app, &settings)?;
        return Ok(settings);
    }

    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read cache settings {}: {err}", path.display()))?;
    // P2-5：设置文件损坏时备份坏文件并回退默认值重建，
    // 不再让所有缓存相关功能因一个损坏的 JSON 永久瘫痪。
    let mut settings: CacheSettings = match serde_json::from_slice(&bytes) {
        Ok(settings) => settings,
        Err(err) => {
            tracing::warn!(
                "cache settings {} corrupt, rebuilding defaults: {err}",
                path.display()
            );
            let backup = PathBuf::from(format!("{}.corrupt", path.display()));
            let _ = fs::copy(&path, &backup);
            let settings = default_cache_settings(app)?;
            ensure_cache_dir(Path::new(&settings.cache_dir))?;
            save_cache_settings(app, &settings)?;
            return Ok(settings);
        }
    };
    if settings.cache_dir.trim().is_empty() {
        settings.cache_dir = default_cache_dir(app)?.to_string_lossy().to_string();
    }
    settings.max_size_mb = settings.max_size_mb.clamp(128, 1024 * 1024);
    Ok(settings)
}

fn save_cache_settings(app: &AppHandle, settings: &CacheSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create cache settings dir: {err}"))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|err| format!("failed to serialize cache settings: {err}"))?;
    // P2-5：temp+rename 原子写，避免写一半崩溃留下截断 JSON。
    // 审2-S4：固定 `{path}.tmp` 改为唯一临时名，防止并发保存交错写坏彼此的临时文件。
    let tmp = unique_temp_path(&path);
    // CACHE-04：改名前先落盘，断电不会留下长度为 0 的设置文件。
    let written = (|| {
        use std::io::Write;
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()
    })();
    written.map_err(|err| {
        let _ = fs::remove_file(&tmp);
        format!(
            "failed to write cache settings temp {}: {err}",
            tmp.display()
        )
    })?;
    fs::rename(&tmp, &path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        format!("failed to replace cache settings {}: {err}", path.display())
    })
}

/// 审2-S4：生成唯一临时文件名（时间纳秒 + 进程内计数器），供 temp+rename
/// 原子写使用；并发写同一目标时各自持有独立临时文件，互不交错/截断。
/// 与 bilibili 下载临时名（temp_download_path）的唯一化模式一致。
pub(crate) fn unique_temp_path(path: &Path) -> PathBuf {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("seraph-temp");
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!("{file_name}.{nanos}-{counter}.tmp"))
}

fn default_cache_settings(app: &AppHandle) -> Result<CacheSettings, String> {
    Ok(CacheSettings {
        cache_dir: default_cache_dir(app)?.to_string_lossy().to_string(),
        max_size_mb: DEFAULT_MAX_SIZE_MB,
        auto_cleanup: true,
        previous_dirs: Vec::new(),
    })
}

fn default_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_data_cache_dir(app)?;
    // S-14：用户级 app_data 优先于 ProgramData——ProgramData 对所有本地用户可写，
    // 其他用户可预置文件/marker 干扰缓存清理；仅保留其为最后候选（历史安装兼容）。
    let candidates = [
        install_dir_cache_dir(),
        Some(app_data_dir.clone()),
        program_data_cache_dir(),
    ];

    for candidate in candidates.into_iter().flatten() {
        if ensure_cache_dir_safe(&candidate).is_ok() {
            return Ok(candidate);
        }
    }

    // M-5：所有候选都未通过安全校验时，不再直接返回未校验的 app_data 目录。
    // 否则 load_cache_settings 会用 ensure_cache_dir 对它无条件补写 marker，
    // 把一个已含用户文件、本不该受管的目录变成受管缓存并参与 mtime 删除。
    // 这里再走一次安全校验：通过才返回，否则把错误上抛，让缓存功能失败而非误删。
    ensure_cache_dir_safe(&app_data_dir)?;
    Ok(app_data_dir)
}

fn app_data_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join(DEFAULT_CACHE_DIR_NAME))
}

fn install_dir_cache_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    Some(exe.parent()?.join(DEFAULT_CACHE_DIR_NAME))
}

fn program_data_cache_dir() -> Option<PathBuf> {
    let program_data = env::var_os("ProgramData")?;
    if program_data.is_empty() {
        return None;
    }

    Some(
        PathBuf::from(program_data)
            .join(PROGRAM_DATA_DIR_NAME)
            .join(DEFAULT_CACHE_DIR_NAME),
    )
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

fn validate_cache_dir(path: &Path) -> Result<(), String> {
    use std::path::Component;
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("缓存路径必须是无 .. 的绝对目录".into());
    }
    if path.parent().is_none() {
        return Err("不能把磁盘根目录设置为缓存目录".into());
    }
    #[cfg(windows)]
    for part in path.components() {
        match part {
            Component::Prefix(prefix)
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) =>
            {
                return Err("缓存目录必须位于本机磁盘，不能使用 UNC 或设备路径".into());
            }
            Component::Normal(name) if name.to_string_lossy().contains(':') => {
                return Err("缓存目录不能使用备用数据流路径".into());
            }
            _ => {}
        }
    }
    for ancestor in path.ancestors() {
        if let Some(metadata) = cache_delete_metadata(ancestor)? {
            reject_cache_link(ancestor, &metadata)?;
            if !metadata.is_dir() {
                return Err("缓存路径或上级路径不是目录".into());
            }
        }
    }
    Ok(())
}

/// 校验目录可作为缓存目录使用：
/// - 不存在：允许，会被创建并写入 marker
/// - 存在但为空：允许，写 marker
/// - 存在且非空但无 marker：拒绝（保护用户已有文件不被自动清理误删）
/// - 存在且有 marker：允许
fn ensure_cache_dir_safe(path: &Path) -> Result<(), String> {
    validate_cache_dir(path)?;
    let marker = path.join(CACHE_MARKER_FILE);
    if let Some(metadata) = cache_delete_metadata(&marker)? {
        reject_cache_link(&marker, &metadata)?;
        if !metadata.is_file() {
            return Err("缓存目录标记不是普通文件".into());
        }
        return Ok(());
    }
    if path.exists()
        && fs::read_dir(path)
            .map_err(|err| format!("无法检查缓存目录内容：{err}"))?
            .next()
            .transpose()
            .map_err(|err| format!("无法读取缓存目录条目：{err}"))?
            .is_some()
    {
        return Err(format!(
            "目录 {} 已包含其他文件且缺少缓存标记，请使用空目录或新目录",
            path.display()
        ));
    }
    fs::create_dir_all(path)
        .map_err(|err| format!("failed to create cache dir {}: {err}", path.display()))?;
    // 不覆盖另一并发初始化刚写好的标记，更不能跟随伪造的标记链接。
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(b"Seraph Audio Player managed cache\n")
                .map_err(|err| format!("无法写入缓存标记：{err}"))?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            require_managed_cache_dir(path)?
        }
        Err(err) => return Err(format!("无法创建缓存标记：{err}")),
    }
    Ok(())
}

fn ensure_cache_dir(path: &Path) -> Result<(), String> {
    // 即使是已保存的目录也重新检查：标记丢失时不能默默收编其中的用户文件。
    ensure_cache_dir_safe(path)
}

/// 缓存清理 / 扫描的前置检查：若目标目录缺少 marker，拒绝继续。
/// 防止用户误把缓存目录指到本地音乐文件夹后又执行清理操作时，
/// 用扩展名白名单按 mtime 批量删除真实音乐文件。
fn require_managed_cache_dir(path: &Path) -> Result<(), String> {
    validate_cache_dir(path)?;
    let marker = path.join(CACHE_MARKER_FILE);
    if let Some(metadata) = cache_delete_metadata(&marker)? {
        reject_cache_link(&marker, &metadata)?;
        if metadata.is_file() {
            return Ok(());
        }
    }
    Err(format!(
        "拒绝在未标记为 Seraph 缓存的目录 {} 上执行清理操作；请在设置里指定受管目录。",
        path.display()
    ))
}

/// P2-3：缓存扫描递归深度上限，与曲库导入端 L-14 方案对齐。
const MAX_CACHE_SCAN_DEPTH: usize = 64;

fn collect_cache_files(path: &Path) -> Result<Vec<CacheFile>, String> {
    let mut files = Vec::new();
    collect_cache_files_inner(path, &mut files, 0)?;
    Ok(files)
}

fn collect_cache_files_inner(
    path: &Path,
    files: &mut Vec<CacheFile>,
    depth: usize,
) -> Result<(), String> {
    if !path.is_dir() || depth >= MAX_CACHE_SCAN_DEPTH {
        return Ok(());
    }

    for entry in fs::read_dir(path)
        .map_err(|err| format!("failed to read cache dir {}: {err}", path.display()))?
    {
        let entry = entry.map_err(|err| err.to_string())?;
        // P2-3：跳过 symlink / Windows junction，防止链接环无限递归，
        // 以及链接指向缓存目录之外时把用户真实音乐文件计入/删除。
        let file_type = entry.file_type().map_err(|err| err.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let metadata = entry.metadata().map_err(|err| err.to_string())?;
        if reject_cache_link(&path, &metadata).is_err() {
            continue;
        }
        if file_type.is_dir() {
            collect_cache_files_inner(&path, files, depth + 1)?;
            continue;
        }
        if !file_type.is_file() || !is_managed_cache_file(&path) {
            continue;
        }
        files.push(CacheFile {
            path,
            size: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }

    Ok(())
}

fn is_managed_cache_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                // P1-2：加入 "eac3"，杜比流缓存必须参与配额统计与清理。
                "m4a" | "flac" | "opus" | "aac" | "mp3" | "eac3" | "download" | "tmp"
            )
        })
        .unwrap_or(false)
}

/// 扫描已知缓存目录下，长时间未被修改的 `.download` / `.tmp`，删除掉孤儿临时文件，
/// 防止下载中断 / ffmpeg 崩溃后的残骸不停堆积。仅在 marker 目录内执行。
fn sweep_orphan_temp_files(
    path: &Path,
    _cleanup: &operations::CacheCleanupSlot<'_>,
) -> Result<(), String> {
    require_managed_cache_dir(path)?;
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let candidate = entry.path();
        if !candidate.is_file() {
            continue;
        }
        let is_temp = candidate
            .extension()
            .and_then(|value| value.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("download") || ext.eq_ignore_ascii_case("tmp"))
            .unwrap_or(false);
        if !is_temp {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if reject_cache_link(&candidate, &metadata).is_err() || !metadata.is_file() {
            continue;
        }
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(_) => continue,
        };
        if modified < cutoff {
            let _ = fs::remove_file(&candidate);
        }
    }
    Ok(())
}

#[derive(Debug)]
struct CacheFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}
