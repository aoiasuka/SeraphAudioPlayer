//! Library IPC handlers.
use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
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

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_year: Option<String>,
    pub cover: String,
    pub format: String,
    pub bitdepth: String,
    pub sample_rate: String,
    pub bitrate: String,
    pub channels: String,
    pub size: String,
    pub path: String,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub cache_missing: bool,
    pub duration: u64,
    pub glow_color: String,
    pub glow1: String,
    pub glow2: String,
    pub lyrics: Vec<LyricLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineLyricsCandidate {
    pub id: String,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<u64>,
    pub lyrics: Vec<LyricLine>,
}

#[derive(Debug, Clone)]
struct ProviderLyricLine {
    start_ms: u64,
    text: String,
}

#[derive(Debug, Default)]
struct ParsedAudioMetadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_year: Option<String>,
    duration: Option<u64>,
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    bit_depth: Option<u8>,
    channels: Option<u8>,
    lyrics: Vec<LyricLine>,
}

#[derive(Debug, Default)]
struct FilenameMetadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

#[tauri::command]
pub fn get_playlist(app: AppHandle) -> Result<Vec<ImportedTrack>, String> {
    read_cached_tracks(&app)
}

#[tauri::command]
pub fn get_track_info(app: AppHandle, track_id: String) -> Result<Option<ImportedTrack>, String> {
    Ok(read_cached_tracks(&app)?
        .into_iter()
        .find(|track| track.id == track_id))
}

#[tauri::command]
pub fn list_devices() -> Result<Vec<OutputDeviceInfo>, String> {
    let devices = list_output_devices().map_err(|err| err.to_string())?;
    Ok(devices
        .into_iter()
        .map(|device| OutputDeviceInfo {
            id: device.id,
            name: device.name,
            is_default: device.is_default,
        })
        .collect())
}

#[tauri::command]
pub async fn import_tracks(app: AppHandle, paths: Vec<String>) -> Result<Vec<ImportedTrack>, String> {
    // L-18：文件遍历 + lofty 解析 + 可能的 ffprobe 子进程都是阻塞 IO，
    // 放到 spawn_blocking，避免占用 Tauri 命令调度线程、阻塞其它 IPC（含播放控制）。
    tauri::async_runtime::spawn_blocking(move || {
        let mut tracks = Vec::new();
        let mut seen_files = HashSet::new();
        let mut visited_dirs = HashSet::new();

        for path in paths {
            collect_audio_files(
                PathBuf::from(path),
                &mut tracks,
                &mut seen_files,
                &mut visited_dirs,
                0,
            )?;
        }

        if !tracks.is_empty() {
            let cached = read_cached_tracks(&app).unwrap_or_default();
            let merged = merge_cached_tracks(cached, &tracks);
            write_cached_tracks(&app, &merged)?;
            return Ok(imported_tracks_from_cache(&merged, &tracks));
        }

        Ok(tracks)
    })
    .await
    .map_err(|err| format!("import task panicked: {err}"))?
}

#[tauri::command]
pub fn save_track_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics_bytes: Vec<u8>,
    track_path: Option<String>,
) -> Result<Vec<LyricLine>, String> {
    if track_id.trim().is_empty() {
        return Err("missing track id".into());
    }

    if lyrics_bytes.is_empty() {
        return Err("lyrics file is empty".into());
    }

    // 后端独立校验大小：前端虽然限制了 2MB，但 IPC 可被绕过；
    // 设为 4MB 留余量，同时阻挡明显异常输入。
    const MAX_LYRICS_BYTES: usize = 4 * 1024 * 1024;
    if lyrics_bytes.len() > MAX_LYRICS_BYTES {
        return Err(format!(
            "lyrics file too large: {} bytes (limit {})",
            lyrics_bytes.len(),
            MAX_LYRICS_BYTES
        ));
    }

    let lyrics = parse_lyrics_bytes(&lyrics_bytes);
    if lyrics.is_empty() {
        return Err("lyrics file has no usable text".into());
    }

    let mut tracks = read_cached_tracks(&app)?;
    apply_track_lyrics(
        &mut tracks,
        &track_id,
        lyrics.clone(),
        track_path.as_deref(),
    )?;
    write_cached_tracks(&app, &tracks)?;

    Ok(lyrics)
}

#[tauri::command]
pub async fn fetch_online_lyrics(
    _track_id: String,
    title: String,
    artist: String,
    duration: u64,
) -> Result<Vec<OnlineLyricsCandidate>, String> {
    let query = online_lyrics_query(&title, &artist);
    if query.is_empty() {
        return Err("missing track title".into());
    }

    let client = online_lyrics_client()?;
    let candidates = fetch_online_lyrics_from_sources(&client, &query, duration).await;
    if candidates.is_empty() {
        return Err("online lyrics not found".into());
    }

    Ok(candidates)
}

#[tauri::command]
pub fn apply_online_lyrics(
    app: AppHandle,
    track_id: String,
    lyrics: Vec<LyricLine>,
    track_path: Option<String>,
) -> Result<Vec<LyricLine>, String> {
    if track_id.trim().is_empty() {
        return Err("missing track id".into());
    }
    if lyrics.is_empty() {
        return Err("lyrics file has no usable text".into());
    }

    let mut tracks = read_cached_tracks(&app)?;
    apply_track_lyrics(
        &mut tracks,
        &track_id,
        lyrics.clone(),
        track_path.as_deref(),
    )?;
    write_cached_tracks(&app, &tracks)?;

    Ok(lyrics)
}

fn online_lyrics_query(title: &str, artist: &str) -> String {
    [title.trim(), artist.trim()]
        .into_iter()
        .filter(|value| !value.is_empty() && *value != "Unknown")
        .collect::<Vec<_>>()
        .join(" ")
}

fn online_lyrics_client() -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        ),
    );

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| format!("failed to create lyrics client: {err}"))
}

async fn fetch_online_lyrics_from_sources(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let mut candidates = Vec::new();
    candidates.extend(fetch_netease_lyrics(client, query, duration).await);
    candidates.extend(fetch_kugou_lyrics(client, query, duration).await);
    candidates.extend(fetch_qq_lyrics(client, query, duration).await);
    dedupe_online_lyrics_candidates(candidates)
}

