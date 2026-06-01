//! Tauri shell 层。
//!
//! 职责：
//! - 启动 Tauri runtime + 注册窗口
//! - 把 IPC 命令转发给 `seraph-core`
//! - 把 `PlayerEvent` 桥接成前端能收到的事件
//!
//! 注意：所有真正的音频逻辑都不在这里写，而是交给 `crates/seraph-*`。

mod ipc;
mod state;

use state::AppState;
#[cfg(debug_assertions)]
use tracing_subscriber::EnvFilter;

pub fn run() {
    #[cfg(debug_assertions)]
    init_tracing();

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            ipc::playback::play,
            ipc::playback::pause,
            ipc::playback::stop,
            ipc::playback::seek,
            ipc::playback::next_track,
            ipc::playback::prev_track,
            ipc::playback::set_volume,
            ipc::playback::select_output_device,
            ipc::library::get_playlist,
            ipc::library::get_track_info,
            ipc::library::import_tracks,
            ipc::library::save_track_lyrics,
            ipc::library::list_devices,
        ])
        .setup(|app| {
            ipc::events::wire_event_bus(app.handle().clone());
            Ok(())
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
