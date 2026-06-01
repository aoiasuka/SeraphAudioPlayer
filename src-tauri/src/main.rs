// Tauri 主程序入口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    seraph_tauri_lib::run();
}