async fn fetch_netease_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let response = client
        .get("https://music.163.com/api/search/get/web")
        .query(&[
            ("s", query),
            ("type", "1"),
            ("offset", "0"),
            ("limit", "5"),
            ("csrf_token", ""),
        ])
        .send()
        .await
        .ok()
        .and_then(|response| response.error_for_status().ok());

    let Some(response) = response else {
        return Vec::new();
    };

    let Ok(response) = response.json::<Value>().await else {
        return Vec::new();
    };

    let Some(songs) = response
        .get("result")
        .and_then(|value| value.get("songs"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for song in ranked_provider_items(songs, duration).into_iter().take(5) {
        let Some(song_id) = song.get("id").and_then(Value::as_u64) else {
            continue;
        };
        let Ok(lyric_data) = client
            .get("https://music.163.com/api/song/lyric")
            .query(&[
                ("id", song_id.to_string()),
                ("lv", "-1".to_string()),
                ("kv", "-1".to_string()),
                ("tv", "-1".to_string()),
                ("yv", "-1".to_string()),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = lyric_data.json::<Value>().await else {
            continue;
        };
        let Some(lyrics) = parse_netease_lyric_payload(&lyric_data) else {
            continue;
        };

        results.push(OnlineLyricsCandidate {
            id: format!("netease-{song_id}"),
            source: "网易云音乐".into(),
            title: value_string(song, "name").unwrap_or_else(|| query.into()),
            artist: netease_artists(song),
            album: song
                .get("album")
                .and_then(|album| value_string(album, "name")),
            duration: provider_duration_ms(song).map(|ms| ms / 1000),
            lyrics,
        });
    }

    results
}

async fn fetch_kugou_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let duration_ms = duration.saturating_mul(1000).to_string();
    let Ok(response) = client
        .get("https://lyrics.kugou.com/search")
        .query(&[
            ("ver", "1"),
            ("man", "yes"),
            ("client", "pc"),
            ("keyword", query),
            ("duration", duration_ms.as_str()),
            ("hash", ""),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
    else {
        return Vec::new();
    };

    let Ok(response) = response.json::<Value>().await else {
        return Vec::new();
    };

    let Some(candidates) = response.get("candidates").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for candidate in ranked_provider_items(candidates, duration)
        .into_iter()
        .take(5)
    {
        let Some(id) = candidate.get("id").and_then(Value::as_u64) else {
            continue;
        };
        let Some(access_key) = candidate.get("accesskey").and_then(Value::as_str) else {
            continue;
        };
        let id = id.to_string();
        let Ok(lyric_data) = client
            .get("https://lyrics.kugou.com/download")
            .query(&[
                ("ver", "1"),
                ("client", "pc"),
                ("id", id.as_str()),
                ("accesskey", access_key),
                ("fmt", "krc"),
                ("charset", "utf8"),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = lyric_data.json::<Value>().await else {
            continue;
        };

        let Some(content) = lyric_data.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(decoded) = BASE64_STANDARD.decode(content) else {
            continue;
        };
        let lyrics = parse_lyrics_bytes(&decoded);
        if lyrics.is_empty() {
            continue;
        }

        let title = value_string(candidate, "song")
            .or_else(|| value_string(candidate, "filename"))
            .unwrap_or_else(|| query.into());
        results.push(OnlineLyricsCandidate {
            id: format!("kugou-{id}"),
            source: "酷狗音乐".into(),
            title,
            artist: value_string(candidate, "singer").unwrap_or_default(),
            album: value_string(candidate, "album"),
            duration: provider_duration_ms(candidate).map(|ms| ms / 1000),
            lyrics,
        });
    }

    results
}

async fn fetch_qq_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let Ok(search_data) = client
        .get("https://c.y.qq.com/soso/fcgi-bin/client_search_cp")
        .query(&[
            ("format", "json"),
            ("p", "1"),
            ("n", "5"),
            ("w", query),
            ("cr", "1"),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
    else {
        return Vec::new();
    };

    let Ok(search_data) = search_data.json::<Value>().await else {
        return Vec::new();
    };

    let Some(songs) = search_data
        .get("data")
        .and_then(|value| value.get("song"))
        .and_then(|value| value.get("list"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for song in ranked_provider_items(songs, duration).into_iter().take(5) {
        let Some(song_mid) = song
            .get("songmid")
            .or_else(|| song.get("mid"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Ok(lyric_data) = client
            .get("https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg")
            .header(REFERER, "https://y.qq.com/")
            .query(&[("format", "json"), ("nobase64", "1"), ("songmid", song_mid)])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = lyric_data.json::<Value>().await else {
            continue;
        };
        let Some(lyrics) = parse_qq_lyric_payload(&lyric_data) else {
            continue;
        };

        results.push(OnlineLyricsCandidate {
            id: format!("qq-{song_mid}"),
            source: "QQ音乐".into(),
            title: value_string(song, "songname")
                .or_else(|| value_string(song, "title"))
                .unwrap_or_else(|| query.into()),
            artist: qq_singers(song),
            album: value_string(song, "albumname"),
            duration: provider_duration_ms(song).map(|ms| ms / 1000),
            lyrics,
        });
    }

    results
}

fn parse_netease_lyric_payload(payload: &Value) -> Option<Vec<LyricLine>> {
    if let Some(yrc) = payload
        .get("yrc")
        .and_then(|value| value.get("lyric"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let lyrics = parse_lyrics_bytes(yrc.as_bytes());
        if !lyrics.is_empty() {
            return Some(lyrics);
        }
    }

    let mut lyrics = Vec::new();
    if let Some(lrc) = payload
        .get("lrc")
        .and_then(|value| value.get("lyric"))
        .and_then(Value::as_str)
    {
        lyrics.extend(parse_lyrics_text(lrc));
    }
    if let Some(tlyric) = payload
        .get("tlyric")
        .and_then(|value| value.get("lyric"))
        .and_then(Value::as_str)
    {
        lyrics.extend(parse_lyrics_text(tlyric));
    }
    normalize_lyric_lines(lyrics)
}

fn parse_qq_lyric_payload(payload: &Value) -> Option<Vec<LyricLine>> {
    let mut lyrics = Vec::new();
    if let Some(lyric) = payload.get("lyric").and_then(Value::as_str) {
        lyrics.extend(parse_online_lyric_text(lyric));
    }
    if let Some(trans) = payload.get("trans").and_then(Value::as_str) {
        lyrics.extend(parse_online_lyric_text(trans));
    }
    normalize_lyric_lines(lyrics)
}

fn parse_online_lyric_text(value: &str) -> Vec<LyricLine> {
    let compact = value.trim();
    if compact.contains('[') && compact.contains(']') {
        let lyrics = parse_lyrics_text(compact);
        if !lyrics.is_empty() {
            return lyrics;
        }
    }

    let Ok(decoded) = BASE64_STANDARD.decode(compact) else {
        return parse_lyrics_text(compact);
    };
    let text = decode_lyric_bytes(&decoded);
    parse_lyrics_text(&text)
}

fn normalize_lyric_lines(mut lyrics: Vec<LyricLine>) -> Option<Vec<LyricLine>> {
    lyrics.retain(|line| !line.text.trim().is_empty());
    lyrics.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn ranked_provider_items(items: &[Value], duration: u64) -> Vec<&Value> {
    let target_ms = duration.saturating_mul(1000);
    let mut ranked = items.iter().collect::<Vec<_>>();
    ranked.sort_by_key(|item| {
        provider_duration_ms(item)
            .map(|item_ms| item_ms.abs_diff(target_ms))
            .unwrap_or(u64::MAX)
    });
    ranked
}

fn dedupe_online_lyrics_candidates(
    candidates: Vec<OnlineLyricsCandidate>,
) -> Vec<OnlineLyricsCandidate> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for candidate in candidates {
        if candidate.lyrics.is_empty() {
            continue;
        }

        // L-5: 用「行数 + 总字符 + 前 3 行 hash + 时长」作为指纹，
        // 同首歌不同来源即使翻译字段不同也能识别为同一份。
        let mut hasher = DefaultHasher::new();
        candidate.lyrics.len().hash(&mut hasher);
        let total_chars: usize = candidate.lyrics.iter().map(|l| l.text.chars().count()).sum();
        total_chars.hash(&mut hasher);
        for line in candidate.lyrics.iter().take(3) {
            normalize_text(&line.text).hash(&mut hasher);
        }
        candidate.duration.unwrap_or_default().hash(&mut hasher);
        let key = hasher.finish();
        if seen.insert(key) {
            deduped.push(candidate);
        }
    }

    deduped
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn value_string(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn netease_artists(song: &Value) -> String {
    song.get("artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .filter_map(|artist| value_string(artist, "name"))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn qq_singers(song: &Value) -> String {
    song.get("singer")
        .and_then(Value::as_array)
        .map(|singers| {
            singers
                .iter()
                .filter_map(|singer| value_string(singer, "name"))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn provider_duration_ms(item: &Value) -> Option<u64> {
    for key in ["duration", "interval", "dt", "song_duration"] {
        if let Some(value) = item.get(key).and_then(Value::as_u64) {
            return Some(if value < 10_000 { value * 1000 } else { value });
        }
        if let Some(value) = item.get(key).and_then(Value::as_str) {
            let parsed = value.parse::<u64>().ok()?;
            return Some(if parsed < 10_000 {
                parsed * 1000
            } else {
                parsed
            });
        }
    }
    None
}

const MAX_IMPORT_RECURSION_DEPTH: usize = 64;

fn collect_audio_files(
    path: PathBuf,
    tracks: &mut Vec<ImportedTrack>,
    seen_files: &mut HashSet<String>,
    visited_dirs: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if path.is_dir() {
        // L-14：symlink / Windows junction 可能指向祖先目录导致无限递归栈溢出。
        // 用 canonicalize 后的真实路径去重 + 递归深度上限双重防护。
        if depth >= MAX_IMPORT_RECURSION_DEPTH {
            return Ok(());
        }
        let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !visited_dirs.insert(real) {
            return Ok(());
        }

        let entries = fs::read_dir(&path)
            .map_err(|err| format!("failed to read directory {}: {err}", path.display()))?;

        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            collect_audio_files(entry.path(), tracks, seen_files, visited_dirs, depth + 1)?;
        }
        return Ok(());
    }

    if path.is_file() && is_audio_file(&path) {
        let key = import_dedupe_key(&path);
        if seen_files.insert(key) {
            tracks.push(track_from_path(&path)?);
        }
    }

    Ok(())
}

fn read_cached_tracks(app: &AppHandle) -> Result<Vec<ImportedTrack>, String> {
    let path = library_cache_path(app)?;
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read library cache {}: {err}", path.display()))?;
    let tracks: Vec<ImportedTrack> = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse library cache {}: {err}", path.display()))?;
    Ok(tracks.into_iter().map(enrich_cached_track).collect())
}

fn write_cached_tracks(app: &AppHandle, tracks: &[ImportedTrack]) -> Result<(), String> {
    let path = library_cache_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create library cache dir {}: {err}",
                parent.display()
            )
        })?;
    }
    let bytes = serde_json::to_vec_pretty(tracks)
        .map_err(|err| format!("failed to serialize library cache: {err}"))?;
    fs::write(&path, bytes)
        .map_err(|err| format!("failed to write library cache {}: {err}", path.display()))
}

pub(super) fn merge_tracks_into_cache(
    app: &AppHandle,
    tracks: &[ImportedTrack],
) -> Result<Vec<ImportedTrack>, String> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }

    let cached = read_cached_tracks(app).unwrap_or_default();
    let merged = merge_cached_tracks(cached, tracks);
    write_cached_tracks(app, &merged)?;
    Ok(imported_tracks_from_cache(&merged, tracks))
}

pub(super) fn mark_tracks_cache_missing_by_paths(
    app: &AppHandle,
    removed_paths: &[PathBuf],
) -> Result<(), String> {
    if removed_paths.is_empty() {
        return Ok(());
    }

    let removed = removed_paths
        .iter()
        .map(|path| import_dedupe_key(path))
        .collect::<HashSet<_>>();
    let tracks = read_cached_tracks(app).unwrap_or_default();
    let updated = tracks
        .into_iter()
        .map(|mut track| {
            // 缓存清理传来的是 *实际磁盘文件路径*。
            // 之前用 `import_track_key(&track)` 比对，但对于 Bilibili 曲目
            // 它返回的是 `source-id:bv...` 而非文件路径，永远无法命中。
            // 改用 track.path 直接归一化，确保物理路径匹配。
            let track_key = import_dedupe_key(Path::new(&track.path));
            if removed.contains(&track_key) && track.source_url.is_some() {
                track.cache_missing = true;
                track.size = "0 MB".into();
            }
            track
        })
        .collect::<Vec<_>>();
    write_cached_tracks(app, &updated)
}

fn library_cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("library-cache.json"))
}

fn merge_cached_tracks(
    mut cached: Vec<ImportedTrack>,
    imported: &[ImportedTrack],
) -> Vec<ImportedTrack> {
    let mut index_by_key = HashMap::with_capacity(cached.len());
    for (index, track) in cached.iter().enumerate() {
        index_by_key.insert(import_track_key(track), index);
    }

    for track in imported {
        let key = import_track_key(track);
        if let Some(index) = index_by_key.get(&key).copied() {
            cached[index] = merge_imported_track(&cached[index], track);
        } else {
            index_by_key.insert(key, cached.len());
            cached.push(track.clone());
        }
    }

    dedupe_cached_tracks(cached)
}

fn dedupe_cached_tracks(tracks: Vec<ImportedTrack>) -> Vec<ImportedTrack> {
    let mut output: Vec<ImportedTrack> = Vec::new();
    let mut index_by_key: HashMap<String, usize> = HashMap::new();

    for track in tracks {
        let key = import_track_key(&track);
        if let Some(index) = index_by_key.get(&key).copied() {
            let existing = &output[index];
            output[index] = if existing.cache_missing && !track.cache_missing {
                merge_imported_track(existing, &track)
            } else {
                merge_imported_track(&track, existing)
            };
        } else {
            index_by_key.insert(key, output.len());
            output.push(track);
        }
    }

    output
}

fn merge_imported_track(cached: &ImportedTrack, imported: &ImportedTrack) -> ImportedTrack {
    let mut merged = imported.clone();
    if merged.lyrics.is_empty() && !cached.lyrics.is_empty() {
        merged.lyrics = cached.lyrics.clone();
    }
    merged
}

fn enrich_cached_track(mut track: ImportedTrack) -> ImportedTrack {
    if track.source_url.is_none() && track.id.starts_with("bilibili-") {
        if let Some(source_id) = track
            .source_id
            .clone()
            .or_else(|| bilibili_source_id_from_path(&track.path))
        {
            track.source_url = Some(format!("https://www.bilibili.com/video/{source_id}"));
            track.source_id = Some(source_id);
        }
    }
    track
}

fn bilibili_source_id_from_path(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?.trim();
    let (source_id, _) = stem.rsplit_once('-')?;
    if source_id.len() >= 12 && source_id.get(..2)?.eq_ignore_ascii_case("BV") {
        Some(source_id.to_string())
    } else {
        None
    }
}

fn imported_tracks_from_cache(
    cached: &[ImportedTrack],
    imported: &[ImportedTrack],
) -> Vec<ImportedTrack> {
    let cached_by_key = cached
        .iter()
        .map(|track| (import_track_key(track), track.clone()))
        .collect::<HashMap<_, _>>();

    imported
        .iter()
        .map(|track| {
            cached_by_key
                .get(&import_track_key(track))
                .cloned()
                .unwrap_or_else(|| track.clone())
        })
        .collect()
}

fn apply_track_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    lyrics: Vec<LyricLine>,
    track_path: Option<&str>,
) -> Result<(), String> {
    let index = ensure_track_for_lyrics(tracks, track_id, track_path)?;
    let track = &mut tracks[index];
    track.lyrics = lyrics;
    Ok(())
}

fn ensure_track_for_lyrics(
    tracks: &mut Vec<ImportedTrack>,
    track_id: &str,
    track_path: Option<&str>,
) -> Result<usize, String> {
    if let Some(index) = tracks.iter().position(|track| track.id == track_id) {
        return Ok(index);
    }

    let Some(track_path) = track_path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Err("track was not found in the library cache".into());
    };

    let path = PathBuf::from(track_path);
    if !path.is_file() {
        return Err(
            "track is not in the library cache and the audio file is unavailable; re-import the audio file first"
                .into(),
        );
    }

    let mut track = track_from_path(&path)?;
    track.id = track_id.to_string();
    tracks.push(track);
    Ok(tracks.len() - 1)
}

fn import_track_key(track: &ImportedTrack) -> String {
    if let Some(source_id) = track
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("source-id:{}", source_id.to_ascii_lowercase());
    }
    if let Some(source_url) = track
        .source_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("source-url:{}", source_url.to_ascii_lowercase());
    }
    import_dedupe_key(Path::new(&track.path))
}

fn import_dedupe_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase()
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some()
        || audio_format_from_magic(path).is_some()
}

fn audio_format_label(path: &Path) -> String {
    audio_format_from_magic(path)
        .or_else(|| {
            path.extension()
                .and_then(|value| value.to_str())
                .and_then(audio_format_from_extension)
        })
        .unwrap_or("AUDIO")
        .to_string()
}

fn audio_format_from_magic(path: &Path) -> Option<&'static str> {
    let mut file = fs::File::open(path).ok()?;
    let mut header = [0_u8; 64];
    let read = file.read(&mut header).ok()?;
    audio_format_from_header(&header[..read])
}

fn audio_format_from_header(header: &[u8]) -> Option<&'static str> {
    if header.starts_with(b"DSD ") {
        return Some("DSF");
    }
    if header.len() >= 16 && &header[0..4] == b"FRM8" && &header[12..16] == b"DSD " {
        return Some("DFF");
    }
    if header.starts_with(b"fLaC") {
        return Some("FLAC");
    }
    if header.len() >= 12 && &header[0..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        return Some("WAV");
    }
    if header.len() >= 12
        && &header[0..4] == b"FORM"
        && (&header[8..12] == b"AIFF" || &header[8..12] == b"AIFC")
    {
        return Some("AIFF");
    }
    if header.starts_with(b"ID3") || is_mpeg_audio_header(header) {
        return Some("MP3");
    }
    if header.starts_with(b"OggS") {
        return Some(if header.windows(8).any(|window| window == b"OpusHead") {
            "OPUS"
        } else {
            "OGG"
        });
    }
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        return Some(if header.windows(4).any(|window| window == b"alac") {
            "ALAC"
        } else {
            "M4A"
        });
    }
    if is_adts_header(header) {
        return Some("AAC");
    }

    None
}

fn audio_format_from_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "aac" => Some("AAC"),
        "aif" | "aiff" => Some("AIFF"),
        "alac" => Some("ALAC"),
        "dff" => Some("DFF"),
        "dsf" => Some("DSF"),
        "flac" => Some("FLAC"),
        "m4a" => Some("M4A"),
        "mp3" => Some("MP3"),
        "ogg" => Some("OGG"),
        "opus" => Some("OPUS"),
        "wav" => Some("WAV"),
        "wma" => Some("WMA"),
        _ => None,
    }
}

