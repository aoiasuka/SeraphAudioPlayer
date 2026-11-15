use super::*;
use operations::OperationRegistry;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "seraph-cache-tests-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn managed(&self) {
        ensure_cache_dir_safe(&self.0).unwrap();
    }

    fn file(&self, name: &str, size: usize, age_seconds: u64) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, vec![0_u8; size]).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(age_seconds))
            .unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 找一个本机不存在的盘符；全部占用（极少见）时返回 None，测试跳过。
#[cfg(windows)]
fn missing_drive_root() -> Option<PathBuf> {
    (b'D'..=b'Z')
        .rev()
        .map(|letter| PathBuf::from(format!("{}:/", letter as char)))
        .find(|root| fs::metadata(root).is_err())
}

#[cfg(windows)]
#[test]
fn saved_dir_on_missing_volume_is_unreachable_not_unsafe() {
    let Some(root) = missing_drive_root() else {
        return;
    };
    let dir = root.join("seraph-cache");
    assert!(cache_volume_unreachable(&dir));
    assert!(matches!(
        saved_dir_state(&dir),
        SavedDirState::Unreachable(_)
    ));
}

#[test]
fn saved_dir_on_present_volume_keeps_strict_safety_checks() {
    let dir = TestDir::new();
    assert!(!cache_volume_unreachable(&dir.0));
    // 卷在、目录非空且无标记：仍按不安全处理，不因 CACHE-01 放宽。
    fs::write(dir.0.join("song.flac"), b"user file").unwrap();
    assert!(matches!(saved_dir_state(&dir.0), SavedDirState::Unsafe(_)));
    assert!(!cache_volume_unreachable(Path::new("relative/dir")));
    let managed = TestDir::new();
    assert_eq!(saved_dir_state(&managed.0), SavedDirState::Usable);
}

#[test]
fn unmarked_nonempty_directory_is_neither_adopted_nor_cleaned() {
    let dir = TestDir::new();
    let song = dir.file("my-music.flac", 20, 0);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    assert!(ensure_cache_dir(&dir.0).is_err());
    assert!(cleanup_cache_files(&dir.0, &[], CleanupMode::Clear, &lease).is_err());
    assert!(song.is_file());
    assert!(!dir.0.join(CACHE_MARKER_FILE).exists());
}

#[test]
fn quota_removes_oldest_files_until_target_but_preserves_requested_files() {
    let dir = TestDir::new();
    dir.managed();
    let preserved = dir.file("preserved.flac", 40, 200);
    let old = dir.file("old.m4a", 30, 100);
    let recent = dir.file("recent.opus", 30, 0);
    fs::write(dir.0.join(".old.m4a.ok"), b"").unwrap();
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    let result = cleanup_cache_files(
        &dir.0,
        std::slice::from_ref(&preserved),
        CleanupMode::Quota(100),
        &lease,
    )
    .unwrap();
    assert_eq!(result.removed_files, 1);
    assert_eq!(result.removed_bytes, 30);
    assert_eq!(result.used_bytes, 70);
    assert!(!old.exists());
    assert!(!dir.0.join(".old.m4a.ok").exists());
    assert!(preserved.is_file() && recent.is_file());
}

#[test]
fn quota_below_threshold_and_measure_only_never_delete() {
    let dir = TestDir::new();
    dir.managed();
    let song = dir.file("below.m4a", 89, 100);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    for mode in [CleanupMode::Quota(100), CleanupMode::Measure] {
        let result = cleanup_cache_files(&dir.0, &[], mode, &lease).unwrap();
        assert_eq!(result.removed_files, 0);
        assert_eq!(result.used_bytes, 89);
        assert!(song.is_file());
    }
}

#[test]
fn clear_removes_zero_byte_temporary_files_but_not_unrelated_files() {
    let dir = TestDir::new();
    dir.managed();
    let temporary = dir.file("unfinished.download", 0, 10);
    let unrelated = dir.file("notes.txt", 3, 10);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    let result = cleanup_cache_files(&dir.0, &[], CleanupMode::Clear, &lease).unwrap();
    assert_eq!(result.removed_files, 1);
    assert!(!temporary.exists());
    assert!(unrelated.is_file());
    assert!(dir.0.join(CACHE_MARKER_FILE).is_file());
}

