//! library 模块单元测试。已从 include! 迁出为独立 `mod tests`，
//! 共享项经 `super::prelude::*`（其 glob 汇聚各子模块 pub(crate) 项）引入。
#![cfg(test)]
use super::deletion::delete_selected_tracks;
use super::prelude::*;
use serde_json::json;
use std::fs;

mod deletion_tests;

struct TestLibraryDir(PathBuf);

#[test]
fn playlist_summary_keeps_metadata_without_cloning_or_serializing_lyrics() {
    use super::snapshot::{LibrarySnapshot, PlaylistSnapshot};
    let mut track = test_imported_track("a", "C:/a.flac", "A");
    track
        .lyrics
        .push(LyricLine::new(1.0, "long lyric".repeat(1000)));
    let snapshot = std::sync::Arc::new(LibrarySnapshot::new(vec![track.clone()]));
    let summary = PlaylistSnapshot {
        snapshot: snapshot.clone(),
        include_lyrics: false,
    };
    let encoded = serde_json::to_vec(&summary).unwrap();
    let restored: Vec<ImportedTrack> = serde_json::from_slice(&encoded).unwrap();
    let mut expected = track.clone();
    expected.lyrics.clear();
    assert_eq!(restored, vec![expected]);
    assert!(encoded.len() < 1000);
    assert_eq!(snapshot.get("a").unwrap().lyrics, track.lyrics);
    let full = PlaylistSnapshot {
        snapshot,
        include_lyrics: true,
    };
    assert_eq!(
        serde_json::from_slice::<Vec<ImportedTrack>>(&serde_json::to_vec(&full).unwrap()).unwrap(),
        vec![track]
    );
}

impl TestLibraryDir {
    fn new() -> Self {
        let path = temp_audio_path("seraph-snapshot", "dir");
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn storage(&self) -> super::storage::LibraryStorage {
        super::storage::LibraryStorage::new(&self.0)
    }
}

impl Drop for TestLibraryDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn snapshot_storage_reuses_unchanged_components_and_keeps_legacy_backup() {
    let dir = TestLibraryDir::new();
    let storage = dir.storage();
    let mut tracks = vec![test_imported_track("a", "C:/a.flac", "A")];
    tracks[0].lyrics.push(LyricLine::new(1.0, "第一行"));
    let legacy_bytes = serde_json::to_vec(&tracks).unwrap();
    fs::write(dir.0.join("library-cache.json"), &legacy_bytes).unwrap();
    assert_eq!(storage.load().unwrap(), tracks);
    storage.save(&tracks, None).unwrap();
    assert_eq!(storage.load().unwrap(), tracks);
    assert_eq!(
        fs::read(dir.0.join("library-cache.json")).unwrap(),
        legacy_bytes
    );

    let mut updated = tracks.clone();
    updated[0].cover = "C:/cover.jpg".into();
    let stats = storage.save(&updated, Some(&tracks)).unwrap();
    assert!(stats.metadata_bytes > 0);
    assert_eq!(stats.lyrics_bytes, 0, "只改封面不能重写歌词");
    assert_eq!(storage.load().unwrap(), updated);

    tracks = updated.clone();
    updated[0].lyrics[0].text = "新的歌词".into();
    let stats = storage.save(&updated, Some(&tracks)).unwrap();
    assert_eq!(stats.metadata_bytes, 0, "只改歌词不能重写元数据");
    assert!(stats.lyrics_bytes > 0);
    assert_eq!(storage.load().unwrap(), updated);
    let manifest = fs::read(dir.0.join("library-snapshot.json")).unwrap();
    let stats = storage.save(&updated, Some(&updated)).unwrap();
    assert_eq!((stats.metadata_bytes, stats.lyrics_bytes), (0, 0));
    assert_eq!(
        fs::read(dir.0.join("library-snapshot.json")).unwrap(),
        manifest
    );

    let original = updated.clone();
    updated[0].lyrics.clear();
    storage.save(&updated, Some(&original)).unwrap();
    assert!(
        storage.load().unwrap()[0].lyrics.is_empty(),
        "清空歌词也必须提交"
    );
}

#[test]
fn snapshot_storage_survives_failure_at_every_commit_stage() {
    for fail_at in 1..=4 {
        let dir = TestLibraryDir::new();
        let storage = dir.storage();
        let mut old = vec![test_imported_track("a", "C:/a.flac", "old")];
        old[0].lyrics.push(LyricLine::new(1.0, "old lyric"));
        storage.save(&old, None).unwrap();
        let mut updated = old.clone();
        updated[0].title = "new".into();
        updated[0].lyrics[0].text = "new lyric".into();
        let mut stage = 0;
        let result = storage.save_with(&updated, Some(&old), |path, bytes| {
            stage += 1;
            if stage == fail_at {
                return Err("模拟磁盘写入失败".into());
            }
            write_json_atomic(path, bytes)
        });
        assert!(result.is_err());
        assert_eq!(
            dir.storage().load().unwrap(),
            old,
            "第 {fail_at} 阶段失败不能读到混合版本"
        );
        storage.save(&updated, Some(&old)).unwrap();
        assert_eq!(dir.storage().load().unwrap(), updated);
        assert!(
            fs::read_dir(dir.0.join("library-snapshots"))
                .unwrap()
                .count()
                <= 4
        );
    }
}

#[test]
fn snapshot_storage_recovers_a_complete_previous_generation() {
    for corrupt_manifest in [false, true] {
        let dir = TestLibraryDir::new();
        let storage = dir.storage();
        let old = vec![test_imported_track("a", "C:/a.flac", "old")];
        storage.save(&old, None).unwrap();
        let mut updated = old.clone();
        updated[0].title = "new".into();
        updated[0].lyrics.push(LyricLine::new(1.0, "new lyric"));
        storage.save(&updated, Some(&old)).unwrap();
        let manifest_path = dir.0.join("library-snapshot.json");
        let corrupt_path = if corrupt_manifest {
            manifest_path
        } else {
            let manifest: Value =
                serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
            dir.0
                .join("library-snapshots")
                .join(manifest["lyrics"].as_str().unwrap())
        };
        fs::write(&corrupt_path, b"{broken").unwrap();
        assert_eq!(storage.load().unwrap(), old);
        assert_eq!(dir.storage().load().unwrap(), old, "恢复结果需要跨重启保持");
        assert!(fs::read_dir(corrupt_path.parent().unwrap())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".corrupt")));
    }
}