fn is_mpeg_audio_header(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && (header[1] & 0xe0) == 0xe0 && (header[1] & 0x06) != 0
}

fn is_adts_header(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && (header[1] & 0xf0) == 0xf0
}

fn is_dsd_format(format: &str) -> bool {
    format.eq_ignore_ascii_case("DSF") || format.eq_ignore_ascii_case("DFF")
}

fn track_from_path(path: &Path) -> Result<ImportedTrack, String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("failed to read file metadata {}: {err}", path.display()))?;
    let path_string = path.to_string_lossy().to_string();
    let mut hasher = DefaultHasher::new();
    path_string.to_ascii_lowercase().hash(&mut hasher);
    let hash = hasher.finish();
    // L-2: 一次性探测 magic（带最多 64 字节读取），后续 format/is_dsd 沿用结果。
    let magic_format = audio_format_from_magic(path);
    let ext_format = path
        .extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension);
    let format = magic_format
        .or(ext_format)
        .unwrap_or("AUDIO")
        .to_string();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Unknown Track");
    let is_dsd = magic_format
        .map(is_dsd_format)
        .unwrap_or_else(|| ext_format.is_some_and(is_dsd_format));
    let audio_metadata = parse_audio_metadata_with_dsd_hint(path, is_dsd);
    let lyrics = external_lrc_lyrics(path).unwrap_or_else(|| audio_metadata.lyrics.clone());
    let filename_metadata = parse_filename_metadata(stem);
    let title = audio_metadata
        .title
        .or(filename_metadata.title)
        .unwrap_or_else(|| clean_metadata_text(stem).unwrap_or_else(|| "Unknown Track".into()));
    let artist = audio_metadata
        .artist
        .or(filename_metadata.artist)
        .unwrap_or_else(|| "Unknown Artist".into());
    let album = audio_metadata
        .album
        .or(filename_metadata.album)
        .unwrap_or_else(|| "Local Files".into());
    let size = format_file_size(metadata.len());
    let (glow1, glow2) = color_pair(hash);

    Ok(ImportedTrack {
        id: format!("local-{hash:016x}"),
        title,
        artist,
        album,
        album_year: audio_metadata.album_year,
        cover: String::new(),
        format: format.clone(),
        bitdepth: format_audio_quality(
            &format,
            audio_metadata.bit_depth,
            audio_metadata.sample_rate,
        ),
        sample_rate: format_sample_rate(&format, audio_metadata.sample_rate),
        bitrate: format_bitrate(audio_metadata.bitrate),
        channels: format_channels(audio_metadata.channels),
        size,
        path: path_string,
        source_url: None,
        source_id: None,
        cache_missing: false,
        duration: audio_metadata.duration.unwrap_or(0),
        glow_color: glow1.clone(),
        glow1,
        glow2,
        lyrics,
    })
}

