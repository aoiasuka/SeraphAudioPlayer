//! 应用配置导出 / 导入 IPC（v0.4.8 设置备份）。
//!
//! 前端把全部持久化设置（播放偏好 / EQ 与 DSP / 声学分析设置）打包为一个
//! JSON 文本；这里只负责文件读写与基本防护，结构校验与应用由前端完成
//! （写回 localStorage 后重载水合）。文件 IO 走 spawn_blocking 不占主线程。

use super::error::{IpcError, IpcErrorCode, IpcResult};

/// 配置文件大小上限：全部 store 序列化通常 <100KB，2MB 足够宽裕，
/// 同时挡住误选巨型文件。
const MAX_CONFIG_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[tauri::command]
pub async fn export_app_config(path: String, content: String) -> IpcResult<()> {
    tauri::async_runtime::spawn_blocking(move || export_app_config_inner(&path, &content))
        .await
        .map_err(|err| IpcError::from(format!("export config task panicked: {err}")))?
}

fn export_app_config_inner(path: &str, content: &str) -> IpcResult<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("导出路径为空"));
    }
    if content.is_empty() {
        return Err(IpcError::invalid_input("没有可导出的配置内容"));
    }
    let mut target = std::path::PathBuf::from(trimmed);
    let has_ext = target
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    if !has_ext {
        target.set_extension("json");
    }
    std::fs::write(&target, content.as_bytes())
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("写入配置失败: {err}")))?;
    Ok(())
}

#[tauri::command]
pub async fn import_app_config(path: String) -> IpcResult<String> {
    tauri::async_runtime::spawn_blocking(move || import_app_config_inner(&path))
        .await
        .map_err(|err| IpcError::from(format!("import config task panicked: {err}")))?
}

fn import_app_config_inner(path: &str) -> IpcResult<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IpcError::invalid_input("配置文件路径为空"));
    }
    let path = std::path::Path::new(trimmed);
    let metadata = std::fs::metadata(path)
        .map_err(|_| IpcError::not_found(format!("配置文件不存在: {trimmed}")))?;
    if !metadata.is_file() {
        return Err(IpcError::invalid_input("目标不是文件"));
    }
    if metadata.len() > MAX_CONFIG_FILE_BYTES {
        return Err(IpcError::invalid_input(format!(
            "配置文件过大（{} KB），上限 2 MB",
            metadata.len() / 1024
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|err| IpcError::new(IpcErrorCode::Io, format!("读取配置失败: {err}")))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("seraph-app-config-{}-{name}", std::process::id()))
    }

    #[test]
    fn export_appends_json_extension_and_writes_content() {
        let base = temp_path("export-base");
        export_app_config_inner(&base.to_string_lossy(), "{\"version\":1}").unwrap();
        let target = base.with_extension("json");
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"version\":1}");
        let _ = fs::remove_file(target);
    }

    #[test]
    fn export_rejects_empty_path_or_content() {
        assert!(export_app_config_inner("", "x").is_err());
        assert!(export_app_config_inner("out.json", "").is_err());
    }

    #[test]
    fn import_round_trips_utf8_and_strips_bom() {
        let file = temp_path("import.json");
        fs::write(&file, "\u{feff}{\"stores\":{}}").unwrap();
        let text = import_app_config_inner(&file.to_string_lossy()).unwrap();
        assert_eq!(text, "{\"stores\":{}}");
        let _ = fs::remove_file(file);
    }

    #[test]
    fn import_rejects_missing_or_empty_path() {
        assert_eq!(
            import_app_config_inner("").unwrap_err().code,
            IpcErrorCode::InvalidInput
        );
        let missing = temp_path("missing.json");
        assert_eq!(
            import_app_config_inner(&missing.to_string_lossy())
                .unwrap_err()
                .code,
            IpcErrorCode::NotFound
        );
    }
}
