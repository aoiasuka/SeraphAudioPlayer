//! 外部图片 URL 的公共白名单(安全审查 N-02 / N-03 / F-04 / S-02)。
//!
//! 此前每个调用点各写一份 host 判断(头像一份、在线封面一份、SMTC 一份漏了),
//! 新增图片源就漏一处——N-02/N-03 就是这么来的。这里收敛成单一事实来源:
//! host 后缀表 + 一个逐要素校验函数,调用点只挑表、不再自己解析 URL。

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
}
