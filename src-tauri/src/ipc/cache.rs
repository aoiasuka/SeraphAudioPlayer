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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSettings {
    pub cache_dir: String,
    pub max_size_mb: u64,
    pub auto_cleanup: bool,
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
    let mut current = load_cache_settings(app)?;

    if let Some(cache_dir) = settings.cache_dir {
        let cache_dir = cache_dir.trim();
        if !cache_dir.is_empty() {
            let path = PathBuf::from(cache_dir);
            // 用户主动设置目录时执行严格校验：非空且无 marker → 拒绝
            ensure_cache_dir_safe(&path)?;
            current.cache_dir = path.to_string_lossy().to_string();
        }
    }

    if let Some(max_size_mb) = settings.max_size_mb {
        current.max_size_mb = max_size_mb.clamp(128, 1024 * 1024);
    }

    if let Some(auto_cleanup) = settings.auto_cleanup {
        current.auto_cleanup = auto_cleanup;
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
    let settings = load_cache_settings(app)?;
    let result = cleanup_cache_files(
        Path::new(&settings.cache_dir),
        &[],
        CleanupMode::Clear,
        &cleanup,
    )?;
    mark_cleanup_result(app, &result)?;
    Ok(result)
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
    let mode = if settings.auto_cleanup && settings.max_size_mb > 0 {
        CleanupMode::Quota(settings.max_size_mb.saturating_mul(1024 * 1024))
    } else {
        CleanupMode::Measure
    };
    sweep_orphan_temp_files(Path::new(&settings.cache_dir), cleanup)?;
    let result = cleanup_cache_files(
        Path::new(&settings.cache_dir),
        preserve_paths,
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
fn cleanup_cache_files(
    path: &Path,
    preserve_paths: &[PathBuf],
    mode: CleanupMode,
    _cleanup: &operations::CacheCleanupSlot<'_>,
) -> Result<CacheCleanupResult, String> {
    require_managed_cache_dir(path)?;
    let mut entries = collect_cache_files(path)?;
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

fn cache_status(app: &AppHandle) -> Result<CacheStatus, String> {
    let settings = load_cache_settings(app)?;
    let cache_dir = PathBuf::from(&settings.cache_dir);
    ensure_cache_dir(&cache_dir)?;
    // 空闲查看缓存时也清扫旧临时文件，不依赖正持有来源租约的下载入口。
    if let Ok(cleanup) = operations::OPERATIONS.acquire_cleanup() {
        let _ = sweep_orphan_temp_files(&cache_dir, &cleanup);
    }
    let entries = collect_cache_files(&cache_dir)?;
    let used_bytes = entries.iter().map(|entry| entry.size).sum::<u64>();
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
        file_count: entries.len(),
    })
}

fn load_cache_settings(app: &AppHandle) -> Result<CacheSettings, String> {
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
    let configured_dir = PathBuf::from(&settings.cache_dir);
    if ensure_cache_dir_safe(&configured_dir).is_err() {
        settings.cache_dir = default_cache_dir(app)?.to_string_lossy().to_string();
        save_cache_settings(app, &settings)?;
    }
    ensure_cache_dir_safe(Path::new(&settings.cache_dir))?;
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
    fs::write(&tmp, bytes).map_err(|err| {
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

struct CacheFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}
