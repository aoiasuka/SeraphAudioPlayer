use super::prelude::*;
use crate::ipc::error::{IpcError, IpcResult};

/// 封面下载大小上限：正常专辑图几百 KB，超限视为异常响应。
const MAX_ONLINE_COVER_BYTES: usize = 4 * 1024 * 1024;

/// 为无内嵌封面的曲目在线匹配专辑封面。
/// 源：QQ 音乐搜索（albummid → y.gtimg.cn 封面，大陆连通性好）优先，
/// iTunes Search 兜底。下载字节经图片魔数校验后按内容哈希落盘 covers 目录，
/// 持锁更新曲库并返回新封面路径。
#[tauri::command]
pub async fn fetch_online_cover(
    app: AppHandle,
    track_id: String,
    title: String,
    artist: String,
) -> IpcResult<String> {
    if track_id.trim().is_empty() {
        return Err(IpcError::invalid_input("missing track id"));
    }
    let query = online_lyrics_query(&title, &artist);
    if query.is_empty() {
        return Err(IpcError::invalid_input("missing track title"));
    }

    let client = online_lyrics_client().map_err(IpcError::network)?;
    let image = match fetch_qq_cover_bytes(&client, &query).await {
        Some(bytes) => Some(bytes),
        None => fetch_itunes_cover_bytes(&client, &query).await,
    };
    let Some(bytes) = image else {
        return Err(IpcError::not_found("在线封面未找到"));
    };

    let Some(ext) = cover_image_extension(None, &bytes) else {
        return Err(IpcError::not_found("在线封面格式无法识别"));
    };
    let art = CoverArt { data: bytes, ext };

    // 网络阶段结束后进 spawn_blocking 持锁读改写（同 import_tracks 模式）
    let cover = tauri::async_runtime::spawn_blocking(move || -> Result<String, IpcError> {
        let covers_dir = covers_dir_path(&app)?;
        let cover = save_cover_art(&covers_dir, &art)
            .ok_or_else(|| IpcError::from("封面写入失败".to_string()))?;

        let _guard = LIBRARY_LOCK.lock();
        let mut tracks = read_cached_tracks_for_update(&app)?;
        let Some(track) = tracks.iter_mut().find(|track| track.id == track_id) else {
            return Err(IpcError::not_found(
                "track was not found in the library cache",
            ));
        };
        track.cover = cover.clone();
        write_cached_tracks(&app, &tracks)?;
        Ok(cover)
    })
    .await
    .map_err(|err| IpcError::from(format!("fetch_online_cover task panicked: {err}")))??;

    Ok(cover)
}

/// QQ 音乐搜索 → 第一个带 albummid 的结果 → 500x500 专辑封面。
async fn fetch_qq_cover_bytes(client: &Client, query: &str) -> Option<Vec<u8>> {
    let search = client
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
        .ok()?
        .json::<Value>()
        .await
        .ok()?;

    let songs = search
        .get("data")
        .and_then(|value| value.get("song"))
        .and_then(|value| value.get("list"))
        .and_then(Value::as_array)?;

    for song in songs {
        let Some(album_mid) = song
            .get("albummid")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        let url = format!("https://y.gtimg.cn/music/photo_new/T002R500x500M000{album_mid}.jpg");
        if let Some(bytes) = download_image(client, &url).await {
            return Some(bytes);
        }
    }
    None
}

/// iTunes Search 兜底：artworkUrl100 放大到 600x600。
async fn fetch_itunes_cover_bytes(client: &Client, query: &str) -> Option<Vec<u8>> {
    let search = client
        .get("https://itunes.apple.com/search")
        .query(&[
            ("term", query),
            ("media", "music"),
            ("entity", "song"),
            ("limit", "3"),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .ok()?
        .json::<Value>()
        .await
        .ok()?;

    let results = search.get("results").and_then(Value::as_array)?;
    for item in results {
        let Some(artwork) = item.get("artworkUrl100").and_then(Value::as_str) else {
            continue;
        };
        let url = artwork.replace("100x100", "600x600");
        if let Some(bytes) = download_image(client, &url).await {
            return Some(bytes);
        }
    }
    None
}

async fn download_image(client: &Client, url: &str) -> Option<Vec<u8>> {
    let bytes = client
        .get(url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .ok()?
        .bytes()
        .await
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_ONLINE_COVER_BYTES {
        return None;
    }
    Some(bytes.to_vec())
}
