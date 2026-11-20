//! Bilibili 音频导入 IPC handlers。
//!
//! 原 `include!` 拼接结构已改为真模块树，共享项收敛到 [`prelude`]。

mod batch;
mod cancellation;
mod commands;
mod constants;
mod download;
mod ffmpeg;
mod impls_and_tests;
mod import_audio;
mod pages;
mod parsing;
mod prelude;
mod qr;
mod session;
mod types;

pub use commands::*;
pub(crate) use import_audio::acquire_audio_operation;
pub(crate) use parsing::extract_bvid;
