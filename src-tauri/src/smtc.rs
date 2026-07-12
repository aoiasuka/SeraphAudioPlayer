//! Windows SMTC（System Media Transport Controls）集成。
//!
//! 接通系统媒体浮窗与媒体键：
//! - 键盘媒体键 / 蓝牙耳机按键 → 播放 / 暂停 / 上一首 / 下一首 / 定位
//! - 系统音量浮窗与锁屏显示曲目标题、艺术家、专辑与封面
//!
//! 设计：
//! - `MediaControls`（WinRT 对象，非 Send）在专用线程创建并常驻，该线程
//!   同时订阅 [`EventBus`](seraph_core::EventBus) 把播放状态同步给系统；
//! - 媒体键回调由 WinRT 在系统线程触发，只经 `AppHandle` 调用线程安全的
//!   [`AppState`] 播放控制方法，事件流（PlaybackResumed 等）随后自然驱动
//!   前端 UI 与本模块自身的状态更新，与应用内操作走同一条路径。

#![cfg(windows)]

use crate::state::{AppState, TrackAdvance};
use seraph_core::{PlayerEvent, PlayerState};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{debug, warn};

/// 在 Tauri setup 阶段调用。初始化失败只记日志，绝不阻断应用启动。
pub fn init(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        warn!("SMTC init skipped: main window not found");
        return;
    };
    let hwnd_addr = match window.hwnd() {
        Ok(hwnd) => hwnd.0 as isize,
        Err(err) => {
            warn!("SMTC init skipped: failed to get hwnd: {err}");
            return;
        }
    };

    let event_rx = app.state::<AppState>().event_bus.subscribe();
    let app_handle = app.clone();
    std::thread::Builder::new()
        .name("smtc".into())
        .spawn(move || run_smtc(app_handle, event_rx, hwnd_addr))
        .map(|_| ())
        .unwrap_or_else(|err| warn!("SMTC thread spawn failed: {err}"));
}

fn run_smtc(app: AppHandle, event_rx: crossbeam_channel::Receiver<PlayerEvent>, hwnd_addr: isize) {
    let config = PlatformConfig {
        display_name: "Seraph Audio Player",
        dbus_name: "seraph_audio_player",
        hwnd: Some(hwnd_addr as *mut std::ffi::c_void),
    };

    let mut controls = match MediaControls::new(config) {
        Ok(controls) => controls,
        Err(err) => {
            warn!("SMTC unavailable: {err:?}");
            return;
        }
    };

    let handler_app = app.clone();
    if let Err(err) = controls.attach(move |event| handle_media_event(&handler_app, event)) {
        warn!("SMTC attach failed: {err:?}");
        return;
    }
    let _ = controls.set_playback(MediaPlayback::Stopped);
    debug!("SMTC attached");

    // Progress 事件频率高于每秒；SMTC 进度只需秒级精度，整秒变化才更新。
    let mut last_progress_sec = u64::MAX;

    while let Ok(event) = event_rx.recv() {
        let result = match &event {
            PlayerEvent::PlaybackStarted { track_id } | PlayerEvent::TrackChanged { track_id } => {
                last_progress_sec = u64::MAX;
                update_track_metadata(&app, &mut controls, track_id)
            }
            PlayerEvent::PlaybackResumed => {
                controls.set_playback(MediaPlayback::Playing { progress: None })
            }
            PlayerEvent::PlaybackPaused => {
                controls.set_playback(MediaPlayback::Paused { progress: None })
            }
            PlayerEvent::PlaybackStopped => controls.set_playback(MediaPlayback::Stopped),
            PlayerEvent::Progress { seconds, .. } => {
                let sec = seconds.max(0.0) as u64;
                if sec == last_progress_sec {
                    Ok(())
                } else {
                    last_progress_sec = sec;
                    let progress = Some(MediaPosition(Duration::from_secs(sec)));
                    let playing =
                        *app.state::<AppState>().player_state.read() == PlayerState::Playing;
                    controls.set_playback(if playing {
                        MediaPlayback::Playing { progress }
                    } else {
                        MediaPlayback::Paused { progress }
                    })
                }
            }
            _ => Ok(()),
        };

        if let Err(err) = result {
            debug!("SMTC update failed: {err:?}");
        }
    }
}

