//! 歌单 M3U8 导入导出。
//!
//! 导入：解析 .m3u8/.m3u 文本中的本地文件路径（相对路径按清单所在目录解析），
//! 返回给前端走既有 import_tracks 流程入库后建歌单。
//! 导出：#EXTM3U + #EXTINF 标准格式，路径写绝对路径。
//!
//! 两个命令均为 async + spawn_blocking（M-6 / H-1 纪律）：导入最多上万次
//! 文件探测（断连网络盘单次可挂数秒），同步命令会把整个主线程冻住。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::error::{IpcError, IpcErrorCode, IpcResult};
use super::library::decode_lyric_bytes;
use super::path_guard::{path_location, validate_export_path, validate_import_path, PathLocation};

/// 防呆上限：超长清单截断（按有效条目数计），避免异常文件拖垮导入。
const MAX_M3U8_ENTRIES: usize = 10_000;
/// 清单大小上限：正常 .m3u/.m3u8 都在几 MB 内，防误选巨型文件整读进内存。
const MAX_M3U8_BYTES: u64 = 32 * 1024 * 1024;
/// 回传给前端的「被拦下的主机名」个数与单个长度上限（只用于提示文案）。
const MAX_REPORTED_BLOCKED_HOSTS: usize = 5;
const MAX_REPORTED_HOST_CHARS: usize = 64;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M3u8Import {
    pub name: String,
    pub paths: Vec<String>,
    /// 被跳过的条目数（网络流 URL、不存在的文件、重复条目、被拦下的网络 / 设备路径等）
    pub skipped: usize,
    /// 有效条目数超过上限被截断
    pub truncated: bool,
    /// PL-01：`skipped` 里因为指向网络共享或设备路径而**没有去探测**的条目数
    pub blocked: usize,
    /// 被拦下的网络主机名（去重，最多 [`MAX_REPORTED_BLOCKED_HOSTS`] 个），供提示文案使用
    pub blocked_hosts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct M3u8ExportEntry {
    pub title: String,
    pub artist: String,
    pub duration: u64,
    pub path: String,
}

#[tauri::command]
pub async fn import_playlist_m3u8(app: tauri::AppHandle, path: String) -> IpcResult<M3u8Import> {
    tauri::async_runtime::spawn_blocking(move || {
        // 曲库里已有曲目所在的网络主机：用户此前自己连过、应用平时也会访问
        let library_servers = super::library::library_network_servers(&app);
        import_playlist_m3u8_inner(&path, &library_servers)
    })
    .await
    .map_err(|err| IpcError::new(IpcErrorCode::Internal, format!("导入任务异常: {err}")))?
}

/// PL-01：清单条目能不能去探测，由路径的写法决定，**在碰文件系统之前**判断。
///
/// 清单是外来文件。此前每一行都直接拿去 `is_file()`，而探测一个 `\\主机\共享\文件` 路径，
/// Windows 就会向那台主机发起 SMB 连接，域环境下还附带 NTLM 认证质询——别人给的 `.m3u8`
/// 里写上自己的主机名，导入时就能收到这台机器的连接（与 L-1 的 SMTC 封面同一类出站）。
///
/// - 本机路径：照旧。
/// - 设备路径（`\\.\…`、卷 GUID、`\??\…`）：一律跳过。
/// - 网络共享路径：只有主机是「清单文件自己所在的主机」或「曲库里已有曲目所在的主机」才探测。
///   前者是用户刚刚亲手打开的位置（放在 NAS 上、条目写相对路径的清单照常可用），后者是
///   用户此前自己从文件对话框导入过的共享（从本应用导出的清单可以原样导回）。
///   两种情况都没有产生「清单让应用去连一台新主机」的效果。其余主机一律跳过，
///   计入 `skipped` 与 `blocked`，主机名回传给前端用于说明。
///
/// 主机名按字符串比较（不区分大小写），`\\nas` 与 `\\nas.lan`、IP 写法算不同主机——
/// 宁可多跳过。映射成盘符的网络驱动器（`Z:\…`）是用户自己连上的，按本机路径处理。
fn may_probe(location: &PathLocation, own_server: Option<&str>, library: &HashSet<String>) -> bool {
    match location {
        PathLocation::Local => true,
        PathLocation::Device => false,
        // 空主机名（`\\\server\…` 这类写法解析出来的）不与任何主机相等
        PathLocation::Network { server, .. } => {
            !server.is_empty()
                && (own_server == Some(server.as_str()) || library.contains(server.as_str()))
        }
    }
}

fn import_playlist_m3u8_inner(
    path: &str,
    library_servers: &HashSet<String>,
) -> IpcResult<M3u8Import> {
    // F-02：只接受 .m3u8/.m3u 且必须是已存在的普通文件
    let source = validate_import_path(path, &["m3u8", "m3u"])?;
    if let Ok(meta) = fs::metadata(&source) {
        if meta.len() > MAX_M3U8_BYTES {
            return Err(IpcError::invalid_input(
                "清单文件过大（超过 32MB），疑似不是有效的 M3U/M3U8",
            ));
        }
    }

    let bytes = fs::read(&source)
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("读取清单失败: {err}")))?;
    // M3U8 规范为 UTF-8，但老播放器（千千静听、中文 foobar2000）导出的 .m3u
    // 常为 GBK——复用歌词模块的探测链（UTF-16 → 严格 UTF-8 → GBK 兜底），
    // 中文路径不再变 U+FFFD 后整单探测失败（M-5）。
    let text = decode_lyric_bytes(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    let base_dir = source.parent().unwrap_or(Path::new("."));
    // 清单自己所在的主机（清单在本机时没有）
    let own_server = match path_location(&source) {
        PathLocation::Network { server, .. } if !server.is_empty() => Some(server),
        _ => None,
    };

    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut skipped = 0usize;
    let mut truncated = false;
    let mut blocked = 0usize;
    let mut blocked_hosts: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if paths.len() >= MAX_M3U8_ENTRIES {
            truncated = true;
            break;
        }
        // 不支持网络流条目
        if line.starts_with("http://") || line.starts_with("https://") {
            skipped += 1;
            continue;
        }
        // VLC 等工具导出的 file:// URI 转本地路径
        let candidate = match file_uri_to_path(line) {
            FileUriOutcome::NotUri => PathBuf::from(line),
            FileUriOutcome::Local(path) => path,
            FileUriOutcome::Unsupported => {
                skipped += 1;
                continue;
            }
        };
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            base_dir.join(candidate)
        };
        // PL-01：判断的是**最终要探测的路径**（相对条目拼上清单目录之后），而且在探测之前
        let location = path_location(&resolved);
        if !may_probe(&location, own_server.as_deref(), library_servers) {
            skipped += 1;
            blocked += 1;
            if let PathLocation::Network { server, .. } = location {
                let shown: String = server
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(MAX_REPORTED_HOST_CHARS)
                    .collect();
                if !shown.is_empty()
                    && blocked_hosts.len() < MAX_REPORTED_BLOCKED_HOSTS
                    && !blocked_hosts.contains(&shown)
                {
                    blocked_hosts.push(shown);
                }
            }
            continue;
        }
        if !resolved.is_file() {
            skipped += 1;
            continue;
        }
        let resolved = resolved.to_string_lossy().to_string();
        // M-9：同一文件重复出现只保留首个。重复 trackId 会破坏歌单“无重复”
        // 不变量（重复 React key、move 错位、remove 一次删光）。Windows 路径
        // 大小写不敏感，去重键用小写 + 分隔符归一（file URI 解码出正斜杠，
        // 与清单里的反斜杠写法指向同一文件）。
        if !seen.insert(resolved.to_lowercase().replace('\\', "/")) {
            skipped += 1;
            continue;
        }
        paths.push(resolved);
    }

    let name = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("导入歌单")
        .to_string();

    Ok(M3u8Import {
        name,
        paths,
        skipped,
        truncated,
        blocked,
        blocked_hosts,
    })
}