#[test]
fn legacy_corrupt_lyrics_are_reported_and_never_read_as_empty() {
    let dir = TestLibraryDir::new();
    let tracks = vec![test_imported_track("a", "C:/a.flac", "A")];
    fs::write(
        dir.0.join("library-cache.json"),
        serde_json::to_vec(&tracks).unwrap(),
    )
    .unwrap();
    let bad = dir.0.join("library-lyrics.json");
    fs::write(&bad, b"broken lyrics").unwrap();
    assert!(dir.storage().load().is_err());
    assert_eq!(fs::read(bad).unwrap(), b"broken lyrics");
    assert!(!dir.0.join("library-snapshot.json").exists());
}

#[test]
fn future_snapshot_version_never_rolls_back_or_overwrites_data() {
    let dir = TestLibraryDir::new();
    let storage = dir.storage();
    let old = vec![test_imported_track("a", "C:/a.flac", "old")];
    storage.save(&old, None).unwrap();
    let mut updated = old.clone();
    updated[0].title = "new".into();
    storage.save(&updated, Some(&old)).unwrap();
    let path = dir.0.join("library-snapshot.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["version"] = json!(2);
    let future = serde_json::to_vec(&manifest).unwrap();
    fs::write(&path, &future).unwrap();
    assert!(storage.load().unwrap_err().contains("版本"));
    assert!(storage.save(&old, Some(&updated)).is_err());
    assert_eq!(fs::read(path).unwrap(), future);
}

#[test]
fn snapshot_cleanup_only_removes_owned_unreferenced_generations() {
    let dir = TestLibraryDir::new();
    let storage = dir.storage();
    let old = vec![test_imported_track("a", "C:/a.flac", "old")];
    storage.save(&old, None).unwrap();
    let snapshots = dir.0.join("library-snapshots");
    let unrelated = [
        "notes-tracks.json",
        "1-2-tracks.json",
        "1-2-3-4-lyrics.json",
        "1-2-3-tracks.json.corrupt",
    ];
    for name in unrelated {
        fs::write(snapshots.join(name), b"preserve").unwrap();
    }
    let orphan = snapshots.join("1-2-3-tracks.json");
    fs::write(&orphan, b"uncommitted snapshot").unwrap();
    let mut updated = old.clone();
    updated[0].title = "new".into();
    storage.save(&updated, Some(&old)).unwrap();
    assert!(!orphan.exists());
    for name in unrelated {
        assert_eq!(fs::read(snapshots.join(name)).unwrap(), b"preserve");
    }
    assert_eq!(storage.load().unwrap(), updated);
    fs::remove_file(dir.0.join("library-snapshot.json")).unwrap();
    assert_eq!(
        storage.load().unwrap(),
        old,
        "当前清单缺失时仍能恢复完整上一版"
    );
}

#[test]
fn library_snapshot_keeps_first_duplicate_and_replaces_index_with_contents() {
    use super::snapshot::LibrarySnapshot;
    let original = LibrarySnapshot::new(vec![
        test_imported_track("a", "C:/a.flac", "first"),
        test_imported_track("a", "C:/duplicate.flac", "duplicate"),
        test_imported_track("b", "C:/b.flac", "B"),
    ]);
    assert_eq!(original.get("a").unwrap().title, "first");
    assert!(original.get("missing").is_none());
    let updated = LibrarySnapshot::new(vec![original.get("b").unwrap().clone()]);
    assert!(updated.get("a").is_none());
    assert_eq!(updated.get("b").unwrap().title, "B");
    assert_eq!(original.tracks.len(), 3);
}

#[test]
fn bug_audit_03_deleted_track_cannot_be_reinserted_by_recache() {
    let a = test_imported_track("a", "C:/cache/a.m4a", "A");
    let b = test_imported_track("b", "C:/cache/b.m4a", "B");
    let mut remaining = Vec::new();
    let result = delete_selected_tracks(vec![a.clone(), b], &["a".into()], |updated| {
        remaining = updated.to_vec();
        Ok(())
    })
    .unwrap();
    assert_eq!(result.deleted_ids, ["a"]);
    assert!(replace_cached_track(&mut remaining, "a", &a).is_err());
    assert_eq!(
        remaining
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b"]
    );
}

#[test]
fn bug_audit_03_recache_preserves_identity_and_checks_source() {
    let mut existing = test_imported_track("stable-id", "C:/cache/old.m4a", "A");
    existing.source_id = Some("BV1234567890".into());
    existing.cache_missing = true;
    let mut incoming = test_imported_track("new-id", "C:/cache/new.flac", "A");
    incoming.source_id = existing.source_id.clone();
    let mut cached = vec![existing];
    let updated = replace_cached_track(&mut cached, "stable-id", &incoming).unwrap();
    assert_eq!(updated.id, "stable-id");
    assert_eq!(updated.path, "C:/cache/new.flac");
    assert!(!updated.cache_missing);
    incoming.source_id = Some("BV0987654321".into());
    assert!(replace_cached_track(&mut cached, "stable-id", &incoming).is_err());
    assert_eq!(cached[0].source_id.as_deref(), Some("BV1234567890"));
}

#[test]
fn parses_artist_and_title_from_filename() {
    let parsed = parse_filename_metadata("01 - 宇多田ヒカル - First Love");

    assert_eq!(parsed.artist.as_deref(), Some("宇多田ヒカル"));
    assert_eq!(parsed.title.as_deref(), Some("First Love"));
    assert_eq!(parsed.album, None);
}

#[test]
fn parses_artist_album_and_title_from_filename() {
    let parsed = parse_filename_metadata("Radiohead - OK Computer - No Surprises");

    assert_eq!(parsed.artist.as_deref(), Some("Radiohead"));
    assert_eq!(parsed.album.as_deref(), Some("OK Computer"));
    assert_eq!(parsed.title.as_deref(), Some("No Surprises"));
}

#[test]
fn keeps_plain_filename_as_title() {
    let parsed = parse_filename_metadata("Track Without Tags");

    assert_eq!(parsed.title.as_deref(), Some("Track Without Tags"));
    assert_eq!(parsed.artist, None);
    assert_eq!(parsed.album, None);
}

#[test]
fn strips_track_number_only_with_separator() {
    // M-10：数字前缀必须带分隔符才当曲号剥离
    assert_eq!(strip_track_number_prefix("01 - Song"), "Song");
    assert_eq!(strip_track_number_prefix("01. Song"), "Song");
    assert_eq!(strip_track_number_prefix("01_Song"), "Song");
    assert_eq!(strip_track_number_prefix("01 Song"), "Song");
    // 数字直接连标题：不是曲号，保持原样
    assert_eq!(strip_track_number_prefix("365天的思念"), "365天的思念");
    assert_eq!(strip_track_number_prefix("24K Magic"), "24K Magic");
    // 4 位以上数字（年份等）不剥
    assert_eq!(strip_track_number_prefix("1999 - Party"), "1999 - Party");
}

#[test]
fn parses_colon_centisecond_lrc_variant() {
    // M-11：千千静听时代 [mm:ss:cc] 冒号百分秒变体（[00:29:26] = 29.26s）
    let time = parse_lrc_time_token("00:29:26").expect("colon centiseconds");
    assert!((time - 29.26).abs() < 1e-6, "got {time}");
    // 标准逗号/点分变体不受影响
    let dot = parse_lrc_time_token("00:29.26").expect("dot variant");
    assert!((dot - 29.26).abs() < 1e-6);
    // 第三段含小数点 → 真 hh:mm:ss
    let hms = parse_lrc_time_token("01:02:03.5").expect("hh:mm:ss");
    assert!((hms - 3723.5).abs() < 1e-6, "got {hms}");
}

#[test]
fn ranked_provider_items_keeps_api_order_without_duration() {
    // M-13：本地时长未知（0）时不能按候选时长升序（30 秒试听会排第一）
    let items = vec![
        json!({ "duration": 240_000 }),
        json!({ "duration": 30_000 }),
        json!({ "duration": 180_000 }),
    ];
    let ranked = ranked_provider_items(&items, 0);
    let durations: Vec<u64> = ranked
        .iter()
        .filter_map(|item| item.get("duration").and_then(serde_json::Value::as_u64))
        .collect();
    assert_eq!(durations, vec![240_000, 30_000, 180_000], "保持接口原序");

    // 有时长时仍按接近度排序
    let ranked = ranked_provider_items(&items, 179);
    let first = ranked[0]
        .get("duration")
        .and_then(serde_json::Value::as_u64);
    assert_eq!(first, Some(180_000));
}

#[test]
fn enriches_dsd_metadata_from_decoder_probe() {
    let path = temp_audio_path("seraph-import-dsd", "dsf");
    write_test_dsf(&path);

    let metadata = parse_audio_metadata(&path);
    assert_eq!(metadata.duration, Some(1));
    assert_eq!(metadata.bit_depth, Some(24));
    assert_eq!(metadata.sample_rate, Some(44_100));
    assert_eq!(metadata.channels, Some(2));

    let _ = fs::remove_file(path);
}

#[test]
fn extracts_dsd_id3_tags_and_cover() {
    use id3::TagLike as _;
    use std::io::{Seek as _, SeekFrom, Write as _};

    let path = temp_audio_path("seraph-dsf-id3", "dsf");
    write_test_dsf(&path);

    // v0.5.1：在文件尾部追加 ID3v2 标签（DSF 规范位置），并把头部
    // offset 20 的 metadata 指针指向它——lofty 不支持 DSD 容器，
    // 这条链路由 dsd_tags 手动解析。
    let id3_offset = fs::metadata(&path).unwrap().len();
    let mut tag = id3::Tag::new();
    tag.set_title("鳥の詩");
    tag.set_artist("Lia");
    tag.set_album("AIR");
    tag.add_frame(id3::frame::Picture {
        mime_type: "image/png".into(),
        picture_type: id3::frame::PictureType::CoverFront,
        description: String::new(),
        data: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4],
    });
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    tag.write_to(&mut file, id3::Version::Id3v24).unwrap();
    drop(file);
    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(20)).unwrap();
    file.write_all(&id3_offset.to_le_bytes()).unwrap();
    drop(file);

    let tags = dsd_tags_from_path(&path).expect("dsd id3 tags");
    assert_eq!(tags.title.as_deref(), Some("鳥の詩"));
    assert_eq!(tags.artist.as_deref(), Some("Lia"));
    assert_eq!(tags.album.as_deref(), Some("AIR"));
    let cover = tags.cover.expect("cover art from APIC");
    assert_eq!(cover.ext, "png", "扩展名按图片魔数推断");

    // 全链路：parse_audio_metadata 的 lofty 失败分支应带出标题与封面
    let metadata = parse_audio_metadata(&path);
    assert_eq!(metadata.title.as_deref(), Some("鳥の詩"));
    assert!(metadata.cover.is_some(), "DSD 曲目应提取到内嵌封面");
    assert_eq!(metadata.sample_rate, Some(44_100), "解码探测仍然生效");

    let _ = fs::remove_file(path);
}

