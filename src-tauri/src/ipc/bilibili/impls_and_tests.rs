impl AudioStream {
    fn audio_urls(&self) -> Vec<String> {
        let mut urls = Vec::with_capacity(self.backup_urls.len() + 1);
        if !self.base_url.trim().is_empty() {
            urls.push(self.base_url.clone());
        }

        for url in &self.backup_urls {
            if !url.trim().is_empty() && !urls.iter().any(|item| item == url) {
                urls.push(url.clone());
            }
        }

        urls
    }

    fn kind_rank(&self) -> u8 {
        match self.kind {
            AudioKind::DolbyAtmos => 4,
            AudioKind::Flac => 3,
            AudioKind::Dolby => 2,
            AudioKind::Normal => 1,
        }
    }

    fn format_label(&self) -> &'static str {
        match self.kind {
            AudioKind::DolbyAtmos | AudioKind::Dolby => "EAC3",
            AudioKind::Flac => "FLAC",
            _ => self
                .codecs
                .as_deref()
                .map(format_from_codec)
                .unwrap_or("M4A"),
        }
    }

    fn output_extension(&self, remuxed: bool) -> &'static str {
        // 即使未 remux 也按真实编码落扩展名，避免 FLAC stream 被命名为 .m4a
        // 导致后续元数据探测失败 (M-2)。
        match self.format_label() {
            "FLAC" => "flac",
            "OPUS" => "opus",
            "EAC3" if remuxed => "eac3",
            "EAC3" => "eac3",
            _ => "m4a",
        }
    }
}

impl BilibiliSession {
    fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }

        let now = now_secs();
        let parts: Vec<String> = self
            .cookies
            .iter()
            .filter(|(name, _)| {
                // 有过期时间且已过期的跳过；session cookie（无 expires 记录）默认保留
                match self.cookie_expires.get(*name) {
                    Some(ts) => *ts > now,
                    None => true,
                }
            })
            .map(|(name, value)| format!("{name}={value}"))
            .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_bvid, extract_media_id, select_audio_stream, AudioKind, DashData};
    use serde_json::json;

    fn dash_with_dolby_and_flac() -> DashData {
        serde_json::from_value(json!({
            "dolby": {
                "type": 2,
                "audio": [{
                    "id": 30250,
                    "baseUrl": "https://example.com/atmos.eac3",
                    "backupUrl": [],
                    "bandwidth": 1_025_000,
                    "codecs": "ec-3"
                }]
            },
            "flac": {
                "audio": {
                    "id": 30251,
                    "baseUrl": "https://example.com/lossless.flac",
                    "backupUrl": [],
                    "bandwidth": 985_000,
                    "codecs": "fLaC"
                }
            },
            "audio": [{
                "id": 30280,
                "baseUrl": "https://example.com/normal.m4a",
                "backupUrl": [],
                "bandwidth": 320_000,
                "codecs": "mp4a.40.2"
            }]
        }))
        .expect("valid dash fixture")
    }

    #[test]
    fn prefers_dolby_atmos_when_ffmpeg_available() {
        let stream = select_audio_stream(dash_with_dolby_and_flac(), true, true, true)
            .expect("a stream should be selected");
        assert!(matches!(stream.kind, AudioKind::DolbyAtmos));
    }

    #[test]
    fn skips_dolby_atmos_when_ffmpeg_missing() {
        // 没有 ffmpeg 时 EAC3 流不可解码，必须回退到 Symphonia 能直接播放的 FLAC。
        let stream = select_audio_stream(dash_with_dolby_and_flac(), true, true, false)
            .expect("a playable stream should be selected");
        assert!(
            matches!(stream.kind, AudioKind::Flac),
            "expected FLAC fallback, got {:?}",
            stream.kind
        );
    }

    #[test]
    fn falls_back_to_normal_when_only_dolby_and_ffmpeg_missing() {
        let dash: DashData = serde_json::from_value(json!({
            "dolby": {
                "type": 2,
                "audio": [{
                    "id": 30250,
                    "baseUrl": "https://example.com/atmos.eac3",
                    "backupUrl": [],
                    "bandwidth": 1_025_000,
                    "codecs": "ec-3"
                }]
            },
            "audio": [{
                "id": 30280,
                "baseUrl": "https://example.com/normal.m4a",
                "backupUrl": [],
                "bandwidth": 320_000,
                "codecs": "mp4a.40.2"
            }]
        }))
        .expect("valid dash fixture");
        let stream = select_audio_stream(dash, true, true, false)
            .expect("a playable stream should be selected");
        assert!(matches!(stream.kind, AudioKind::Normal));
    }

    #[cfg(windows)]
    #[test]
    fn extracts_ffmpeg_tools_from_nested_zip() {
        use super::extract_ffmpeg_tools;
        use std::io::Write as _;
        use zip::write::SimpleFileOptions;

        // 模拟 gyan/BtbN 包结构：可执行文件位于 ffmpeg-xxx/bin/ 子目录内。
        let dir = std::env::temp_dir().join(format!("seraph-ffmpeg-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("pkg.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let opts = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in [
                ("ffmpeg-7.0-essentials_build/bin/ffmpeg.exe", b"FFMPEGBIN" as &[u8]),
                ("ffmpeg-7.0-essentials_build/bin/ffprobe.exe", b"FFPROBEBIN"),
                ("ffmpeg-7.0-essentials_build/bin/ffplay.exe", b"IGNORED"),
                ("ffmpeg-7.0-essentials_build/README.txt", b"docs"),
            ] {
                writer.start_file(name, opts).unwrap();
                writer.write_all(body).unwrap();
            }
            writer.finish().unwrap();
        }

        extract_ffmpeg_tools(&zip_path, &dir).expect("extraction should succeed");
        assert!(dir.join("ffmpeg.exe").is_file());
        assert!(dir.join("ffprobe.exe").is_file());
        assert!(!dir.join("ffplay.exe").exists(), "only wanted tools extracted");
        assert_eq!(std::fs::read(dir.join("ffmpeg.exe")).unwrap(), b"FFMPEGBIN");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_bvid_from_plain_text() {
        assert_eq!(
            extract_bvid("BV1xx411c7mD").as_deref(),
            Some("BV1xx411c7mD")
        );
    }

    #[test]
    fn extracts_bvid_from_url() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD/?spm_id_from=333").as_deref(),
            Some("BV1xx411c7mD")
        );
    }

    #[test]
    fn extracts_media_id_from_favorite_url() {
        assert_eq!(
            extract_media_id("https://space.bilibili.com/1/favlist?fid=123456&ftype=create")
                .as_deref(),
            Some("123456")
        );
        assert_eq!(
            extract_media_id("https://www.bilibili.com/medialist/detail/ml987654").as_deref(),
            Some("987654")
        );
        assert_eq!(extract_media_id("123").as_deref(), Some("123"));
    }
}