fn parse_audio_metadata_with_dsd_hint(path: &Path, is_dsd_hint: bool) -> ParsedAudioMetadata {
    let Ok(tagged_file) = lofty::read_from_path(path) else {
        let mut parsed = ParsedAudioMetadata::default();
        enrich_with_decoder_probe_dsd(path, &mut parsed, is_dsd_hint);
        return parsed;
    };

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs();

    let mut parsed = ParsedAudioMetadata {
        duration: (duration > 0).then_some(duration),
        bitrate: properties
            .audio_bitrate()
            .or_else(|| properties.overall_bitrate()),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth(),
        channels: properties.channels(),
        ..ParsedAudioMetadata::default()
    };

    if let Some(tag) = tag {
        parsed.title = tag
            .title()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.artist = tag
            .artist()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album = tag
            .album()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album_year = tag
            .date()
            .and_then(|value| (value.year > 0).then(|| value.year.to_string()));
    }

    parsed.lyrics = lyrics_from_tags(tagged_file.tags());
    enrich_with_decoder_probe_dsd(path, &mut parsed, is_dsd_hint);

    parsed
}

fn enrich_with_decoder_probe_dsd(
    path: &Path,
    parsed: &mut ParsedAudioMetadata,
    is_dsd_hint: bool,
) {
    if parsed.duration.is_some()
        && parsed.bit_depth.is_some()
        && parsed.sample_rate.is_some()
        && parsed.channels.is_some()
        && !is_dsd_hint
    {
        return;
    }

    let Ok(info) = probe_stream_info(path) else {
        return;
    };

    if parsed.duration.is_none() && info.duration_seconds > 0.0 {
        parsed.duration = Some(info.duration_seconds.round() as u64);
    }
    if parsed.bit_depth.is_none() && info.bit_depth.0 <= u8::MAX as u16 {
        parsed.bit_depth = Some(info.bit_depth.0 as u8);
    }
    if parsed.sample_rate.is_none() && info.sample_rate.0 > 0 {
        parsed.sample_rate = Some(info.sample_rate.0);
    }
    if parsed.channels.is_none() && info.channels.0 <= u8::MAX as u16 {
        parsed.channels = Some(info.channels.0 as u8);
    }
}

fn parse_audio_metadata(path: &Path) -> ParsedAudioMetadata {
    let Ok(tagged_file) = lofty::read_from_path(path) else {
        let mut parsed = ParsedAudioMetadata::default();
        enrich_with_decoder_probe(path, &mut parsed);
        return parsed;
    };

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs();

    let mut parsed = ParsedAudioMetadata {
        duration: (duration > 0).then_some(duration),
        bitrate: properties
            .audio_bitrate()
            .or_else(|| properties.overall_bitrate()),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth(),
        channels: properties.channels(),
        ..ParsedAudioMetadata::default()
    };

    if let Some(tag) = tag {
        parsed.title = tag
            .title()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.artist = tag
            .artist()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album = tag
            .album()
            .and_then(|value| clean_metadata_text(value.as_ref()));
        parsed.album_year = tag
            .date()
            .and_then(|value| (value.year > 0).then(|| value.year.to_string()));
    }

    parsed.lyrics = lyrics_from_tags(tagged_file.tags());
    enrich_with_decoder_probe(path, &mut parsed);

    parsed
}

fn enrich_with_decoder_probe(path: &Path, parsed: &mut ParsedAudioMetadata) {
    if parsed.duration.is_some()
        && parsed.bit_depth.is_some()
        && parsed.sample_rate.is_some()
        && parsed.channels.is_some()
        && !is_dsd_file(path)
    {
        return;
    }

    let Ok(info) = probe_stream_info(path) else {
        return;
    };

    if parsed.duration.is_none() && info.duration_seconds > 0.0 {
        parsed.duration = Some(info.duration_seconds.round() as u64);
    }
    if parsed.bit_depth.is_none() && info.bit_depth.0 <= u8::MAX as u16 {
        parsed.bit_depth = Some(info.bit_depth.0 as u8);
    }
    if parsed.sample_rate.is_none() && info.sample_rate.0 > 0 {
        parsed.sample_rate = Some(info.sample_rate.0);
    }
    if parsed.channels.is_none() && info.channels.0 <= u8::MAX as u16 {
        parsed.channels = Some(info.channels.0 as u8);
    }
}

#[cfg(test)]
fn write_test_dsf(path: &Path) {
    use std::io::Write;

    let channels = 2_u32;
    let dsd_rate = 2_822_400_u32;
    let sample_count = dsd_rate as u64;
    let block_size_per_channel = 8_u32;
    let data_len = channels as u64 * block_size_per_channel as u64;
    let file_size = 28_u64 + 52 + 12 + data_len;

    let mut file = fs::File::create(path).expect("create dsf");
    file.write_all(b"DSD ").unwrap();
    file.write_all(&28_u64.to_le_bytes()).unwrap();
    file.write_all(&file_size.to_le_bytes()).unwrap();
    file.write_all(&0_u64.to_le_bytes()).unwrap();

    file.write_all(b"fmt ").unwrap();
    file.write_all(&52_u64.to_le_bytes()).unwrap();
    file.write_all(&1_u32.to_le_bytes()).unwrap();
    file.write_all(&0_u32.to_le_bytes()).unwrap();
    file.write_all(&2_u32.to_le_bytes()).unwrap();
    file.write_all(&channels.to_le_bytes()).unwrap();
    file.write_all(&dsd_rate.to_le_bytes()).unwrap();
    file.write_all(&1_u32.to_le_bytes()).unwrap();
    file.write_all(&sample_count.to_le_bytes()).unwrap();
    file.write_all(&block_size_per_channel.to_le_bytes())
        .unwrap();
    file.write_all(&0_u32.to_le_bytes()).unwrap();

    file.write_all(b"data").unwrap();
    file.write_all(&(12_u64 + data_len).to_le_bytes()).unwrap();
    file.write_all(&[0xff; 8]).unwrap();
    file.write_all(&[0x00; 8]).unwrap();
}

#[cfg(test)]
fn temp_audio_path(prefix: &str, extension: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
}

fn is_dsd_file(path: &Path) -> bool {
    if let Some(format) = audio_format_from_magic(path) {
        return is_dsd_format(format);
    }

    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some_and(is_dsd_format)
}

fn external_lrc_lyrics(path: &Path) -> Option<Vec<LyricLine>> {
    let lyrics_path = find_lyrics_file(path)?;
    let bytes = fs::read(lyrics_path).ok()?;
    let lyrics = parse_lyrics_bytes(&bytes);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn find_lyrics_file(path: &Path) -> Option<PathBuf> {
    for extension in ["lrc", "qrc", "krc", "yrc"] {
        let exact = path.with_extension(extension);
        if exact.is_file() {
            return Some(exact);
        }
    }

    let expected_stem = path.file_stem()?.to_string_lossy().to_lowercase();
    let parent = path.parent()?;
    let entries = fs::read_dir(parent).ok()?;

    for entry in entries.flatten() {
        let candidate = entry.path();
        let is_lyrics = candidate
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| {
                ["lrc", "qrc", "krc", "yrc"]
                    .iter()
                    .any(|lyrics_ext| ext.eq_ignore_ascii_case(lyrics_ext))
            });
        let same_stem = candidate
            .file_stem()
            .map(|value| value.to_string_lossy().to_lowercase() == expected_stem)
            .unwrap_or(false);

        if is_lyrics && same_stem {
            return Some(candidate);
        }
    }

    None
}