/// TrackChanged / PlaybackStarted 时把队列内曲目元数据推给系统浮窗。
fn update_track_metadata(
    app: &AppHandle,
    controls: &mut MediaControls,
    track_id: &str,
) -> Result<(), souvlaki::Error> {
    let state = app.state::<AppState>();
    let Some(track) = state.queue_track_by_id(track_id) else {
        return Ok(());
    };

    let cover_url = cover_to_uri(&track.cover);
    controls.set_metadata(MediaMetadata {
        title: Some(&track.title),
        artist: Some(&track.artist),
        album: Some(&track.album),
        cover_url: cover_url.as_deref(),
        duration: (track.duration > 0).then(|| Duration::from_secs(track.duration)),
    })?;

    let playing = *state.player_state.read() == PlayerState::Playing;
    controls.set_playback(if playing {
        MediaPlayback::Playing { progress: None }
    } else {
        MediaPlayback::Paused { progress: None }
    })
}

/// 封面地址转系统可加载的 URI：https 直接用；本地绝对路径转 file:/// 并
/// 对非 ASCII 与空格做最小百分号编码（WinRT Uri 解析要求）。
fn cover_to_uri(cover: &str) -> Option<String> {
    let cover = cover.trim();
    if cover.is_empty() {
        return None;
    }
    if cover.starts_with("http://") || cover.starts_with("https://") {
        return Some(cover.to_string());
    }

    let forward = cover.replace('\\', "/");
    let mut encoded = String::with_capacity(forward.len() + 8);
    for byte in forward.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'-' | b'_' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    Some(format!("file:///{encoded}"))
}

/// 媒体键事件 → 播放控制。走与 IPC 命令相同的 AppState 路径，
/// 引擎随后发布的 PlayerEvent 自然同步前端 UI 与 SMTC 显示。
fn handle_media_event(app: &AppHandle, event: MediaControlEvent) {
    let state = app.state::<AppState>();
    let result: Result<(), String> = match event {
        MediaControlEvent::Play => smtc_play(&state),
        MediaControlEvent::Pause => smtc_pause(&state),
        MediaControlEvent::Toggle => {
            if *state.player_state.read() == PlayerState::Playing {
                smtc_pause(&state)
            } else {
                smtc_play(&state)
            }
        }
        MediaControlEvent::Next => state.advance_track(TrackAdvance::Next),
        MediaControlEvent::Previous => state.advance_track(TrackAdvance::Previous),
        MediaControlEvent::Stop => {
            let result = state.audio.stop().map_err(|err| err.to_string());
            if result.is_ok() {
                *state.player_state.write() = PlayerState::Stopped;
            }
            result
        }
        MediaControlEvent::SetPosition(position) => state
            .audio
            .seek(position.0.as_secs_f64())
            .map_err(|err| err.to_string()),
        _ => Ok(()),
    };

    if let Err(err) = result {
        warn!("SMTC media key action failed: {err}");
    }
}

fn smtc_play(state: &AppState) -> Result<(), String> {
    // 先尝试恢复既有会话；没有已加载文件（如启动后直接按媒体键）则从
    // 队列当前曲目从头播放。
    if state.audio.resume().is_ok() {
        *state.player_state.write() = PlayerState::Playing;
        return Ok(());
    }
    state.play_current_track()
}

fn smtc_pause(state: &AppState) -> Result<(), String> {
    state.audio.pause().map_err(|err| err.to_string())?;
    *state.player_state.write() = PlayerState::Paused;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cover_to_uri;

    #[test]
    fn https_cover_passes_through() {
        assert_eq!(
            cover_to_uri("https://i0.hdslb.com/x.jpg").as_deref(),
            Some("https://i0.hdslb.com/x.jpg")
        );
    }

    #[test]
    fn empty_cover_is_none() {
        assert_eq!(cover_to_uri(""), None);
        assert_eq!(cover_to_uri("  "), None);
    }

    #[test]
    fn local_path_becomes_percent_encoded_file_uri() {
        let uri = cover_to_uri(r"C:\Users\音乐 库\covers\abc.jpg").unwrap();
        assert!(uri.starts_with("file:///C:/Users/"));
        assert!(!uri.contains(' '), "空格必须被编码: {uri}");
        assert!(!uri.contains('\\'));
        assert!(uri.ends_with("/covers/abc.jpg"));
    }
}