#[test]
fn detects_dsd_by_magic_when_extension_differs() {
    let path = temp_audio_path("seraph-import-dsd-magic", "bin");
    write_test_dsf(&path);

    assert!(is_audio_file(&path));
    assert_eq!(audio_format_label(&path), "DSF");

    let track = track_from_path(&path, None).expect("track from dsf magic");
    assert_eq!(track.format, "DSF");
    assert_eq!(track.bitdepth, "DSF 24-bit / 44.1 kHz PCM");
    assert_eq!(track.sample_rate, "44.1 kHz PCM");
    assert_eq!(track.channels, "Stereo");
    assert_eq!(track.duration, 1);

    let _ = fs::remove_file(path);
}

#[test]
fn trusts_riff_magic_over_dsf_extension() {
    let path = temp_audio_path("seraph-import-fake-dsf", "dsf");
    fs::write(&path, b"RIFF\0\0\0\0WAVE").expect("write fake dsf");

    assert_eq!(audio_format_label(&path), "WAV");
    assert!(!is_dsd_file(&path));

    let _ = fs::remove_file(path);
}

/// 手工拼一个尾部带 ID3v2.4 APIC 封面的最小 WAV。
/// `u32_frame_size` 复刻违反规范的打标签工具：帧大小写普通 u32
/// （v2.4 规范要求 syncsafe），封面超过 127 字节时两种解读不同，
/// 按规范解析会把图片拦腰截断——用户库中 WAV 封面显示不全的病灶。
/// 布局按真实病例：RIFF 声明大小止于 data，`id3 ` chunk 追加在其后。
fn write_test_wav_with_apic(path: &Path, image: &[u8], u32_frame_size: bool) {
    // APIC 内容：encoding=0(latin1) + MIME + pic_type=3(CoverFront) + 空描述 + 图片
    let mut apic = vec![0u8];
    apic.extend_from_slice(b"image/jpeg\0");
    apic.push(3);
    apic.push(0);
    apic.extend_from_slice(image);

    let frame_size = apic.len() as u32;
    let size_bytes = if u32_frame_size {
        frame_size.to_be_bytes()
    } else {
        test_syncsafe_bytes(frame_size)
    };
    let mut id3 = Vec::new();
    id3.extend_from_slice(b"ID3\x04\x00\x00");
    // header 里的 tag size 真实病例中仍是正确的 syncsafe
    id3.extend_from_slice(&test_syncsafe_bytes(10 + frame_size));
    id3.extend_from_slice(b"APIC");
    id3.extend_from_slice(&size_bytes);
    id3.extend_from_slice(&[0, 0]);
    id3.extend_from_slice(&apic);

    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    // "WAVE"(4) + fmt(8+16) + data(8+32) = 68，不含尾部 id3
    wav.extend_from_slice(&68u32.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&44_100u32.to_le_bytes());
    wav.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&32u32.to_le_bytes());
    wav.extend_from_slice(&[0u8; 32]);
    wav.extend_from_slice(b"id3 ");
    wav.extend_from_slice(&(id3.len() as u32).to_le_bytes());
    wav.extend_from_slice(&id3);

    fs::write(path, wav).expect("write test wav");
}

