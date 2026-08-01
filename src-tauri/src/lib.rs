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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
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
            ipc::playback::position_taskbar_bar,
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
        ])
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

#[cfg(debug_assertions)]
fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("seraph=debug,info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
