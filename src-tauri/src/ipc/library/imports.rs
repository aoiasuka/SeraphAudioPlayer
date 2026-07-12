use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use des::{
    cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
    TdesEde3,
};
use encoding_rs::GBK;
use flate2::read::ZlibDecoder;
use lofty::{
    file::{AudioFile, TaggedFileExt},
    picture::{MimeType, PictureType},
    prelude::Accessor,
    tag::{ItemKey, Tag},
};
use regex::Regex;
use reqwest::{
    header::{HeaderMap, HeaderValue, REFERER, USER_AGENT},
    Client,
};
use seraph_audio::list_output_devices;
use seraph_decoder::probe_stream_info;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
