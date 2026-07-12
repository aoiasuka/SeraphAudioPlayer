#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

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
        cached_track.lyrics = vec![LyricLine {
            time: 1.5,
            text: "cached line".into(),
        }];
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

        let (updated, removed) = remove_cached_track(tracks, "a", None);

        assert!(removed);
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].id, "b");
    }

    #[test]
    fn removes_cached_track_by_streaming_source_key() {
        let mut track = test_imported_track("old-id", "C:/Cache/BV1xx-1.flac", "Stream");
        track.source_id = Some("BV1xx".into());
        let request = DeleteTrackRequest {
            id: "new-id".into(),
            path: "C:/Cache/BV1xx-1.flac".into(),
            source_url: None,
            source_id: Some("bv1XX".into()),
        };
        let key = delete_track_request_key(&request);

        let (updated, removed) = remove_cached_track(vec![track], &request.id, key.as_deref());

        assert!(removed);
        assert!(updated.is_empty());
    }

    #[test]
    fn imported_tracks_from_cache_returns_preserved_lyrics() {
        let mut cached_track = test_imported_track("new", "c:/music/a.flac", "Updated");
        cached_track.lyrics = vec![LyricLine {
            time: 1.5,
            text: "cached line".into(),
        }];
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
        let lyrics = vec![LyricLine {
            time: 2.0,
            text: "imported line".into(),
        }];

        apply_track_lyrics(&mut tracks, "b", lyrics, None, None).expect("apply lyrics");

        assert!(tracks[0].lyrics.is_empty());
        assert_eq!(tracks[1].lyrics.len(), 1);
        assert_eq!(tracks[1].lyrics[0].text, "imported line");
    }

    #[test]
    fn errors_when_applying_lyrics_to_missing_track() {
        let mut tracks = vec![test_imported_track("a", "C:/Music/a.flac", "A")];
        let lyrics = vec![LyricLine {
            time: 0.0,
            text: "line".into(),
        }];

        let err = apply_track_lyrics(&mut tracks, "missing", lyrics, None, None)
            .expect_err("missing track");

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
    }

    #[test]
    fn parses_netease_yrc_word_lines() {
        let lyrics = parse_lyrics_bytes(
            b"[1200,800](1200,200,0)he(1400,200,0)llo\n[2500,500](2500,500,0)world",
        );

        assert_eq!(lyrics.len(), 2);
        assert!((lyrics[0].time - 1.2).abs() < 0.001);
        assert_eq!(lyrics[0].text, "hello");
        assert!((lyrics[1].time - 2.5).abs() < 0.001);
        assert_eq!(lyrics[1].text, "world");
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

        let backup = backup_corrupt_file(&path);
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
}
