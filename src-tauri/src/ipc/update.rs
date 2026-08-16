//! 应用内检查更新。
//!
//! 查询 GitHub Releases 最新版本与当前版本比较；不做自动下载/安装
//! （完整 updater 需要签名密钥托管），发现新版时引导用户到 Release 页下载。

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

use super::error::{IpcError, IpcResult};
use super::http_util::{guarded_redirect_policy, read_json_capped, MAX_EXTERNAL_JSON_BYTES};
use super::url_guard::is_safe_github_api_url;

const RELEASES_API: &str =
    "https://api.github.com/repos/aoiasuka/SeraphAudioPlayer/releases/latest";
/// open_release_page 只允许打开本仓库 Release 页，防止外部数据注入任意 URL。
const RELEASE_PATH_PREFIX: &str = "/aoiasuka/SeraphAudioPlayer/releases";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
    pub release_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
}

#[tauri::command]
pub async fn check_for_update() -> IpcResult<UpdateCheckResult> {
    let current = env!("CARGO_PKG_VERSION").to_string();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        // L-3（2026-08-16 审查）：逐跳复验重定向，GitHub API 正常零跳，
        // 出现 302 也只许留在 api.github.com（F-01 同型防线）
        .redirect(guarded_redirect_policy(is_safe_github_api_url))
        .build()
        .map_err(|err| IpcError::network(format!("创建网络客户端失败: {err}")))?;

    let response = client
        .get(RELEASES_API)
        .header("User-Agent", "SeraphAudioPlayer-UpdateCheck")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| IpcError::network(format!("检查更新失败: {err}")))?
        .error_for_status()
        .map_err(|err| IpcError::network(format!("检查更新失败: {err}")))?;
    // S-03：外部 JSON capped 读取，防异常超大响应
    let release: LatestRelease = read_json_capped(response, MAX_EXTERNAL_JSON_BYTES)
        .await
        .map_err(|err| IpcError::network(format!("解析更新信息失败: {err}")))?;

    let latest = release.tag_name.trim_start_matches('v').to_string();
    let update_available = is_newer_version(&latest, &current);

    Ok(UpdateCheckResult {
        current_version: current,
        latest_version: latest,
        update_available,
        release_url: release.html_url,
        release_notes: release.body,
    })
}

/// S-04：Release 页 URL 精确校验。此前的 `starts_with` 前缀匹配有两类绕过：
/// path 拼接（`…/releases.evil.com/…`）与点段穿越（`…/releases/../../其他仓库`，
/// WHATWG 解析器连 `%2e%2e` 变体也会规范化成上级路径）。改为 URL 解析后逐要素
/// 校验：scheme/host/端口/userinfo 全部固定，path 在规范化后按段边界匹配。
fn is_allowed_release_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let path = url.path();
    // rust-url 解析时已消解 `.`/`..`（含 %2e 编码变体）；再显式拒绝残余点段，
    // 防不同 URL 实现（此处校验 vs 系统浏览器）规范化差异。
    if path.split('/').any(|seg| {
        let lower = seg.to_ascii_lowercase();
        lower == ".." || lower == "." || lower.contains("%2e")
    }) {
        return false;
    }
    path == RELEASE_PATH_PREFIX || path.starts_with(&format!("{RELEASE_PATH_PREFIX}/"))
}

/// 用系统默认浏览器打开 Release 页。URL 必须是本仓库 Release 页（精确校验）。
#[tauri::command]
pub fn open_release_page(url: String) -> IpcResult<()> {
    if !is_allowed_release_url(&url) {
        return Err(IpcError::invalid_input(format!(
            "拒绝打开非发布页地址: {url}"
        )));
    }

    #[cfg(windows)]
    {
        // F-05：走 System32 绝对路径，避免裸名被同目录同名 EXE 劫持
        Command::new(crate::ipc::path_guard::system32_tool("explorer.exe"))
            .arg(&url)
            .spawn()
            .map_err(|err| IpcError::from(format!("打开浏览器失败: {err}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err(IpcError::invalid_input("当前平台不支持"))
    }
}

/// 语义化版本比较：`latest` 是否严格大于 `current`。
/// 解析失败（非 x.y.z 数字段）按不可比较处理，返回 false 避免误报。
fn is_newer_version(latest: &str, current: &str) -> bool {
    let (Some(latest), Some(current)) = (parse_version(latest), parse_version(current)) else {
        return false;
    };
    latest > current
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let parts = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semantic_versions() {
        assert!(is_newer_version("0.4.0", "0.3.8"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(is_newer_version("0.3.10", "0.3.9"));
        assert!(!is_newer_version("0.3.8", "0.3.8"));
        assert!(!is_newer_version("0.3.7", "0.3.8"));
    }

    #[test]
    fn tolerates_v_prefix_and_garbage() {
        assert!(is_newer_version("v0.4.0", "0.3.8"));
        assert!(!is_newer_version("not-a-version", "0.3.8"));
        assert!(!is_newer_version("", "0.3.8"));
    }

    #[test]
    fn shorter_version_compares_lexicographically() {
        // Vec 比较语义：前缀相同长度短的更小
        assert!(is_newer_version("0.4", "0.3.8"));
        assert!(is_newer_version("0.3.8.1", "0.3.8"));
    }

    #[test]
    fn release_url_allowlist_accepts_repo_release_pages() {
        for url in [
            "https://github.com/aoiasuka/SeraphAudioPlayer/releases",
            "https://github.com/aoiasuka/SeraphAudioPlayer/releases/tag/v0.5.6",
            "https://github.com/aoiasuka/SeraphAudioPlayer/releases/latest",
        ] {
            assert!(is_allowed_release_url(url), "should accept {url}");
        }
    }

    #[test]
    fn release_url_allowlist_rejects_bypass_attempts() {
        for url in [
            // 前缀拼接绕过（S-04 报告原始场景）
            "https://github.com/aoiasuka/SeraphAudioPlayer/releases.evil.com/x",
            // 点段穿越到 github.com 上任意仓库（含 %2e 编码变体）
            "https://github.com/aoiasuka/SeraphAudioPlayer/releases/../../evil/repo",
            "https://github.com/aoiasuka/SeraphAudioPlayer/releases/%2e%2e/%2e%2e/evil/repo",
            // userinfo / 明文 / 异 host / 非常规端口
            "https://github.com@evil.example/aoiasuka/SeraphAudioPlayer/releases",
            "http://github.com/aoiasuka/SeraphAudioPlayer/releases",
            "https://evil.example/aoiasuka/SeraphAudioPlayer/releases",
            "https://github.com:8443/aoiasuka/SeraphAudioPlayer/releases",
            "",
            "not a url",
        ] {
            assert!(!is_allowed_release_url(url), "should reject {url:?}");
        }
    }
}