const QRC_MAGIC_HEADER: &[u8] = b"\x98%\xb0\xac\xe3\x02\x83h\xe8\xfcl";
const KRC_MAGIC_HEADER: &[u8] = b"krc18";
const QRC_KEY: &[u8] = b"!@#)(*$%123ZXC!@!@#)(NHL";
const KRC_KEY: &[u8] = b"@Gaw^2tGQ61-\xce\xd2ni";
const QMC1_PRIVKEY: [u8; 128] = [
    0xc3, 0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52, 0xd8, 0xa1, 0x66, 0x62, 0x9f, 0x5b, 0x09, 0x00,
    0xc3, 0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e, 0xd8, 0x92, 0x3f, 0xbc, 0x90, 0xbb, 0x74, 0x0e,
    0xc3, 0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51, 0xd8, 0xf4, 0x11, 0x84, 0x9f, 0xde, 0x95, 0x1d,
    0xc3, 0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9, 0xd8, 0xf0, 0xf7, 0xa0, 0x90, 0xa1, 0xd6, 0xf3,
    0xc3, 0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0, 0xd8, 0xf9, 0x66, 0xfa, 0x9f, 0xd5, 0x09, 0xc6,
    0xc3, 0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4, 0xd8, 0x51, 0x3f, 0xaa, 0x90, 0x3d, 0x74, 0x47,
    0xc3, 0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92, 0xd8, 0x7e, 0x11, 0x13, 0x9f, 0x23, 0x95, 0x5e,
    0xc3, 0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1, 0xd8, 0x52, 0xf7, 0x67, 0x90, 0xca, 0xd6, 0x4a,
];

fn parse_lyrics_bytes(bytes: &[u8]) -> Vec<LyricLine> {
    if bytes.starts_with(QRC_MAGIC_HEADER) {
        if let Some(lyrics) = parse_encrypted_qrc_lyrics(bytes) {
            return lyrics;
        }
    }

    if bytes.starts_with(KRC_MAGIC_HEADER) {
        if let Some(lyrics) = parse_encrypted_krc_lyrics(bytes) {
            return lyrics;
        }
    }

    let text = decode_lyric_bytes(bytes);
    let provider_lyrics = parse_provider_lyrics_text(&text);
    if !provider_lyrics.is_empty() {
        return provider_lyrics;
    }

    parse_lyrics_text(&text)
}

