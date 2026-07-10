use std::{
    collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Windows 上启动子进程时隐藏控制台窗口，避免点击曲目 / 后台 remux 时
/// 出现 cmd 黑窗一闪而过。0x0800_0000 = CREATE_NO_WINDOW。
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::{
    header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE,
        ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
    },
    Client,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::{
    cache::{cache_dir, enforce_cache_limit_preserving},
    library::{merge_tracks_into_cache, ImportedTrack},
};
