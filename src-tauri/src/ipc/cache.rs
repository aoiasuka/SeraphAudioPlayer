use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

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
}

#[tauri::command]
pub fn get_cache_status(app: AppHandle) -> Result<CacheStatus, String> {
    cache_status(&app)
}

#[tauri::command]
pub fn update_cache_settings(
    app: AppHandle,
    settings: UpdateCacheSettings,
) -> Result<CacheStatus, String> {
    let mut current = load_cache_settings(&app)?;

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
    save_cache_settings(&app, &current)?;
    enforce_cache_limit(&app)?;
    cache_status(&app)
}

#[tauri::command]
pub fn clear_cache(app: AppHandle) -> Result<CacheCleanupResult, String> {
    let settings = load_cache_settings(&app)?;
    let cache_dir = PathBuf::from(&settings.cache_dir);
    ensure_cache_dir(&cache_dir)?;
    require_managed_cache_dir(&cache_dir)?;

    let entries = collect_cache_files(&cache_dir)?;
    let mut removed_paths = Vec::new();
    let mut removed_bytes = 0;

    for entry in entries {
        fs::remove_file(&entry.path).map_err(|err| {
            format!(
                "failed to remove cache file {}: {err}",
                entry.path.display()
            )
        })?;
        removed_bytes += entry.size;
        removed_paths.push(entry.path);
    }

    mark_tracks_cache_missing_by_paths(&app, &removed_paths)?;
    let used_bytes = cache_size(&cache_dir)?;
    Ok(CacheCleanupResult {
        removed_files: removed_paths.len(),
        removed_bytes,
        used_bytes,
        removed_paths: removed_paths
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
    })
}

pub(super) fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let settings = load_cache_settings(app)?;
    let path = PathBuf::from(settings.cache_dir);
    ensure_cache_dir(&path)?;
    // 每次拿 cache_dir 时机会成本极低地扫一次孤儿 .download/.tmp
    let _ = sweep_orphan_temp_files(&path);
    Ok(path)
}

pub(super) fn enforce_cache_limit(app: &AppHandle) -> Result<CacheCleanupResult, String> {
    enforce_cache_limit_inner(app, None)
}

pub(super) fn enforce_cache_limit_preserving(
    app: &AppHandle,
    preserve_path: &Path,
) -> Result<CacheCleanupResult, String> {
    enforce_cache_limit_inner(app, Some(preserve_path))
}

fn enforce_cache_limit_inner(
    app: &AppHandle,
    preserve_path: Option<&Path>,
) -> Result<CacheCleanupResult, String> {
    let settings = load_cache_settings(app)?;
    let cache_dir = PathBuf::from(&settings.cache_dir);
    ensure_cache_dir(&cache_dir)?;
    require_managed_cache_dir(&cache_dir)?;

    if !settings.auto_cleanup || settings.max_size_mb == 0 {
        return Ok(CacheCleanupResult {
            removed_files: 0,
            removed_bytes: 0,
            used_bytes: cache_size(&cache_dir)?,
            removed_paths: Vec::new(),
        });
    }

    let max_bytes = settings.max_size_mb.saturating_mul(1024 * 1024);
    let threshold_bytes = max_bytes.saturating_mul(CLEANUP_THRESHOLD_PERCENT) / 100;
    let target_bytes = max_bytes.saturating_mul(CLEANUP_TARGET_PERCENT) / 100;
    let mut entries = collect_cache_files(&cache_dir)?;
    let mut used_bytes = entries.iter().map(|entry| entry.size).sum::<u64>();

    if used_bytes < threshold_bytes {
        return Ok(CacheCleanupResult {
            removed_files: 0,
            removed_bytes: 0,
            used_bytes,
            removed_paths: Vec::new(),
        });
    }

    entries.sort_by_key(|entry| entry.modified);
    let preserve_key = preserve_path.map(normalized_path_key);
    let mut removed_paths = Vec::new();
    let mut removed_bytes = 0;

    for entry in entries {
        if used_bytes <= target_bytes {
            break;
        }
        if preserve_key
            .as_deref()
            .is_some_and(|key| key == normalized_path_key(&entry.path))
        {
            continue;
        }

        fs::remove_file(&entry.path).map_err(|err| {
            format!(
                "failed to remove cache file {}: {err}",
                entry.path.display()
            )
        })?;
        used_bytes = used_bytes.saturating_sub(entry.size);
        removed_bytes += entry.size;
        removed_paths.push(entry.path);
    }

    mark_tracks_cache_missing_by_paths(app, &removed_paths)?;
    Ok(CacheCleanupResult {
        removed_files: removed_paths.len(),
        removed_bytes,
        used_bytes,
        removed_paths: removed_paths
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
    })
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
    let mut settings: CacheSettings = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse cache settings {}: {err}", path.display()))?;
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
    fs::write(&path, bytes)
        .map_err(|err| format!("failed to write cache settings {}: {err}", path.display()))
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
    let candidates = [
        install_dir_cache_dir(),
        program_data_cache_dir(),
        Some(app_data_dir.clone()),
    ];

    for candidate in candidates.into_iter().flatten() {
        if ensure_cache_dir_safe(&candidate).is_ok() {
            return Ok(candidate);
        }
    }

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
    if path.as_os_str().is_empty() {
        return Err("缓存路径不能为空".into());
    }
    if path.parent().is_none() {
        return Err("不能把磁盘根目录设置为缓存目录".into());
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

    if path.exists() && path.is_dir() {
        let marker = path.join(CACHE_MARKER_FILE);
        if !marker.is_file() {
            let has_entries = fs::read_dir(path)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false);
            if has_entries {
                return Err(format!(
                    "目录 {} 已包含其他文件，且缺少 Seraph 缓存标记 ({})，\
                     为防止误删请改用空目录或新建子目录作为缓存路径。",
                    path.display(),
                    CACHE_MARKER_FILE
                ));
            }
        }
    }

    fs::create_dir_all(path)
        .map_err(|err| format!("failed to create cache dir {}: {err}", path.display()))?;
    let marker = path.join(CACHE_MARKER_FILE);
    if !marker.is_file() {
        fs::write(&marker, b"Seraph Audio Player managed cache\n")
            .map_err(|err| format!("failed to write cache marker {}: {err}", marker.display()))?;
    }
    Ok(())
}

