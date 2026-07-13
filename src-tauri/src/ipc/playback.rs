//! 播放控制 IPC handlers（骨架）。
//!
//! 当前所有命令都只更新前端可见的状态机并返回 `Ok(())`；
//! 真正接通 `seraph-audio` 后再补实现。

use crate::state::{AppState, PlaybackQueueTrack, TrackAdvance};
use seraph_core::PlayerState;
use std::path::PathBuf;
use tauri::State;
use tracing::debug;

#[tauri::command]
pub fn sync_playback_queue(
    state: State<'_, AppState>,
    tracks: Vec<PlaybackQueueTrack>,
    current_track_index: usize,
    recent_track_ids: Vec<String>,
    shuffle_mode: bool,
    loop_mode: bool,
) -> Result<(), String> {
    debug!(
        "ipc::sync_playback_queue -> {} tracks, index {current_track_index}",
        tracks.len()
    );
    state.sync_playback_queue(
        tracks,
        current_track_index,
        recent_track_ids,
        shuffle_mode,
        loop_mode,
    );
    Ok(())
}

#[tauri::command]
pub fn set_playback_modes(
    state: State<'_, AppState>,
    shuffle_mode: bool,
    loop_mode: bool,
) -> Result<(), String> {
    debug!("ipc::set_playback_modes -> shuffle={shuffle_mode}, loop={loop_mode}");
    state.set_playback_modes(shuffle_mode, loop_mode);
    Ok(())
}

#[tauri::command]
pub fn play(
    state: State<'_, AppState>,
    path: Option<String>,
    track_id: Option<String>,
    start_seconds: Option<f64>,
) -> Result<(), String> {
    debug!("ipc::play");
    if let Some(path) = path {
        state
            .audio
            .play_file(
                PathBuf::from(path),
                track_id.unwrap_or_default(),
                start_seconds.unwrap_or(0.0),
            )
            .map_err(|err| err.to_string())?;
    } else {
        state.audio.resume().map_err(|err| err.to_string())?;
    }
    *state.player_state.write() = PlayerState::Playing;
    Ok(())
}

#[tauri::command]
pub fn pause(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::pause");
    state.audio.pause().map_err(|err| err.to_string())?;
    *state.player_state.write() = PlayerState::Paused;
    Ok(())
}

#[tauri::command]
pub fn stop(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::stop");
    state.audio.stop().map_err(|err| err.to_string())?;
    *state.player_state.write() = PlayerState::Stopped;
    Ok(())
}

#[tauri::command]
pub fn seek(state: State<'_, AppState>, seconds: f64) -> Result<(), String> {
    debug!("ipc::seek -> {seconds}s");
    // P2-7：IPC 层最后防线，拒绝 NaN / Infinity / 负值直达音频引擎。
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(format!("无效的跳转位置: {seconds}"));
    }
    state.audio.seek(seconds).map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn next_track(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::next_track");
    state.advance_track(TrackAdvance::Next)?;
    Ok(())
}

#[tauri::command]
pub fn prev_track(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::prev_track");
    state.advance_track(TrackAdvance::Previous)?;
    Ok(())
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, volume: f32) -> Result<(), String> {
    debug!("ipc::set_volume -> {volume}");
    // P2-7：NaN/Infinity 直接拒绝，范围收敛到 0..=1，防止异常增益爆音。
    if !volume.is_finite() {
        return Err(format!("无效的音量值: {volume}"));
    }
    let volume = volume.clamp(0.0, 1.0);
    state
        .audio
        .set_volume(volume)
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn select_output_device(state: State<'_, AppState>, device_id: String) -> Result<(), String> {
    debug!("ipc::select_output_device -> {device_id}");
    state
        .audio
        .set_output_device(device_id)
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_output_driver(state: State<'_, AppState>, driver: String) -> Result<(), String> {
    debug!("ipc::set_output_driver -> {driver}");
    state
        .audio
        .set_driver(driver)
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// 启用/停用系统媒体控件（SMTC）。设置由前端持久化，启动水合后同步。
#[tauri::command]
pub fn set_smtc_enabled(enabled: bool) -> Result<(), String> {
    debug!("ipc::set_smtc_enabled -> {enabled}");
    #[cfg(windows)]
    crate::smtc::set_enabled(enabled);
    #[cfg(not(windows))]
    let _ = enabled;
    Ok(())
}