fn test_syncsafe_bytes(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn test_jpeg_bytes(len: usize, fill: u8) -> Vec<u8> {
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.resize(len - 2, fill);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

#[test]
fn extracts_full_cover_from_wav_with_nonstandard_u32_apic_size() {
    let path = temp_audio_path("seraph-wav-u32-apic", "wav");
    let jpeg = test_jpeg_bytes(300, 0xaa);
    write_test_wav_with_apic(&path, &jpeg, true);

    let metadata = parse_audio_metadata_with_dsd_hint(&path, false);
    let _ = fs::remove_file(&path);
    let cover = metadata
        .cover
        .expect("非规范 u32 帧大小的 WAV 封面应被完整提取");
    assert_eq!(cover.ext, "jpg");
    assert_eq!(cover.data, jpeg, "封面不应被 syncsafe 误读截断");
}

#[test]
fn extracts_cover_from_spec_compliant_wav_id3() {
    let path = temp_audio_path("seraph-wav-syncsafe-apic", "wav");
    let jpeg = test_jpeg_bytes(300, 0xbb);
    write_test_wav_with_apic(&path, &jpeg, false);

    let metadata = parse_audio_metadata_with_dsd_hint(&path, false);
    let _ = fs::remove_file(&path);
    let cover = metadata.cover.expect("规范 syncsafe WAV 封面应正常提取");
    assert_eq!(cover.data, jpeg);
}

#[test]
fn detects_truncated_saved_cover_files() {
    let dir = std::env::temp_dir();
    let truncated = dir.join("seraph-test-truncated-cover.jpg");
    let complete = dir.join("seraph-test-complete-cover.jpg");
    fs::write(&truncated, [0xff, 0xd8, 0xff, 0xe0, 0xaa]).unwrap();
    fs::write(&complete, [0xff, 0xd8, 0xff, 0xe0, 0xff, 0xd9]).unwrap();

    assert!(cover_file_looks_truncated(&truncated.to_string_lossy()));
    assert!(!cover_file_looks_truncated(&complete.to_string_lossy()));
    assert!(!cover_file_looks_truncated(""), "空 cover 交给缺失补扫分支");
    assert!(
        !cover_file_looks_truncated("https://example.com/x.jpg"),
        "在线封面不参与"
    );
    assert!(
        !cover_file_looks_truncated(r"C:\seraph-nonexistent\cover.jpg"),
        "文件读不到不触发重提取"
    );

    let _ = fs::remove_file(truncated);
    let _ = fs::remove_file(complete);
}

#[test]
fn backfill_reextracts_full_cover_from_u32_wav() {
    // 存量用户升级路径：backfill 用 extract_embedded_cover 重提取
    // 截断封面，该函数必须与导入路径同样挂 WAV 兜底。
    let path = temp_audio_path("seraph-wav-backfill", "wav");
    let jpeg = test_jpeg_bytes(300, 0xcc);
    write_test_wav_with_apic(&path, &jpeg, true);
    let covers_dir = std::env::temp_dir().join("seraph-test-covers-backfill");
    let _ = fs::remove_dir_all(&covers_dir);

    let cover = extract_embedded_cover(&path, &covers_dir).expect("cover saved");
    let saved = fs::read(&cover).expect("read saved cover");
    assert_eq!(saved, jpeg, "backfill 重提取应得到完整封面");

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&covers_dir);
}

#[test]
fn formats_quality_with_sample_rate() {
    assert_eq!(
        format_audio_quality("FLAC", Some(24), Some(96_000)),
        "FLAC 24-bit / 96 kHz"
    );
    assert_eq!(
        format_audio_quality("WAV", Some(16), Some(44_100)),
        "WAV 16-bit / 44.1 kHz"
    );
    assert_eq!(
        format_audio_quality("DSF", Some(24), Some(44_100)),
        "DSF 24-bit / 44.1 kHz PCM"
    );
}

#[test]
fn merges_cached_tracks_by_path() {
    let cached = vec![test_imported_track("old", "C:/Music/a.flac", "Old")];
    let imported = vec![
        test_imported_track("new", "c:/music/a.flac", "Updated"),
        test_imported_track("b", "C:/Music/b.flac", "Added"),
    ];

    let merged = merge_cached_tracks(cached, &imported);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].id, "new");
    assert_eq!(merged[0].title, "Updated");
    assert_eq!(merged[1].id, "b");
}

