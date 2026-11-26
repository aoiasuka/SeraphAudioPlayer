//! 系统外壳集成命令。
//!
//! v0.4.3 右键菜单的「打开文件所在位置」：在资源管理器中定位并选中曲目文件。

use std::path::{Path, PathBuf};

use super::error::{IpcError, IpcResult};
use super::path_guard::{path_location, PathLocation};
use tauri::Manager;

/// 唤起并聚焦主窗口（任务栏歌词条的 ⌂ 按钮）。
/// async：窗口操作不占用主线程同步等待（同 set_taskbar_lyrics_enabled）。
#[tauri::command]
pub async fn focus_main_window(app: tauri::AppHandle) -> IpcResult<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Err(IpcError::not_found("主窗口不存在"));
    };
    let _ = window.unminimize();
    let _ = window.show();
    window
        .set_focus()
        .map_err(|err| IpcError::from(err.to_string()))
}

/// 不碰文件系统的那一半校验：非空、绝对路径，以及路径指向哪里（SEC3-01）。
///
/// UNC 路径（`\\主机\共享\…`）在 Windows 上也是绝对路径，而随后的存在性检查
/// （`fs::metadata`）就会向路径里的主机发起 SMB 连接，域环境下还带 NTLM 认证质询——
/// 与 L-1（SMTC 封面）、PL-01（M3U8 导入）是同一类出站。所以这一步必须在探测**之前**：
/// - 设备路径（`\\.\…`、卷 GUID 等）一律拒绝，曲目文件不会长这样；
/// - 网络共享路径只在 `in_library` 确认「曲库里确实有一首曲目就是这个路径」时放行。
///   曲库里的网络路径只有两个来源：用户自己在文件对话框里选的，或与清单文件同一台主机的
///   M3U8 条目（见 `playlist_io`）——都是用户已经连过的主机。渲染进程不能再借这条命令
///   让后端去探测任意主机。
/// - 本机路径照旧。
fn check_reveal_path(
    path: &str,
    in_library: impl FnOnce(&Path) -> bool,
) -> Result<PathBuf, IpcError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("文件路径为空"));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(IpcError::invalid_input(format!("拒绝相对路径: {trimmed}")));
    }
    match path_location(path) {
        PathLocation::Local => {}
        PathLocation::Network { .. } if in_library(path) => {}
        PathLocation::Network { .. } => {
            return Err(IpcError::invalid_input(format!(
                "只能定位曲库中已有的网络共享文件: {trimmed}"
            )));
        }
        PathLocation::Device => {
            return Err(IpcError::invalid_input(format!(
                "不支持设备路径: {trimmed}"
            )));
        }
    }
    Ok(path.to_path_buf())
}

/// 校验待定位的文件路径：先过 [`check_reveal_path`]，再确认它确实是一个存在的文件。
/// 前端传来的路径可能指向已被移动/删除的文件（如失效的 B 站缓存），此处兜底拒绝。
fn validate_reveal_target(
    path: &str,
    in_library: impl FnOnce(&Path) -> bool,
) -> Result<PathBuf, IpcError> {
    let target = check_reveal_path(path, in_library)?;
    let shown = target.display();
    let metadata = std::fs::metadata(&target)
        .map_err(|_| IpcError::not_found(format!("文件不存在或已被移动: {shown}")))?;
    if !metadata.is_file() {
        return Err(IpcError::invalid_input(format!("目标不是文件: {shown}")));
    }
    Ok(target)
}

/// 在 Windows 资源管理器中打开文件所在文件夹并选中该文件。
/// 存在性检查是磁盘 IO（慢速/网络盘可能阻塞），移到阻塞线程池执行。
#[tauri::command]
pub async fn reveal_in_explorer(app: tauri::AppHandle, path: String) -> IpcResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        reveal_in_explorer_inner(&path, |target| {
            super::library::library_references_path(&app, target)
        })
    })
    .await
    .map_err(|err| IpcError::from(format!("reveal task panicked: {err}")))?
}