fn ensure_cache_dir(path: &Path) -> Result<(), String> {
    // 内部调用：默认目录已确认安全，跳过保护检查。
    validate_cache_dir(path)?;
    fs::create_dir_all(path)
        .map_err(|err| format!("failed to create cache dir {}: {err}", path.display()))?;
    let marker = path.join(CACHE_MARKER_FILE);
    if !marker.is_file() {
        fs::write(&marker, b"Seraph Audio Player managed cache\n")
            .map_err(|err| format!("failed to write cache marker {}: {err}", marker.display()))?;
    }
    Ok(())
}

/// 缓存清理 / 扫描的前置检查：若目标目录缺少 marker，拒绝继续。
/// 防止用户误把缓存目录指到本地音乐文件夹后又执行清理操作时，
/// 用扩展名白名单按 mtime 批量删除真实音乐文件。
fn require_managed_cache_dir(path: &Path) -> Result<(), String> {
    let marker = path.join(CACHE_MARKER_FILE);
    if !marker.is_file() {
        return Err(format!(
            "拒绝在未标记为 Seraph 缓存的目录 {} 上执行清理操作；\
             请到设置里重新指定一个由 Seraph 管理的缓存目录。",
            path.display()
        ));
    }
    Ok(())
}

fn cache_size(path: &Path) -> Result<u64, String> {
    Ok(collect_cache_files(path)?
        .into_iter()
        .map(|entry| entry.size)
        .sum())
}

fn collect_cache_files(path: &Path) -> Result<Vec<CacheFile>, String> {
    let mut files = Vec::new();
    collect_cache_files_inner(path, &mut files)?;
    Ok(files)
}

fn collect_cache_files_inner(path: &Path, files: &mut Vec<CacheFile>) -> Result<(), String> {
    if !path.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(path)
        .map_err(|err| format!("failed to read cache dir {}: {err}", path.display()))?
    {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_cache_files_inner(&path, files)?;
            continue;
        }
        if !is_managed_cache_file(&path) {
            continue;
        }

        let metadata = fs::metadata(&path)
            .map_err(|err| format!("failed to read cache file {}: {err}", path.display()))?;
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
                "m4a" | "flac" | "opus" | "aac" | "mp3" | "download" | "tmp"
            )
        })
        .unwrap_or(false)
}

/// 扫描已知缓存目录下，长时间未被修改的 `.download` / `.tmp`，删除掉孤儿临时文件，
/// 防止下载中断 / ffmpeg 崩溃后的残骸不停堆积。仅在 marker 目录内执行。
fn sweep_orphan_temp_files(path: &Path) -> std::io::Result<()> {
    let marker = path.join(CACHE_MARKER_FILE);
    if !marker.is_file() {
        return Ok(());
    }
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
        let modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
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
