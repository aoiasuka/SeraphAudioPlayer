//! 歌单 M3U8 导入导出。
//!
//! 导入：解析 .m3u8/.m3u 文本中的本地文件路径（相对路径按清单所在目录解析），
//! 返回给前端走既有 import_tracks 流程入库后建歌单。
//! 导出：#EXTM3U + #EXTINF 标准格式，路径写绝对路径。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::error::{IpcError, IpcErrorCode, IpcResult};

/// 防呆上限：超长清单截断，避免异常文件拖垮导入。
const MAX_M3U8_ENTRIES: usize = 10_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M3u8Import {
    pub name: String,
    pub paths: Vec<String>,
    /// 被跳过的行数（网络流 URL、不存在的文件等）
    pub skipped: usize,
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
pub fn import_playlist_m3u8(path: String) -> IpcResult<M3u8Import> {
    let source = PathBuf::from(path.trim());
    if !source.is_file() {
        return Err(IpcError::not_found("清单文件不存在"));
    }

    let bytes = fs::read(&source)
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("读取清单失败: {err}")))?;
    // M3U8 规范为 UTF-8；容忍 BOM 与个别非法字节
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    let base_dir = source.parent().unwrap_or(Path::new("."));

    let mut paths = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines().take(MAX_M3U8_ENTRIES) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 不支持网络流条目
        if line.starts_with("http://") || line.starts_with("https://") {
            skipped += 1;
            continue;
        }

        let candidate = PathBuf::from(line);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            base_dir.join(candidate)
        };
        if resolved.is_file() {
            paths.push(resolved.to_string_lossy().to_string());
        } else {
            skipped += 1;
        }
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
    })
}

#[tauri::command]
pub fn export_playlist_m3u8(path: String, entries: Vec<M3u8ExportEntry>) -> IpcResult<()> {
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
    for entry in &entries {
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

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("seraph-m3u8-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn imports_absolute_relative_and_skips_urls() {
        let dir = temp_dir();
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

        let imported = import_playlist_m3u8(list_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(imported.name, "test-list");
        assert_eq!(imported.paths.len(), 2);
        assert_eq!(imported.skipped, 2, "URL 与缺失文件都应计入 skipped");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_appends_extension_and_writes_extinf() {
        let dir = temp_dir();
        let target = dir.join("out-list");
        export_playlist_m3u8(
            target.to_string_lossy().to_string(),
            vec![M3u8ExportEntry {
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
        assert!(import_playlist_m3u8("Z:/definitely/missing.m3u8".into()).is_err());
        assert!(export_playlist_m3u8("out.m3u8".into(), Vec::new()).is_err());
    }
}
