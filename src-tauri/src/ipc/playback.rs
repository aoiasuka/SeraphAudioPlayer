//! 播放控制 IPC handlers。
//!
//! H-1/M-1：播放命令内部会同步等待音频引擎线程返回真实结果（可达数百毫秒，
//! 引擎挂死时更久）。这些命令改为 async + spawn_blocking，把阻塞等待移出主线程，
//! 避免独占初始化 / 慢速磁盘 / 引擎 hang 冻结整个窗口。AppState 全字段均为
//! Arc/句柄，Clone 只复制句柄、共享同一底层状态，可安全移入 spawn_blocking。

use crate::state::{AppState, PlaybackQueueTrack, TrackAdvance};
use seraph_core::PlayerState;
use std::path::PathBuf;
use tauri::State;
use tracing::debug;

/// 把闭包丢到阻塞线程池执行并等待结果，join 失败归一化为错误字符串。
async fn run_blocking<T, F>(job: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|err| format!("playback task panicked: {err}"))?
}

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
pub async fn play(
    state: State<'_, AppState>,
    path: Option<String>,
    track_id: Option<String>,
    start_seconds: Option<f64>,
) -> Result<(), String> {
    debug!("ipc::play");
    let state = (*state).clone();
    run_blocking(move || {
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
    })
    .await
}

