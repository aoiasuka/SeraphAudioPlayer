use super::prelude::*;
use crate::ipc::error::{IpcError, IpcResult};

/// 封面下载大小上限：正常专辑图几百 KB，超限视为异常响应。
const MAX_ONLINE_COVER_BYTES: usize = 4 * 1024 * 1024;

/// F-04：封面**图片**下载专用 client——逐跳复验重定向目标必须仍在图床白名单内。
/// 与搜索接口共用一个 client 不行：搜索打的是 itunes.apple.com / c.y.qq.com,
/// 与图床是两套 host 白名单。
fn cover_image_client() -> Result<Client, String> {
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
        .redirect(guarded_redirect_policy(is_safe_cover_url))
        .build()
        .map_err(|err| format!("failed to create cover image client: {err}"))
}

/// F-04：封面图 URL 必须是 https + 官方图床域。
/// N-03：host 白名单收敛到 `ipc::url_guard` 单一事实来源。
fn is_safe_cover_url(raw: &str) -> bool {
    crate::ipc::url_guard::is_https_url_with_host_suffix(
        raw,
        crate::ipc::url_guard::ONLINE_COVER_HOST_SUFFIXES,
    )
}

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
    // F-04：搜索接口与图床是两套 host 白名单，图片下载走单独的受限 client
    let image_client = cover_image_client().map_err(IpcError::network)?;
    // M-12：区分“源访问失败”与“确实没有”——QQ 优先，失败或无图再走 iTunes
    let mut source_failed = false;
    let qq = fetch_qq_cover_bytes(&client, &image_client, &query).await;
    let image = match qq {
        Ok(Some(bytes)) => Some(bytes),
        outcome => {
            if outcome.is_err() {
                source_failed = true;
            }
            match fetch_itunes_cover_bytes(&client, &image_client, &query).await {
                Ok(bytes) => bytes,
                Err(()) => {
                    source_failed = true;
                    None
                }
            }
        }
    };
    let Some(bytes) = image else {
        if source_failed {
            return Err(IpcError::network(
                "在线封面源访问失败，请检查网络连接后重试",
            ));
        }
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
/// Err(()) = 搜索请求失败（网络/解析）；Ok(None) = 接口正常但没匹配到封面。
async fn fetch_qq_cover_bytes(
    client: &Client,
    image_client: &Client,
    query: &str,
) -> Result<Option<Vec<u8>>, ()> {
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
        .map_err(|_| ())?;
    // S-03：外部 JSON 一律 capped 读取
    let search = read_json_capped::<Value>(search, MAX_EXTERNAL_JSON_BYTES)
        .await
        .map_err(|_| ())?;

    let Some(songs) = search
        .get("data")
        .and_then(|value| value.get("song"))
        .and_then(|value| value.get("list"))
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };

    for song in songs {
        let Some(album_mid) = song
            .get("albummid")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        let url = format!("https://y.gtimg.cn/music/photo_new/T002R500x500M000{album_mid}.jpg");
        if let Some(bytes) = download_image(image_client, &url).await {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

/// iTunes Search 兜底：artworkUrl100 放大到 600x600。
/// Err(()) = 搜索请求失败；Ok(None) = 接口正常但没匹配到封面。
async fn fetch_itunes_cover_bytes(
    client: &Client,
    image_client: &Client,
    query: &str,
) -> Result<Option<Vec<u8>>, ()> {
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
        .map_err(|_| ())?;
    // S-03：外部 JSON 一律 capped 读取
    let search = read_json_capped::<Value>(search, MAX_EXTERNAL_JSON_BYTES)
        .await
        .map_err(|_| ())?;

    let Some(results) = search.get("results").and_then(Value::as_array) else {
        return Ok(None);
    };
    for item in results {
        let Some(artwork) = item.get("artworkUrl100").and_then(Value::as_str) else {
            continue;
        };
        let url = artwork.replace("100x100", "600x600");
        if let Some(bytes) = download_image(image_client, &url).await {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

async fn download_image(client: &Client, url: &str) -> Option<Vec<u8>> {
    // F-04：外部 API 返回的图片地址必须先过 https + 官方图床域白名单。
    // 注意这里用的是 online_lyrics_client（跟随重定向），所以下面还依赖
    // 该 client 的逐跳复验策略挡住 302 到任意主机。
    if !is_safe_cover_url(url) {
        return None;
    }
    let response = client
        .get(url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .ok()?;
    // S-03：边读边限（原先 `.bytes()` 全量读入后才比对大小，超大响应仍会先吃满内存）
    let bytes = read_bytes_capped(response, MAX_ONLINE_COVER_BYTES as u64)
        .await
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::is_safe_cover_url;

    #[test]
    fn cover_url_whitelist_only_allows_official_https_image_hosts() {
        for url in [
            "https://is1-ssl.mzstatic.com/image/thumb/a/600x600bb.jpg",
            "https://y.gtimg.cn/music/photo_new/T002R500x500M000abc.jpg",
            "https://p.qpic.cn/music_cover/abc/300.jpg",
        ] {
            assert!(is_safe_cover_url(url), "should accept {url}");
        }

        for url in [
            // http 明文
            "http://is1-ssl.mzstatic.com/image/a.jpg",
            // 后缀伪装：mzstatic.com.evil.com
            "https://is1-ssl.mzstatic.com.evil.com/a.jpg",
            // 裸后缀本身不算（host.len() > suffix.len() 要求）
            "https://mzstatic.com/a.jpg",
            // 完全无关的域
            "https://evil.example.com/a.jpg",
            // 内网探测
            "https://127.0.0.1:8080/a.jpg",
            "http://localhost/a.jpg",
            // userinfo 混淆
            "https://is1-ssl.mzstatic.com@evil.com/a.jpg",
            // 非 URL
            "",
            "not a url",
        ] {
            assert!(!is_safe_cover_url(url), "should reject {url:?}");
        }
    }
}
