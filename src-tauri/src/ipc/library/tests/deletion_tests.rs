use super::super::deletion::delete_selected_tracks;
use super::*;
use crate::ipc::{bilibili::acquire_audio_operation, cache::delete_streaming_cache_file};

fn managed_cache(directory: &TestLibraryDir) -> PathBuf {
    let cache = directory.0.join("managed-cache");
    fs::create_dir(&cache).unwrap();
    fs::write(
        cache.join(".seraph-cache"),
        b"Seraph Audio Player managed cache\n",
    )
    .unwrap();
    cache
}

fn streaming_track(id: &str, path: &Path) -> ImportedTrack {
    static SOURCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let mut track = test_imported_track(id, &path.to_string_lossy(), id);
    track.album = "Bilibili".into();
    track.source_id = Some(format!(
        "BV{:010}",
        SOURCE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    track
}

fn audio_with_sentinel(path: &Path) -> PathBuf {
    fs::write(path, b"temporary test audio").unwrap();
    let sentinel = path.with_file_name(format!(
        ".{}.ok",
        path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&sentinel, b"").unwrap();
    sentinel
}

#[test]
fn batch_delete_removes_streaming_files_preserves_local_files_and_commits_once() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let audio = cache.join("selected.flac.m4a");
    let sentinel = audio_with_sentinel(&audio);
    let keep_audio = cache.join("unselected.m4a");
    audio_with_sentinel(&keep_audio);
    let local_audio = dir.0.join("original.flac");
    fs::write(&local_audio, b"original local music").unwrap();
    let stream = streaming_track("stream", &audio);
    let source = stream.source_id.clone().unwrap();
    let tracks = vec![
        test_imported_track("local", &local_audio.to_string_lossy(), "Local"),
        stream,
        streaming_track("keep", &keep_audio),
    ];
    let storage = dir.storage();
    storage.save(&tracks, None).unwrap();
    let mut saves = 0;
    let result = delete_selected_tracks(
        tracks,
        &["local".into(), "stream".into(), "stream".into()],
        |updated| {
            saves += 1;
            assert!(
                acquire_audio_operation(&source).is_err(),
                "提交存储前仍持有来源占用"
            );
            storage.save(updated, None).map(|_| ())
        },
    )
    .unwrap();
    assert_eq!(saves, 1);
    assert_eq!(result.deleted_ids, ["local", "stream"]);
    assert_eq!(result.deleted_files, 1);
    assert!(result.failures.is_empty());
    assert!(!audio.exists());
    assert!(!sentinel.exists());
    assert!(local_audio.is_file());
    assert!(keep_audio.is_file());
    assert!(cache.join(".seraph-cache").is_file());
    assert_eq!(
        storage
            .load()
            .unwrap()
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        ["keep"]
    );
    assert!(acquire_audio_operation(&source).is_ok(), "完成后释放占用");
}

#[test]
fn batch_delete_missing_files_and_records_is_idempotent_without_creating_cache_markers() {
    let dir = TestLibraryDir::new();
    let missing = dir.0.join("old-cache").join("missing.m4a");
    let result = delete_selected_tracks(
        vec![streaming_track("missing", &missing)],
        &["missing".into(), "already-deleted".into()],
        |updated| {
            assert!(updated.is_empty());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result.deleted_ids, ["missing", "already-deleted"]);
    assert_eq!(result.deleted_files, 0);
    assert!(result.failures.is_empty());
    assert!(!dir.0.join("old-cache").exists());
}

#[test]
fn batch_delete_keeps_failed_records_and_still_commits_successful_items() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let protected = dir.0.join("outside.m4a");
    audio_with_sentinel(&protected);
    let good = cache.join("good.m4a");
    audio_with_sentinel(&good);
    let result = delete_selected_tracks(
        vec![
            streaming_track("protected", &protected),
            streaming_track("good", &good),
        ],
        &["protected".into(), "good".into()],
        |updated| {
            assert_eq!(updated.len(), 1);
            assert_eq!(updated[0].id, "protected");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result.deleted_ids, ["good"]);
    assert_eq!(result.deleted_files, 1);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].id, "protected");
    assert!(result.failures[0].message.contains("管理的缓存目录"));
    assert!(protected.is_file());
    assert!(!good.exists());
    assert!(!dir.0.join(".seraph-cache").exists());
}

#[test]
fn batch_delete_refuses_active_download_and_succeeds_after_operation_finishes() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let audio = cache.join("recaching.flac.m4a");
    let sentinel = audio_with_sentinel(&audio);
    let track = streaming_track("recaching", &audio);
    let slot =
        acquire_audio_operation(&track.source_id.as_ref().unwrap().to_ascii_uppercase()).unwrap();
    let result =
        delete_selected_tracks(vec![track.clone()], std::slice::from_ref(&track.id), |_| {
            panic!("失败时不提交")
        })
        .unwrap();
    assert!(result.deleted_ids.is_empty());
    assert_eq!(result.failures.len(), 1);
    assert!(result.failures[0].message.contains("正在下载"));
    assert!(audio.exists() && sentinel.exists());
    drop(slot);
    let result = delete_selected_tracks(vec![track], &["recaching".into()], |_| Ok(())).unwrap();
    assert_eq!(result.deleted_files, 1);
    assert!(!audio.exists() && !sentinel.exists());
}

