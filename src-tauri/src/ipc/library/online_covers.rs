use super::prelude::*;
use crate::ipc::error::{IpcError, IpcResult};

/// 封面下载大小上限：正常专辑图几百 KB，超限视为异常响应。
const MAX_ONLINE_COVER_BYTES: usize = 4 * 1024 * 1024;

/// F-04：封面**图片**下载专用 client——逐跳复验重定向目标必须仍在图床白名单内。
/// 与搜索接口共用一个 client 不行：搜索打的是 itunes.apple.com / c.y.qq.com,
/// 与图床是两套 host 白名单。
fn cover_image_client() -> Result<Client, String> {
    // NET-02：进程内复用（此前每次匹配都重建一个 client，TLS 配置与连接池都白费）
    static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
    cached_client(&CLIENT, build_cover_image_client)
}

fn build_cover_image_client() -> Result<Client, String> {
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

/// 多艺术家字段的分隔符（与本地歌词目录、AMLL 元数据核对同一套）。
const ARTIST_SEPARATORS: [char; 6] = ['/', '&', ',', '，', '、', ';'];
/// 一次匹配最多尝试下载几张候选图（搜索结果本身只有 3～5 条）。
const MAX_COVER_DOWNLOAD_ATTEMPTS: usize = 3;

/// 比对用的键：简繁统一（都转繁体）后再做 `normalize_for_match`（全半角、大小写、标点空白）。
/// 本地标签是简体而搜索结果是繁体（或反过来）时仍算同一个名字。
fn match_key(value: &str) -> String {
    normalize_for_match(&to_traditional(value))
}

fn artist_keys(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(ARTIST_SEPARATORS)
        .map(match_key)
        // 导入时没有艺术家标签的曲目写的是 "Unknown Artist"
        .filter(|key| !key.is_empty() && key != "unknown" && key != "unknownartist")
}

/// NET-01：要找封面的那首曲目。搜索结果必须与它的曲名（和艺术家）对得上才会被采用。
///
/// 此前直接取搜索结果里第一个有图的——曲名常见、或第一条是翻唱 / 合辑 / 完全不相干的
/// 结果时（iTunes 默认商店对中文关键词经常如此），别的专辑的封面就被写进了曲库。
/// 歌词那边早就有核对（AMLL 命中后比对文件头的曲名与艺术家），封面是一键直接应用，
/// 更需要这一步。
struct CoverTarget {
    title: String,
    /// 去掉尾部括号段（版本 / 现场后缀）的曲名
    title_base: String,
    artists: Vec<String>,
}

impl CoverTarget {
    fn new(title: &str, artist: &str) -> Self {
        Self {
            title: match_key(title),
            title_base: match_key(strip_trailing_parens(title)),
            artists: artist_keys(artist).collect(),
        }
    }

    /// 一条搜索结果与目标曲目的匹配档次：0 = 不是同一首歌，不采用；2 = 曲名完全相同；
    /// 1 = 去掉括号后缀后相同（`晴天` 对 `晴天 (Live)`——多半是另一张专辑，排在后面）。
    ///
    /// - 曲名只认相等，不认「包含」：串烧、remix 合集的标题里常常含着原曲名。
    /// - 曲目有艺术家时，结果里必须有一位对得上（多艺术家任一即可）；结果没给艺术家就无从
    ///   核对，不采用。
    /// - 曲目没有艺术家信息时只剩曲名可比，要求完全相同。
    fn rank(&self, title: &str, artists: &[&str]) -> u8 {
        let full = match_key(title);
        if full.is_empty() || self.title.is_empty() {
            return 0;
        }
        let title_rank = if full == self.title {
            2
        } else if match_key(strip_trailing_parens(title)) == self.title_base {
            1
        } else {
            return 0;
        };
        if self.artists.is_empty() {
            return if title_rank == 2 { 2 } else { 0 };
        }
        let artist_matches = artists
            .iter()
            .flat_map(|name| artist_keys(name))
            .any(|key| {
                self.artists
                    .iter()
                    .any(|own| *own == key || own.contains(&key) || key.contains(own.as_str()))
            });
        if artist_matches {
            title_rank
        } else {
            0
        }
    }
}

/// 匹配档次高的在前，同档保持接口返回的顺序；同一张图只留一次，
/// 封顶 [`MAX_COVER_DOWNLOAD_ATTEMPTS`] 个。
fn ordered_cover_urls(mut ranked: Vec<(u8, String)>) -> Vec<String> {
    ranked.sort_by_key(|(rank, _)| std::cmp::Reverse(*rank));
    let mut urls: Vec<String> = Vec::new();
    for (_, url) in ranked {
        if urls.len() >= MAX_COVER_DOWNLOAD_ATTEMPTS {
            break;
        }
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// QQ 搜索结果里与目标曲目相符、而且带专辑 ID 的条目 → 500x500 专辑封面地址。
fn qq_cover_urls(search: &Value, target: &CoverTarget) -> Vec<String> {
    let songs = search
        .get("data")
        .and_then(|value| value.get("song"))
        .and_then(|value| value.get("list"))
        .and_then(Value::as_array);
    let ranked = songs
        .into_iter()
        .flatten()
        .filter_map(|song| {
            let album_mid = song
                .get("albummid")
                .and_then(Value::as_str)
                .map(str::trim)
                // 这个值要拼进图片地址：只接受接口实际使用的字母数字形态
                .filter(|mid| {
                    !mid.is_empty()
                        && mid.len() <= 32
                        && mid.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })?;
            let title = song.get("songname").and_then(Value::as_str)?;
            let singers: Vec<&str> = song
                .get("singer")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|singer| singer.get("name").and_then(Value::as_str))
                .collect();
            let rank = target.rank(title, &singers);
            (rank > 0).then(|| {
                (
                    rank,
                    format!("https://y.gtimg.cn/music/photo_new/T002R500x500M000{album_mid}.jpg"),
                )
            })
        })
        .collect();
    ordered_cover_urls(ranked)
}

/// iTunes 搜索结果里与目标曲目相符的条目 → 600x600 封面地址。
fn itunes_cover_urls(search: &Value, target: &CoverTarget) -> Vec<String> {
    let results = search.get("results").and_then(Value::as_array);
    let ranked = results
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let artwork = item.get("artworkUrl100").and_then(Value::as_str)?;
            let title = item.get("trackName").and_then(Value::as_str)?;
            let artist = item.get("artistName").and_then(Value::as_str);
            let rank = target.rank(title, artist.as_slice());
            (rank > 0).then(|| (rank, artwork.replace("100x100", "600x600")))
        })
        .collect();
    ordered_cover_urls(ranked)
}

