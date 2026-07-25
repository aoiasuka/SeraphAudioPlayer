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

/// 防呆上限：超长清单截断（按有效条目数计），避免异常文件拖垮导入。
const MAX_M3U8_ENTRIES: usize = 10_000;
/// 清单大小上限：正常 .m3u/.m3u8 都在几 MB 内，防误选巨型文件整读进内存。
const MAX_M3U8_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M3u8Import {
    pub name: String,
    pub paths: Vec<String>,
    /// 被跳过的条目数（网络流 URL、不存在的文件、重复条目等）
    pub skipped: usize,
    /// 有效条目数超过上限被截断
    pub truncated: bool,
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
pub async fn import_playlist_m3u8(path: String) -> IpcResult<M3u8Import> {
    tauri::async_runtime::spawn_blocking(move || import_playlist_m3u8_inner(&path))
        .await
        .map_err(|err| IpcError::new(IpcErrorCode::Internal, format!("导入任务异常: {err}")))?
}

fn import_playlist_m3u8_inner(path: &str) -> IpcResult<M3u8Import> {
    let source = PathBuf::from(path.trim());
    if !source.is_file() {
        return Err(IpcError::not_found("清单文件不存在"));
    }
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

    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut skipped = 0usize;
    let mut truncated = false;
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

fn export_playlist_m3u8_inner(path: &str, entries: &[M3u8ExportEntry]) -> IpcResult<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("missing export path"));
    }
    if entries.is_empty() {
        return Err(IpcError::invalid_input("歌单没有可导出的曲目"));
    }

    let mut target = PathBuf::from(trimmed);
    let has_m3u_ext = target
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u8") || ext.eq_ignore_ascii_case("m3u"));
    if !has_m3u_ext {
        target.set_extension("m3u8");
    }

    let mut content = String::from("#EXTM3U\n");
    for entry in entries {
        content.push_str(&format!(
            "#EXTINF:{},{} - {}\n{}\n",
            entry.duration, entry.artist, entry.title, entry.path
        ));
    }

    fs::write(&target, content.as_bytes())
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

        let imported = import_playlist_m3u8_inner(&list_path.to_string_lossy()).unwrap();
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

        let imported = import_playlist_m3u8_inner(&list_path.to_string_lossy()).unwrap();
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

        let imported = import_playlist_m3u8_inner(&list_path.to_string_lossy()).unwrap();
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
        assert!(import_playlist_m3u8_inner("Z:/definitely/missing.m3u8").is_err());
        assert!(export_playlist_m3u8_inner("out.m3u8", &[]).is_err());
    }
}