#[test]
fn batch_delete_protects_files_shared_with_unselected_tracks() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let audio = cache.join("shared.m4a");
    audio_with_sentinel(&audio);
    let stream = streaming_track("selected", &audio);
    let mut other = stream.clone();
    other.id = "other".into();
    let tracks = vec![stream, other];
    let result = delete_selected_tracks(tracks.clone(), &["selected".into()], |_| {
        panic!("共享文件仍需保留")
    })
    .unwrap();
    assert_eq!(result.failures.len(), 1);
    assert!(audio.is_file());
    let result = delete_selected_tracks(tracks, &["selected".into(), "other".into()], |updated| {
        assert!(updated.is_empty());
        Ok(())
    })
    .unwrap();
    assert_eq!(result.deleted_ids.len(), 2);
    assert_eq!(result.deleted_files, 1);
    assert!(!audio.exists());
}

#[test]
fn batch_delete_reports_storage_failure_and_retains_persisted_records() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let audio = cache.join("save-failure.m4a");
    audio_with_sentinel(&audio);
    let tracks = vec![streaming_track("save-failure", &audio)];
    let storage = dir.storage();
    storage.save(&tracks, None).unwrap();
    let err = delete_selected_tracks(tracks.clone(), &["save-failure".into()], |_| {
        Err("磁盘写入失败".into())
    })
    .unwrap_err();
    assert!(err.contains("保存曲库失败"));
    assert_eq!(storage.load().unwrap(), tracks);
}

#[test]
fn cache_delete_rejects_traversal_and_non_audio_or_non_file_targets() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let original = dir.0.join("original.m4a");
    audio_with_sentinel(&original);
    assert!(delete_streaming_cache_file(&cache.join("..").join("original.m4a")).is_err());
    assert!(original.is_file());
    let temp = cache.join("pending.download");
    fs::write(&temp, b"pending").unwrap();
    assert!(delete_streaming_cache_file(&temp).is_err());
    assert!(temp.is_file());
    let folder = cache.join("folder.m4a");
    fs::create_dir(&folder).unwrap();
    assert!(delete_streaming_cache_file(&folder).is_err());
    assert!(folder.is_dir());
    let audio = cache.join("bad-sentinel.m4a");
    fs::write(&audio, b"audio").unwrap();
    fs::create_dir(cache.join(".bad-sentinel.m4a.ok")).unwrap();
    assert!(delete_streaming_cache_file(&audio).is_err());
    assert!(audio.is_file());
}

#[cfg(windows)]
#[test]
fn batch_delete_keeps_locked_windows_audio_for_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let audio = cache.join("locked.m4a");
    audio_with_sentinel(&audio);
    let track = streaming_track("locked", &audio);
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&audio)
        .unwrap();
    let result = delete_selected_tracks(vec![track.clone()], &["locked".into()], |_| {
        panic!("占用时保留记录")
    })
    .unwrap();
    assert!(result.deleted_ids.is_empty());
    assert_eq!(result.failures.len(), 1);
    assert!(audio.is_file());
    drop(file);
    let result = delete_selected_tracks(vec![track], &["locked".into()], |_| Ok(())).unwrap();
    assert_eq!(result.deleted_ids, ["locked"]);
    assert!(!audio.exists());
}

#[cfg(any(unix, windows))]
#[test]
fn cache_delete_refuses_symlink_or_windows_junction_ancestors() {
    let dir = TestLibraryDir::new();
    let cache = managed_cache(&dir);
    let originals = dir.0.join("originals");
    fs::create_dir(&originals).unwrap();
    let audio = originals.join("protected.m4a");
    audio_with_sentinel(&audio);
    let link = cache.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&originals, &link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&originals)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let outcome = delete_streaming_cache_file(&link.join("protected.m4a"));
    // 仅移除自行创建的链接，原始临时目录由 TestLibraryDir 按固定路径回收。
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    assert!(outcome.unwrap_err().contains("链接或重解析点"));
    assert!(audio.is_file());
}