enum FileUriOutcome {
    /// 普通路径行，不是 file URI
    NotUri,
    Local(PathBuf),
    /// file:// 形态但无法映射到本地路径（远端主机等）
    Unsupported,
}

/// `file:///C:/Music/a.flac`、`file://localhost/C:/a.flac` → 本地路径；
/// 指向其它主机的 file URI 视为不支持（计入 skipped）。
fn file_uri_to_path(line: &str) -> FileUriOutcome {
    let lower = line.to_ascii_lowercase();
    if !lower.starts_with("file://") {
        return FileUriOutcome::NotUri;
    }
    let rest = &line["file://".len()..];
    let path_part = if let Some(part) = rest.strip_prefix('/') {
        part
    } else if let Some(part) = rest
        .strip_prefix("localhost/")
        .or_else(|| rest.strip_prefix("LOCALHOST/"))
    {
        part
    } else {
        return FileUriOutcome::Unsupported;
    };
    let decoded = percent_decode(path_part);
    if decoded.is_empty() {
        return FileUriOutcome::Unsupported;
    }
    // `file:///C:/x` 剥前缀后是 "C:/x"（Windows 盘符路径原样用）；
    // 非盘符开头（POSIX 绝对路径）补回前导斜杠。
    let bytes = decoded.as_bytes();
    let is_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let path = if is_drive {
        decoded
    } else {
        format!("/{decoded}")
    };
    FileUriOutcome::Local(PathBuf::from(path))
}