fn parse_encrypted_qrc_lyrics(bytes: &[u8]) -> Option<Vec<LyricLine>> {
    let text = decrypt_qrc(bytes).ok()?;
    let lyrics = parse_qrc_text(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn parse_encrypted_krc_lyrics(bytes: &[u8]) -> Option<Vec<LyricLine>> {
    let text = decrypt_krc(bytes).ok()?;
    let lyrics = parse_krc_text(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn decrypt_qrc(bytes: &[u8]) -> Result<String, String> {
    let mut data = bytes.to_vec();
    qmc1_decrypt(&mut data);
    let encrypted = data
        .get(QRC_MAGIC_HEADER.len()..)
        .ok_or_else(|| "invalid qrc data".to_string())?;
    if encrypted.len() % 8 != 0 {
        return Err("invalid qrc block length".into());
    }

    let cipher = TdesEde3::new_from_slice(QRC_KEY).map_err(|err| err.to_string())?;
    let mut decrypted = Vec::with_capacity(encrypted.len());
    for chunk in encrypted.chunks_exact(8) {
        let mut block = *GenericArray::from_slice(chunk);
        cipher.decrypt_block(&mut block);
        decrypted.extend_from_slice(&block);
    }

    inflate_zlib_utf8(&decrypted)
}

fn decrypt_krc(bytes: &[u8]) -> Result<String, String> {
    let encrypted = bytes
        .get(4..)
        .ok_or_else(|| "invalid krc data".to_string())?;
    let decrypted = encrypted
        .iter()
        .enumerate()
        .map(|(index, value)| value ^ KRC_KEY[index % KRC_KEY.len()])
        .collect::<Vec<_>>();

    inflate_zlib_utf8(&decrypted)
}

fn qmc1_decrypt(data: &mut [u8]) {
    for (index, value) in data.iter_mut().enumerate() {
        let key_index = if index > 0x7fff {
            (index % 0x7fff) & 0x7f
        } else {
            index & 0x7f
        };
        *value ^= QMC1_PRIVKEY[key_index];
    }
}

fn inflate_zlib_utf8(bytes: &[u8]) -> Result<String, String> {
    // 防御 zlib bomb：解压超过 8MB 即视为异常输入。
    // 正常歌词解压后通常 < 100 KB；保留一个安全余量。
    const MAX_INFLATED_BYTES: u64 = 8 * 1024 * 1024;
    let decoder = ZlibDecoder::new(bytes);
    let mut limited = decoder.take(MAX_INFLATED_BYTES);
    let mut text = String::new();
    limited
        .read_to_string(&mut text)
        .map_err(|err| err.to_string())?;
    // 命中上限：极有可能是 zlib bomb，拒绝继续。
    if text.len() as u64 >= MAX_INFLATED_BYTES {
        return Err(format!(
            "lyrics inflated payload exceeds {MAX_INFLATED_BYTES} bytes; rejected"
        ));
    }
    Ok(text)
}

fn parse_provider_lyrics_text(text: &str) -> Vec<LyricLine> {
    let qrc_lyrics = parse_qrc_text(text);
    if !qrc_lyrics.is_empty() {
        return qrc_lyrics;
    }

    if text.contains("<") {
        let krc_lyrics = parse_krc_text(text);
        if !krc_lyrics.is_empty() {
            return krc_lyrics;
        }
    }

    if contains_tuple_marker(text, '(', ')', 3) {
        let yrc_lyrics = parse_yrc_text(text);
        if !yrc_lyrics.is_empty() {
            return yrc_lyrics;
        }
    }

    if contains_tuple_marker(text, '(', ')', 2) {
        let qrc_content_lyrics = provider_lines_to_lyrics(parse_qrc_content(text));
        if !qrc_content_lyrics.is_empty() {
            return qrc_content_lyrics;
        }
    }

    Vec::new()
}

fn parse_qrc_text(text: &str) -> Vec<LyricLine> {
    let Some(content) = extract_qrc_lyric_content(text) else {
        return Vec::new();
    };
    provider_lines_to_lyrics(parse_qrc_content(&decode_xml_entities(&content)))
}

fn parse_qrc_content(text: &str) -> Vec<ProviderLyricLine> {
    parse_timed_provider_lines(text, qrc_line_text)
}

fn parse_krc_text(text: &str) -> Vec<LyricLine> {
    let mut language_tag = None;
    let mut original = Vec::new();

    for raw_line in normalized_lyric_lines(text) {
        let line = raw_line.trim();
        if line.is_empty() || !line.starts_with('[') {
            continue;
        }

        if let Some((key, value)) = split_metadata_tag(line) {
            if key.eq_ignore_ascii_case("language") {
                language_tag = Some(value.to_string());
            }
            continue;
        }

        let Some((start_ms, _, body)) = split_provider_timed_line(line) else {
            continue;
        };
        if let Some(text) = clean_lyric_text(&tagged_line_text(body, '<', '>', 3)) {
            original.push(ProviderLyricLine { start_ms, text });
        }
    }

    let mut lyrics = provider_lines_to_lyrics(original.clone());
    if let Some(language_tag) = language_tag {
        lyrics.extend(parse_krc_translation_lines(&language_tag, &original));
        lyrics.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    }

    lyrics
}

fn parse_yrc_text(text: &str) -> Vec<LyricLine> {
    provider_lines_to_lyrics(parse_timed_provider_lines(text, |body| {
        tagged_line_text(body, '(', ')', 3)
    }))
}

fn parse_timed_provider_lines(
    text: &str,
    body_to_text: impl Fn(&str) -> String,
) -> Vec<ProviderLyricLine> {
    normalized_lyric_lines(text)
        .filter_map(|raw_line| {
            let line = raw_line.trim();
            let (start_ms, _, body) = split_provider_timed_line(line)?;
            clean_lyric_text(&body_to_text(body)).map(|text| ProviderLyricLine { start_ms, text })
        })
        .collect()
}

fn provider_lines_to_lyrics(mut lines: Vec<ProviderLyricLine>) -> Vec<LyricLine> {
    lines.sort_by_key(|line| line.start_ms);
    let mut lyrics = lines
        .into_iter()
        .map(|line| LyricLine {
            time: line.start_ms as f64 / 1000.0,
            text: line.text,
        })
        .collect::<Vec<_>>();
    lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    lyrics
}

fn normalized_lyric_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .flat_map(|line| line.split('\r'))
        .map(|line| line.trim_start_matches('\u{feff}'))
}

fn split_provider_timed_line(line: &str) -> Option<(u64, u64, &str)> {
    let stripped = line.strip_prefix('[')?;
    let end = stripped.find(']')?;
    let (start, duration) = stripped[..end].split_once(',')?;
    if !start.chars().all(|ch| ch.is_ascii_digit())
        || !duration.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }

    Some((
        start.parse().ok()?,
        duration.parse().ok()?,
        &stripped[end + 1..],
    ))
}

fn split_metadata_tag(line: &str) -> Option<(&str, &str)> {
    let content = lrc_tag_content(line)?;
    let (key, value) = content.split_once(':')?;
    if key.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '_') {
        Some((key.trim(), value.trim()))
    } else {
        None
    }
}

fn extract_qrc_lyric_content(text: &str) -> Option<String> {
    let pattern =
        Regex::new(r#"(?s)<Lyric_1\s+[^>]*LyricContent="(?P<content>.*?)"[^>]*/?>"#).ok()?;
    pattern
        .captures(text)
        .and_then(|captures| captures.name("content"))
        .map(|content| content.as_str().to_string())
}

fn qrc_line_text(body: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    let mut matched = false;

    while let Some(relative_open) = body[cursor..].find('(') {
        let open = cursor + relative_open;
        let Some(relative_close) = body[open + 1..].find(')') else {
            break;
        };
        let close = open + 1 + relative_close;
        let token = &body[open + 1..close];
        if is_numeric_tuple(token, 2) {
            let content = strip_provider_prefix_timestamp(&body[cursor..open]);
            output.push_str(content);
            matched = true;
            cursor = close + 1;
        } else {
            cursor = open + 1;
        }
    }

    if matched {
        output
    } else {
        body.to_string()
    }
}

fn tagged_line_text(body: &str, open: char, close: char, tuple_len: usize) -> String {
    let markers = find_tuple_markers(body, open, close, tuple_len);
    if markers.is_empty() {
        return body.to_string();
    }

    let mut output = String::new();
    for (index, (_, marker_end)) in markers.iter().enumerate() {
        let content_start = *marker_end;
        let content_end = markers
            .get(index + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(body.len());
        output.push_str(&body[content_start..content_end]);
    }

    output
}

fn find_tuple_markers(
    value: &str,
    open: char,
    close: char,
    tuple_len: usize,
) -> Vec<(usize, usize)> {
    let mut markers = Vec::new();
    let mut cursor = 0;
    let open_len = open.len_utf8();
    let close_len = close.len_utf8();

    while let Some(relative_open) = value[cursor..].find(open) {
        let start = cursor + relative_open;
        let token_start = start + open_len;
        let Some(relative_close) = value[token_start..].find(close) else {
            break;
        };
        let end = token_start + relative_close;
        if is_numeric_tuple(&value[token_start..end], tuple_len) {
            markers.push((start, end + close_len));
            cursor = end + close_len;
        } else {
            cursor = token_start;
        }
    }

    markers
}

fn is_numeric_tuple(token: &str, expected_len: usize) -> bool {
    let parts = token.split(',').collect::<Vec<_>>();
    parts.len() == expected_len
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn contains_tuple_marker(value: &str, open: char, close: char, tuple_len: usize) -> bool {
    !find_tuple_markers(value, open, close, tuple_len).is_empty()
}

fn strip_provider_prefix_timestamp(value: &str) -> &str {
    let trimmed = value.trim_start();
    let Some(stripped) = trimmed.strip_prefix('[') else {
        return value;
    };
    let Some(end) = stripped.find(']') else {
        return value;
    };
    if is_numeric_tuple(&stripped[..end], 2) {
        stripped[end + 1..].trim_start()
    } else {
        value
    }
}

fn parse_krc_translation_lines(
    language_tag: &str,
    original: &[ProviderLyricLine],
) -> Vec<LyricLine> {
    let Ok(decoded) = BASE64_STANDARD.decode(language_tag.trim()) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_slice::<Value>(&decoded) else {
        return Vec::new();
    };

    json.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|language| language.get("type").and_then(Value::as_i64) == Some(1))
        .flat_map(|language| {
            language
                .get("lyricContent")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, line)| {
                    let original_line = original.get(index)?;
                    let text = line
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    clean_lyric_text(&text).map(|text| LyricLine {
                        time: original_line.start_ms as f64 / 1000.0,
                        text,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn decode_xml_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let after_amp = &rest[start + 1..];
        let Some(end) = after_amp.find(';') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let entity = &after_amp[..end];
        if let Some(decoded) = decode_xml_entity(entity) {
            output.push(decoded);
        } else {
            output.push('&');
            output.push_str(entity);
            output.push(';');
        }
        rest = &after_amp[end + 1..];
    }

    output.push_str(rest);
    output
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn decode_lyric_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if looks_like_utf16_le(bytes) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if looks_like_utf16_be(bytes) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let (text, _, _) = GBK.decode(bytes);
    text.into_owned()
}

fn looks_like_utf16_le(bytes: &[u8]) -> bool {
    looks_like_utf16(bytes, 1)
}

fn looks_like_utf16_be(bytes: &[u8]) -> bool {
    looks_like_utf16(bytes, 0)
}

fn looks_like_utf16(bytes: &[u8], zero_offset: usize) -> bool {
    if bytes.len() < 8 || !bytes.len().is_multiple_of(2) {
        return false;
    }

    let pairs = bytes.len() / 2;
    let zero_count = bytes
        .chunks_exact(2)
        .filter(|pair| pair[zero_offset] == 0)
        .count();

    zero_count * 100 / pairs >= 60
}

fn lyrics_from_tags(tags: &[Tag]) -> Vec<LyricLine> {
    for tag in tags {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            for value in tag.get_strings(key) {
                let lyrics = parse_lyrics_text(value);
                if !lyrics.is_empty() {
                    return lyrics;
                }
            }
        }
    }

    Vec::new()
}

fn parse_lyrics_text(text: &str) -> Vec<LyricLine> {
    let normalized = text
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n");
    let mut offset_ms = 0_i64;
    let mut timed = Vec::new();
    let mut unsynced = Vec::new();

    for raw_line in normalized.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }

        if let Some(offset) = parse_lrc_offset(line) {
            offset_ms = offset;
            continue;
        }

        let (times, body) = split_lrc_time_tags(line);
        if !times.is_empty() {
            if let Some(text) = clean_lyric_text(body) {
                for time in times {
                    // L-9：LRC 通行约定——正 offset 让歌词提前显示（time - offset）。
                    let shifted = ((time * 1000.0).round() as i64 - offset_ms).max(0);
                    timed.push(LyricLine {
                        time: shifted as f64 / 1000.0,
                        text: text.clone(),
                    });
                }
            }
            continue;
        }

        if !is_lrc_metadata_line(line) {
            if let Some(text) = clean_lyric_text(line) {
                unsynced.push(text);
            }
        }
    }

    if !timed.is_empty() {
        timed.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        timed.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
        return timed;
    }

    unsynced
        .into_iter()
        .enumerate()
        .map(|(index, text)| LyricLine {
            time: index as f64 * 4.0,
            text,
        })
        .collect()
}

fn split_lrc_time_tags(line: &str) -> (Vec<f64>, &str) {
    let mut rest = line.trim_start();
    let mut times = Vec::new();

    while let Some(stripped) = rest.strip_prefix('[') {
        let Some(end) = stripped.find(']') else {
            break;
        };
        let token = &stripped[..end];
        let Some(time) = parse_lrc_time_token(token) else {
            break;
        };

        times.push(time);
        rest = stripped[end + 1..].trim_start();
    }

    (times, rest)
}

fn parse_lrc_offset(line: &str) -> Option<i64> {
    let content = lrc_tag_content(line)?;
    let (key, value) = content.split_once(':')?;
    if !key.trim().eq_ignore_ascii_case("offset") {
        return None;
    }

    value.trim().parse::<i64>().ok()
}

fn parse_lrc_time_token(token: &str) -> Option<f64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    if !token.contains(':') {
        return parse_millisecond_lrc_token(token);
    }

    let normalized = token.replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0, minutes.parse::<u64>().ok()?, seconds),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds,
        ),
        _ => return None,
    };

    let seconds = seconds.parse::<f64>().ok()?;
    if seconds.is_nan() || seconds.is_sign_negative() {
        return None;
    }

    Some(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

fn parse_millisecond_lrc_token(token: &str) -> Option<f64> {
    let (start_ms, _) = token.split_once(',')?;
    if start_ms.is_empty() || !start_ms.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some(start_ms.parse::<u64>().ok()? as f64 / 1000.0)
}

fn is_lrc_metadata_line(line: &str) -> bool {
    let Some(content) = lrc_tag_content(line) else {
        return false;
    };
    let Some((key, _)) = content.split_once(':') else {
        return false;
    };

    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "al" | "ar" | "au" | "by" | "length" | "offset" | "re" | "ti" | "ve"
    )
}

