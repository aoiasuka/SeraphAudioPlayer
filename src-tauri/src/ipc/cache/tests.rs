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
    assert!(cleanup_cache_files(&dir.0, &[], &[], CleanupMode::Clear, &lease).is_err());
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
        &[],
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
        let result = cleanup_cache_files(&dir.0, &[], &[], mode, &lease).unwrap();
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
    let result = cleanup_cache_files(&dir.0, &[], &[], CleanupMode::Clear, &lease).unwrap();
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
    let result = cleanup_cache_files(&dir.0, &[], &[], CleanupMode::Clear, &lease).unwrap();
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

    let result =
        cleanup_cache_files(&dir.0, &[], &[], CleanupMode::Quota(1_200), &cleanup).unwrap();
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
    let result = cleanup_cache_files(&dir.0, &[], &preserve, CleanupMode::Clear, &cleanup).unwrap();
    assert_eq!(result.removed_files, 1);
    assert!(playing.exists() && !other.exists());
}

// ───────────── CACHE-03：换目录后旧目录继续受管 ─────────────

fn settings_at(dir: &Path) -> CacheSettings {
    CacheSettings {
        cache_dir: dir.to_string_lossy().to_string(),
        max_size_mb: DEFAULT_MAX_SIZE_MB,
        auto_cleanup: true,
        previous_dirs: Vec::new(),
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[test]
fn switching_cache_dir_keeps_the_old_one_on_record() {
    let old = TestDir::new();
    old.managed();
    old.file("downloaded.flac", 10, 100);
    let new = TestDir::new();
    new.managed();
    let mut settings = settings_at(&old.0);

    reject_overlapping_cache_dir(&settings, &new.0).unwrap();
    switch_cache_dir(&mut settings, &new.0);
    assert_eq!(settings.cache_dir, text(&new.0));
    assert_eq!(settings.previous_dirs, vec![text(&old.0)]);
    assert_eq!(previous_cache_dirs(&settings), vec![old.0.clone()]);

    // 换回以前用过的目录：它变回当前目录，不再留在旧目录清单里；
    // 刚离开的那个目录里有文件，轮到它被记下。
    new.file("later.m4a", 10, 50);
    reject_overlapping_cache_dir(&settings, &old.0).unwrap();
    switch_cache_dir(&mut settings, &old.0);
    assert_eq!(settings.cache_dir, text(&old.0));
    assert_eq!(settings.previous_dirs, vec![text(&new.0)]);
}

#[test]
fn switching_does_not_record_dirs_with_nothing_left_to_manage() {
    // 空的旧目录：没有文件可管，不记
    let empty = TestDir::new();
    empty.managed();
    let new = TestDir::new();
    new.managed();
    let mut settings = settings_at(&empty.0);
    switch_cache_dir(&mut settings, &new.0);
    assert!(settings.previous_dirs.is_empty());

    // 清单里已经删掉的目录、丢了标记的目录，换目录时顺手剔除
    let gone = TestDir::new();
    let gone_path = gone.0.clone();
    drop(gone);
    let unmarked = TestDir::new();
    unmarked.file("user-music.flac", 10, 10);
    let kept = TestDir::new();
    kept.managed();
    kept.file("cached.opus", 10, 10);
    let next = TestDir::new();
    next.managed();
    settings.previous_dirs = vec![text(&gone_path), text(&unmarked.0), text(&kept.0)];
    switch_cache_dir(&mut settings, &next.0);
    assert_eq!(settings.cache_dir, text(&next.0));
    assert_eq!(settings.previous_dirs, vec![text(&kept.0)]);
}

#[test]
fn respelling_the_same_dir_is_not_a_switch() {
    let dir = TestDir::new();
    dir.managed();
    dir.file("a.flac", 10, 10);
    let mut settings = settings_at(&dir.0);
    let respelled = PathBuf::from(format!("{}/", text(&dir.0).to_uppercase()));
    reject_overlapping_cache_dir(&settings, &respelled).unwrap();
    switch_cache_dir(&mut settings, &respelled);
    assert!(settings.previous_dirs.is_empty());
    assert_eq!(settings.cache_dir, text(&respelled));
}

#[test]
fn nested_cache_dirs_are_rejected_before_anything_is_created() {
    let old = TestDir::new();
    old.managed();
    old.file("a.flac", 10, 10);
    let other = TestDir::new();
    other.managed();
    other.file("b.flac", 10, 10);
    let mut settings = settings_at(&old.0);
    settings.previous_dirs = vec![text(&other.0)];

    // 当前目录里面、当前目录的上级、旧目录里面，都不行
    let inside_current = old.0.join("sub");
    let inside_previous = other.0.join("deeper").join("still");
    for candidate in [
        inside_current.clone(),
        old.0.parent().unwrap().to_path_buf(),
        inside_previous.clone(),
    ] {
        let err = reject_overlapping_cache_dir(&settings, &candidate).unwrap_err();
        assert!(err.contains("互不包含"), "{err}");
    }
    // 检查发生在建目录、写标记之前：被拒绝的目录没有留下痕迹
    assert!(!inside_current.exists() && !inside_previous.exists());

    // 只是名字有共同前缀的兄弟目录不算包含
    let sibling = PathBuf::from(format!("{}-2", text(&old.0)));
    reject_overlapping_cache_dir(&settings, &sibling).unwrap();
    assert!(!cache_dirs_overlap(&old.0, &sibling));
    assert!(cache_dirs_overlap(&old.0, &inside_current));
    assert!(cache_dirs_overlap(&inside_current, &old.0));
}

#[test]
fn quota_evicts_the_least_recently_played_file_across_current_and_previous_dirs() {
    let current = TestDir::new();
    current.managed();
    let previous = TestDir::new();
    previous.managed();
    // 旧目录里的文件最久没播放：先淘汰它，而不是当前目录里更新的文件
    let stale = previous.file("stale.flac", 400, 3_000);
    let warm = current.file("warm.flac", 400, 2_000);
    let fresh = previous.file("fresh.flac", 400, 1_000);
    fs::write(previous.0.join(".stale.flac.ok"), b"").unwrap();
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();

    // 只看当前目录时用量 400，远低于阈值，不会清理——这正是 CACHE-03 修之前的行为
    let alone =
        cleanup_cache_files(&current.0, &[], &[], CleanupMode::Quota(1_200), &lease).unwrap();
    assert_eq!((alone.used_bytes, alone.removed_files), (400, 0));

    let result = cleanup_cache_files(
        &current.0,
        std::slice::from_ref(&previous.0),
        &[],
        CleanupMode::Quota(1_200),
        &lease,
    )
    .unwrap();
    assert_eq!(result.removed_paths, vec![text(&stale)]);
    assert_eq!(result.used_bytes, 800);
    assert!(!stale.exists() && !previous.0.join(".stale.flac.ok").exists());
    assert!(warm.is_file() && fresh.is_file());
}

#[test]
fn clear_empties_previous_dirs_too_and_measure_counts_them() {
    let current = TestDir::new();
    current.managed();
    let previous = TestDir::new();
    previous.managed();
    let a = current.file("a.m4a", 30, 10);
    let b = previous.file("b.m4a", 50, 10);
    let playing = previous.file("playing.m4a", 20, 10);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    let dirs = [previous.0.clone()];

    let measured =
        cleanup_cache_files(&current.0, &dirs, &[], CleanupMode::Measure, &lease).unwrap();
    assert_eq!((measured.used_bytes, measured.removed_files), (100, 0));

    let preserve = [playing.clone()];
    let result =
        cleanup_cache_files(&current.0, &dirs, &preserve, CleanupMode::Clear, &lease).unwrap();
    assert_eq!(result.removed_files, 2);
    assert!(!a.exists() && !b.exists());
    // 旧目录里正在播放的文件同样保留
    assert!(playing.is_file());
    // 目录和标记留着；还有文件的旧目录继续留在清单里，清空了的不再需要
    assert!(previous.0.join(CACHE_MARKER_FILE).is_file());
    assert!(previous_dir_still_relevant(&previous.0));
    fs::remove_file(&playing).unwrap();
    assert!(!previous_dir_still_relevant(&previous.0));
}

#[test]
fn a_previous_dir_that_lost_its_marker_is_never_cleaned() {
    let current = TestDir::new();
    current.managed();
    let cached = current.file("cached.flac", 10, 10);
    // 旧目录的标记没了：它现在可能就是用户自己的音乐文件夹
    let unmarked = TestDir::new();
    let user_song = unmarked.file("my-song.flac", 500, 9_000);
    // 记录里的旧目录已经整个不存在
    let gone = TestDir::new();
    let gone_path = gone.0.clone();
    drop(gone);
    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    let dirs = [unmarked.0.clone(), gone_path.clone()];

    assert!(scan_previous_cache_dir(&unmarked.0).is_err());
    let status = previous_dir_status(&unmarked.0);
    assert_eq!((status.used_bytes, status.file_count), (0, 0));
    assert!(status.unavailable_reason.is_some());

    let result = cleanup_cache_files(&current.0, &dirs, &[], CleanupMode::Clear, &lease).unwrap();
    assert_eq!(result.removed_paths, vec![text(&cached)]);
    assert!(result.errors.is_empty());
    assert!(user_song.is_file());
    assert!(!unmarked.0.join(CACHE_MARKER_FILE).exists());
    assert!(!gone_path.exists());
    // 两个都确定没有东西可管了，下次整理清单时剔除
    assert!(!previous_dir_still_relevant(&unmarked.0));
    assert!(!previous_dir_still_relevant(&gone_path));
}

#[test]
fn previous_dir_status_reports_its_own_usage() {
    let dir = TestDir::new();
    dir.managed();
    dir.file("a.flac", 30, 10);
    dir.file("b.eac3", 12, 10);
    dir.file("notes.txt", 99, 10);
    let status = previous_dir_status(&dir.0);
    assert_eq!(status.path, text(&dir.0));
    assert_eq!((status.used_bytes, status.file_count), (42, 2));
    assert!(status.unavailable_reason.is_none());
}

#[cfg(windows)]
#[test]
fn previous_dir_on_a_missing_volume_stays_on_record_and_is_skipped() {
    let Some(root) = missing_drive_root() else {
        return;
    };
    let unplugged = root.join("seraph-cache");
    // 盘没接：留在清单里（接回后继续管理），但这次不计入、不清理
    assert!(previous_dir_still_relevant(&unplugged));
    assert!(scan_previous_cache_dir(&unplugged)
        .unwrap_err()
        .contains("磁盘当前不可用"));

    // 从不可用的盘换到别处（CACHE-01 的场景）：原目录照样记下
    let new = TestDir::new();
    new.managed();
    let mut settings = settings_at(&unplugged);
    reject_overlapping_cache_dir(&settings, &new.0).unwrap();
    switch_cache_dir(&mut settings, &new.0);
    assert_eq!(settings.previous_dirs, vec![text(&unplugged)]);

    let registry = OperationRegistry::default();
    let lease = registry.acquire_cleanup().unwrap();
    let cached = new.file("cached.flac", 10, 10);
    let result = cleanup_cache_files(
        &new.0,
        &previous_cache_dirs(&settings),
        &[],
        CleanupMode::Clear,
        &lease,
    )
    .unwrap();
    assert_eq!(result.removed_files, 1);
    assert!(!cached.exists());
}

#[test]
fn overlapping_entries_in_a_hand_edited_list_are_never_scanned_twice() {
    let current = TestDir::new();
    current.managed();
    let other = TestDir::new();
    other.managed();
    let mut settings = settings_at(&current.0);
    settings.previous_dirs = vec![
        // 当前目录自己、当前目录的子目录、当前目录的上级
        text(&current.0),
        text(&current.0.join("sub")),
        text(current.0.parent().unwrap()),
        // 同一个旧目录写了两遍（第二遍换了大小写）
        text(&other.0),
        text(&other.0).to_uppercase(),
    ];
    assert_eq!(previous_cache_dirs(&settings), vec![other.0.clone()]);

    // 清单超长时只取前 MAX_PREVIOUS_CACHE_DIRS 个
    let many = (0..MAX_PREVIOUS_CACHE_DIRS + 4)
        .map(|index| text(&other.0.with_file_name(format!("seraph-prev-{index}"))))
        .collect::<Vec<_>>();
    settings.previous_dirs = many;
    assert_eq!(
        previous_cache_dirs(&settings).len(),
        MAX_PREVIOUS_CACHE_DIRS
    );
}

#[test]
fn only_the_newest_retired_dirs_are_kept_when_there_are_too_many() {
    let dirs = (0..MAX_PREVIOUS_CACHE_DIRS + 2)
        .map(|_| {
            let dir = TestDir::new();
            dir.managed();
            dir.file("cached.flac", 1, 10);
            dir
        })
        .collect::<Vec<_>>();
    let mut settings = settings_at(&dirs[0].0);
    for dir in &dirs[1..] {
        switch_cache_dir(&mut settings, &dir.0);
    }
    // 一共离开过 MAX + 1 个目录，最早的那个被丢掉
    assert_eq!(settings.previous_dirs.len(), MAX_PREVIOUS_CACHE_DIRS);
    assert_eq!(settings.previous_dirs[0], text(&dirs[1].0));
    assert_eq!(
        settings.previous_dirs.last().unwrap(),
        &text(&dirs[MAX_PREVIOUS_CACHE_DIRS].0)
    );
}

#[test]
fn settings_file_from_before_cache_03_still_loads_and_empty_list_is_not_written() {
    let legacy = r#"{"cacheDir":"D:\\cache","maxSizeMb":5120,"autoCleanup":true}"#;
    let settings: CacheSettings = serde_json::from_str(legacy).unwrap();
    assert!(settings.previous_dirs.is_empty());
    assert!(!serde_json::to_string(&settings)
        .unwrap()
        .contains("previousDirs"));

    let mut with_previous = settings.clone();
    with_previous.previous_dirs = vec![r"E:\old-cache".into()];
    let json = serde_json::to_string(&with_previous).unwrap();
    assert!(
        json.contains(r#""previousDirs":["E:\\old-cache"]"#),
        "{json}"
    );
    let back: CacheSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.previous_dirs, with_previous.previous_dirs);
}
