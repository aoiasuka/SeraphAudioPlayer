//! DSP 链 IPC 命令（v0.4.4 EQ 均衡器）。
//!
//! - `set_dsp_settings`：前端下发 EQ + crossfeed 配置，写共享槽后立即对
//!   正在播放的曲目生效（解码线程按版本号热更新）。写锁 + 原子递增，微秒级，同步命令即可。
//! - `import_eq_preset` / `export_eq_preset`：预设文件读写（AutoEq/EqualizerAPO 的
//!   txt 或本应用导出的 JSON），文件 IO 走 spawn_blocking 不占主线程。

use crate::state::AppState;
use seraph_dsp::DspSettings;
use tauri::State;
use tracing::debug;

use super::error::{IpcError, IpcErrorCode, IpcResult};

/// 预设文件大小上限：AutoEq ParametricEQ.txt 通常 <2KB，JSON 预设 <64KB，
/// 512KB 足够宽裕，同时挡住误选巨型文件。
const MAX_PRESET_FILE_BYTES: u64 = 512 * 1024;

#[tauri::command]
pub fn set_dsp_settings(state: State<'_, AppState>, settings: DspSettings) -> IpcResult<()> {
    debug!(
        "ipc::set_dsp_settings -> enabled={}, bands={}, crossfeed={}, applyToDsd={}",
        settings.enabled,
        settings.bands.len(),
        settings.crossfeed.enabled,
        settings.apply_to_dsd
    );
    state.audio.set_dsp_settings(settings);
    Ok(())
}

#[tauri::command]
pub async fn import_eq_preset(path: String) -> IpcResult<String> {
    tauri::async_runtime::spawn_blocking(move || import_eq_preset_inner(&path))
        .await
        .map_err(|err| IpcError::from(format!("import preset task panicked: {err}")))?
}

fn import_eq_preset_inner(path: &str) -> IpcResult<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("预设文件路径为空"));
    }
    let path = std::path::Path::new(trimmed);
    let metadata = std::fs::metadata(path)
        .map_err(|_| IpcError::not_found(format!("预设文件不存在: {trimmed}")))?;
    if !metadata.is_file() {
        return Err(IpcError::invalid_input("目标不是文件"));
    }
    if metadata.len() > MAX_PRESET_FILE_BYTES {
        return Err(IpcError::invalid_input(format!(
            "预设文件过大（{} KB），上限 512 KB",
            metadata.len() / 1024
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("读取预设失败: {err}")))?;
    // AutoEq/APO 文件均为 UTF-8 文本；容忍 BOM 与个别非法字节
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

#[tauri::command]
pub async fn export_eq_preset(path: String, content: String) -> IpcResult<()> {
    tauri::async_runtime::spawn_blocking(move || export_eq_preset_inner(&path, &content))
        .await
        .map_err(|err| IpcError::from(format!("export preset task panicked: {err}")))?
}

fn export_eq_preset_inner(path: &str, content: &str) -> IpcResult<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("导出路径为空"));
    }
    if content.is_empty() {
        return Err(IpcError::invalid_input("没有可导出的预设内容"));
    }
    let mut target = std::path::PathBuf::from(trimmed);
    let has_ext = target
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("txt"));
    if !has_ext {
        target.set_extension("json");
    }
    std::fs::write(&target, content.as_bytes())
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("写入预设失败: {err}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("seraph-eq-preset-{}-{name}", std::process::id()))
    }

    #[test]
    fn import_rejects_missing_and_oversized() {
        assert_eq!(
            import_eq_preset_inner("").unwrap_err().code,
            IpcErrorCode::InvalidInput
        );
        let missing = temp_path("missing.txt");
        assert_eq!(
            import_eq_preset_inner(&missing.to_string_lossy())
                .unwrap_err()
                .code,
            IpcErrorCode::NotFound
        );
    }

    #[test]
    fn import_strips_bom_and_reads_text() {
        let file = temp_path("bom.txt");
        fs::write(
            &file,
            "\u{feff}Preamp: -6.0 dB\nFilter 1: ON PK Fc 100 Hz Gain 2 dB Q 1.0",
        )
        .unwrap();
        let text = import_eq_preset_inner(&file.to_string_lossy()).unwrap();
        let _ = fs::remove_file(&file);
        assert!(text.starts_with("Preamp:"), "BOM 应被剥离");
    }

    #[test]
    fn export_appends_json_extension_and_writes() {
        let base = temp_path("export-out");
        export_eq_preset_inner(&base.to_string_lossy(), "{\"enabled\":true}").unwrap();
        let target = base.with_extension("json");
        let written = fs::read_to_string(&target).unwrap();
        let _ = fs::remove_file(&target);
        assert_eq!(written, "{\"enabled\":true}");
    }

    #[test]
    fn export_rejects_empty_input() {
        assert!(export_eq_preset_inner("", "x").is_err());
        assert!(export_eq_preset_inner("out.json", "").is_err());
    }
}
