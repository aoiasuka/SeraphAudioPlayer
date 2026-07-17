//! 系统外壳集成命令。
//!
//! v0.4.3 右键菜单的「打开文件所在位置」：在资源管理器中定位并选中曲目文件。

use std::path::{Path, PathBuf};

use super::error::{IpcError, IpcResult};

/// 校验待定位的文件路径：非空、绝对路径、且确实是一个存在的文件。
/// 前端传来的路径可能指向已被移动/删除的文件（如失效的 B 站缓存），此处兜底拒绝。
fn validate_reveal_target(path: &str) -> Result<PathBuf, IpcError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("文件路径为空"));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(IpcError::invalid_input(format!("拒绝相对路径: {trimmed}")));
    }
    let metadata = std::fs::metadata(path)
        .map_err(|_| IpcError::not_found(format!("文件不存在或已被移动: {trimmed}")))?;
    if !metadata.is_file() {
        return Err(IpcError::invalid_input(format!("目标不是文件: {trimmed}")));
    }
    Ok(path.to_path_buf())
}

/// 在 Windows 资源管理器中打开文件所在文件夹并选中该文件。
/// 存在性检查是磁盘 IO（慢速/网络盘可能阻塞），移到阻塞线程池执行。
#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> IpcResult<()> {
    tauri::async_runtime::spawn_blocking(move || reveal_in_explorer_inner(&path))
        .await
        .map_err(|err| IpcError::from(format!("reveal task panicked: {err}")))?
}

fn reveal_in_explorer_inner(path: &str) -> IpcResult<()> {
    let target = validate_reveal_target(path)?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer 的定位参数格式是 `/select,<path>`，含空格路径需整体加引号。
        // Windows 文件名不允许出现引号字符，直接包裹安全；用 raw_arg 绕过
        // std 的自动引号规则，确保 explorer 收到的就是这串原始参数。
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", target.display()))
            .spawn()
            .map_err(|err| IpcError::from(format!("打开资源管理器失败: {err}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err(IpcError::invalid_input("当前平台不支持在文件管理器中定位"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::error::IpcErrorCode;

    #[test]
    fn rejects_empty_and_relative_paths() {
        assert_eq!(
            validate_reveal_target("").unwrap_err().code,
            IpcErrorCode::InvalidInput
        );
        assert_eq!(
            validate_reveal_target("   ").unwrap_err().code,
            IpcErrorCode::InvalidInput
        );
        assert_eq!(
            validate_reveal_target("relative/file.flac")
                .unwrap_err()
                .code,
            IpcErrorCode::InvalidInput
        );
    }

    #[test]
    fn rejects_missing_file_with_not_found() {
        let missing = std::env::temp_dir().join("seraph-reveal-missing-does-not-exist.flac");
        let err = validate_reveal_target(&missing.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::NotFound);
    }

    #[test]
    fn rejects_directory_but_accepts_real_file() {
        let dir = std::env::temp_dir();
        assert_eq!(
            validate_reveal_target(&dir.to_string_lossy())
                .unwrap_err()
                .code,
            IpcErrorCode::InvalidInput
        );

        let file = dir.join(format!("seraph-reveal-test-{}.tmp", std::process::id()));
        std::fs::write(&file, b"x").expect("写入临时文件失败");
        let verdict = validate_reveal_target(&file.to_string_lossy());
        let _ = std::fs::remove_file(&file);
        assert!(verdict.is_ok());
    }
}
