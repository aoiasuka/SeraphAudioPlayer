//! 外部图片 URL 的公共白名单(安全审查 N-02 / N-03 / F-04 / S-02)。
//!
//! 此前每个调用点各写一份 host 判断(头像一份、在线封面一份、SMTC 一份漏了),
//! 新增图片源就漏一处——N-02/N-03 就是这么来的。这里收敛成单一事实来源:
//! host 后缀表 + 一个逐要素校验函数,调用点只挑表、不再自己解析 URL。
//!
//! L-3(2026-08-16 审查)追加:**所有**出站 HTTP client 的逐跳重定向白名单
//! 也从这里取表——歌词三源、GitHub 更新检查、B 站 API、ffmpeg 下载此前
//! 未挂 `guarded_redirect_policy`,上游 302 指向内网地址即是盲 SSRF 探针。
//! 新增出站 client 一律:在这里建表 + 挂 `http_util::guarded_redirect_policy`。

/// B 站官方图床与主站域(头像、视频封面)。
/// 无前导点 = 裸域与子域都接受(`hdslb.com` 本身也是有效图床入口)。
pub(crate) const BILIBILI_IMAGE_HOST_SUFFIXES: &[&str] = &["hdslb.com", "bilibili.com"];

/// 在线封面匹配用到的第三方图床(iTunes / QQ 音乐)。
/// **带前导点 = 只接受子域**——这些厂商的裸顶级域是官网/API 入口而非图床,
/// 图片实际都落在 `is1-ssl.mzstatic.com`、`y.gtimg.cn` 这类子域上,收窄没有代价。
pub(crate) const ONLINE_COVER_HOST_SUFFIXES: &[&str] = &[
    ".mzstatic.com",
    ".apple.com",
    ".gtimg.cn",
    ".qpic.cn",
    ".qq.com",
];

/// 在线歌词三源(网易云 music.163.com / 酷狗 lyrics.kugou.com / QQ c.y.qq.com)。
/// 带前导点 = 只认子域:三家的 API 全在子域上,裸顶级域没有歌词接口。
pub(crate) const LYRICS_HOST_SUFFIXES: &[&str] = &[".163.com", ".kugou.com", ".qq.com"];

/// AMLL TTML DB「预设」模式白名单——社区镜像 amlldb.bikonoo.com(2026-09-17 起唯一预设;
/// GitHub raw / jsDelivr 路径在 amll-dev 迁移后已失效,一并移除)。自定义模式另走
/// `is_public_https_url` + 禁重定向。
pub(crate) const AMLL_TTML_HOST_SUFFIXES: &[&str] = &["amlldb.bikonoo.com"];

/// AMLL 官方索引 API(api.amll.dev,2026-09-18 实测的 `/v1/lyrics/search|get`)。
/// 与镜像 DB 分表:API 走官方域,不受用户 DB 地址模式影响;正常零重定向。
pub(crate) const AMLL_API_HOST_SUFFIXES: &[&str] = &["api.amll.dev"];

/// GitHub 更新检查(check_for_update 只请求 api.github.com,正常零重定向;
/// 出现 302 时也只许留在同 host)。
pub(crate) const GITHUB_API_HOST_SUFFIXES: &[&str] = &["api.github.com"];

/// B 站站点域(API / 网页 / 短链),与 `import_audio::is_bilibili_host` 同口径。
/// b23.tv 短链的合法跳转目标就是 bilibili.com 主站。
pub(crate) const BILIBILI_SITE_HOST_SUFFIXES: &[&str] = &["bilibili.com", "b23.tv", "acg.tv"];

/// ffmpeg 下载双源(constants::FFMPEG_DOWNLOADS)。GitHub release 资产会 302 到
/// `release-assets.githubusercontent.com`(2026-08-16 实测,历史上还有
/// `objects.githubusercontent.com`),带点后缀两者都覆盖;裸 githubusercontent.com
/// 不提供资产,只认子域。
pub(crate) const FFMPEG_DOWNLOAD_HOST_SUFFIXES: &[&str] =
    &["gyan.dev", "github.com", ".githubusercontent.com"];

/// 逐要素校验:必须 https、无 userinfo、host 命中白名单。
///
/// 后缀表的约定见上面两个常量:**带前导点只认子域,不带则裸域与子域都认**。
/// 一律不做前缀匹配——`mzstatic.com.evil.com` 这类后缀伪装必须被拒;
/// userinfo 形式(`https://good.com@evil.com/`)同样拒绝,它的真实 host 是 evil.com。
pub(crate) fn is_https_url_with_host_suffix(raw: &str, suffixes: &[&str]) -> bool {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return false;
    };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    suffixes.iter().any(|suffix| {
        if let Some(apex) = suffix.strip_prefix('.') {
            // 仅子域：必须真的多一段，且边界落在点上
            host.len() > apex.len() + 1 && host.ends_with(suffix)
        } else {
            host == *suffix
                || (host.len() > suffix.len() + 1
                    && host.ends_with(suffix)
                    && host.as_bytes()[host.len() - suffix.len() - 1] == b'.')
        }
    })
}

