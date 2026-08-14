//! 本地文件与外部程序访问的公共防线(安全审查 F-02 / F-05)。
//!
//! 这里收口两件事:
//! 1. 文件读写 IPC 的路径约束——命令边界不能相信渲染进程传来的任意路径;
//! 2. Windows 系统工具的绝对路径解析——裸名 `Command::new` 会走 CreateProcess
//!    的搜索顺序(含应用目录与当前工作目录),存在同名 EXE 种植面。

use super::error::{IpcError, IpcResult};
use std::path::{Path, PathBuf};

/// F-05：把系统工具名解析为 `%SystemRoot%\System32\<name>` 绝对路径。
///
/// `Command::new("icacls")` / `Command::new("explorer.exe")` 走的是 CreateProcess
/// 搜索顺序,其中包含**应用自身所在目录**与(部分情形下的)当前工作目录——
/// 应用从下载目录/共享盘直接运行时,同目录放一个 `icacls.exe` 即被优先执行。
/// 系统工具一律走绝对路径,不给搜索顺序留缝。
///
/// `SystemRoot` 缺失(异常环境)时回落到裸名,保持功能可用而不是直接失败。
#[cfg(windows)]
pub(crate) fn system32_tool(name: &str) -> PathBuf {
    match std::env::var_os("SystemRoot") {
        Some(root) if !root.is_empty() => Path::new(&root).join("System32").join(name),
        _ => PathBuf::from(name),
    }
}

/// F-02：**写入**类 IPC 的路径校验。
///
/// 路径正常来自 dialog 选择结果,但命令边界不校验的话,任一渲染进程被攻破即等于
/// 任意文件写入(可覆盖 library-cache.json / cache-settings.json 或用户文档)。
/// 要求:非空、绝对路径、扩展名在白名单内、父目录已存在且确实是目录。
///
/// 返回补齐扩展名后的目标路径。
pub(crate) fn validate_export_path(raw: &str, allowed_extensions: &[&str]) -> IpcResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("导出路径为空"));
    }
    let mut target = PathBuf::from(trimmed);
    if !target.is_absolute() {
        return Err(IpcError::invalid_input("拒绝相对导出路径"));
    }
    // 点段必须在这里拒掉:`C:\a\..\..\Windows\x.json` 规范化后会跑出预期目录,
    // 只看扩展名挡不住(与 S-04 的 URL 点段同理)。
    if target
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(IpcError::invalid_input("导出路径不得包含 .. 点段"));
    }

    let has_allowed_ext = target
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            allowed_extensions
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        });
    if !has_allowed_ext {
        // 沿用既有行为:没写扩展名就补一个默认的,而不是直接报错
        target.set_extension(allowed_extensions[0]);
    }

    let parent = target
        .parent()
        .ok_or_else(|| IpcError::invalid_input("导出路径没有父目录"))?;
    if !parent.is_dir() {
        return Err(IpcError::invalid_input("导出目录不存在"));
    }
    Ok(target)
}

/// F-02：**读取**类 IPC 的路径校验。
///
/// 除扩展名白名单外还要求目标是已存在的普通文件——否则 `import_app_config`
/// 之流等于「任意 ≤2 MB 文件读取并回传渲染进程」。
pub(crate) fn validate_import_path(raw: &str, allowed_extensions: &[&str]) -> IpcResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("文件路径为空"));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(IpcError::invalid_input("拒绝相对路径"));
    }
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(IpcError::invalid_input("路径不得包含 .. 点段"));
    }
    let ext_ok = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            allowed_extensions
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        });
    if !ext_ok {
        return Err(IpcError::invalid_input(format!(
            "只接受 {} 文件",
            allowed_extensions
                .iter()
                .map(|ext| format!(".{ext}"))
                .collect::<Vec<_>>()
                .join(" / ")
        )));
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|_| IpcError::not_found(format!("文件不存在: {trimmed}")))?;
    if !metadata.is_file() {
        return Err(IpcError::invalid_input("目标不是文件"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn export_path_requires_absolute_and_existing_parent() {
        let ok = temp_dir().join("seraph-export-test.json");
        assert!(validate_export_path(ok.to_str().unwrap(), &["json"]).is_ok());

        assert!(validate_export_path("relative.json", &["json"]).is_err());
        assert!(validate_export_path("", &["json"]).is_err());
        assert!(validate_export_path(
            temp_dir()
                .join("no-such-dir-xyz")
                .join("a.json")
                .to_str()
                .unwrap(),
            &["json"]
        )
        .is_err());
    }

    #[test]
    fn export_path_rejects_parent_dir_segments() {
        let sneaky = temp_dir().join("..").join("evil.json");
        assert!(validate_export_path(sneaky.to_str().unwrap(), &["json"]).is_err());
    }

    #[test]
    fn export_path_appends_default_extension() {
        let target =
            validate_export_path(temp_dir().join("seraph-noext").to_str().unwrap(), &["json"])
                .expect("should accept and fix up");
        assert_eq!(target.extension().unwrap(), "json");
    }

    #[test]
    fn import_path_rejects_wrong_extension_and_missing_file() {
        let path = temp_dir().join("seraph-import-test.json");
        std::fs::write(&path, b"{}").unwrap();
        assert!(validate_import_path(path.to_str().unwrap(), &["json"]).is_ok());
        // 扩展名不在白名单 → 拒绝（原先任意 ≤2MB 文件都能读回）
        assert!(validate_import_path(path.to_str().unwrap(), &["m3u8"]).is_err());
        let _ = std::fs::remove_file(&path);
        // 文件已删除 → 拒绝
        assert!(validate_import_path(path.to_str().unwrap(), &["json"]).is_err());
    }
}