/// 为无内嵌封面的曲目在线匹配专辑封面。
/// 源：QQ 音乐搜索（albummid → y.gtimg.cn 封面，大陆连通性好）优先，
/// iTunes Search 兜底。只采用曲名（和艺术家）与这首曲目对得上的搜索结果（NET-01）；
/// 下载字节经图片魔数校验后按内容哈希落盘 covers 目录，持锁更新曲库并返回新封面路径。
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

    let target = CoverTarget::new(&title, &artist);
    let client = online_lyrics_client().map_err(IpcError::network)?;
    // F-04：搜索接口与图床是两套 host 白名单，图片下载走单独的受限 client
    let image_client = cover_image_client().map_err(IpcError::network)?;
    // M-12：区分“源访问失败”与“确实没有”——QQ 优先，失败或无图再走 iTunes
    let mut source_failed = false;
    let qq = fetch_qq_cover_bytes(&client, &image_client, &query, &target).await;
    let image = match qq {
        Ok(Some(bytes)) => Some(bytes),
        outcome => {
            if outcome.is_err() {
                source_failed = true;
            }
            match fetch_itunes_cover_bytes(&client, &image_client, &query, &target).await {
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
        write_cached_tracks(&app, tracks)?;
        Ok(cover)
    })
    .await
    .map_err(|err| IpcError::from(format!("fetch_online_cover task panicked: {err}")))??;

    Ok(cover)
}