#[test]
fn merge_preserves_cached_lyrics_when_reimport_has_none() {
    let mut cached_track = test_imported_track("old", "C:/Music/a.flac", "Old");
    cached_track.lyrics = vec![LyricLine::new(1.5, "cached line")];
    let imported = vec![test_imported_track("new", "c:/music/a.flac", "Updated")];

    let merged = merge_cached_tracks(vec![cached_track], &imported);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].id, "new");
    assert_eq!(merged[0].title, "Updated");
    assert_eq!(merged[0].lyrics.len(), 1);
    assert!((merged[0].lyrics[0].time - 1.5).abs() < 0.001);
    assert_eq!(merged[0].lyrics[0].text, "cached line");
}

#[test]
fn removes_cached_track_by_id() {
    let tracks = vec![
        test_imported_track("a", "C:/Music/a.flac", "A"),
        test_imported_track("b", "C:/Music/b.flac", "B"),
    ];

    let result = delete_selected_tracks(tracks, &["a".into()], |updated| {
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].id, "b");
        Ok(())
    })
    .unwrap();
    assert_eq!(result.deleted_ids, ["a"]);
}

#[test]
fn matches_legacy_delete_request_by_streaming_source_key() {
    let mut track = test_imported_track("old-id", "C:/Cache/BV1xx-1.flac", "Stream");
    track.source_id = Some("BV1xx".into());
    let request = DeleteTrackRequest {
        id: "new-id".into(),
        path: "C:/Cache/BV1xx-1.flac".into(),
        source_url: None,
        source_id: Some("bv1XX".into()),
    };
    let key = delete_track_request_key(&request);

    assert!(cached_track_matches_delete(
        &track,
        &request.id,
        key.as_deref()
    ));
}

#[test]
fn imported_tracks_from_cache_returns_preserved_lyrics() {
    let mut cached_track = test_imported_track("new", "c:/music/a.flac", "Updated");
    cached_track.lyrics = vec![LyricLine::new(1.5, "cached line")];
    let imported = vec![test_imported_track("new", "C:/Music/a.flac", "Updated")];

    let returned = imported_tracks_from_cache(&[cached_track], &imported);

    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0].id, "new");
    assert_eq!(returned[0].lyrics.len(), 1);
    assert_eq!(returned[0].lyrics[0].text, "cached line");
}

#[test]
fn applies_track_lyrics_by_id() {
    let mut tracks = vec![
        test_imported_track("a", "C:/Music/a.flac", "A"),
        test_imported_track("b", "C:/Music/b.flac", "B"),
    ];
    let lyrics = vec![LyricLine::new(2.0, "imported line")];

    apply_track_lyrics(&mut tracks, "b", lyrics, None, None).expect("apply lyrics");

    assert!(tracks[0].lyrics.is_empty());
    assert_eq!(tracks[1].lyrics.len(), 1);
    assert_eq!(tracks[1].lyrics[0].text, "imported line");
}

#[test]
fn errors_when_applying_lyrics_to_missing_track() {
    let mut tracks = vec![test_imported_track("a", "C:/Music/a.flac", "A")];
    let lyrics = vec![LyricLine::new(0.0, "line")];

    let err =
        apply_track_lyrics(&mut tracks, "missing", lyrics, None, None).expect_err("missing track");

    assert!(err.contains("track was not found"));
    assert!(tracks[0].lyrics.is_empty());
}

fn test_imported_track(id: &str, path: &str, title: &str) -> ImportedTrack {
    ImportedTrack {
        id: id.into(),
        title: title.into(),
        artist: "Artist".into(),
        album: "Album".into(),
        album_year: None,
        cover: String::new(),
        format: "FLAC".into(),
        bitdepth: "FLAC 24-bit / 96 kHz".into(),
        sample_rate: "96 kHz".into(),
        bitrate: "Unknown".into(),
        channels: "Stereo".into(),
        size: "1.0 MB".into(),
        path: path.into(),
        source_url: None,
        source_id: None,
        cache_missing: false,
        duration: 1,
        glow_color: "#fff".into(),
        glow1: "#fff".into(),
        glow2: "#000".into(),
        lyrics: Vec::new(),
        lyrics_lookup_keys: Vec::new(),
    }
}

#[test]
fn parses_timestamped_lrc_lines() {
    let lyrics = parse_lyrics_text("[ti:Test]\n[00:01.20]第一句\n[00:03.40][00:05.00]重复一句");

    assert_eq!(lyrics.len(), 3);
    assert!((lyrics[0].time - 1.2).abs() < 0.001);
    assert_eq!(lyrics[0].text, "第一句");
    assert!((lyrics[1].time - 3.4).abs() < 0.001);
    assert_eq!(lyrics[1].text, "重复一句");
    assert!((lyrics[2].time - 5.0).abs() < 0.001);
}

#[test]
fn decodes_gbk_lrc_bytes() {
    let bytes = vec![
        b'[', b'0', b'0', b':', b'0', b'1', b'.', b'0', b'0', b']', 0xd6, 0xd0, 0xce, 0xc4,
    ];

    let lyrics = parse_lyrics_text(&decode_lyric_bytes(&bytes));

    assert_eq!(lyrics.len(), 1);
    assert_eq!(lyrics[0].text, "\u{4e2d}\u{6587}");
}