#[tauri::command]
pub async fn pause(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::pause");
    let state = (*state).clone();
    run_blocking(move || {
        state.audio.pause().map_err(|err| err.to_string())?;
        *state.player_state.write() = PlayerState::Paused;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn stop(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::stop");
    let state = (*state).clone();
    run_blocking(move || {
        state.audio.stop().map_err(|err| err.to_string())?;
        *state.player_state.write() = PlayerState::Stopped;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn seek(state: State<'_, AppState>, seconds: f64) -> Result<(), String> {
    debug!("ipc::seek -> {seconds}s");
    // P2-7：IPC 层最后防线，拒绝 NaN / Infinity / 负值直达音频引擎。
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(format!("无效的跳转位置: {seconds}"));
    }
    let state = (*state).clone();
    run_blocking(move || state.audio.seek(seconds).map_err(|err| err.to_string())).await
}

#[tauri::command]
pub async fn next_track(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::next_track");
    let state = (*state).clone();
    run_blocking(move || state.advance_track(TrackAdvance::Next)).await
}

#[tauri::command]
pub async fn prev_track(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::prev_track");
    let state = (*state).clone();
    run_blocking(move || state.advance_track(TrackAdvance::Previous)).await
}

#[tauri::command]
pub async fn set_volume(state: State<'_, AppState>, volume: f32) -> Result<(), String> {
    debug!("ipc::set_volume -> {volume}");
    // P2-7：NaN/Infinity 直接拒绝，范围收敛到 0..=1，防止异常增益爆音。
    if !volume.is_finite() {
        return Err(format!("无效的音量值: {volume}"));
    }
    let volume = volume.clamp(0.0, 1.0);
    let state = (*state).clone();
    run_blocking(move || {
        state
            .audio
            .set_volume(volume)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn select_output_device(
    state: State<'_, AppState>,
    device_id: String,
) -> Result<(), String> {
    debug!("ipc::select_output_device -> {device_id}");
    let state = (*state).clone();
    run_blocking(move || {
        state
            .audio
            .set_output_device(device_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn set_output_driver(state: State<'_, AppState>, driver: String) -> Result<(), String> {
    debug!("ipc::set_output_driver -> {driver}");
    let state = (*state).clone();
    run_blocking(move || {
        state
            .audio
            .set_driver(driver)
            .map_err(|err| err.to_string())
    })
    .await
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

/// 任务栏集成开关（缩略图播控按钮 / 图标进度条）。设置由前端持久化，
/// 启动水合后同步；后端默认两者均启用。
#[tauri::command]
pub fn set_taskbar_features(buttons: bool, progress: bool) -> Result<(), String> {
    debug!("ipc::set_taskbar_features -> buttons={buttons}, progress={progress}");
    #[cfg(windows)]
    crate::taskbar::set_features(buttons, progress);
    #[cfg(not(windows))]
    let _ = (buttons, progress);
    Ok(())
}

/// 启用/停用任务栏歌词条窗口。默认关闭；启用即创建、停用即销毁
/// （不保留隐藏窗口的 WebView2 内存开销）。
///
/// 必须是 async 命令：同步命令在主线程执行，而 WebviewWindowBuilder::build
/// 需要主线程事件循环配合派发，同步调用会互相等待死锁冻结整个应用
/// （Tauri 文档明确禁止在主线程同步命令中创建窗口）。set_lyrics_enabled
/// 内部有等待旧窗口销毁的短阻塞循环，移到阻塞线程池执行。
#[tauri::command]
pub async fn set_taskbar_lyrics_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    debug!("ipc::set_taskbar_lyrics_enabled -> {enabled}");
    #[cfg(windows)]
    {
        let result = run_blocking(move || crate::taskbar::set_lyrics_enabled(&app, enabled)).await;
        if let Err(err) = &result {
            tracing::warn!("set_taskbar_lyrics_enabled failed: {err}");
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (app, enabled);
        Ok(())
    }
}

/// 歌词条仅歌词模式(鼠标穿透):开启后条不响应鼠标,点击落到任务栏,
/// 恢复交互只能走设置页(条本身收不到鼠标)。async 同上:窗口操作不在
/// 主线程同步执行。
#[tauri::command]
pub async fn set_taskbar_lyrics_click_through(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    debug!("ipc::set_taskbar_lyrics_click_through -> {enabled}");
    #[cfg(windows)]
    {
        let result =
            run_blocking(move || crate::taskbar::set_lyrics_click_through(&app, enabled)).await;
        if let Err(err) = &result {
            tracing::warn!("set_taskbar_lyrics_click_through failed: {err}");
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (app, enabled);
        Ok(())
    }
}

/// 歌词条定位：x=None 用默认位置，Some 为拖拽记忆的横向物理坐标。
/// 返回钳制回任务栏范围后的实际横向位置，供前端持久化。
/// async 同 set_taskbar_lyrics_enabled：避免占用主线程等待窗口操作。
#[tauri::command]
pub async fn position_taskbar_bar(app: tauri::AppHandle, x: Option<f64>) -> Result<f64, String> {
    let x = x.filter(|value| value.is_finite());
    #[cfg(windows)]
    return crate::taskbar::position_lyric_bar(&app, x);
    #[cfg(not(windows))]
    {
        let _ = (app, x);
        Ok(0.0)
    }
}

/// 播放快照（任务栏歌词条启动初始化用）。进度取自任务栏事件折叠状态——
/// 引擎无位置查询接口，暂停期间该状态仍保留最后一次 Progress 的位置。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub track_id: Option<String>,
    pub playing: bool,
    pub seconds: f64,
    pub total: f64,
    /// 任务栏是否深色主题(歌词条据此选墨签/纸签配色;非 Windows 恒 false)
    pub dark_taskbar: bool,
}

#[tauri::command]
pub fn get_playback_snapshot(state: State<'_, AppState>) -> Result<PlaybackSnapshot, String> {
    let track_id = state.current_queue_track().map(|track| track.id);
    let fallback_playing = *state.player_state.read() == PlayerState::Playing;
    #[cfg(windows)]
    let (playing, seconds, total) = crate::taskbar::playback_snapshot()
        .map(|(playing, seconds, total)| (playing, seconds as f64, total as f64))
        .unwrap_or((fallback_playing, 0.0, 0.0));
    #[cfg(not(windows))]
    let (playing, seconds, total) = (fallback_playing, 0.0, 0.0);
    #[cfg(windows)]
    let dark_taskbar = !crate::taskbar::taskbar_uses_light_theme();
    #[cfg(not(windows))]
    let dark_taskbar = false;
    Ok(PlaybackSnapshot {
        track_id,
        playing,
        seconds,
        total,
        dark_taskbar,
    })
}

/// 播放/暂停切换（任务栏歌词条按钮）。与 SMTC Toggle 同一语义：
/// 无已加载会话时从队列当前曲目从头播放。
#[tauri::command]
pub async fn toggle_play(state: State<'_, AppState>) -> Result<(), String> {
    debug!("ipc::toggle_play");
    let state = (*state).clone();
    run_blocking(move || {
        #[cfg(windows)]
        {
            crate::smtc::toggle_playback(&state)
        }
        #[cfg(not(windows))]
        {
            let _ = state;
            Ok(())
        }
    })
    .await
}