/// N-02：可以交给**系统组件加载**的图片 URL(SMTC 缩略图)。
///
/// `track.cover` 的合法取值只有两种:本地 covers 目录路径,或 B 站导入时记下的
/// 视频封面 https 地址。曲库缓存是磁盘文件、可被离线篡改,而 SMTC 的缩略图是交给
/// **Windows Shell 进程**去取的——放任意 URL 过去等于借系统组件做出站请求
/// (且 http:// 明文可被中间人替换)。因此这里只认 B 站图床。
pub(crate) fn is_safe_system_image_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, BILIBILI_IMAGE_HOST_SUFFIXES)
}

/// L-3：在线歌词 client 的重定向白名单。
pub(crate) fn is_safe_lyrics_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, LYRICS_HOST_SUFFIXES)
}

/// AMLL TTML DB 请求与重定向白名单(base URL 校验与逐跳复验共用)。
pub(crate) fn is_safe_amll_ttml_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, AMLL_TTML_HOST_SUFFIXES)
}

/// AMLL 官方索引 API 请求与重定向白名单。
pub(crate) fn is_safe_amll_api_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, AMLL_API_HOST_SUFFIXES)
}

/// 「自定义地址」模式的底线:https、无 userinfo、host 是带点的公网域名——
/// 拒绝 IP 直连(含 IPv6 字面量)、localhost、`.local`/`.internal` 与无点的内网主机名。
/// 调用方还必须禁用重定向(自定义域不可信,不能让它把请求转向内网)。
pub(crate) fn is_public_https_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return false;
    };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    // IP 字面量：IPv6 带方括号，IPv4 全数字点分
    if host.starts_with('[') || host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    let name = host.to_ascii_lowercase();
    if !name.contains('.') || name == "localhost" {
        return false;
    }
    !(name.ends_with(".local")
        || name.ends_with(".localhost")
        || name.ends_with(".internal")
        || name.ends_with(".lan")
        || name.ends_with(".home.arpa"))
}

/// L-3：GitHub 更新检查 client 的重定向白名单。
pub(crate) fn is_safe_github_api_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, GITHUB_API_HOST_SUFFIXES)
}

/// L-3：B 站 API/短链 client 的重定向白名单(带 Cookie 的 client 也走这份——
/// 凭据只许发往 B 站域是既有基线,重定向也不例外)。
pub(crate) fn is_safe_bilibili_site_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, BILIBILI_SITE_HOST_SUFFIXES)
}