#[test]
fn decodes_utf16_le_without_bom() {
    let bytes = "[00:01.00]hello"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();

    let lyrics = parse_lyrics_text(&decode_lyric_bytes(&bytes));

    assert_eq!(lyrics.len(), 1);
    assert_eq!(lyrics[0].text, "hello");
}

#[test]
fn parses_common_lrc_time_variants() {
    let lyrics = parse_lyrics_text(
        "[OFFSET:-500]\n[00:01,20]comma\n[1234,567]krc\n[00:02.00]a <00:02.10>b [00:02.20]c",
    );

    assert_eq!(lyrics.len(), 3);
    // L-9：OFFSET:-500（负 offset）让歌词延后 0.5s（time - offset = time + 0.5）。
    assert!((lyrics[0].time - 1.7).abs() < 0.001);
    assert_eq!(lyrics[0].text, "comma");
    assert!((lyrics[1].time - 1.734).abs() < 0.001);
    assert_eq!(lyrics[1].text, "krc");
    assert!((lyrics[2].time - 2.5).abs() < 0.001);
    assert_eq!(lyrics[2].text, "a b c");
}

#[test]
fn parses_qq_qrc_lyric_content() {
    let text = r#"<Lyric_1 LyricType="1" LyricContent="[1000,2000]he(1000,500)llo(1500,500)&#10;[3000,1000]world(3000,1000)"/>"#;

    let lyrics = parse_lyrics_bytes(text.as_bytes());

    assert_eq!(lyrics.len(), 2);
    assert!((lyrics[0].time - 1.0).abs() < 0.001);
    assert_eq!(lyrics[0].text, "hello");
    assert!((lyrics[1].time - 3.0).abs() < 0.001);
    assert_eq!(lyrics[1].text, "world");

    // QRC 逐字：`(start,dur)` 绝对毫秒、标签在文本后；行 end = start + dur
    assert_eq!(lyrics[0].end, Some(3.0));
    let words = lyrics[0].words.as_ref().expect("qrc words");
    assert_eq!(
        words
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect::<Vec<_>>(),
        [("he", 1.0, 1.5), ("llo", 1.5, 2.0)]
    );
    // 只有一个音节且等于整行也保留
    let words = lyrics[1].words.as_ref().expect("single word");
    assert_eq!(words.len(), 1);
    assert_eq!(
        (words[0].text.as_str(), words[0].start, words[0].end),
        ("world", 3.0, 4.0)
    );
    assert_eq!(lyrics[1].end, Some(4.0));
}

#[test]
fn qrc_words_merge_whitespace_syllables_and_keep_plain_lines_line_level() {
    // 空白音节并入前一音节（词间空格属前一音节）；没有标签的行按行级输出
    let text = "[1000,2000]he(1000,500) (1500,100)llo(1600,400)\n[3000,1000]plain line";
    let lyrics = parse_lyrics_bytes(text.as_bytes());
    assert_eq!(lyrics.len(), 2);
    assert_eq!(lyrics[0].text, "he llo");
    let words = lyrics[0].words.as_ref().expect("words");
    assert_eq!(
        words
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect::<Vec<_>>(),
        [("he ", 1.0, 1.6), ("llo", 1.6, 2.0)]
    );
    assert_eq!(lyrics[1].text, "plain line");
    assert!(lyrics[1].words.is_none());
    assert_eq!(lyrics[1].end, Some(4.0));
}

#[test]
fn parses_netease_yrc_word_lines() {
    let lyrics =
        parse_lyrics_bytes(b"[1200,800](1200,200,0)he(1400,200,0)llo\n[2500,500](2500,500,0)world");

    assert_eq!(lyrics.len(), 2);
    assert!((lyrics[0].time - 1.2).abs() < 0.001);
    assert_eq!(lyrics[0].text, "hello");
    assert!((lyrics[1].time - 2.5).abs() < 0.001);
    assert_eq!(lyrics[1].text, "world");

    // YRC 逐字：`(start,dur,0)` 绝对毫秒、标签在文本前
    assert_eq!(lyrics[0].end, Some(2.0));
    let words = lyrics[0].words.as_ref().expect("yrc words");
    assert_eq!(
        words
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect::<Vec<_>>(),
        [("he", 1.2, 1.4), ("llo", 1.4, 1.6)]
    );
    let words = lyrics[1].words.as_ref().expect("single yrc word");
    assert_eq!(
        (words[0].text.as_str(), words[0].start, words[0].end),
        ("world", 2.5, 3.0)
    );
}