/// 最小 percent 解码（UTF-8 字节级），无效转义序列原样保留。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(value) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
            {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tauri::command]
pub async fn export_playlist_m3u8(path: String, entries: Vec<M3u8ExportEntry>) -> IpcResult<()> {
    tauri::async_runtime::spawn_blocking(move || export_playlist_m3u8_inner(&path, &entries))
        .await
        .map_err(|err| IpcError::new(IpcErrorCode::Internal, format!("导出任务异常: {err}")))?
}

/// N-04：M3U8 是**行敏感**格式——一行一条记录、`#` 开头为指令行。曲目元数据
/// （来自文件标签，属外部输入）里的 CR/LF 会把一条记录劈成多条，攻击者可借此
/// 往导出的清单里注入伪造条目或 `#EXT` 指令；导出的清单常被分享给别的播放器读。
/// 这里把所有换行类字符归一为空格。
fn sanitize_m3u8_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\r' || ch == '\n' || ch == '\u{2028}' || ch == '\u{2029}' {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn export_playlist_m3u8_inner(path: &str, entries: &[M3u8ExportEntry]) -> IpcResult<()> {
    if entries.is_empty() {
        return Err(IpcError::invalid_input("歌单没有可导出的曲目"));
    }
    // F-02：命令边界校验路径（绝对路径 + 扩展名白名单 + 父目录必须存在）
    let target = validate_export_path(path, &["m3u8", "m3u"])?;

    let mut content = String::from("#EXTM3U\n");
    for entry in entries {
        // N-04：路径含换行的条目直接跳过——路径本身不可能合法含换行，
        // 归一成空格反而会写出一条指向错误文件的记录。
        let entry_path = sanitize_m3u8_field(&entry.path);
        if entry_path.is_empty() || entry_path != entry.path.trim() {
            continue;
        }
        // N-04：`#` 开头的路径会被解析成指令行，加 `./` 前缀消歧
        let entry_path = if entry_path.starts_with('#') {
            format!("./{entry_path}")
        } else {
            entry_path
        };
        content.push_str(&format!(
            "#EXTINF:{},{} - {}\n{}\n",
            entry.duration,
            sanitize_m3u8_field(&entry.artist),
            sanitize_m3u8_field(&entry.title),
            entry_path
        ));
    }

    // PL-02：临时文件 + 改名。此前直接覆盖写，导出到已有清单时中途失败会把原文件截断
    super::fs_util::write_atomic(&target, content.as_bytes())
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("写入清单失败: {err}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// 每个测试独立子目录：cargo test 并行执行，共用目录会在
    /// remove_dir_all 清理时互删对方文件（CI 上稳定复现的竞态）。
    fn temp_dir(case: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("seraph-m3u8-test-{}-{case}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// 曲库里没有任何网络主机时的导入。
    fn import(path: &str) -> IpcResult<M3u8Import> {
        import_playlist_m3u8_inner(path, &HashSet::new())
    }

    #[cfg(windows)]
    #[test]
    fn network_and_device_entries_are_skipped_without_being_probed() {
        // PL-01：别人给的清单里写着网络共享路径。`blocked` 只在「探测之前就跳过」的分支上
        // 累加，所以它等于条目数就说明这些路径一条都没有拿去碰文件系统。
        let dir = temp_dir("pl01-block");
        let local = dir.join("local.flac");
        fs::write(&local, b"x").unwrap();
        let list_path = dir.join("untrusted.m3u8");
        let entries = [
            r"\\attacker.invalid\share\a.flac",
            "//attacker.invalid/share/b.flac",
            r"\\?\UNC\Attacker.invalid\share\c.flac",
            "file:////attacker.invalid/share/d.flac",
            r"\\other.invalid\x\e.flac",
            r"\\.\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device\Mup\third.invalid\share\f.flac",
        ];
        // 多写一个分隔符的不是合法 UNC 前缀：它不算绝对路径，会被拼到清单所在的盘上，
        // 探测的是本机的 `<盘符>:\empty-host.invalid\share\g.flac`——不出站，按「文件不存在」跳过
        let malformed = r"\\\empty-host.invalid\share\g.flac";
        let candidate = PathBuf::from(malformed);
        assert!(!candidate.is_absolute());
        let resolved = dir.join(&candidate);
        assert_eq!(
            path_location(&resolved),
            PathLocation::Local,
            "{}",
            resolved.display()
        );
        let mut content = String::from("#EXTM3U\n");
        for entry in entries {
            content.push_str(entry);
            content.push('\n');
        }
        content.push_str(malformed);
        content.push_str("\nlocal.flac\n");
        fs::write(&list_path, content).unwrap();

        let started = std::time::Instant::now();
        let imported = import(&list_path.to_string_lossy()).unwrap();
        assert_eq!(imported.paths.len(), 1, "{:?}", imported.paths);
        assert!(imported.paths[0].ends_with("local.flac"));
        assert_eq!(imported.blocked, entries.len());
        assert_eq!(imported.skipped, entries.len() + 1);
        assert_eq!(
            imported.blocked_hosts,
            ["attacker.invalid", "other.invalid"],
            "主机名去重；设备路径不报主机"
        );
        // 真去解析这几个主机名，单是名字解析超时就远不止这点时间
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );

        // 曲库里有别的主机也不放行这些
        let library = HashSet::from(["media-nas.invalid".to_string()]);
        let imported = import_playlist_m3u8_inner(&list_path.to_string_lossy(), &library).unwrap();
        assert_eq!(imported.blocked, entries.len());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn network_entries_are_probed_only_for_already_trusted_hosts() {
        // PL-01 的放行规则（纯判断，不碰网络）
        let network = |server: &str| PathLocation::Network {
            server: server.into(),
            share: "music".into(),
        };
        let none = HashSet::new();
        let library = HashSet::from(["nas".to_string()]);

        assert!(may_probe(&PathLocation::Local, None, &none));
        assert!(!may_probe(&PathLocation::Device, Some("nas"), &library));

        // 清单在本机、曲库里没有这台主机 → 不探测
        assert!(!may_probe(&network("nas"), None, &none));
        // 清单自己就在这台主机上（用户刚亲手打开的位置）
        assert!(may_probe(&network("nas"), Some("nas"), &none));
        assert!(!may_probe(&network("evil"), Some("nas"), &none));
        // 曲库里已有这台主机上的曲目（用户此前自己导入过）
        assert!(may_probe(&network("nas"), None, &library));
        assert!(!may_probe(&network("evil"), None, &library));
        // 空主机名永远不算已信任，哪怕两边都是空的
        assert!(!may_probe(
            &network(""),
            Some(""),
            &HashSet::from([String::new()])
        ));

        // 放在共享上的清单里写相对路径：拼出来的仍是同一台主机，所以照常可用
        #[cfg(windows)]
        {
            let playlist = Path::new(r"\\NAS\Music\lists\fav.m3u8");
            let resolved = playlist.parent().unwrap().join(r"..\album\01.flac");
            assert_eq!(path_location(playlist), network("nas"));
            assert_eq!(path_location(&resolved), network("nas"));
        }
    }

    #[test]
    fn blocked_host_names_are_capped_for_the_notification() {
        let dir = temp_dir("pl01-cap");
        let list_path = dir.join("many-hosts.m3u8");
        let mut content = String::from("#EXTM3U\n");
        // 超长主机名排在最前，才能进到回传的那几个里、考到长度上限
        content.push_str(&format!("\\\\{}\\share\\a.flac\n", "h".repeat(300)));
        for index in 0..20 {
            content.push_str(&format!("\\\\host{index}.invalid\\share\\a.flac\n"));
        }
        fs::write(&list_path, content).unwrap();
        let imported = import(&list_path.to_string_lossy()).unwrap();
        // 非 Windows 上这些不是绝对路径，会被拼到清单目录下当成普通的不存在文件
        if cfg!(windows) {
            assert_eq!(imported.blocked, 21);
            assert_eq!(imported.blocked_hosts.len(), MAX_REPORTED_BLOCKED_HOSTS);
            assert_eq!(
                imported.blocked_hosts[0],
                "h".repeat(MAX_REPORTED_HOST_CHARS)
            );
            assert_eq!(imported.blocked_hosts[1], "host0.invalid");
        }
        assert!(imported.paths.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_absolute_relative_and_skips_urls() {
        let dir = temp_dir("import");
        let audio_abs = dir.join("abs.flac");
        let audio_rel = dir.join("rel.mp3");
        fs::write(&audio_abs, b"x").unwrap();
        fs::write(&audio_rel, b"x").unwrap();

        let list_path = dir.join("test-list.m3u8");
        let mut file = fs::File::create(&list_path).unwrap();
        writeln!(file, "\u{feff}#EXTM3U").unwrap();
        writeln!(file, "#EXTINF:120,Artist - Song").unwrap();
        writeln!(file, "{}", audio_abs.display()).unwrap();
        writeln!(file, "rel.mp3").unwrap();
        writeln!(file, "https://example.com/stream.m3u8").unwrap();
        writeln!(file, "missing-file.flac").unwrap();
        drop(file);

        let imported = import(&list_path.to_string_lossy()).unwrap();
        assert_eq!(imported.name, "test-list");
        assert_eq!(imported.paths.len(), 2);
        assert_eq!(imported.skipped, 2, "URL 与缺失文件都应计入 skipped");
        assert!(!imported.truncated);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decodes_gbk_playlist_paths() {
        let dir = temp_dir("gbk");
        let audio = dir.join("思念.mp3");
        fs::write(&audio, b"x").unwrap();

        // 老播放器导出的 GBK .m3u：中文相对路径
        let (encoded, _, _) = encoding_rs::GBK.encode("#EXTM3U\r\n思念.mp3\r\n");
        let list_path = dir.join("gbk-list.m3u");
        fs::write(&list_path, &encoded).unwrap();

        let imported = import(&list_path.to_string_lossy()).unwrap();
        assert_eq!(imported.paths.len(), 1, "GBK 中文路径应能解码命中文件");
        assert!(imported.paths[0].ends_with("思念.mp3"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deduplicates_entries_and_accepts_file_uri() {
        let dir = temp_dir("dedup-uri");
        let audio = dir.join("song a.flac");
        fs::write(&audio, b"x").unwrap();

        let abs = audio.to_string_lossy().to_string();
        // file:/// URI：反斜杠转正斜杠 + 空格转 %20（VLC 导出风格）
        let uri = format!("file:///{}", abs.replace('\\', "/").replace(' ', "%20"));

        let list_path = dir.join("dup-list.m3u8");
        let content = format!("#EXTM3U\n{abs}\n{abs}\n{uri}\nfile://otherhost/share/x.flac\n");
        fs::write(&list_path, content).unwrap();

        let imported = import(&list_path.to_string_lossy()).unwrap();
        assert_eq!(
            imported.paths.len(),
            1,
            "重复路径与等价 file URI 只保留首个"
        );
        assert_eq!(imported.skipped, 3, "两条重复 + 一条远端 URI 计入 skipped");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_appends_extension_and_writes_extinf() {
        let dir = temp_dir("export");
        let target = dir.join("out-list");
        export_playlist_m3u8_inner(
            &target.to_string_lossy(),
            &[M3u8ExportEntry {
                title: "Song".into(),
                artist: "Artist".into(),
                duration: 95,
                path: r"C:\Music\song.flac".into(),
            }],
        )
        .unwrap();

        let written = fs::read_to_string(dir.join("out-list.m3u8")).unwrap();
        assert!(written.starts_with("#EXTM3U\n"));
        assert!(written.contains("#EXTINF:95,Artist - Song"));
        assert!(written.contains(r"C:\Music\song.flac"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_list_and_empty_export() {
        assert!(import("Z:/definitely/missing.m3u8").is_err());
        assert!(export_playlist_m3u8_inner("out.m3u8", &[]).is_err());
    }
}

#[cfg(test)]
mod m3u8_injection_tests {
    use super::*;

    fn temp_target(case: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("seraph-m3u8-inj-{case}-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir.join("out.m3u8")
    }

    #[test]
    fn export_neutralizes_crlf_in_metadata() {
        // N-04：标签里的 CR/LF 会把一条记录劈成多条，可伪造条目或注入 #EXT 指令
        let target = temp_target("crlf");
        let entries = vec![M3u8ExportEntry {
            duration: 100,
            artist: "恶意\r\n#EXTINF:1,注入 - 伪造".into(),
            title: "标题\n../../evil.mp3".into(),
            path: r"C:\music\real.flac".into(),
        }];
        export_playlist_m3u8_inner(target.to_str().unwrap(), &entries).expect("export");

        let text = fs::read_to_string(&target).expect("read back");
        // 头部 1 行 + 该条目的 EXTINF 1 行 + 路径 1 行 = 3 行，多一行就是被注入了
        let lines: Vec<_> = text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 3, "注入产生了额外行: {lines:?}");
        assert_eq!(lines[0], "#EXTM3U");
        assert!(lines[1].starts_with("#EXTINF:100,"));
        // M3U8 按行解析，只有**行首**的 # 才是指令。注入文本被压进同一行后无害，
        // 但绝不能出现第二条行首指令，也不能多出可被当作条目的裸行。
        assert_eq!(
            lines.iter().filter(|l| l.starts_with('#')).count(),
            2,
            "出现了注入的指令行: {lines:?}"
        );
        assert_eq!(lines[2], r"C:\music\real.flac");

        let _ = fs::remove_dir_all(target.parent().unwrap());
    }

    #[test]
    fn export_skips_entries_whose_path_contains_newlines() {
        let target = temp_target("path");
        let entries = vec![
            M3u8ExportEntry {
                duration: 1,
                artist: "a".into(),
                title: "t".into(),
                path: "C:\\music\\a.flac\nC:\\evil.exe".into(),
            },
            M3u8ExportEntry {
                duration: 2,
                artist: "b".into(),
                title: "u".into(),
                path: r"C:\music\ok.flac".into(),
            },
        ];
        export_playlist_m3u8_inner(target.to_str().unwrap(), &entries).expect("export");
        let text = fs::read_to_string(&target).expect("read back");
        assert!(!text.contains("evil.exe"), "含换行的路径条目必须被跳过");
        assert!(text.contains(r"C:\music\ok.flac"));
        assert_eq!(text.matches("#EXTINF").count(), 1);

        let _ = fs::remove_dir_all(target.parent().unwrap());
    }

    #[test]
    fn export_disambiguates_hash_prefixed_paths() {
        // `#` 开头的路径会被解析器当指令行吞掉
        let target = temp_target("hash");
        let entries = vec![M3u8ExportEntry {
            duration: 3,
            artist: "a".into(),
            title: "t".into(),
            path: "#weird.flac".into(),
        }];
        export_playlist_m3u8_inner(target.to_str().unwrap(), &entries).expect("export");
        let text = fs::read_to_string(&target).expect("read back");
        assert!(text.contains("./#weird.flac"), "实际内容: {text}");

        let _ = fs::remove_dir_all(target.parent().unwrap());
    }
}
