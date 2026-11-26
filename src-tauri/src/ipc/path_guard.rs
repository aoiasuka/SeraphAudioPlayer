//! 本地文件与外部程序访问的公共防线(安全审查 F-02 / F-05)。
//!
//! 这里收口两件事:
//! 1. 文件读写 IPC 的路径约束——命令边界不能相信渲染进程传来的任意路径;
//! 2. Windows 系统工具的绝对路径解析——裸名 `Command::new` 会走 CreateProcess
//!    的搜索顺序(含应用目录与当前工作目录),存在同名 EXE 种植面。

use super::error::{IpcError, IpcResult};
use std::path::{Path, PathBuf};

/// F-05：把系统工具名解析为 `<系统目录>\<name>` 绝对路径（`C:\Windows\System32\icacls.exe`）。
///
/// `Command::new("icacls")` / `Command::new("explorer.exe")` 走的是 CreateProcess
/// 搜索顺序,其中包含**应用自身所在目录**与(部分情形下的)当前工作目录——
/// 应用从下载目录/共享盘直接运行时,同目录放一个 `icacls.exe` 即被优先执行。
/// 系统工具一律走绝对路径,不给搜索顺序留缝。
///
/// SEC3-02：系统目录由 `GetSystemDirectoryW` 给出，不读环境变量；取不到就**报错**。
/// 此前读 `%SystemRoot%`，变量缺失时回落到裸名——正是上面要避免的搜索顺序，
/// 与 M-1「外部工具找不到就报错，严禁回退裸名」的口径不一致。
#[cfg(windows)]
pub(crate) fn system32_tool(name: &str) -> Result<PathBuf, String> {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    // SAFETY: 回调只把这两个参数原样交给系统函数，缓冲区与长度由 `query_directory` 保证匹配
    query_directory(|buffer, len| unsafe { GetSystemDirectoryW(buffer, len) })
        .map(|dir| dir.join(name))
        .map_err(|err| format!("无法确定系统目录，已放弃调用 {name}：{err}"))
}

/// `<Windows 目录>\explorer.exe` 绝对路径（打开 URL / 定位文件都靠它）。
///
/// 2026-09-22 教训：explorer.exe **不在 System32**（那里只有 SysWOW64 的 32 位副本与
/// Windows 目录根下的 64 位本体），`system32_tool("explorer.exe")` 从 v0.5.7 起一直
/// 指向不存在的文件，「前往下载」与「在资源管理器中显示」静默失败。系统工具路径必须
/// 按各自真实位置解析，回归闸 `resolved_system_tools_actually_exist` 在 Windows 上实测存在性。
///
/// SEC3-02：Windows 目录由 `GetSystemWindowsDirectoryW` 给出（终端服务下 `GetWindowsDirectoryW`
/// 可能返回每用户的私有目录），取不到就报错，不回落到裸名。
#[cfg(windows)]
pub(crate) fn explorer_exe() -> Result<PathBuf, String> {
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
    // SAFETY: 同 `system32_tool`
    query_directory(|buffer, len| unsafe { GetSystemWindowsDirectoryW(buffer, len) })
        .map(|dir| dir.join("explorer.exe"))
        .map_err(|err| format!("无法确定 Windows 目录，已放弃调用资源管理器：{err}"))
}

/// 调用「把目录写进调用方缓冲区、返回长度」这一类 Win32 函数：成功时返回值是不含结尾 NUL 的
/// 字符数；缓冲区不够时返回需要的大小（含 NUL）；失败返回 0。
#[cfg(windows)]
fn query_directory(query: impl Fn(*mut u16, u32) -> u32) -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0_u16; 260];
    // 正常一次就够；缓冲区不够时按系统报的大小重试，最多三轮
    for _ in 0..3 {
        let written = query(buffer.as_mut_ptr(), buffer.len() as u32) as usize;
        if written == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if written < buffer.len() {
            let dir = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written]));
            // 结果要拿去拼可执行文件路径：不是「盘符 + 绝对路径」的一律不用
            return if matches!(path_location(&dir), PathLocation::Local) && dir.is_absolute() {
                Ok(dir)
            } else {
                Err(format!("系统返回了意外的目录：{}", dir.display()))
            };
        }
        buffer.resize(written + 1, 0);
    }
    Err("目录长度反复变化".into())
}

/// 一个路径指向哪里（PL-01 / SEC3-01）。只看路径的写法，不碰文件系统——判断它的目的
/// 正是决定「要不要去碰」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathLocation {
    /// 盘符路径与相对路径；`\\?\C:\…` 只是关掉了路径规范化的盘符路径，也算本机。
    /// （映射成盘符的网络驱动器同样落在这里：那是用户自己连上的主机。）
    Local,
    /// UNC 网络共享：`\\server\share\…`，以及它的 `\\?\UNC\…` / `\\.\UNC\…` 写法。
    /// 探测或打开它，Windows 会向 `server` 发起 SMB 连接，域环境下还会带上 NTLM 认证。
    Network { server: String, share: String },
    /// 其余设备命名空间路径（`\\.\PhysicalDrive0`、`\\?\Volume{…}`、`\\?\GLOBALROOT\…` 等）。
    Device,
}