fn clean_lyric_text(value: &str) -> Option<String> {
    let text = strip_inline_time_tags(value)
        .replace(['\u{3000}', '\t'], " ")
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .trim_matches('\0')
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    (!text.is_empty()).then_some(text)
}

fn lrc_tag_content(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
}

fn strip_inline_time_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    while let Some((start, open, close)) = find_next_time_tag_open(rest) {
        output.push_str(&rest[..start]);
        let after_open = start + open.len_utf8();

        let Some(close_at) = rest[after_open..].find(close) else {
            output.push(open);
            rest = &rest[after_open..];
            continue;
        };

        let token = &rest[after_open..after_open + close_at];
        let after_close = after_open + close_at + close.len_utf8();
        if parse_lrc_time_token(token).is_some() {
            rest = &rest[after_close..];
            continue;
        }

        output.push(open);
        rest = &rest[after_open..];
    }

    output.push_str(rest);
    output
}

fn find_next_time_tag_open(value: &str) -> Option<(usize, char, char)> {
    match (value.find('['), value.find('<')) {
        (Some(square), Some(angle)) if square <= angle => Some((square, '[', ']')),
        (Some(_), Some(angle)) => Some((angle, '<', '>')),
        (Some(square), None) => Some((square, '[', ']')),
        (None, Some(angle)) => Some((angle, '<', '>')),
        (None, None) => None,
    }
}

fn parse_filename_metadata(stem: &str) -> FilenameMetadata {
    let normalized = strip_track_number_prefix(stem);
    let parts: Vec<String> = normalized
        .split(" - ")
        .filter_map(clean_metadata_text)
        .collect();

    match parts.as_slice() {
        [artist, title] => FilenameMetadata {
            artist: Some(artist.clone()),
            title: Some(title.clone()),
            album: None,
        },
        [artist, album, title] => FilenameMetadata {
            artist: Some(artist.clone()),
            album: Some(album.clone()),
            title: Some(title.clone()),
        },
        [artist, middle @ .., title] if !middle.is_empty() => FilenameMetadata {
            artist: Some(artist.clone()),
            album: Some(middle.join(" - ")),
            title: Some(title.clone()),
        },
        [title] => FilenameMetadata {
            title: Some(title.clone()),
            ..FilenameMetadata::default()
        },
        _ => FilenameMetadata::default(),
    }
}

fn strip_track_number_prefix(value: &str) -> &str {
    let trimmed = value.trim();
    let digit_end = trimmed
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);

    if (1..=3).contains(&digit_end) {
        let rest = trimmed[digit_end..]
            .trim_start()
            .trim_start_matches(['-', '.', '_', ' '])
            .trim_start();
        if !rest.is_empty() {
            return rest;
        }
    }

    trimmed
}

fn clean_metadata_text(value: &str) -> Option<String> {
    let text = value
        .trim_matches('\0')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    (!text.is_empty()).then_some(text)
}

fn format_audio_quality(format: &str, bit_depth: Option<u8>, sample_rate: Option<u32>) -> String {
    let mut label = match bit_depth {
        Some(bits) if bits > 0 => format!("{format} {bits}-bit"),
        _ => format!("{format} Local"),
    };

    if let Some(sample_rate) = sample_rate.and_then(sample_rate_label) {
        label.push_str(" / ");
        label.push_str(&sample_rate);
        if is_dsd_format(format) {
            label.push_str(" PCM");
        }
    }

    label
}

fn format_sample_rate(format: &str, sample_rate: Option<u32>) -> String {
    match sample_rate.and_then(sample_rate_label) {
        Some(mut label) => {
            if is_dsd_format(format) {
                label.push_str(" PCM");
            }
            label
        }
        None => "Unknown".into(),
    }
}

fn sample_rate_label(sample_rate: u32) -> Option<String> {
    if sample_rate == 0 {
        return None;
    }

    if sample_rate >= 1000 {
        let mut khz = if sample_rate.is_multiple_of(1000) {
            format!("{}", sample_rate / 1000)
        } else if sample_rate.is_multiple_of(100) {
            format!("{:.1}", sample_rate as f64 / 1000.0)
        } else {
            format!("{:.3}", sample_rate as f64 / 1000.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        };
        khz.push_str(" kHz");
        return Some(khz);
    }

    Some(format!("{sample_rate} Hz"))
}

fn format_bitrate(bitrate: Option<u32>) -> String {
    match bitrate {
        Some(value) if value > 0 => format!("{value} kbps"),
        _ => "Unknown".into(),
    }
}

fn format_channels(channels: Option<u8>) -> String {
    match channels {
        Some(1) => "Mono".into(),
        Some(2) => "Stereo".into(),
        Some(6) => "5.1".into(),
        Some(8) => "7.1".into(),
        Some(value) if value > 0 => format!("{value} ch"),
        _ => "Unknown".into(),
    }
}

fn format_file_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    format!("{mb:.1} MB")
}

