//! Tauri shell 层。
//!
//! 职责：
//! - 启动 Tauri runtime + 注册窗口
//! - 把 IPC 命令转发给 `seraph-core`
//! - 把 `PlayerEvent` 桥接成前端能收到的事件
//!
//! 注意：所有真正的音频逻辑都不在这里写，而是交给 `crates/seraph-*`。

mod ipc;
#[cfg(windows)]
mod smtc;
mod state;
#[cfg(windows)]
mod taskbar;

use state::AppState;
use tauri::Manager;
#[cfg(debug_assertions)]
use tracing_subscriber::EnvFilter;

pub fn run() {
    #[cfg(debug_assertions)]
    init_tracing();

    // F-03：先把生成的 handler 绑成具名值，好在外面套一层窗口白名单拦截。
    // 需要显式标注 Runtime（generate_handler! 本身是泛型的，单独绑定推不出来）。
    let handler: Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync> =
        Box::new(tauri::generate_handler![
            ipc::playback::play,
            ipc::playback::sync_playback_queue,
            ipc::playback::set_playback_modes,
            ipc::playback::pause,
            ipc::playback::stop,
            ipc::playback::seek,
            ipc::playback::next_track,
            ipc::playback::prev_track,
            ipc::playback::set_volume,
            ipc::playback::select_output_device,
            ipc::playback::set_output_driver,
            ipc::playback::set_smtc_enabled,
            ipc::playback::set_taskbar_features,
            ipc::playback::set_taskbar_lyrics_enabled,
            ipc::playback::set_taskbar_lyrics_click_through,
            ipc::playback::position_taskbar_bar,
            ipc::playback::set_taskbar_lyrics_position,
            ipc::playback::get_playback_snapshot,
            ipc::playback::toggle_play,
            ipc::cache::clear_cache,
            ipc::cache::get_cache_status,
            ipc::cache::update_cache_settings,
            ipc::library::get_playlist,
            ipc::library::get_track_info,
            ipc::library::delete_track,
            ipc::library::import_tracks,
            ipc::bilibili::bilibili_ffmpeg_status,
            ipc::bilibili::download_ffmpeg,
            ipc::bilibili::bilibili_login_qrcode,
            ipc::bilibili::bilibili_login_status,
            ipc::bilibili::bilibili_logout,
            ipc::bilibili::bilibili_poll_login,
            ipc::bilibili::import_bilibili_audio,
            ipc::bilibili::import_bilibili_audio_with_options,
            ipc::bilibili::import_bilibili_favorites,
            ipc::bilibili::cancel_bilibili_favorites_import,
            ipc::library::apply_online_lyrics,
            ipc::library::fetch_online_lyrics,
            ipc::library::fetch_online_cover,
            ipc::library::save_track_lyrics,
            ipc::library::list_devices,
            ipc::update::check_for_update,
            ipc::update::open_release_page,
            ipc::playlist_io::import_playlist_m3u8,
            ipc::playlist_io::export_playlist_m3u8,
            ipc::system::reveal_in_explorer,
            ipc::system::focus_main_window,
            ipc::dsp::set_dsp_settings,
            ipc::dsp::import_eq_preset,
            ipc::dsp::export_eq_preset,
            ipc::config::export_app_config,
            ipc::config::import_app_config,
            ipc::visualizer::get_spectrum_frame,
            ipc::visualizer::get_analysis_frame,
            ipc::visualizer::reset_analysis_meters,
        ]);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        // F-03：Tauri v2 的 capabilities 只约束 core/插件命令，`generate_handler!`
        // 注册的自定义命令对**所有**窗口一律开放。歌词条窗口渲染的是外部歌词与
        // 元数据，一旦其渲染面被攻破就能调用任意文件读写 / 子进程命令。
        // 这里做中央拦截：非主窗口只放行它实际需要的只读命令。
        .invoke_handler(move |invoke| {
            let label = invoke.message.webview().label().to_string();
            let command = invoke.message.command().to_string();
            if !command_allowed_for_window(&label, &command) {
                invoke.resolver.reject(format!(
                    "command `{command}` is not available to window `{label}`"
                ));
                return true;
            }
            handler(invoke)
        })
        .setup(|app| {
            if let Ok(app_dir) = app.path().app_data_dir() {
                seraph_decoder::configure_ffmpeg_search_dirs([app_dir.join("ffmpeg")]);
                // 本地曲目内嵌封面提取到 covers 目录后经 asset 协议供 <img> 加载，
                // 范围只放开这一个目录
                let _ = app
                    .asset_protocol_scope()
                    .allow_directory(app_dir.join("covers"), false);
            }
            ipc::events::wire_event_bus(app.handle().clone());
            // Windows 系统媒体控件：媒体键 + 锁屏/音量浮窗曲目展示
            #[cfg(windows)]
            smtc::init(app.handle());
            // Windows 任务栏：缩略图播控按钮 + 图标播放进度条
            #[cfg(windows)]
            taskbar::init(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // 主窗口关闭 = 退出应用：任务栏歌词条窗口必须联动销毁，否则
            // Tauri 因残留窗口不退出——播放继续、歌词条还在、主界面却回不来。
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                if let Some(bar) = window.app_handle().get_webview_window("taskbar-lyrics") {
                    let _ = bar.destroy();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Seraph Audio Player");
}

/// F-03：任务栏歌词条窗口允许调用的自定义命令白名单。
///
/// 歌词条走 IPC 拉数据（见 CLAUDE.md「第二窗口」约定）：切歌时拉一次曲目元数据、
/// 轮询播放快照、✕ 与拖拽回传主窗口，另有 ⌂ 唤起主窗口。**只需要这四个**，
/// 其余一律拒绝——尤其是文件读写（config/eq/m3u8 导入导出）、子进程
/// （reveal/download_ffmpeg）与登录凭据相关命令。
///
/// 新增给歌词条用的命令必须显式加进这里；给主窗口用的不要动这个名单。
const TASKBAR_LYRICS_ALLOWED_COMMANDS: &[&str] = &[
    "get_playback_snapshot",
    "get_track_info",
    "focus_main_window",
    "set_taskbar_lyrics_position",
];

fn command_allowed_for_window(label: &str, command: &str) -> bool {
    match label {
        "main" => true,
        "taskbar-lyrics" => TASKBAR_LYRICS_ALLOWED_COMMANDS.contains(&command),
        // 未知 label：应用只建这两个窗口，出现第三个即视为异常，一律拒绝
        _ => false,
    }
}

#[cfg(debug_assertions)]
fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("seraph=debug,info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taskbar_lyrics_window_cannot_reach_sensitive_commands() {
        // F-03：歌词条被攻破也不得触到文件读写 / 子进程 / 凭据命令
        for command in [
            "export_app_config",
            "import_app_config",
            "import_eq_preset",
            "export_eq_preset",
            "import_playlist_m3u8",
            "export_playlist_m3u8",
            "reveal_in_explorer",
            "download_ffmpeg",
            "import_tracks",
            "delete_track",
            "save_track_lyrics",
            "bilibili_login_qrcode",
            "clear_cache",
        ] {
            assert!(
                !command_allowed_for_window("taskbar-lyrics", command),
                "taskbar-lyrics must not be allowed to call {command}"
            );
            assert!(
                command_allowed_for_window("main", command),
                "main window must still be allowed to call {command}"
            );
        }
    }

    #[test]
    fn taskbar_lyrics_window_keeps_the_commands_it_actually_needs() {
        for command in TASKBAR_LYRICS_ALLOWED_COMMANDS {
            assert!(command_allowed_for_window("taskbar-lyrics", command));
        }
    }

    #[test]
    fn unknown_windows_are_denied_everything() {
        assert!(!command_allowed_for_window(
            "popup",
            "get_playback_snapshot"
        ));
        assert!(!command_allowed_for_window("", "play"));
    }
}