#[test]
fn parses_kugou_krc_word_lines_and_translation() {
    let language = BASE64_STANDARD
        .encode(r#"{"content":[{"type":1,"lyricContent":[["greeting"],["planet"]]}]}"#);
    let text = format!(
        "[language:{language}]\n[1000,2000]<0,500,0>he<500,500,0>llo\n[3000,1000]<0,1000,0>world"
    );

    let lyrics = parse_lyrics_bytes(text.as_bytes());

    assert_eq!(lyrics.len(), 4);
    assert!((lyrics[0].time - 1.0).abs() < 0.001);
    assert_eq!(lyrics[0].text, "hello");
    assert_eq!(lyrics[1].text, "greeting");
    assert!((lyrics[2].time - 3.0).abs() < 0.001);
    assert_eq!(lyrics[2].text, "world");
    assert_eq!(lyrics[3].text, "planet");

    // KRC 逐字：`<offset,dur,0>` offset 相对行 start；译文行不带 words/end
    assert_eq!(lyrics[0].end, Some(3.0));
    let words = lyrics[0].words.as_ref().expect("krc words");
    assert_eq!(
        words
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect::<Vec<_>>(),
        [("he", 1.0, 1.5), ("llo", 1.5, 2.0)]
    );
    assert!(lyrics[1].words.is_none());
    assert!(lyrics[1].end.is_none());
    let words = lyrics[2].words.as_ref().expect("single krc word");
    assert_eq!(
        (words[0].text.as_str(), words[0].start, words[0].end),
        ("world", 3.0, 4.0)
    );
    assert!(lyrics[3].words.is_none());
}

#[test]
fn parses_ttml_bytes_without_extension_by_content_sniffing() {
    // 手动导入只有裸字节：以 `<?xml`/`<tt` 开头且含 `<tt` 即走 TTML 解析
    let ttml = "\u{feff}  <?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tt xmlns=\"http://www.w3.org/ns/ttml\"><body><div><p begin=\"00:01.000\" end=\"00:03.000\"><span begin=\"00:01.000\" end=\"00:01.500\">Hel</span><span begin=\"00:01.500\" end=\"00:03.000\">lo</span></p></div></body></tt>";
    let lyrics = parse_lyrics_bytes(ttml.as_bytes());
    assert_eq!(lyrics.len(), 1);
    assert_eq!(lyrics[0].text, "Hello");
    assert_eq!(lyrics[0].end, Some(3.0));
    let words = lyrics[0].words.as_ref().expect("ttml words");
    assert_eq!(
        words
            .iter()
            .map(|w| (w.text.as_str(), w.start, w.end))
            .collect::<Vec<_>>(),
        [("Hel", 1.0, 1.5), ("lo", 1.5, 3.0)]
    );

    // 大小写不敏感的 `<TT`；不是 TTML 的 XML 走后续解析而不是返回空
    assert!(looks_like_ttml("<TT xmlns=\"x\"></TT>"));
    assert!(!looks_like_ttml("[00:01.00]<tt>"));
    let not_ttml = "<?xml version=\"1.0\"?><root>x</root>";
    assert!(!looks_like_ttml(not_ttml));
}

#[test]
fn detects_unsynced_plain_text_lyrics() {
    // 纯文本兜底：index * 4s 等差、无 words/end
    let plain = parse_lyrics_bytes("first line\nsecond line\nthird line".as_bytes());
    assert_eq!(plain.len(), 3);
    assert!(lyrics_are_unsynced(&plain));

    let timed = parse_lyrics_bytes(b"[00:00.00]a\n[00:04.00]b\n[00:09.00]c");
    assert!(!lyrics_are_unsynced(&timed));
    assert!(!lyrics_are_unsynced(&[]));

    // 恰好 4 秒等差但带 words → 是真时间轴
    let mut karaoke = LyricLine::new(0.0, "a");
    karaoke.words = Some(vec![LyricWord {
        start: 0.0,
        end: 1.0,
        text: "a".into(),
    }]);
    assert!(!lyrics_are_unsynced(&[karaoke]));
}

#[test]
fn sidecar_lookup_prefers_ttml_over_lrc() {
    let dir = std::env::temp_dir().join(format!("seraph-sidecar-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let audio = dir.join("song.flac");
    fs::write(&audio, b"").unwrap();
    fs::write(dir.join("song.lrc"), "[00:01.00]line level").unwrap();
    assert!(find_lyrics_file(&audio).unwrap().ends_with("song.lrc"));

    fs::write(
        dir.join("song.ttml"),
        "<tt xmlns=\"http://www.w3.org/ns/ttml\"><body><div><p begin=\"00:01.000\" end=\"00:02.000\"><span begin=\"00:01.000\" end=\"00:02.000\">word</span></p></div></body></tt>",
    )
    .unwrap();
    assert!(find_lyrics_file(&audio).unwrap().ends_with("song.ttml"));
    let lyrics = external_lrc_lyrics(&audio).expect("ttml sidecar");
    assert_eq!(lyrics[0].text, "word");
    assert!(lyrics[0].words.is_some());

    // 大小写不同的 stem（Windows 上精确分支即命中，返回 `SONG.ttml`）同样按 ttml 优先
    let mixed = dir.join("SONG.mp3");
    fs::write(&mixed, b"").unwrap();
    let found = find_lyrics_file(&mixed).unwrap();
    assert!(found
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ttml")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn krc_translation_stays_aligned_when_original_line_is_cleaned_away() {
    // 审2-S6：第二行原文清洗后为空（纯 word 标记无文本），主歌词不展示它，
    // 但后续行的译文必须仍按原始行号对齐到各自时间点，不整体前移错位。
    let language = BASE64_STANDARD.encode(
        r#"{"content":[{"type":1,"lyricContent":[["greeting"],["interlude"],["planet"]]}]}"#,
    );
    let text = format!(
            "[language:{language}]\n[1000,2000]<0,500,0>he<500,500,0>llo\n[2000,500]<0,500,0>\n[3000,1000]<0,1000,0>world"
        );

    let lyrics = parse_lyrics_bytes(text.as_bytes());

    assert_eq!(lyrics.len(), 5);
    assert!((lyrics[0].time - 1.0).abs() < 0.001);
    assert_eq!(lyrics[0].text, "hello");
    assert_eq!(lyrics[1].text, "greeting");
    // 空原文行本身被过滤，但它的译文仍锚定在原始行的时间点。
    assert!((lyrics[2].time - 2.0).abs() < 0.001);
    assert_eq!(lyrics[2].text, "interlude");
    assert!((lyrics[3].time - 3.0).abs() < 0.001);
    assert_eq!(lyrics[3].text, "world");
    assert!((lyrics[4].time - 3.0).abs() < 0.001);
    assert_eq!(lyrics[4].text, "planet");
}

#[test]
fn provider_duration_skips_unparsable_string_keys() {
    // 审2-S7：单个候选键是无法解析的字符串（如 "N/A"）时，
    // 应继续尝试后续候选键，而不是放弃全部候选。
    assert_eq!(
        provider_duration_ms(&json!({"duration": "N/A", "interval": 200})),
        Some(200_000)
    );
    assert_eq!(provider_duration_ms(&json!({"duration": "N/A"})), None);
    assert_eq!(
        provider_duration_ms(&json!({"interval": "185"})),
        Some(185_000)
    );
    assert_eq!(provider_duration_ms(&json!({"dt": 240_000})), Some(240_000));
}

#[test]
fn applies_lrc_offset() {
    // L-9：正 offset 让歌词提前显示 → 1.00s 标签 - 0.5s offset = 0.5s
    let lyrics = parse_lyrics_text("[offset:500]\n[00:01.00]提前半秒");

    assert_eq!(lyrics.len(), 1);
    assert!((lyrics[0].time - 0.5).abs() < 0.001);
}

#[test]
fn converts_unsynced_lyrics_to_display_lines() {
    let lyrics = parse_lyrics_text("第一行\n\n第二行");

    assert_eq!(lyrics.len(), 2);
    assert_eq!(lyrics[0].time, 0.0);
    assert_eq!(lyrics[0].text, "第一行");
    assert_eq!(lyrics[1].time, 4.0);
    assert_eq!(lyrics[1].text, "第二行");
}

#[test]
fn atomic_write_replaces_existing_file_and_leaves_no_temp() {
    let path = temp_audio_path("seraph-atomic-write", "json");
    fs::write(&path, b"old-content").unwrap();

    write_json_atomic(&path, b"[]").expect("atomic write should succeed");

    assert_eq!(fs::read(&path).unwrap(), b"[]");
    assert!(
        !PathBuf::from(format!("{}.tmp", path.display())).exists(),
        "临时文件必须被 rename 消耗掉"
    );
    let _ = fs::remove_file(&path);
}

#[test]
fn corrupt_library_cache_is_reported_and_backed_up_not_emptied() {
    let path = temp_audio_path("seraph-corrupt-cache", "json");
    fs::write(&path, b"{ this is not valid json").unwrap();

    // P0-2：解析失败必须报错，绝不能当成空库。
    let result = read_tracks_from_file(&path);
    assert!(result.is_err(), "损坏缓存必须返回 Err 而不是空列表");

    let backup = backup_corrupt_file(&path).unwrap();
    assert!(backup.is_file(), "坏文件应被备份为 .corrupt");
    assert_eq!(fs::read(&backup).unwrap(), b"{ this is not valid json");
    assert!(path.is_file(), "原始坏文件保留现场，不被移动");

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(&backup);
}

#[test]
fn missing_library_cache_reads_as_empty() {
    let path = temp_audio_path("seraph-missing-cache", "json");
    assert_eq!(read_tracks_from_file(&path).unwrap().len(), 0);
}

#[test]
fn splits_and_merges_lyrics_round_trip() {
    let mut with_lyrics = test_imported_track("a", "C:/Music/a.flac", "A");
    with_lyrics.lyrics = vec![LyricLine::new(1.0, "line one")];
    let without = test_imported_track("b", "C:/Music/b.flac", "B");

    let (stripped, sidecar) = split_lyrics_for_storage(&[with_lyrics.clone(), without.clone()]);

    // 主记录歌词被清空，只有带歌词的曲目进边车
    assert!(stripped[0].lyrics.is_empty());
    assert!(stripped[1].lyrics.is_empty());
    assert_eq!(sidecar.len(), 1);
    assert_eq!(sidecar.get("a").unwrap()[0].text, "line one");
    assert!(!sidecar.contains_key("b"));

    // 合并回来后与原始曲目完全一致
    let restored = merge_lyrics_from_storage(stripped, &sidecar);
    assert_eq!(restored[0].lyrics.len(), 1);
    assert_eq!(restored[0].lyrics[0].text, "line one");
    assert!(restored[1].lyrics.is_empty());
}

#[test]
fn merge_lyrics_keeps_inline_when_sidecar_absent() {
    // 旧格式迁移：主文件内联歌词、边车缺失时，内联歌词必须保留
    let mut inline = test_imported_track("a", "C:/Music/a.flac", "A");
    inline.lyrics = vec![LyricLine::new(2.0, "legacy inline")];

    let restored = merge_lyrics_from_storage(vec![inline], &std::collections::HashMap::new());
    assert_eq!(restored[0].lyrics.len(), 1);
    assert_eq!(restored[0].lyrics[0].text, "legacy inline");
}

#[test]
fn cover_key_normalizes_case_and_separators() {
    assert_eq!(
        normalize_cover_key(r"C:\Users\X\covers\ABC.jpg"),
        normalize_cover_key("c:/users/x/COVERS/abc.JPG")
    );
}

#[test]
fn cover_art_from_tags_skips_oversized_pictures() {
    use lofty::picture::Picture;
    use lofty::tag::TagType;

    // S-07：恶意音频可内嵌超大「封面」，超过上限必须按无封面处理
    let mut tag = Tag::new(TagType::Id3v2);
    tag.push_picture(
        Picture::unchecked(vec![0u8; MAX_EMBEDDED_COVER_BYTES + 1])
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Jpeg)
            .build(),
    );
    assert!(
        cover_art_from_tags(std::slice::from_ref(&tag)).is_none(),
        "超限内嵌图必须被跳过"
    );

    // 合规大小的图片不受影响
    let mut ok_tag = Tag::new(TagType::Id3v2);
    ok_tag.push_picture(
        Picture::unchecked(vec![0xff, 0xd8, 0xff, 0x00])
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Jpeg)
            .build(),
    );
    assert!(cover_art_from_tags(std::slice::from_ref(&ok_tag)).is_some());
}

#[test]
fn save_cover_art_rejects_oversized_data() {
    // S-07：落盘入口的兜底体积校验
    let dir = std::env::temp_dir().join(format!("seraph-cover-limit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);

    let oversized = CoverArt {
        data: vec![0u8; MAX_EMBEDDED_COVER_BYTES + 1],
        ext: "jpg",
    };
    assert!(
        save_cover_art(&dir, &oversized).is_none(),
        "超限封面不得落盘"
    );

    let within = CoverArt {
        data: vec![0xff, 0xd8, 0xff, 0x00],
        ext: "jpg",
    };
    assert!(save_cover_art(&dir, &within).is_some());

    let _ = fs::remove_dir_all(&dir);
}
