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

    /// SESS-01：带登录态的 client 按 Cookie 内容缓存——Cookie 没变就复用同一个
    /// client（连接池与 TLS 会话随之复用），变了才重建；退出登录时清空。
    #[test]
    fn sess01_cookie_client_cache_rebuilds_only_when_cookie_changes() {
        use super::super::session::CookieClientCache;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let cache = CookieClientCache::new();
        let builds = AtomicUsize::new(0);
        let get = |cookie: Option<&str>| {
            cache
                .get_or_build(cookie, |cookie| {
                    builds.fetch_add(1, Ordering::SeqCst);
                    bilibili_client_with_cookie(cookie)
                })
                .expect("client builds")
        };

        get(Some("SESSDATA=a"));
        get(Some("SESSDATA=a"));
        // 首尾空白不算变化（请求头里本来就会 trim）
        get(Some("  SESSDATA=a "));
        assert_eq!(builds.load(Ordering::SeqCst), 1, "同一 Cookie 只建一次");

        get(Some("SESSDATA=b"));
        assert_eq!(builds.load(Ordering::SeqCst), 2, "Cookie 变了要重建");

        // 未登录（无 Cookie）与空串是同一个键，和登录态互不混用
        get(None);
        get(Some(""));
        assert_eq!(builds.load(Ordering::SeqCst), 3);

        // 退出登录后清空：带旧 Cookie 的 client 不再留在缓存里
        get(Some("SESSDATA=b"));
        assert_eq!(builds.load(Ordering::SeqCst), 4);
        cache.clear();
        get(Some("SESSDATA=b"));
        assert_eq!(builds.load(Ordering::SeqCst), 5, "清空后必须重建");

        // 构建失败不缓存，下次重试
        let failing = CookieClientCache::new();
        assert!(failing
            .get_or_build(Some("x=1"), |_| Err("boom".into()))
            .is_err());
        assert!(failing
            .get_or_build(Some("x=1"), bilibili_client_with_cookie)
            .is_ok());
    }

    /// SESS-01：缓存键是 Cookie 的摘要，静态量里不多留一份 Cookie 明文。
    #[test]
    fn sess01_cookie_cache_key_is_a_digest_not_the_cookie() {
        use super::super::session::cookie_cache_key;

        let key = cookie_cache_key(Some("SESSDATA=secret-value"));
        assert_eq!(key, cookie_cache_key(Some(" SESSDATA=secret-value ")));
        assert_ne!(key, cookie_cache_key(Some("SESSDATA=other")));
        assert_eq!(cookie_cache_key(None), cookie_cache_key(Some("   ")));
        let needle = b"secret-value";
        assert!(
            !key.windows(needle.len()).any(|window| window == needle),
            "键里不能出现 Cookie 原文"
        );
    }

    /// BILI-04：下载前的同步准备收进一个函数（调用方放阻塞线程）——
    /// 有效缓存直接命中；缺完成标记的残留被清掉；fallback `.m4a` 也算命中。
    #[test]
    fn bili04_prepare_audio_cache_slot_hits_valid_cache_and_clears_partial_files() {
        use super::{
            ok_sentinel_path, prepare_audio_cache_slot, remux_fallback_cache_path,
            write_ok_sentinel,
        };

        let dir = std::env::temp_dir().join(format!(
            "seraph-bili04-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let target = dir.join("cache").join("BV1xx-1.flac");

        // 目录不存在：创建目录，未命中
        assert_eq!(prepare_audio_cache_slot(&target).unwrap(), None);
        assert!(target.parent().unwrap().is_dir());

        // 音频在但没有完成标记：视为残留，清掉
        std::fs::write(&target, b"partial").unwrap();
        assert_eq!(prepare_audio_cache_slot(&target).unwrap(), None);
        assert!(!target.exists());

        // 只有孤立的完成标记：同样清掉
        std::fs::write(ok_sentinel_path(&target), b"").unwrap();
        assert_eq!(prepare_audio_cache_slot(&target).unwrap(), None);
        assert!(!ok_sentinel_path(&target).exists());

        // fallback 文件完整：命中 fallback 路径
        let fallback = remux_fallback_cache_path(&target);
        std::fs::write(&fallback, b"audio").unwrap();
        write_ok_sentinel(&fallback);
        assert_eq!(
            prepare_audio_cache_slot(&target).unwrap(),
            Some(fallback.clone())
        );

        // 正主文件完整：优先命中正主
        std::fs::write(&target, b"audio").unwrap();
        write_ok_sentinel(&target);
        assert_eq!(
            prepare_audio_cache_slot(&target).unwrap(),
            Some(target.clone())
        );
        assert!(target.is_file() && ok_sentinel_path(&target).is_file());

        let _ = std::fs::remove_dir_all(&dir);
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
        // FFDL-01：成功后不留临时文件
        let leftovers = |dir: &std::path::Path| {
            std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .count()
        };
        assert_eq!(leftovers(&dir), 0);

        // FFDL-01：缺 ffprobe 的包解压失败时，不能留下任何「看起来已安装」的最终文件
        let partial = dir.join("partial");
        std::fs::create_dir_all(&partial).unwrap();
        let partial_zip = partial.join("pkg.zip");
        {
            let file = std::fs::File::create(&partial_zip).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("bin/ffmpeg.exe", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"FFMPEGBIN").unwrap();
            writer.finish().unwrap();
        }
        assert!(extract_ffmpeg_tools(&partial_zip, &partial).is_err());
        assert!(!partial.join("ffmpeg.exe").exists());
        assert_eq!(leftovers(&partial), 0);

        // 中断残留的临时文件在下次下载前被清掉，无关文件不动
        std::fs::write(partial.join("ffmpeg.exe.123-0.tmp"), b"half").unwrap();
        std::fs::write(partial.join("notes.tmp"), b"keep").unwrap();
        super::sweep_stale_ffmpeg_temps(&partial);
        assert!(!partial.join("ffmpeg.exe.123-0.tmp").exists());
        assert!(partial.join("notes.tmp").exists());

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
            "https://user:password@cdn.bilivideo.com/a.m4s",
            "https://cdn.bilivideo.com:8443/a.m4s",
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

    #[cfg(windows)]
    #[test]
    fn verifies_download_sha256_case_insensitively() {
        use super::verify_file_sha256;

        let dir = std::env::temp_dir().join(format!("seraph-sha-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload.bin");
        std::fs::write(&path, b"hello world").unwrap();

        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_file_sha256(&path, expected).is_ok());
        assert!(
            verify_file_sha256(&path, &expected.to_uppercase()).is_ok(),
            "官方 .sha256 文件可能大写，比对须大小写不敏感"
        );
        assert!(
            verify_file_sha256(&path, "deadbeef").is_err(),
            "哈希不匹配必须拒绝"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn rejects_zip_entry_exceeding_decompressed_limit() {
        use super::extract_ffmpeg_tools_with_limit;
        use std::io::Write as _;
        use zip::write::SimpleFileOptions;

        let dir = std::env::temp_dir().join(format!("seraph-zipbomb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("bomb.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            // 高压缩比条目：4 KB 零字节，压缩后极小；限 1 KB 时必须被硬截断拒绝。
            writer.start_file("bin/ffmpeg.exe", opts).unwrap();
            writer.write_all(&vec![0u8; 4096]).unwrap();
            writer.start_file("bin/ffprobe.exe", opts).unwrap();
            writer.write_all(b"FFPROBEBIN").unwrap();
            writer.finish().unwrap();
        }

        let err = extract_ffmpeg_tools_with_limit(&zip_path, &dir, 1024)
            .expect_err("超限条目必须导致解压失败");
        assert!(err.contains("上限"), "错误信息应指明超限: {err}");
        assert!(
            !dir.join("ffmpeg.exe").exists(),
            "超限的半截输出文件必须被清理"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn avatar_url_whitelist_only_allows_official_https_hosts() {
        for url in [
            "https://i0.hdslb.com/bfs/face/abc.jpg",
            "https://i2.hdslb.com/bfs/face/x.webp",
            "https://www.bilibili.com/face.png",
            "https://hdslb.com/face.png",
        ] {
            assert!(is_safe_avatar_url(url), "should accept {url}");
        }
        for url in [
            // 明文 http 拒绝（防被动监听）
            "http://i0.hdslb.com/bfs/face/abc.jpg",
            // 第三方 / 仿冒 host
            "https://evil.example/face.jpg",
            "https://hdslb.com.evil.example/face.jpg",
            "https://xhdslb.com/face.jpg",
            "",
            "not a url",
        ] {
            assert!(!is_safe_avatar_url(url), "should reject {url:?}");
        }
    }

    #[test]
    fn cookie_merge_only_persists_allowlisted_names() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            "SESSDATA=abc; Path=/; Secure; HttpOnly".parse().unwrap(),
        );
        headers.append(SET_COOKIE, "bili_jct=tok".parse().unwrap());
        headers.append(SET_COOKIE, "evil_tracker=x; Max-Age=99999".parse().unwrap());

        let mut cookies = BTreeMap::new();
        let mut expires = BTreeMap::new();
        merge_set_cookie_headers(&headers, &mut cookies, &mut expires);

        assert_eq!(cookies.get("SESSDATA").map(String::as_str), Some("abc"));
        assert_eq!(cookies.get("bili_jct").map(String::as_str), Some("tok"));
        assert!(
            !cookies.contains_key("evil_tracker"),
            "白名单外的 Set-Cookie 不得落盘"
        );
        assert!(!expires.contains_key("evil_tracker"));
    }
}