pub(crate) fn path_location(path: &Path) -> PathLocation {
    let text = path.to_string_lossy();
    // NT 对象命名空间写法（`\??\UNC\server\share`、`\??\C:\…`）：部分 Win32 接口会把它当成
    // 已经转换好的 NT 路径原样接受。正常的曲目路径不会长这样，一律按设备路径处理。
    if text.starts_with(r"\??\") {
        return PathLocation::Device;
    }
    // Windows 把正反斜杠都当分隔符：`//server/share` 与 `\\server\share` 是同一个 UNC 路径
    let is_separator = |ch: char| ch == '\\' || ch == '/';
    let mut chars = text.chars();
    if !(chars.next().is_some_and(is_separator) && chars.next().is_some_and(is_separator)) {
        return PathLocation::Local;
    }
    let network = |server: Option<&str>, share: Option<&str>| PathLocation::Network {
        server: server.unwrap_or_default().to_lowercase(),
        share: share.unwrap_or_default().to_lowercase(),
    };
    let mut parts = chars.as_str().split(is_separator);
    let first = parts.next().unwrap_or_default();
    if first != "?" && first != "." {
        // 多写了分隔符（`\\\server\share`）时主机名是空串，照样归为网络路径、不放行
        return network(Some(first), parts.next());
    }
    let second = parts.next().unwrap_or_default();
    if second.eq_ignore_ascii_case("UNC") {
        return network(parts.next(), parts.next());
    }
    let drive = second.as_bytes();
    let is_drive = drive.len() == 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':';
    if first == "?" && is_drive {
        PathLocation::Local
    } else {
        PathLocation::Device
    }
}

/// F-02：**写入**类 IPC 的路径校验。
///
/// 路径正常来自 dialog 选择结果,但命令边界不校验的话,任一渲染进程被攻破即等于
/// 任意文件写入(可覆盖 library-cache.json / cache-settings.json 或用户文档)。
/// 要求:非空、绝对路径、扩展名在白名单内、父目录已存在且确实是目录。
///
/// L-2(2026-08-16 审查,威胁模型明示):此防线**不限制目标目录**——主窗口
/// 渲染进程被攻破时,仍可以任意内容覆盖用户可写的任何白名单扩展名文件
/// (如别的项目的 `.json` 配置)。这是有意的权衡:导出目标本就允许在任意
/// 用户目录(外接盘/文档/桌面),IPC 层无法区分「dialog 确认过的路径」与
/// 「渲染进程直构的路径」,而主窗口在本应用威胁模型中即全信任(见 lib.rs
/// F-03 注释——真正暴露的歌词条窗口摸不到任何文件命令)。若未来引入
/// dialog 结果 token 机制可进一步收窄,当前接受该残余风险。
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

    #[cfg(windows)]
    #[test]
    fn resolved_system_tools_actually_exist() {
        // 2026-09-22：explorer.exe 不在 System32（只有 SysWOW64 与 Windows 目录根下），
        // `system32_tool("explorer.exe")` 让「前往下载」「在资源管理器中显示」从 v0.5.7 起一直失败
        let explorer = explorer_exe().unwrap();
        assert!(explorer.is_file(), "{}", explorer.display());
        assert!(system32_tool("icacls.exe").unwrap().is_file());
    }

    #[cfg(windows)]
    #[test]
    fn system_tool_paths_are_absolute_and_do_not_depend_on_the_environment() {
        // SEC3-02：路径来自系统 API，不读 `%SystemRoot%`。这里不去改进程环境（并行测试里
        // 改环境变量会互相踩），只核对结果是绝对的盘符路径、而且与环境变量给出的位置一致。
        for tool in [
            system32_tool("icacls.exe").unwrap(),
            explorer_exe().unwrap(),
        ] {
            assert!(tool.is_absolute(), "{}", tool.display());
            assert_eq!(path_location(&tool), PathLocation::Local);
            assert!(tool.parent().is_some_and(Path::is_dir));
        }
        if let Some(root) = std::env::var_os("SystemRoot").filter(|root| !root.is_empty()) {
            let lower = |path: PathBuf| path.to_string_lossy().to_lowercase();
            assert_eq!(
                lower(system32_tool("icacls.exe").unwrap()),
                lower(Path::new(&root).join("System32").join("icacls.exe"))
            );
            assert_eq!(
                lower(explorer_exe().unwrap()),
                lower(Path::new(&root).join("explorer.exe"))
            );
        }
    }

    #[test]
    fn path_location_tells_local_network_and_device_paths_apart() {
        let network = |server: &str, share: &str| PathLocation::Network {
            server: server.into(),
            share: share.into(),
        };
        for local in [
            r"C:\Music\a.flac",
            "D:/Music/a.flac",
            r"relative\a.flac",
            r"\rooted\a.flac",
            "a.flac",
            r"\\?\C:\Music\a.flac",
            "",
        ] {
            assert_eq!(
                path_location(Path::new(local)),
                PathLocation::Local,
                "{local}"
            );
        }
        for (path, server, share) in [
            (r"\\nas\music\a.flac", "nas", "music"),
            (r"\\NAS\Music\a.flac", "nas", "music"),
            ("//nas/music/a.flac", "nas", "music"),
            (r"\/nas\music\a.flac", "nas", "music"),
            (r"\\?\UNC\nas\music\a.flac", "nas", "music"),
            (r"\\.\unc\nas\music\a.flac", "nas", "music"),
            (r"\\192.168.1.9\share", "192.168.1.9", "share"),
            (r"\\attacker.example\x", "attacker.example", "x"),
            (r"\\server", "server", ""),
            // 多写一个分隔符：主机名解析为空串，仍然不是本机路径
            (r"\\\nas\music\a.flac", "", "nas"),
        ] {
            assert_eq!(
                path_location(Path::new(path)),
                network(server, share),
                "{path}"
            );
        }
        for device in [
            r"\\.\PhysicalDrive0",
            r"\\.\C:\Music\a.flac",
            r"\\.\pipe\name",
            r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\a.flac",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\a.flac",
            r"\\?\GLOBALROOT\Device\Mup\nas\music\a.flac",
            r"\??\UNC\nas\music\a.flac",
            r"\??\C:\Music\a.flac",
        ] {
            assert_eq!(
                path_location(Path::new(device)),
                PathLocation::Device,
                "{device}"
            );
        }
    }

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
