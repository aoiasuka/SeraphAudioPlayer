//! Windows 任务栏图标集成:缩略图播控按钮(ITaskbarList3 ThumbBar)+
//! 图标播放进度条。
//!
//! 分层:
//! - [`plan`] — 纯逻辑:事件 → 渲染计划,可单测;
//! - [`icons`] — 纯光栅化(⏮⏯⏭ 形状 → 预乘 BGRA)+ HICON 胶水;
//! - [`thumbbar`] — Win32/COM 接线(窗口子类化、消息泵、ITaskbarList3)。
//!
//! 与 smtc.rs 同一原则:初始化失败只记日志,绝不阻断应用启动;按钮点击
//! 走与 IPC 命令相同的 AppState 路径,事件流自然回推各处 UI。

#![cfg(windows)]

mod icons;
mod lyric_bar;
mod plan;
mod thumbbar;

pub use lyric_bar::{position as position_lyric_bar, set_enabled as set_lyrics_enabled};
pub use thumbbar::{init, playback_snapshot, set_features};