#[test]
fn bad_completion_marker_preserves_its_audio_and_does_not_abort_other_deletions() {
    let dir = TestDir::new();
    dir.managed();
    let blocked = dir.file("blocked.m4a", 20, 10);
    fs::create_dir(dir.0.join(".blocked.m4a.ok")).unwrap();
    let other = dir.file("other.m4a", 10, 0);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    let result = cleanup_cache_files(&dir.0, &[], CleanupMode::Clear, &lease).unwrap();
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.removed_files, 1);
    assert!(blocked.is_file());
    assert!(!other.exists());
}

#[test]
fn orphan_sweep_removes_only_old_temporaries() {
    let dir = TestDir::new();
    dir.managed();
    let old = dir.file("old.download", 3, 7200);
    let fresh = dir.file("new.tmp", 3, 0);
    let audio = dir.file("audio.m4a", 3, 7200);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    sweep_orphan_temp_files(&dir.0, &lease).unwrap();
    assert!(!old.exists());
    assert!(fresh.is_file() && audio.is_file());
}

#[test]
fn cache_paths_reject_relative_paths_and_parent_components() {
    assert!(validate_cache_dir(Path::new("music/cache")).is_err());
    assert!(validate_cache_dir(Path::new("")).is_err());
    let dir = TestDir::new();
    assert!(validate_cache_dir(&dir.0.join("child/../other")).is_err());
    assert!(validate_cache_dir(&dir.0).is_ok());
    #[cfg(windows)]
    for path in [r"C:\", r"\\server\share\cache", r"C:\cache:stream"] {
        assert!(validate_cache_dir(Path::new(path)).is_err(), "{path}");
    }
}

#[test]
fn cache_marker_must_be_a_regular_file() {
    let dir = TestDir::new();
    fs::create_dir(dir.0.join(CACHE_MARKER_FILE)).unwrap();
    assert!(require_managed_cache_dir(&dir.0).is_err());
    assert!(ensure_cache_dir_safe(&dir.0).is_err());
}

#[test]
fn playback_touch_makes_quota_evict_least_recently_played_not_oldest_download() {
    let dir = TestDir::new();
    dir.managed();
    let registry = OperationRegistry::default();
    let cleanup = registry.acquire_cleanup().unwrap();
    // old 最早下载，但刚刚播放过；mid 下载得晚一些却没再听过
    let old = dir.file("old.flac", 400, 3_000);
    let mid = dir.file("mid.flac", 400, 2_000);
    let new = dir.file("new.flac", 400, 1_000);
    touch_managed_cache_file(&old).unwrap();

    let result = cleanup_cache_files(&dir.0, &[], CleanupMode::Quota(1_200), &cleanup).unwrap();
    assert_eq!(
        result.removed_paths,
        vec![mid.to_string_lossy().to_string()]
    );
    assert!(old.exists() && new.exists());
}

#[test]
fn playback_touch_leaves_unmanaged_files_alone() {
    let dir = TestDir::new();
    // 没有缓存标记的目录 = 用户自己的音乐文件夹：修改时间不能被改
    let song = dir.file("song.flac", 16, 5_000);
    let before = fs::metadata(&song).unwrap().modified().unwrap();
    touch_managed_cache_file(&song).unwrap();
    assert_eq!(fs::metadata(&song).unwrap().modified().unwrap(), before);
    touch_managed_cache_file(Path::new("relative.flac")).unwrap();
    touch_managed_cache_file(&dir.0.join("missing.flac")).unwrap();
}

#[test]
fn now_playing_file_is_preserved_even_by_clear() {
    let dir = TestDir::new();
    dir.managed();
    let registry = OperationRegistry::default();
    let cleanup = registry.acquire_cleanup().unwrap();
    let playing = dir.file("playing.flac", 64, 9_000);
    let other = dir.file("other.flac", 64, 10);
    let preserve = vec![playing.clone()];
    let result = cleanup_cache_files(&dir.0, &preserve, CleanupMode::Clear, &cleanup).unwrap();
    assert_eq!(result.removed_files, 1);
    assert!(playing.exists() && !other.exists());
}