fn color_pair(hash: u64) -> (String, String) {
    const PAIRS: [(&str, &str); 6] = [
        ("#67e8f9", "#a5b4fc"),
        ("#7dd3fc", "#f0abfc"),
        ("#5eead4", "#93c5fd"),
        ("#f9a8d4", "#86efac"),
        ("#fde68a", "#67e8f9"),
        ("#c4b5fd", "#fda4af"),
    ];
    let pair = PAIRS[(hash as usize) % PAIRS.len()];
    (pair.0.into(), pair.1.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_artist_and_title_from_filename() {
        let parsed = parse_filename_metadata("01 - 宇多田ヒカル - First Love");

        assert_eq!(parsed.artist.as_deref(), Some("宇多田ヒカル"));
        assert_eq!(parsed.title.as_deref(), Some("First Love"));
        assert_eq!(parsed.album, None);
    }

    #[test]
    fn parses_artist_album_and_title_from_filename() {
        let parsed = parse_filename_metadata("Radiohead - OK Computer - No Surprises");

        assert_eq!(parsed.artist.as_deref(), Some("Radiohead"));
        assert_eq!(parsed.album.as_deref(), Some("OK Computer"));
        assert_eq!(parsed.title.as_deref(), Some("No Surprises"));
    }

    #[test]
    fn keeps_plain_filename_as_title() {
        let parsed = parse_filename_metadata("Track Without Tags");

        assert_eq!(parsed.title.as_deref(), Some("Track Without Tags"));
        assert_eq!(parsed.artist, None);
        assert_eq!(parsed.album, None);
    }

    #[test]
    fn enriches_dsd_metadata_from_decoder_probe() {
        let path = temp_audio_path("seraph-import-dsd", "dsf");
        write_test_dsf(&path);

        let metadata = parse_audio_metadata(&path);
        assert_eq!(metadata.duration, Some(1));
        assert_eq!(metadata.bit_depth, Some(24));
        assert_eq!(metadata.sample_rate, Some(44_100));
        assert_eq!(metadata.channels, Some(2));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn detects_dsd_by_magic_when_extension_differs() {
        let path = temp_audio_path("seraph-import-dsd-magic", "bin");
        write_test_dsf(&path);

        assert!(is_audio_file(&path));
        assert_eq!(audio_format_label(&path), "DSF");

        let track = track_from_path(&path).expect("track from dsf magic");
        assert_eq!(track.format, "DSF");
        assert_eq!(track.bitdepth, "DSF 24-bit / 44.1 kHz PCM");
        assert_eq!(track.sample_rate, "44.1 kHz PCM");
        assert_eq!(track.channels, "Stereo");
        assert_eq!(track.duration, 1);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn trusts_riff_magic_over_dsf_extension() {
        let path = temp_audio_path("seraph-import-fake-dsf", "dsf");
        fs::write(&path, b"RIFF\0\0\0\0WAVE").expect("write fake dsf");

        assert_eq!(audio_format_label(&path), "WAV");
        assert!(!is_dsd_file(&path));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn formats_quality_with_sample_rate() {
        assert_eq!(
            format_audio_quality("FLAC", Some(24), Some(96_000)),
            "FLAC 24-bit / 96 kHz"
        );
        assert_eq!(
            format_audio_quality("WAV", Some(16), Some(44_100)),
            "WAV 16-bit / 44.1 kHz"
        );
        assert_eq!(
            format_audio_quality("DSF", Some(24), Some(44_100)),
            "DSF 24-bit / 44.1 kHz PCM"
        );
    }

    #[test]
    fn merges_cached_tracks_by_path() {
        let cached = vec![test_imported_track("old", "C:/Music/a.flac", "Old")];
        let imported = vec![
            test_imported_track("new", "c:/music/a.flac", "Updated"),
            test_imported_track("b", "C:/Music/b.flac", "Added"),
        ];

        let merged = merge_cached_tracks(cached, &imported);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "new");
        assert_eq!(merged[0].title, "Updated");
        assert_eq!(merged[1].id, "b");
    }

    #[test]
    fn merge_preserves_cached_lyrics_when_reimport_has_none() {
        let mut cached_track = test_imported_track("old", "C:/Music/a.flac", "Old");
        cached_track.lyrics = vec![LyricLine {
            time: 1.5,
            text: "cached line".into(),
        }];
        let imported = vec![test_imported_track("new", "c:/music/a.flac", "Updated")];

        let merged = merge_cached_tracks(vec![cached_track], &imported);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "new");
        assert_eq!(merged[0].title, "Updated");
        assert_eq!(merged[0].lyrics.len(), 1);
        assert!((merged[0].lyrics[0].time - 1.5).abs() < 0.001);
        assert_eq!(merged[0].lyrics[0].text, "cached line");
    }

    #[test]
    fn imported_tracks_from_cache_returns_preserved_lyrics() {
        let mut cached_track = test_imported_track("new", "c:/music/a.flac", "Updated");
        cached_track.lyrics = vec![LyricLine {
            time: 1.5,
            text: "cached line".into(),
        }];
        let imported = vec![test_imported_track("new", "C:/Music/a.flac", "Updated")];

        let returned = imported_tracks_from_cache(&[cached_track], &imported);

        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].id, "new");
        assert_eq!(returned[0].lyrics.len(), 1);
        assert_eq!(returned[0].lyrics[0].text, "cached line");
    }

    #[test]
    fn applies_track_lyrics_by_id() {
        let mut tracks = vec![
            test_imported_track("a", "C:/Music/a.flac", "A"),
            test_imported_track("b", "C:/Music/b.flac", "B"),
        ];
        let lyrics = vec![LyricLine {
            time: 2.0,
            text: "imported line".into(),
        }];

        apply_track_lyrics(&mut tracks, "b", lyrics, None).expect("apply lyrics");

        assert!(tracks[0].lyrics.is_empty());
        assert_eq!(tracks[1].lyrics.len(), 1);
        assert_eq!(tracks[1].lyrics[0].text, "imported line");
    }

    #[test]
    fn errors_when_applying_lyrics_to_missing_track() {
        let mut tracks = vec![test_imported_track("a", "C:/Music/a.flac", "A")];
        let lyrics = vec![LyricLine {
            time: 0.0,
            text: "line".into(),
        }];

        let err =
            apply_track_lyrics(&mut tracks, "missing", lyrics, None).expect_err("missing track");

        assert!(err.contains("track was not found"));
        assert!(tracks[0].lyrics.is_empty());
    }

    fn test_imported_track(id: &str, path: &str, title: &str) -> ImportedTrack {
        ImportedTrack {
            id: id.into(),
            title: title.into(),
            artist: "Artist".into(),
            album: "Album".into(),
            album_year: None,
            cover: String::new(),
            format: "FLAC".into(),
            bitdepth: "FLAC 24-bit / 96 kHz".into(),
            sample_rate: "96 kHz".into(),
            bitrate: "Unknown".into(),
            channels: "Stereo".into(),
            size: "1.0 MB".into(),
            path: path.into(),
            source_url: None,
            source_id: None,
            cache_missing: false,
            duration: 1,
            glow_color: "#fff".into(),
            glow1: "#fff".into(),
            glow2: "#000".into(),
            lyrics: Vec::new(),
        }
    }

    #[test]
    fn parses_timestamped_lrc_lines() {
        let lyrics = parse_lyrics_text("[ti:Test]\n[00:01.20]第一句\n[00:03.40][00:05.00]重复一句");

        assert_eq!(lyrics.len(), 3);
        assert!((lyrics[0].time - 1.2).abs() < 0.001);
        assert_eq!(lyrics[0].text, "第一句");
        assert!((lyrics[1].time - 3.4).abs() < 0.001);
        assert_eq!(lyrics[1].text, "重复一句");
        assert!((lyrics[2].time - 5.0).abs() < 0.001);
    }

    #[test]
    fn decodes_gbk_lrc_bytes() {
        let bytes = vec![
            b'[', b'0', b'0', b':', b'0', b'1', b'.', b'0', b'0', b']', 0xd6, 0xd0, 0xce, 0xc4,
        ];

        let lyrics = parse_lyrics_text(&decode_lyric_bytes(&bytes));

        assert_eq!(lyrics.len(), 1);
        assert_eq!(lyrics[0].text, "\u{4e2d}\u{6587}");
    }

    #[test]
    fn decodes_utf16_le_without_bom() {
        let bytes = "[00:01.00]hello"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        let lyrics = parse_lyrics_text(&decode_lyric_bytes(&bytes));

        assert_eq!(lyrics.len(), 1);
        assert_eq!(lyrics[0].text, "hello");
    }

    #[test]
    fn parses_common_lrc_time_variants() {
        let lyrics = parse_lyrics_text(
            "[OFFSET:-500]\n[00:01,20]comma\n[1234,567]krc\n[00:02.00]a <00:02.10>b [00:02.20]c",
        );

        assert_eq!(lyrics.len(), 3);
        // L-9：OFFSET:-500（负 offset）让歌词延后 0.5s（time - offset = time + 0.5）。
        assert!((lyrics[0].time - 1.7).abs() < 0.001);
        assert_eq!(lyrics[0].text, "comma");
        assert!((lyrics[1].time - 1.734).abs() < 0.001);
        assert_eq!(lyrics[1].text, "krc");
        assert!((lyrics[2].time - 2.5).abs() < 0.001);
        assert_eq!(lyrics[2].text, "a b c");
    }

    #[test]
    fn parses_qq_qrc_lyric_content() {
        let text = r#"<Lyric_1 LyricType="1" LyricContent="[1000,2000]he(1000,500)llo(1500,500)&#10;[3000,1000]world(3000,1000)"/>"#;

        let lyrics = parse_lyrics_bytes(text.as_bytes());

        assert_eq!(lyrics.len(), 2);
        assert!((lyrics[0].time - 1.0).abs() < 0.001);
        assert_eq!(lyrics[0].text, "hello");
        assert!((lyrics[1].time - 3.0).abs() < 0.001);
        assert_eq!(lyrics[1].text, "world");
    }

    #[test]
    fn parses_netease_yrc_word_lines() {
        let lyrics = parse_lyrics_bytes(
            b"[1200,800](1200,200,0)he(1400,200,0)llo\n[2500,500](2500,500,0)world",
        );

        assert_eq!(lyrics.len(), 2);
        assert!((lyrics[0].time - 1.2).abs() < 0.001);
        assert_eq!(lyrics[0].text, "hello");
        assert!((lyrics[1].time - 2.5).abs() < 0.001);
        assert_eq!(lyrics[1].text, "world");
    }

    #[test]
    fn parses_kugou_krc_word_lines_and_translation() {
        let language = BASE64_STANDARD
            .encode(r#"{"content":[{"type":1,"lyricContent":[["greeting"],["planet"]]}]}"#);
        let text = format!(
            "[language:{language}]\n[1000,2000]<0,500,0>he<500,500,0>llo\n[3000,1000]<0,1000,0>world"
        );

        let lyrics = parse_lyrics_bytes(text.as_bytes());

        assert_eq!(lyrics.len(), 4);
        assert!((lyrics[0].time - 1.0).abs() < 0.001);
        assert_eq!(lyrics[0].text, "hello");
        assert_eq!(lyrics[1].text, "greeting");
        assert!((lyrics[2].time - 3.0).abs() < 0.001);
        assert_eq!(lyrics[2].text, "world");
        assert_eq!(lyrics[3].text, "planet");
    }

    #[test]
    fn applies_lrc_offset() {
        // L-9：正 offset 让歌词提前显示 → 1.00s 标签 - 0.5s offset = 0.5s
        let lyrics = parse_lyrics_text("[offset:500]\n[00:01.00]提前半秒");

        assert_eq!(lyrics.len(), 1);
        assert!((lyrics[0].time - 0.5).abs() < 0.001);
    }

    #[test]
    fn converts_unsynced_lyrics_to_display_lines() {
        let lyrics = parse_lyrics_text("第一行\n\n第二行");

        assert_eq!(lyrics.len(), 2);
        assert_eq!(lyrics[0].time, 0.0);
        assert_eq!(lyrics[0].text, "第一行");
        assert_eq!(lyrics[1].time, 4.0);
        assert_eq!(lyrics[1].text, "第二行");
    }
}
