//! 应用内检查更新。
//!
//! 查询 GitHub Releases 最新版本与当前版本比较；不做自动下载/安装
//! （完整 updater 需要签名密钥托管），发现新版时引导用户到 Release 页下载。

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

use super::error::{IpcError, IpcResult};

const RELEASES_API: &str =
    "https://api.github.com/repos/aoiasuka/SeraphAudioPlayer/releases/latest";
/// open_release_page 只允许打开本仓库 Release 页，防止外部数据注入任意 URL。
const RELEASE_URL_PREFIX: &str = "https://github.com/aoiasuka/SeraphAudioPlayer/releases";

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
        .build()
        .map_err(|err| IpcError::network(format!("创建网络客户端失败: {err}")))?;

    let release: LatestRelease = client
        .get(RELEASES_API)
        .header("User-Agent", "SeraphAudioPlayer-UpdateCheck")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| IpcError::network(format!("检查更新失败: {err}")))?
        .error_for_status()
        .map_err(|err| IpcError::network(format!("检查更新失败: {err}")))?
        .json()
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

/// 用系统默认浏览器打开 Release 页。URL 必须是本仓库 Release 页（前缀白名单）。
#[tauri::command]
pub fn open_release_page(url: String) -> IpcResult<()> {
    if !url.starts_with(RELEASE_URL_PREFIX) {
        return Err(IpcError::invalid_input(format!(
            "拒绝打开非发布页地址: {url}"
        )));
    }

    #[cfg(windows)]
    {
        Command::new("explorer")
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
}
