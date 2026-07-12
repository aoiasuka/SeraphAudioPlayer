//! library 模块共享 prelude：外部依赖 + 跨子模块共享项统一从这里 glob 引入。
//!
//! 子模块统一 `use super::prelude::*;`，跨模块共享的顶层项标 `pub(crate)`。

pub(crate) use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

pub(crate) use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
pub(crate) use des::{
    cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
    TdesEde3,
};
pub(crate) use encoding_rs::GBK;
pub(crate) use flate2::read::ZlibDecoder;
pub(crate) use lofty::{
    file::{AudioFile, TaggedFileExt},
    picture::{MimeType, PictureType},
    prelude::Accessor,
    tag::{ItemKey, Tag},
};
pub(crate) use regex::Regex;
pub(crate) use reqwest::{
    header::{HeaderMap, HeaderValue, REFERER, USER_AGENT},
    Client,
};
pub(crate) use seraph_audio::list_output_devices;
pub(crate) use seraph_decoder::probe_stream_info;
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::Value;
pub(crate) use tauri::{AppHandle, Manager};

// 兄弟模块共享项（glob 汇聚，供子模块一站式引入）
#[allow(unused_imports)]
pub(crate) use super::{lyrics::*, media_library::*, metadata::*, online_lyrics::*, types::*};
