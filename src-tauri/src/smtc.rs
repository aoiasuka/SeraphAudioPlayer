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
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{debug, warn};

/// 设置页开关 → SMTC 线程的控制通道（true=启用 / false=停用）。
static SMTC_CONTROL: OnceLock<crossbeam_channel::Sender<bool>> = OnceLock::new();

/// 运行时启用/停用 SMTC（set_smtc_enabled 命令调用）。
/// SMTC 线程未初始化（init 失败等）时静默忽略。
pub fn set_enabled(enabled: bool) {
    if let Some(sender) = SMTC_CONTROL.get() {
        let _ = sender.send(enabled);
    }
}

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

    let (control_tx, control_rx) = crossbeam_channel::unbounded();
    if SMTC_CONTROL.set(control_tx).is_err() {
        warn!("SMTC init skipped: already initialized");
        return;
    }

    let event_rx = app.state::<AppState>().event_bus.subscribe();
    let app_handle = app.clone();
    std::thread::Builder::new()
        .name("smtc".into())
        .spawn(move || run_smtc(app_handle, event_rx, control_rx, hwnd_addr))
        .map(|_| ())
        .unwrap_or_else(|err| warn!("SMTC thread spawn failed: {err}"));
}

fn run_smtc(
    app: AppHandle,
    event_rx: crossbeam_channel::Receiver<PlayerEvent>,
    control_rx: crossbeam_channel::Receiver<bool>,
    hwnd_addr: isize,
) {
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

    // 默认启用注册；用户此前关过开关时，前端水合后会立即发停用消息
    let mut attached = attach_controls(&mut controls, &app);
    if attached {
        let _ = controls.set_playback(MediaPlayback::Stopped);
        debug!("SMTC attached");
    }

    // Progress 事件频率高于每秒；SMTC 进度只需秒级精度，整秒变化才更新。
    let mut last_progress_sec = u64::MAX;
    // 停用期间仍跟踪当前曲目，重新启用时立即恢复系统浮窗显示
    let mut last_track_id: Option<String> = None;

    loop {
        crossbeam_channel::select! {
            recv(control_rx) -> message => {
                let Ok(enable) = message else { break };
                if enable && !attached {
                    attached = attach_controls(&mut controls, &app);
                    if attached {
                        last_progress_sec = u64::MAX;
                        if let Some(track_id) = last_track_id.clone() {
                            let _ = update_track_metadata(&app, &mut controls, &track_id);
                        }
                        debug!("SMTC re-attached");
                    }
                } else if !enable && attached {
                    let _ = controls.set_playback(MediaPlayback::Stopped);
                    if let Err(err) = controls.detach() {
                        warn!("SMTC detach failed: {err:?}");
                    }
                    attached = false;
                    debug!("SMTC detached");
                }
            }
            recv(event_rx) -> message => {
                let Ok(event) = message else { break };
                if let PlayerEvent::PlaybackStarted { track_id }
                | PlayerEvent::TrackChanged { track_id } = &event
                {
                    last_track_id = Some(track_id.clone());
                }
                if !attached {
                    continue;
                }

                let result = match &event {
                    PlayerEvent::PlaybackStarted { track_id }
                    | PlayerEvent::TrackChanged { track_id } => {
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
                            let playing = *app.state::<AppState>().player_state.read()
                                == PlayerState::Playing;
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
    }
}

fn attach_controls(controls: &mut MediaControls, app: &AppHandle) -> bool {
    let handler_app = app.clone();
    match controls.attach(move |event| handle_media_event(&handler_app, event)) {
        Ok(()) => true,
        Err(err) => {
            warn!("SMTC attach failed: {err:?}");
            false
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

    // L-1：本地封面只信应用自己的 covers 目录（合法值域见 cover_to_uri 注释）
    let covers_dir = app.path().app_data_dir().ok().map(|dir| dir.join("covers"));
    let cover_url = cover_to_uri(&track.cover, covers_dir.as_deref());
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

/// 封面地址转 souvlaki 可加载的形式：https 直接透传；本地路径按 souvlaki
/// Windows 实现的私有约定传 `file://` + **原生路径**——souvlaki 对 file://
/// 前缀做 trim_start_matches 后把剩余部分直接丢给
/// StorageFile::GetFileFromPathAsync（要求裸 Windows 路径）。若传规范的
/// file:///C:/… URI（百分号编码），剥前缀后剩 `/C:/…%E9%9F%B3…` 这样的
/// 坏路径名，SetThumbnail 报 0x800700A1（ERROR_BAD_PATHNAME），封面推不
/// 上系统浮窗。
///
/// N-02：网络地址必须过 https + B 站图床白名单。缩略图是交给 **Windows Shell
/// 进程**去取的，此前 `http://` 与任意 host 一律透传——曲库缓存是磁盘文件、
/// 可被离线篡改，等于借系统组件做任意出站请求。不合规的网络地址直接不推封面
/// （返回 None），不影响曲目信息本身的显示。
fn cover_to_uri(cover: &str, covers_dir: Option<&Path>) -> Option<String> {
    let cover = cover.trim();
    if cover.is_empty() {
        return None;
    }
    if cover.starts_with("http://") || cover.starts_with("https://") {
        return crate::ipc::url_guard::is_safe_system_image_url(cover).then(|| cover.to_string());
    }
    // L-1（2026-08-16 审查）：本地分支同样要收口。此前任意非 http 字符串都被拼上
    // file:// 交给 Shell——`\\attacker.com\share\a.jpg` 会让系统组件向攻击者的
    // SMB 服务器发起出站连接（域环境下伴随 NTLM 质询，认证材料可被离线破解）。
    // 合法本地取值只有应用 covers 目录内的封面文件（见 is_safe_system_image_url
    // 注释），据此收口：拒 UNC、拒相对路径，canonicalize 后必须落在 covers 目录内
    // 且是普通文件。校验通过后仍返回**原始路径**——souvlaki 契约要求裸 Windows
    // 路径，canonicalize 产物带 \\?\ 前缀，不保证被 GetFileFromPathAsync 接受。
    if cover.starts_with(r"\\") || cover.starts_with("//") {
        return None;
    }
    let path = Path::new(cover);
    if !path.is_absolute() {
        return None;
    }
    let canonical_dir = covers_dir?.canonicalize().ok()?;
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(&canonical_dir) || !canonical.is_file() {
        return None;
    }
    Some(format!("file://{cover}"))
}

/// 媒体键事件 → 播放控制。走与 IPC 命令相同的 AppState 路径，
/// 引擎随后发布的 PlayerEvent 自然同步前端 UI 与 SMTC 显示。
fn handle_media_event(app: &AppHandle, event: MediaControlEvent) {
    let state = app.state::<AppState>();
    let result: Result<(), String> = match event {
        MediaControlEvent::Play => smtc_play(&state),
        MediaControlEvent::Pause => smtc_pause(&state),
        MediaControlEvent::Toggle => toggle_playback(&state),
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

/// 播放/暂停切换:SMTC Toggle 与任务栏缩略图播放按钮共用同一语义。
pub fn toggle_playback(state: &AppState) -> Result<(), String> {
    if *state.player_state.read() == PlayerState::Playing {
        smtc_pause(state)
    } else {
        smtc_play(state)
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
    use std::path::PathBuf;

    /// 建一个真实存在的 covers 目录 + 封面文件（canonicalize 需要真实路径）。
    fn temp_covers(tag: &str) -> (PathBuf, PathBuf) {
        let covers = std::env::temp_dir().join(format!("seraph-smtc-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&covers).expect("create covers dir");
        let file = covers.join("音乐 库 abc.jpg");
        std::fs::write(&file, b"jpg").expect("write cover file");
        (covers, file)
    }

    #[test]
    fn n02_rejects_untrusted_network_cover_urls() {
        // N-02：缩略图是交给 Windows Shell 进程去取的，曲库缓存可被离线篡改，
        // 非官方图床的网络地址一律不推封面（返回 None，不影响曲目信息显示）。
        for cover in [
            "http://i0.hdslb.com/x.jpg",
            "https://evil.example.com/x.jpg",
            "https://hdslb.com.evil.com/x.jpg",
            "https://i0.hdslb.com@evil.com/x.jpg",
            "http://127.0.0.1:8080/probe.jpg",
        ] {
            assert_eq!(cover_to_uri(cover, None), None, "should reject {cover}");
        }
    }

    #[test]
    fn https_cover_passes_through() {
        assert_eq!(
            cover_to_uri("https://i0.hdslb.com/x.jpg", None).as_deref(),
            Some("https://i0.hdslb.com/x.jpg")
        );
    }

    #[test]
    fn empty_cover_is_none() {
        assert_eq!(cover_to_uri("", None), None);
        assert_eq!(cover_to_uri("  ", None), None);
    }

    /// souvlaki 契约：file:// 前缀之后必须是 GetFileFromPathAsync 能直接
    /// 打开的裸 Windows 路径——不做百分号编码、不换正斜杠、无第三个斜杠。
    /// L-1 之后：本地封面还必须真实存在于 covers 目录内。
    #[test]
    fn local_path_keeps_raw_windows_path_after_prefix() {
        let (covers, file) = temp_covers("raw");
        let cover = file.to_string_lossy().to_string();

        let uri = cover_to_uri(&cover, Some(&covers)).expect("covers 内真实文件应通过");
        assert!(uri.starts_with("file://"));
        assert_eq!(uri.trim_start_matches("file://"), cover);

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&covers);
    }

    /// L-1：UNC / 相对路径 / covers 目录之外的文件一律不得透传——
    /// `\\attacker.com\share\a.jpg` 会让 Windows Shell 向攻击者 SMB 服务器
    /// 发起出站连接（NTLM 质询泄露面）。
    #[test]
    fn l1_rejects_unc_relative_and_out_of_covers_paths() {
        let (covers, file) = temp_covers("l1");
        // covers 之外的真实文件：存在性骗不过目录约束
        let outside =
            std::env::temp_dir().join(format!("seraph-smtc-l1-outside-{}.jpg", std::process::id()));
        std::fs::write(&outside, b"jpg").expect("write outside file");

        for cover in [
            r"\\attacker.example\share\a.jpg",  // UNC → SMB 出站
            "//attacker.example/share/a.jpg",   // UNC 正斜杠变体
            r"covers\a.jpg",                    // 相对路径
            r"C:\definitely\missing\cover.jpg", // 不存在的文件
        ] {
            assert_eq!(
                cover_to_uri(cover, Some(&covers)),
                None,
                "should reject {cover}"
            );
        }
        assert_eq!(
            cover_to_uri(&outside.to_string_lossy(), Some(&covers)),
            None,
            "covers 目录之外的真实文件也必须拒绝"
        );
        // covers 目录不可用（app_data_dir 失败）时本地封面一律不推——fail closed
        assert_eq!(cover_to_uri(&file.to_string_lossy(), None), None);

        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&covers);
    }
}
