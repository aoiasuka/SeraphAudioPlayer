//! bilibili 模块共享 prelude：外部依赖 + 跨子模块共享项统一从这里 glob 引入。
//!
//! 子模块统一 `use super::prelude::*;`，跨模块共享的顶层项标 `pub(crate)`。

pub(crate) use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
pub(crate) use std::os::unix::fs::PermissionsExt;

/// Windows 上启动子进程时隐藏控制台窗口，避免点击曲目 / 后台 remux 时
/// 出现 cmd 黑窗一闪而过。0x0800_0000 = CREATE_NO_WINDOW。
#[cfg(windows)]
pub(crate) fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
pub(crate) fn hide_console_window(_command: &mut Command) {}

pub(crate) use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
pub(crate) use reqwest::{
    header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE,
        ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
    },
    Client,
};
pub(crate) use serde::{de::DeserializeOwned, Deserialize, Serialize};
pub(crate) use serde_json::Value;
pub(crate) use tauri::{AppHandle, Manager};

pub(crate) use crate::ipc::{
    cache::{cache_dir, enforce_cache_limit_preserving_many, unique_temp_path},
    library::{merge_tracks_into_cache, ImportedTrack},
};

// 兄弟模块共享项（glob 汇聚，供子模块一站式引入）
#[allow(unused_imports)]
pub(crate) use super::{
    commands::*, constants::*, ffmpeg::*, import_audio::*, parsing::*, session::*, types::*,
};