/// L-3：ffmpeg 下载 client 的重定向白名单(下载物另有 SHA-256 锚定,
/// 这里拦的是重定向中间跳的盲 SSRF 探测面)。
pub(crate) fn is_safe_ffmpeg_download_url(raw: &str) -> bool {
    is_https_url_with_host_suffix(raw, FFMPEG_DOWNLOAD_HOST_SUFFIXES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_official_hosts_and_their_subdomains() {
        for url in [
            "https://hdslb.com/a.jpg",
            "https://i0.hdslb.com/bfs/archive/x.jpg",
            "https://bilibili.com/a.jpg",
            "https://www.bilibili.com/a.jpg",
        ] {
            assert!(is_safe_system_image_url(url), "should accept {url}");
        }
        for url in [
            "https://is1-ssl.mzstatic.com/image/600x600bb.jpg",
            "https://y.gtimg.cn/music/photo_new/T002R500x500M000a.jpg",
        ] {
            assert!(
                is_https_url_with_host_suffix(url, ONLINE_COVER_HOST_SUFFIXES),
                "should accept {url}"
            );
        }
    }

    #[test]
    fn rejects_plaintext_suffix_spoofing_and_userinfo() {
        for url in [
            // 明文
            "http://i0.hdslb.com/a.jpg",
            // 后缀伪装
            "https://hdslb.com.evil.com/a.jpg",
            "https://evilhdslb.com/a.jpg",
            // userinfo 混淆：真实 host 是 evil.com
            "https://i0.hdslb.com@evil.com/a.jpg",
            // 内网探测
            "https://127.0.0.1/a.jpg",
            "http://localhost/a.jpg",
            // 无关域
            "https://evil.example.com/a.jpg",
            // 非 URL / 本地路径
            "",
            "not a url",
            r"C:\Users\me\covers\a.jpg",
        ] {
            assert!(!is_safe_system_image_url(url), "should reject {url:?}");
        }
    }

    #[test]
    fn cover_hosts_and_bilibili_hosts_are_separate_lists() {
        // 图床白名单不得互相串用：iTunes 图不该被当成可交给系统组件的地址
        assert!(!is_safe_system_image_url(
            "https://is1-ssl.mzstatic.com/image/600x600bb.jpg"
        ));
    }

    #[test]
    fn leading_dot_means_subdomain_only() {
        // 带前导点的封面图床：裸顶级域是官网/API 入口，不是图床，必须拒
        for url in [
            "https://mzstatic.com/a.jpg",
            "https://apple.com/a.jpg",
            "https://qq.com/a.jpg",
        ] {
            assert!(
                !is_https_url_with_host_suffix(url, ONLINE_COVER_HOST_SUFFIXES),
                "裸顶级域应被拒: {url}"
            );
        }
        // 不带前导点的 B 站列表：裸域本身也是有效入口，接受
        assert!(is_https_url_with_host_suffix(
            "https://hdslb.com/a.jpg",
            BILIBILI_IMAGE_HOST_SUFFIXES
        ));
    }

    /// L-3：四组重定向白名单——真实跳转目标放行，内网/伪装/明文一律拒。
    #[test]
    fn l3_redirect_allowlists_accept_real_targets_and_reject_probes() {
        // 各 client 的真实请求/跳转目标
        assert!(is_safe_lyrics_url("https://music.163.com/api/song/lyric"));
        assert!(is_safe_lyrics_url(
            "https://lyrics.kugou.com/download?ver=1"
        ));
        assert!(is_safe_lyrics_url(
            "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg"
        ));
        // QQ 客户端取词接口（逐字 QRC）也在 .qq.com 子域内
        assert!(is_safe_lyrics_url("https://u.y.qq.com/cgi-bin/musicu.fcg"));
        assert!(is_safe_github_api_url(
            "https://api.github.com/repos/aoiasuka/SeraphAudioPlayer/releases/latest"
        ));
        assert!(is_safe_bilibili_site_url(
            "https://www.bilibili.com/video/BV1xx411c7mD"
        ));
        assert!(is_safe_bilibili_site_url("https://b23.tv/abc123"));
        assert!(is_safe_ffmpeg_download_url(
            "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0-essentials_build.zip"
        ));
        assert!(is_safe_ffmpeg_download_url(
            "https://github.com/GyanD/codexffmpeg/releases/download/9.0/ffmpeg.zip"
        ));
        // GitHub release 资产的真实 302 目标（2026-08-16 实测）
        assert!(is_safe_ffmpeg_download_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/x"
        ));
        assert!(is_safe_ffmpeg_download_url(
            "https://objects.githubusercontent.com/github-production-release-asset/x"
        ));
        assert!(is_safe_amll_ttml_url(
            "https://amlldb.bikonoo.com/ncm-lyrics/1.ttml"
        ));
        assert!(!is_safe_amll_ttml_url(
            "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/ncm-lyrics/1.ttml"
        ));
        assert!(!is_safe_amll_ttml_url(
            "https://cdn.jsdelivr.net/gh/amll-dev/amll-ttml-db@main/x"
        ));
        assert!(!is_safe_amll_ttml_url("https://bikonoo.com/x"));
        // 官方索引 API：只认 api.amll.dev，镜像域与裸 amll.dev 都不放行
        assert!(is_safe_amll_api_url(
            "https://api.amll.dev/v1/lyrics/search?musicName=a"
        ));
        assert!(!is_safe_amll_api_url("https://amll.dev/v1/lyrics/search"));
        assert!(!is_safe_amll_api_url(
            "https://amlldb.bikonoo.com/ncm-lyrics/1.ttml"
        ));
        assert!(!is_safe_amll_api_url(
            "http://api.amll.dev/v1/lyrics/get?id=1"
        ));

        // 自定义模式守卫：公网域名放行，其余拒
        assert!(is_public_https_url(
            "https://amlldb.bikonoo.com/ncm-lyrics/1.ttml"
        ));
        for url in [
            "http://amlldb.bikonoo.com/x",
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "https://10.0.0.5/x",
            "https://localhost/x",
            "https://nas/x",
            "https://printer.local/x",
            "https://db.internal/x",
            "https://user@amlldb.bikonoo.com/x",
        ] {
            assert!(
                !is_public_https_url(url),
                "public guard should reject {url}"
            );
        }

        // 盲 SSRF 探针与伪装：全部表都必须拒
        for url in [
            "http://music.163.com/api",          // 明文
            "https://127.0.0.1:8080/",           // 内网
            "https://localhost/",                // 内网
            "https://music.163.com.evil.com/x",  // 后缀伪装
            "https://api.github.com@evil.com/x", // userinfo 混淆
            "https://githubusercontent.com/x",   // 裸域（只认子域）
            "https://evilb23.tv.evil.com/x",     // 无关域
        ] {
            assert!(!is_safe_lyrics_url(url), "lyrics should reject {url}");
            assert!(!is_safe_amll_ttml_url(url), "amll should reject {url}");
            assert!(!is_safe_amll_api_url(url), "amll api should reject {url}");
            assert!(!is_safe_github_api_url(url), "github should reject {url}");
            assert!(
                !is_safe_bilibili_site_url(url),
                "bilibili should reject {url}"
            );
            assert!(
                !is_safe_ffmpeg_download_url(url),
                "ffmpeg should reject {url}"
            );
        }
    }
}