/// QQ 音乐搜索 → 与目标曲目相符的结果 → 500x500 专辑封面。
/// Err(()) = 搜索请求失败（网络/解析）；Ok(None) = 接口正常但没匹配到封面。
async fn fetch_qq_cover_bytes(
    client: &Client,
    image_client: &Client,
    query: &str,
    target: &CoverTarget,
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

    for url in qq_cover_urls(&search, target) {
        if let Some(bytes) = download_image(image_client, &url).await {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

/// iTunes Search 兜底：与目标曲目相符的结果，artworkUrl100 放大到 600x600。
/// Err(()) = 搜索请求失败；Ok(None) = 接口正常但没匹配到封面。
async fn fetch_itunes_cover_bytes(
    client: &Client,
    image_client: &Client,
    query: &str,
    target: &CoverTarget,
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

    for url in itunes_cover_urls(&search, target) {
        if let Some(bytes) = download_image(image_client, &url).await {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

async fn download_image(client: &Client, url: &str) -> Option<Vec<u8>> {
    // F-04：外部 API 返回的图片地址必须先过 https + 官方图床域白名单；
    // 之后的重定向由 `cover_image_client` 的逐跳复验策略挡住 302 到任意主机。
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
    use super::*;
    use serde_json::json;

    /// 结构取自 2026-11-27 对两个接口的匿名实测（查询「晴天 周杰伦」）。
    fn qq_search() -> Value {
        json!({ "code": 0, "data": { "song": { "list": [
            { "songname": "晴天 (Live)", "albummid": "004fXSyj3bWTMN",
              "singer": [{ "name": "周杰伦" }] },
            { "songname": "晴天", "albummid": "000MkMni19ClKG",
              "singer": [{ "name": "周杰伦" }] },
            { "songname": "晴天 (Live)", "albummid": "",
              "singer": [{ "name": "林俊杰" }, { "name": "周杰伦" }] },
            { "songname": "晴天+安静+修炼爱情+最长的电影 (2017周杰伦地表最强演唱会台北站)",
              "albummid": "003medley0000",
              "singer": [{ "name": "周杰伦" }, { "name": "林俊杰" }] },
            { "songname": "晴天", "albummid": "002cover00000",
              "singer": [{ "name": "某翻唱歌手" }] },
        ] } } })
    }

    const QQ_ALBUM: &str = "https://y.gtimg.cn/music/photo_new/T002R500x500M000000MkMni19ClKG.jpg";
    const QQ_LIVE: &str = "https://y.gtimg.cn/music/photo_new/T002R500x500M000004fXSyj3bWTMN.jpg";

    #[test]
    fn qq_results_must_match_title_and_artist() {
        // NET-01：此前取第一个带 albummid 的——这里就是现场专辑，而不是原专辑
        let urls = qq_cover_urls(&qq_search(), &CoverTarget::new("晴天", "周杰伦"));
        assert_eq!(urls, [QQ_ALBUM, QQ_LIVE], "曲名完全相同的排在带后缀的前面");
        // 串烧（标题里含着曲名）与别人的翻唱都不采用
        assert!(urls
            .iter()
            .all(|url| !url.contains("medley") && !url.contains("cover")));

        // 曲库里的就是现场版：现场专辑排前，原版排后
        let live = qq_cover_urls(&qq_search(), &CoverTarget::new("晴天 (Live)", "周杰伦"));
        assert_eq!(live, [QQ_LIVE, QQ_ALBUM]);

        // 对不上 → 一张都不要（宁可没有封面，也不写进别人的专辑图）
        assert!(qq_cover_urls(&qq_search(), &CoverTarget::new("晴天", "孙燕姿")).is_empty());
        assert!(qq_cover_urls(&qq_search(), &CoverTarget::new("阴天", "周杰伦")).is_empty());
    }

    #[test]
    fn matching_tolerates_script_width_case_and_multiple_artists() {
        let target = CoverTarget::new("说好的幸福呢", "周杰伦 / 方文山");
        // 简繁、全角空格、多艺术家任一相符
        assert_eq!(target.rank("說好的幸福呢", &["周杰倫"]), 2);
        assert_eq!(target.rank("说好的　幸福呢", &["方文山"]), 2);
        assert_eq!(target.rank("說好的幸福呢 (Live)", &["周杰倫"]), 1);
        assert_eq!(target.rank("说好的幸福呢", &["林俊杰"]), 0);
        // iTunes 把多位艺术家写在一个字段里
        assert_eq!(target.rank("说好的幸福呢", &["Jay Chou & 周杰伦"]), 2);
        // 结果没给艺术家：无从核对，不采用
        assert_eq!(target.rank("说好的幸福呢", &[]), 0);

        let latin = CoverTarget::new("Hotel California", "Eagles");
        assert_eq!(latin.rank("HOTEL CALIFORNIA", &["eagles"]), 2);
        assert_eq!(
            latin.rank("Hotel California (2013 Remaster)", &["Eagles"]),
            1
        );
        assert_eq!(latin.rank("Hotel California Dreaming", &["Eagles"]), 0);
    }

    #[test]
    fn tracks_without_artist_require_an_exact_title() {
        // 没有艺术家可比时只剩曲名：要求完全相同，带后缀的版本不算
        for artist in ["", "Unknown", "Unknown Artist", "  "] {
            let target = CoverTarget::new("晴天", artist);
            assert_eq!(target.rank("晴天", &["任何人"]), 2, "{artist:?}");
            assert_eq!(target.rank("晴天 (Live)", &["任何人"]), 0, "{artist:?}");
        }
        assert_eq!(CoverTarget::new("", "周杰伦").rank("", &["周杰伦"]), 0);
        assert_eq!(
            CoverTarget::new("晴天", "周杰伦").rank("  ", &["周杰伦"]),
            0
        );
    }

    #[test]
    fn itunes_results_must_match_and_unrelated_ones_are_dropped() {
        // 实测：默认商店对这条中文关键词返回的三条结果与查询毫不相干。
        // 此前第一条的封面会被直接写进曲库。
        let unrelated = json!({ "resultCount": 2, "results": [
            { "trackName": "Tainha", "artistName": "Mestra Ana & Alessandra",
              "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/a/100x100bb.jpg" },
            { "trackName": "Sebastião", "artistName": "Mestra Ana & Alessandra",
              "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/b/100x100bb.jpg" },
        ] });
        let target = CoverTarget::new("晴天", "周杰伦");
        assert!(itunes_cover_urls(&unrelated, &target).is_empty());

        let matching = json!({ "results": [
            { "trackName": "晴天 (Live)", "artistName": "周杰倫",
              "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/live/100x100bb.jpg" },
            { "trackName": "晴天", "artistName": "周杰倫",
              "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/album/100x100bb.jpg" },
            { "trackName": "晴天", "artistName": "周杰倫" },
        ] });
        assert_eq!(
            itunes_cover_urls(&matching, &target),
            [
                "https://is1-ssl.mzstatic.com/image/thumb/album/600x600bb.jpg",
                "https://is1-ssl.mzstatic.com/image/thumb/live/600x600bb.jpg",
            ]
        );
        // 响应结构不对时没有候选，不 panic
        assert!(itunes_cover_urls(&json!({ "results": "oops" }), &target).is_empty());
        assert!(qq_cover_urls(&json!({ "data": null }), &target).is_empty());
    }

    #[test]
    fn candidate_urls_are_capped_deduplicated_and_album_ids_are_validated() {
        let song = |mid: &str| json!({ "songname": "晴天", "albummid": mid, "singer": [{ "name": "周杰伦" }] });
        let mut many = vec![song("mid000"), song("mid000")];
        many.extend((1..10).map(|index| song(&format!("mid{index:03}"))));
        let search = json!({ "data": { "song": { "list": many } } });
        let target = CoverTarget::new("晴天", "周杰伦");
        let urls = qq_cover_urls(&search, &target);
        assert_eq!(urls.len(), MAX_COVER_DOWNLOAD_ATTEMPTS);
        assert!(urls[0].ends_with("M000mid000.jpg"), "{urls:?}");
        assert!(urls[1].ends_with("M000mid001.jpg"), "同一张专辑只试一次");

        // 专辑 ID 要拼进地址：带路径、查询或其它符号的一律不用
        let too_long = "a".repeat(64);
        for bad in [
            "../../x", "abc/def", "abc?x=1", "abc def", "abc.jpg#", &too_long,
        ] {
            let search = json!({ "data": { "song": { "list": [song(bad)] } } });
            assert!(qq_cover_urls(&search, &target).is_empty(), "{bad}");
        }
    }

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
