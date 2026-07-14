use super::prelude::*;

impl AudioStream {
    pub(crate) fn audio_urls(&self) -> Vec<String> {
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

    pub(crate) fn kind_rank(&self) -> u8 {
        match self.kind {
            AudioKind::DolbyAtmos => 4,
            AudioKind::Flac => 3,
            AudioKind::Dolby => 2,
            AudioKind::Normal => 1,
        }
    }

    pub(crate) fn format_label(&self) -> &'static str {
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

    pub(crate) fn output_extension(&self) -> &'static str {
        // 即使未 remux 也按真实编码落扩展名，避免 FLAC stream 被命名为 .m4a
        // 导致后续元数据探测失败 (M-2)。
        match self.format_label() {
            "FLAC" => "flac",
            "OPUS" => "opus",
            "EAC3" => "eac3",
            _ => "m4a",
        }
    }
}

impl BilibiliSession {
    pub(crate) fn cookie_header(&self) -> Option<String> {
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
    // 迁出 include! 后被测符号分散在各兄弟子模块，统一经 prelude 引入
    use super::super::prelude::*;
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
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in [
                (
                    "ffmpeg-7.0-essentials_build/bin/ffmpeg.exe",
                    b"FFMPEGBIN" as &[u8],
                ),
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
        assert!(
            !dir.join("ffplay.exe").exists(),
            "only wanted tools extracted"
        );
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

    #[test]
    fn bilibili_host_whitelist_accepts_official_hosts() {
        for url in [
            "https://b23.tv/abc123",
            "https://acg.tv/av170001",
            "https://bilibili.com/video/BV1xx411c7mD",
            "https://www.bilibili.com/video/BV1xx411c7mD",
            "https://m.bilibili.com/video/BV1xx411c7mD",
            "https://WWW.BILIBILI.COM/video/BV1xx411c7mD",
        ] {
            let parsed = reqwest::Url::parse(url).unwrap();
            assert!(is_bilibili_host(&parsed), "should accept {url}");
        }
    }

    #[test]
    fn bilibili_host_whitelist_rejects_lookalike_hosts() {
        for url in [
            "https://evil.example/BV-share",
            "https://bilibili.com.evil.example/video",
            "https://fakebilibili.com/video",
            "https://notb23.tv/xyz",
            "https://bilibili.com@evil.example/",
            "https://xbilibili.com/",
        ] {
            let parsed = reqwest::Url::parse(url).unwrap();
            assert!(!is_bilibili_host(&parsed), "should reject {url}");
        }
    }

    #[test]
    fn download_url_whitelist_accepts_official_cdn_https() {
        for url in [
            "https://upos-sz-mirrorcos.bilivideo.com/upgcxcode/a.m4s?e=x",
            "https://cn-gotcha01.bilivideo.cn/xxx/audio.m4s",
            "https://xy1x2x3.hdslb.com/audio.m4s",
            "https://a.b.akamaized.net/audio.m4s",
        ] {
            assert!(is_safe_bilibili_download_url(url), "should accept {url}");
        }
    }

    #[test]
    fn download_url_whitelist_rejects_unsafe() {
        for url in [
            // 明文 http 一律拒绝（防被动监听截获 Cookie）
            "http://upos-sz-mirrorcos.bilivideo.com/a.m4s",
            // 第三方 / 仿冒 host
            "https://evil.example/a.m4s",
            "https://bilivideo.com.evil.example/a.m4s",
            "https://notbilivideo.com/a.m4s",
            // 裸后缀（无子域）不放行，避免仅凭后缀匹配放过奇怪构造
            "https://bilivideo.com/a.m4s",
            "https://akamaized.net/a.m4s",
            "",
            "not a url",
        ] {
            assert!(!is_safe_bilibili_download_url(url), "should reject {url:?}");
        }
    }

    #[test]
    fn parses_expires_date_case_insensitively() {
        // 上游会把整段属性 to_ascii_lowercase，月份必须大小写不敏感匹配。
        let expected = parse_http_date_to_unix("Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(expected, Some(784_111_777));
        assert_eq!(
            parse_http_date_to_unix("sun, 06 nov 1994 08:49:37 gmt"),
            expected
        );
        assert_eq!(
            parse_http_date_to_unix("SUN, 06 NOV 1994 08:49:37 GMT"),
            expected
        );
        assert_eq!(
            parse_http_date_to_unix("sun, 06 xxx 1994 08:49:37 gmt"),
            None
        );
    }

    #[test]
    fn temp_download_paths_are_unique() {
        let target = std::path::Path::new("C:/cache/BV1xx411c7mD-123.flac");
        let first = temp_download_path(target);
        let second = temp_download_path(target);
        assert_ne!(first, second, "并发下载的临时文件路径必须唯一");
        for path in [&first, &second] {
            assert_eq!(
                path.extension().and_then(|value| value.to_str()),
                Some("download")
            );
        }
    }
}