fn reveal_in_explorer_inner(path: &str, in_library: impl FnOnce(&Path) -> bool) -> IpcResult<()> {
    let target = validate_reveal_target(path, in_library)?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer 的定位参数格式是 `/select,<path>`，含空格路径需整体加引号。
        // Windows 文件名不允许出现引号字符，直接包裹安全；用 raw_arg 绕过
        // std 的自动引号规则，确保 explorer 收到的就是这串原始参数。
        // F-05：走绝对路径，避免裸名被同目录同名 EXE 劫持（explorer 本体在 Windows 目录根下）
        let explorer = super::path_guard::explorer_exe().map_err(IpcError::from)?;
        std::process::Command::new(explorer)
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

    /// 本机路径的校验不应该去问曲库。
    fn never_asked(_: &Path) -> bool {
        panic!("本机路径不需要查曲库")
    }

    #[test]
    fn rejects_empty_and_relative_paths() {
        assert_eq!(
            validate_reveal_target("", never_asked).unwrap_err().code,
            IpcErrorCode::InvalidInput
        );
        assert_eq!(
            validate_reveal_target("   ", never_asked).unwrap_err().code,
            IpcErrorCode::InvalidInput
        );
        assert_eq!(
            validate_reveal_target("relative/file.flac", never_asked)
                .unwrap_err()
                .code,
            IpcErrorCode::InvalidInput
        );
    }

    #[test]
    fn rejects_missing_file_with_not_found() {
        let missing = std::env::temp_dir().join("seraph-reveal-missing-does-not-exist.flac");
        let err = validate_reveal_target(&missing.to_string_lossy(), never_asked).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::NotFound);
    }

    #[test]
    fn rejects_directory_but_accepts_real_file() {
        let dir = std::env::temp_dir();
        assert_eq!(
            validate_reveal_target(&dir.to_string_lossy(), never_asked)
                .unwrap_err()
                .code,
            IpcErrorCode::InvalidInput
        );

        let file = dir.join(format!("seraph-reveal-test-{}.tmp", std::process::id()));
        std::fs::write(&file, b"x").expect("写入临时文件失败");
        let verdict = validate_reveal_target(&file.to_string_lossy(), never_asked);
        let _ = std::fs::remove_file(&file);
        assert!(verdict.is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn network_paths_are_only_revealed_when_the_library_has_them() {
        // SEC3-01：这里只测不碰文件系统的那一半（`check_reveal_path`）——真去探测
        // `.invalid` 主机要等名字解析超时，而且「没有探测」正是要保证的事。
        let unc = r"\\media-nas.invalid\music\song.flac";
        let verbatim = r"\\?\UNC\media-nas.invalid\music\song.flac";
        for path in [unc, verbatim] {
            let err = check_reveal_path(path, |_| false).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidInput, "{path}");
            assert!(err.message.contains("网络共享"), "{}", err.message);
        }
        // 走完整校验也是在探测之前就被拒：拿到的是「拒绝」而不是「文件不存在」
        let err = validate_reveal_target(unc, |_| false).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidInput);

        // 曲库里确实有这首曲目（用户自己从这台主机导入的）才放行，查的就是这个路径
        let mut asked = None;
        let accepted = check_reveal_path(unc, |path| {
            asked = Some(path.to_path_buf());
            true
        })
        .unwrap();
        assert_eq!(accepted, Path::new(unc));
        assert_eq!(asked.as_deref(), Some(Path::new(unc)));
    }

    #[cfg(windows)]
    #[test]
    fn device_paths_are_never_revealed() {
        for path in [
            r"\\.\PhysicalDrive0",
            r"\\.\C:\Music\song.flac",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\song.flac",
        ] {
            // 即使「曲库里有」也不行：曲目文件不会是设备路径
            let err = check_reveal_path(path, |_| true).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidInput, "{path}");
            assert!(err.message.contains("设备路径"), "{}", err.message);
        }
        // `\\?\C:\…` 只是关掉规范化的盘符路径，按本机路径处理、不问曲库
        let verbatim_local = r"\\?\C:\Music\song.flac";
        assert_eq!(
            check_reveal_path(verbatim_local, never_asked).unwrap(),
            Path::new(verbatim_local)
        );
    }
}
