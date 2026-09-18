//! AMLL TTML DB（amll-dev/amll-ttml-db）逐字歌词二次查找。
//!
//! 仓库按平台 ID 落文件：`ncm-lyrics/<网易云歌曲 id>.ttml`、`qq-lyrics/<QQ 歌曲 id>.ttml`。
//! 我们没有独立搜索接口，做法是：三源搜索拿到网易云/QQ 候选后，用它们的平台
//! ID 去 DB 试取 TTML，命中即作为新的候选插到最前面（逐字 + 译文 + 音译）。
//!
//! 地址是**模板**：`{dir}` = 目录（ncm-lyrics / qq-lyrics），`{id}` 或 `%s` = 平台 ID；
//! 不含占位符的旧式 base URL 自动补 `/{dir}/{id}.ttml`。
//!
//! 两种安全模式：
//! - 预设镜像（默认）：host 只认白名单，重定向逐跳复验；
//! - 自定义地址：任意公网 HTTPS host，但拒绝 IP 直连/localhost/内网名，且**禁止重定向**
//!   （自定义域不可信，不给它把请求转向别处的机会）。响应体两种模式都 capped 读取。
//!
//! 2026-09-18 起追加官方索引 API（`api.amll.dev`，见 [`AMLL_API_BASE`]）：DB 直取没有任何
//! 命中时，用曲名/艺术家搜索索引，**只在高置信度匹配时**取回 TTML 作为候选。API 固定走
//! 官方域、挂独立白名单，不受用户 DB 地址模式影响。两条路径都过进程级内存缓存
//! （正向 30 分钟、404 负缓存 5 分钟、网络错误不缓存、上限 256 条）。

use super::prelude::*;
use crate::ipc::url_guard::{is_public_https_url, is_safe_amll_api_url, is_safe_amll_ttml_url};
use std::sync::LazyLock;
use std::time::Instant;

/// 默认 DB 地址（社区镜像 amlldb.bikonoo.com，网易云目录模板）。
pub(crate) const DEFAULT_AMLL_TTML_DB_URL: &str = "https://amlldb.bikonoo.com/ncm-lyrics/%s.ttml";

/// 官方索引 API 根地址（2026-09-18 实测 `/v1/lyrics/search` 与 `/v1/lyrics/get`）。
pub(crate) const AMLL_API_BASE: &str = "https://api.amll.dev";

/// 每次搜索最多试取多少个候选的 TTML（每个 = 一次 GET，绝大多数 404）。
const MAX_TTML_LOOKUPS: usize = 6;

/// 索引搜索每页条数：只为挑一条高置信度匹配，5 条足够。
const API_SEARCH_PAGE_SIZE: usize = 5;

/// 自动绑定的最低匹配分（标题完全相等 = 3；包含 1 + 艺术家 2 = 3）。
const API_MATCH_THRESHOLD: u8 = 3;

pub(crate) const AMLL_SOURCE_LABEL: &str = "AMLL TTML DB";

// ---------------------------------------------------------------------------
// 进程级内存缓存
// ---------------------------------------------------------------------------

/// 正向命中有效期。
const CACHE_TTL_HIT: Duration = Duration::from_secs(30 * 60);
/// 未收录（404）负缓存有效期。
const CACHE_TTL_MISS: Duration = Duration::from_secs(5 * 60);
/// 容量上限，满了清最老的一条。
const CACHE_CAPACITY: usize = 256;

/// 缓存命中的内容：DB 直取只有歌词；API 路径还带索引条目（构造候选要用）。
#[derive(Debug, Clone)]
pub(crate) struct CachedHit {
    pub(crate) lyrics: Vec<LyricLine>,
    pub(crate) item: Option<AmllApiItem>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    at: Instant,
    /// None = 负缓存（服务端明确 404 未收录）。
    value: Option<CachedHit>,
}

static CACHE: LazyLock<parking_lot::Mutex<HashMap<String, CacheEntry>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

/// 查缓存。外层 None = 未缓存或已过期；`Some(None)` = 负缓存命中（已知未收录）。
fn cache_get(key: &str) -> Option<Option<CachedHit>> {
    cache_get_at(key, Instant::now())
}

fn cache_get_at(key: &str, now: Instant) -> Option<Option<CachedHit>> {
    let mut cache = CACHE.lock();
    let entry = cache.get(key)?;
    let ttl = if entry.value.is_some() {
        CACHE_TTL_HIT
    } else {
        CACHE_TTL_MISS
    };
    if now.saturating_duration_since(entry.at) >= ttl {
        cache.remove(key);
        return None;
    }
    Some(entry.value.clone())
}

fn cache_put(key: &str, value: Option<CachedHit>) {
    cache_put_at(key, value, Instant::now());
}

fn cache_put_at(key: &str, value: Option<CachedHit>, at: Instant) {
    let mut cache = CACHE.lock();
    if !cache.contains_key(key) && cache.len() >= CACHE_CAPACITY {
        // 满了淘汰时间戳最老的一条（简单线性扫描，256 条量级无所谓）
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.at)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(key.to_string(), CacheEntry { at, value });
}

fn url_guard_for(custom: bool) -> fn(&str) -> bool {
    if custom {
        is_public_https_url
    } else {
        is_safe_amll_ttml_url
    }
}

/// 把用户地址规整成模板：去尾部斜杠；空串回默认；无占位符则补 `/{dir}/{id}.ttml`；
/// 再按模式校验 host（用占位替换成示例值做一次解析）。不合规返回 None。
pub(crate) fn normalize_amll_db_url(raw: &str, custom: bool) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let base = if trimmed.is_empty() {
        DEFAULT_AMLL_TTML_DB_URL
    } else {
        trimmed
    };
    let template = if base.contains("{id}") || base.contains("%s") {
        base.to_string()
    } else {
        format!("{base}/{{dir}}/{{id}}.ttml")
    };
    let probe = expand_template(&template, "ncm-lyrics", "1");
    url_guard_for(custom)(&probe).then_some(template)
}

fn expand_template(template: &str, dir: &str, id: &str) -> String {
    template
        .replace("{dir}", dir)
        .replace("{id}", id)
        .replace("%s", id)
}

pub(crate) fn amll_client(custom: bool) -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("SeraphAudioPlayer"));
    let redirect = if custom {
        reqwest::redirect::Policy::none()
    } else {
        guarded_redirect_policy(is_safe_amll_ttml_url)
    };
    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(12))
        .redirect(redirect)
        .build()
        .map_err(|err| format!("failed to create AMLL client: {err}"))
}

pub(crate) fn lookup_url(template: &str, key: &str, custom: bool) -> Option<String> {
    // key 形如 `ncm-lyrics/123`：只允许 [目录]/[id] 两段、字符集受限，
    // 防止候选 ID 里混入 `..` 或查询串拼出白名单外的路径
    let (dir, id) = key.split_once('/')?;
    if !is_valid_lookup_key_segment(dir) || !is_valid_lookup_key_segment(id) {
        return None;
    }
    // 模板写死了目录（`/ncm-lyrics/%s.ttml`）又没有 {dir} 占位时，只对该平台的候选生效，
    // 免得拿 QQ 的 songid 去网易云目录白打一枪
    if !template.contains("{dir}") {
        let pinned_other_dir = ["ncm-lyrics", "qq-lyrics"]
            .iter()
            .any(|known| *known != dir && template.contains(&format!("/{known}/")));
        if pinned_other_dir {
            return None;
        }
    }
    let url = expand_template(template, dir, id);
    url_guard_for(custom)(&url).then_some(url)
}

async fn fetch_ttml(client: &Client, url: &str) -> Option<Vec<LyricLine>> {
    // 缓存键 = 完整请求 URL（模板/模式不同即不同 URL，天然隔离）
    if let Some(cached) = cache_get(url) {
        return cached.map(|hit| hit.lyrics);
    }
    let response = client.get(url).send().await.ok()?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        // 明确未收录：负缓存，短时间内不再重复打这一枪
        cache_put(url, None);
        return None;
    }
    if !status.is_success() {
        return None;
    }
    let bytes = read_bytes_capped(response, super::ttml::MAX_TTML_BYTES)
        .await
        .ok()?;
    let text = decode_lyric_bytes(&bytes);
    let lyrics = super::ttml::parse_ttml_lyrics(&text);
    if lyrics.is_empty() {
        return None;
    }
    cache_put(
        url,
        Some(CachedHit {
            lyrics: lyrics.clone(),
            item: None,
        }),
    );
    Some(lyrics)
}

// ---------------------------------------------------------------------------
// 官方索引 API（api.amll.dev）
// ---------------------------------------------------------------------------

/// 索引 API 的一条歌词记录（`/v1/lyrics/search` 的 items 元素 / `/v1/lyrics/get` 的 data）。
/// 只保留我们用到的字段；`id` 实测是数字，但为兼容未来改成字符串也一并接受。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct AmllApiItem {
    #[serde(deserialize_with = "deserialize_u64_lenient")]
    pub(crate) id: u64,
    pub(crate) music_names: Vec<String>,
    pub(crate) artist_names: Vec<String>,
    pub(crate) album_names: Vec<String>,
    pub(crate) ncm_music_ids: Vec<String>,
    /// 注意：这里是 QQ 的 songmid 字符串（如 `0032UZe62rZk9K`），**不是**
    /// DB 目录键 `qq-lyrics/<数字 songid>` 用的 songid，不能拿来拼查找键。
    pub(crate) qq_music_ids: Vec<String>,
}

/// 数字或数字字符串都解析成 u64；其它形态（null / 非数字串）得 0。
fn deserialize_u64_lenient<'de, D: serde::Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    let value = Value::deserialize(de)?;
    Ok(match value {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[serde(default)]
    status: u16,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct ApiSearchData {
    #[serde(default)]
    items: Vec<AmllApiItem>,
}

/// `/v1/lyrics/get` 的 data：索引字段 + TTML 全文。
#[derive(Debug, Deserialize)]
struct ApiGetData {
    #[serde(flatten)]
    item: AmllApiItem,
    #[serde(default)]
    lyrics: String,
}

pub(crate) fn amll_api_client() -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("SeraphAudioPlayer"));
    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(12))
        .redirect(guarded_redirect_policy(is_safe_amll_api_url))
        .build()
        .map_err(|err| format!("failed to create AMLL API client: {err}"))
}

/// 匹配分：标题归一化完全相等 3 / 一方包含另一方 1 / 否则 0；艺术家任一归一化相等 +2。
/// 标题 0 分直接丢弃（艺术家分不单独成立）。
fn api_match_score(item: &AmllApiItem, title_norm: &str, artist_norm: &str) -> u8 {
    if title_norm.is_empty() {
        return 0;
    }
    let mut title_score = 0u8;
    for name in &item.music_names {
        let norm = normalize_for_match(name);
        if norm.is_empty() {
            continue;
        }
        let score = if norm == title_norm {
            3
        } else if norm.contains(title_norm) || title_norm.contains(&norm) {
            1
        } else {
            0
        };
        title_score = title_score.max(score);
    }
    if title_score == 0 {
        return 0;
    }
    let artist_hit = !artist_norm.is_empty()
        && item
            .artist_names
            .iter()
            .any(|name| normalize_for_match(name) == artist_norm);
    title_score + if artist_hit { 2 } else { 0 }
}

/// 从搜索结果里挑出唯一一条可自动绑定的高置信度匹配（≥ 阈值且分最高；同分取先出现的）。
fn pick_best_api_item(items: Vec<AmllApiItem>, title: &str, artist: &str) -> Option<AmllApiItem> {
    let title_norm = normalize_for_match(title);
    let artist_norm = normalize_for_match(artist);
    let mut best: Option<(u8, AmllApiItem)> = None;
    for item in items {
        let score = api_match_score(&item, &title_norm, &artist_norm);
        if score == 0 {
            continue;
        }
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, item));
        }
    }
    best.and_then(|(score, item)| (score >= API_MATCH_THRESHOLD).then_some(item))
}

/// 搜索官方索引；返回**至多一条**高置信度匹配（模糊结果不自动绑定）。
/// `duration` 目前索引不提供时长，仅保留参数位以便将来打分。
pub(crate) async fn search_amll_api(
    title: &str,
    artist: &str,
    _duration: Option<u64>,
) -> Result<Vec<AmllApiItem>, String> {
    let title = title.trim();
    let artist = artist.trim();
    if title.is_empty() {
        return Err("missing title".into());
    }
    let cache_key = format!(
        "api-search:{}|{}",
        normalize_for_match(title),
        normalize_for_match(artist)
    );
    if let Some(cached) = cache_get(&cache_key) {
        // 搜索缓存只存「挑中的那条」；负缓存 = 上次没有高置信度命中
        return Ok(cached.and_then(|hit| hit.item).into_iter().collect());
    }

    let page_size = API_SEARCH_PAGE_SIZE.to_string();
    let mut params = vec![("musicName", title), ("pageSize", page_size.as_str())];
    if !artist.is_empty() {
        params.push(("artistName", artist));
    }
    let url =
        reqwest::Url::parse_with_params(&format!("{AMLL_API_BASE}/v1/lyrics/search"), &params)
            .map_err(|err| format!("failed to build AMLL API url: {err}"))?;
    if !is_safe_amll_api_url(url.as_str()) {
        return Err("AMLL API url rejected by allowlist".into());
    }
    let client = amll_api_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("AMLL API search request failed: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("AMLL API search returned HTTP {status}"));
    }
    let envelope: ApiEnvelope<ApiSearchData> =
        read_json_capped(response, MAX_EXTERNAL_JSON_BYTES).await?;
    if envelope.status != 200 {
        return Err(format!("AMLL API search status {}", envelope.status));
    }
    let items = envelope.data.map(|data| data.items).unwrap_or_default();
    let picked = pick_best_api_item(items, title, artist);
    cache_put(
        &cache_key,
        picked.clone().map(|item| CachedHit {
            lyrics: Vec::new(),
            item: Some(item),
        }),
    );
    Ok(picked.into_iter().collect())
}

/// 按索引 id 取 TTML 全文并解析；未收录 / 任何失败返回 None（细节只 debug 日志）。
pub(crate) async fn fetch_amll_api_lyrics(id: u64) -> Option<(AmllApiItem, Vec<LyricLine>)> {
    let cache_key = format!("api:{id}");
    if let Some(cached) = cache_get(&cache_key) {
        return cached.and_then(|hit| hit.item.map(|item| (item, hit.lyrics)));
    }
    let id_text = id.to_string();
    let url = reqwest::Url::parse_with_params(
        &format!("{AMLL_API_BASE}/v1/lyrics/get"),
        &[("id", id_text.as_str())],
    )
    .ok()?;
    if !is_safe_amll_api_url(url.as_str()) {
        return None;
    }
    let client = amll_api_client().ok()?;
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::debug!("AMLL API get {id} request failed: {err}");
            return None;
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        cache_put(&cache_key, None);
        return None;
    }
    if !status.is_success() {
        tracing::debug!("AMLL API get {id} returned HTTP {status}");
        return None;
    }
    let envelope: ApiEnvelope<ApiGetData> =
        match read_json_capped(response, MAX_EXTERNAL_JSON_BYTES).await {
            Ok(envelope) => envelope,
            Err(err) => {
                tracing::debug!("AMLL API get {id} body invalid: {err}");
                return None;
            }
        };
    if envelope.status != 200 {
        return None;
    }
    let data = envelope.data?;
    // TTML 单文件上限与 DB 直取同口径
    if data.lyrics.len() as u64 > super::ttml::MAX_TTML_BYTES {
        tracing::debug!("AMLL API get {id} lyrics exceed MAX_TTML_BYTES");
        return None;
    }
    let lyrics = super::ttml::parse_ttml_lyrics(&data.lyrics);
    if lyrics.is_empty() {
        return None;
    }
    let mut item = data.item;
    if item.id == 0 {
        item.id = id;
    }
    cache_put(
        &cache_key,
        Some(CachedHit {
            lyrics: lyrics.clone(),
            item: Some(item.clone()),
        }),
    );
    Some((item, lyrics))
}

/// 官方索引兜底：搜索 → 取 TTML → 构造候选。任何失败返回 None，只 debug 日志。
pub(crate) async fn fetch_amll_api_candidate(
    title: &str,
    artist: &str,
    duration: Option<u64>,
) -> Option<OnlineLyricsCandidate> {
    let items = match search_amll_api(title, artist, duration).await {
        Ok(items) => items,
        Err(err) => {
            tracing::debug!("AMLL API search skipped: {err}");
            return None;
        }
    };
    let picked = items.into_iter().next()?;
    let (item, lyrics) = fetch_amll_api_lyrics(picked.id).await?;
    let word_synced = lyrics.iter().any(|line| line.words.is_some());
    let first = |names: &[String]| names.first().cloned().unwrap_or_default();
    Some(OnlineLyricsCandidate {
        id: format!("ttml-api-{}", item.id),
        source: if word_synced {
            format!("{AMLL_SOURCE_LABEL} · 逐字")
        } else {
            AMLL_SOURCE_LABEL.to_string()
        },
        title: {
            let name = first(&item.music_names);
            if name.is_empty() {
                title.to_string()
            } else {
                name
            }
        },
        artist: {
            let name = first(&item.artist_names);
            if name.is_empty() {
                artist.to_string()
            } else {
                name
            }
        },
        album: item.album_names.first().cloned().filter(|s| !s.is_empty()),
        duration,
        lyrics,
        // 只映射网易云 ID；qqMusicIds 是 songmid，与 DB 目录键不同，不混用
        ttml_lookup_keys: item
            .ncm_music_ids
            .iter()
            .filter(|id| is_valid_lookup_key_segment(id))
            .map(|id| format!("ncm-lyrics/{id}"))
            .collect(),
    })
}

/// 查找键单段字符集（目录名 / 平台 ID）：非空、仅字母数字 `-` `_`。
pub(crate) fn is_valid_lookup_key_segment(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// 完整查找键 `dir/id` 是否合法（`apply_online_lyrics` 回写曲目前校验）。
pub(crate) fn is_valid_lookup_key(key: &str) -> bool {
    key.split_once('/').is_some_and(|(dir, id)| {
        is_valid_lookup_key_segment(dir) && is_valid_lookup_key_segment(id)
    })
}

// ---------------------------------------------------------------------------
// 「测试连接」
// ---------------------------------------------------------------------------

/// 歌词设置页「测试连接」：按模板对一个样例键真发一次 GET，把结果分类成机器可读的 kind。
pub(crate) async fn run_amll_ttml_db_test(
    template: &str,
    custom: bool,
    sample_key: Option<&str>,
) -> AmllTestResult {
    let fail = |kind: &str, message: String, url: String| AmllTestResult {
        ok: false,
        kind: kind.to_string(),
        message,
        url,
        lines: 0,
    };
    let Some(template) = normalize_amll_db_url(template, custom) else {
        return fail(
            "invalid_url",
            if custom {
                "地址无效：需为公网 HTTPS 域名（不接受 IP 直连、localhost 或内网主机）".into()
            } else {
                "地址不在预设镜像列表内；如需其它域名请切换到自定义模式".into()
            },
            String::new(),
        );
    };
    // 样例键：默认网易云；模板写死 qq 目录时换 QQ 前缀
    let pinned_qq = !template.contains("{dir}") && template.contains("/qq-lyrics/");
    let default_key = if pinned_qq {
        "qq-lyrics/2116462216"
    } else {
        "ncm-lyrics/2116462216"
    };
    let key = sample_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(default_key);
    let Some(url) = lookup_url(&template, key, custom) else {
        return fail(
            "invalid_url",
            format!("样例键 {key} 无法按该模板拼出合法地址"),
            String::new(),
        );
    };
    let client = match amll_client(custom) {
        Ok(client) => client,
        Err(err) => return fail("unreachable", err, url),
    };
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(err) => {
            return fail(
                "unreachable",
                format!(
                    "无法连接：{}",
                    if err.is_timeout() {
                        "请求超时".to_string()
                    } else {
                        err.to_string()
                    }
                ),
                url,
            )
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return fail(
            "not_found",
            "服务器返回 404：地址可达但样例文件不存在（目录结构或模板可能不对）".into(),
            url,
        );
    }
    if !status.is_success() {
        return fail(
            "http_error",
            format!("服务器返回 HTTP {}", status.as_u16()),
            url,
        );
    }
    let bytes = match read_bytes_capped(response, super::ttml::MAX_TTML_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => return fail("http_error", format!("读取响应失败：{err}"), url),
    };
    let text = decode_lyric_bytes(&bytes);
    classify_ttml_body(&text, url)
}

/// 把响应正文分类：HTML 页面 / 非 TTML / TTML 但 0 行 / ok。
fn classify_ttml_body(text: &str, url: String) -> AmllTestResult {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let lower_head: String = trimmed
        .chars()
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    let fail = |kind: &str, message: &str| AmllTestResult {
        ok: false,
        kind: kind.to_string(),
        message: message.to_string(),
        url: url.clone(),
        lines: 0,
    };
    if lower_head.starts_with("<!doctype html") || lower_head.starts_with("<html") {
        return fail(
            "html",
            "返回的是网页而非 TTML（镜像可能把 404 渲染成了页面，请检查模板路径）",
        );
    }
    // 跳过 XML 声明 / 注释后必须是 <tt 根节点
    let mut rest = trimmed;
    loop {
        if let Some(after) = rest.strip_prefix("<?") {
            match after.find("?>") {
                Some(end) => rest = after[end + 2..].trim_start(),
                None => break,
            }
        } else if let Some(after) = rest.strip_prefix("<!--") {
            match after.find("-->") {
                Some(end) => rest = after[end + 3..].trim_start(),
                None => break,
            }
        } else {
            break;
        }
    }
    let is_tt_root = rest.strip_prefix("<tt").is_some_and(|after| {
        after.starts_with(|ch: char| ch.is_whitespace() || ch == '>' || ch == '/')
    });
    if !is_tt_root {
        return fail("invalid_xml", "响应不是 TTML 文档（根节点不是 <tt>）");
    }
    let lyrics = super::ttml::parse_ttml_lyrics(text);
    if lyrics.is_empty() {
        return fail(
            "unsupported_ttml",
            "是 TTML 文档但解析不出任何歌词行（可能是不支持的结构）",
        );
    }
    let word_synced = lyrics.iter().any(|line| line.words.is_some());
    AmllTestResult {
        ok: true,
        kind: "ok".into(),
        message: format!(
            "连接成功：解析出 {} 行{}",
            lyrics.len(),
            if word_synced {
                "逐字歌词"
            } else {
                "歌词（无逐字时间轴）"
            }
        ),
        url,
        lines: lyrics.len(),
    }
}

/// 用三源候选携带的平台 ID 去 DB 找 TTML；返回新候选（已去重，按原候选顺序）。
pub(crate) async fn fetch_amll_ttml_candidates(
    template: &str,
    custom: bool,
    seeds: &[OnlineLyricsCandidate],
) -> Vec<OnlineLyricsCandidate> {
    let Ok(client) = amll_client(custom) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut jobs = Vec::new();
    for seed in seeds {
        for key in &seed.ttml_lookup_keys {
            if jobs.len() >= MAX_TTML_LOOKUPS {
                break;
            }
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(url) = lookup_url(template, key, custom) {
                jobs.push((seed, key.clone(), url));
            }
        }
    }
    if jobs.is_empty() {
        return Vec::new();
    }

    let futures = jobs
        .iter()
        .map(|(_, _, url)| fetch_ttml(&client, url))
        .collect::<Vec<_>>();
    let results = join_all(futures).await;

    let mut candidates = Vec::new();
    for ((seed, key, _), lyrics) in jobs.into_iter().zip(results) {
        let Some(lyrics) = lyrics else {
            continue;
        };
        let word_synced = lyrics.iter().any(|line| line.words.is_some());
        candidates.push(OnlineLyricsCandidate {
            id: format!("ttml-{}", key.replace('/', "-")),
            source: if word_synced {
                format!("{AMLL_SOURCE_LABEL} · 逐字")
            } else {
                AMLL_SOURCE_LABEL.to_string()
            },
            title: seed.title.clone(),
            artist: seed.artist.clone(),
            album: seed.album.clone(),
            duration: seed.duration,
            lyrics,
            ttml_lookup_keys: Vec::new(),
        });
    }
    candidates
}

/// 极简 join_all：避免为此引入 futures crate。所有 future 并发推进，按原序收集。
async fn join_all<F: std::future::Future>(futures: Vec<F>) -> Vec<F::Output> {
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct JoinAll<F: std::future::Future> {
        pending: Vec<Option<Pin<Box<F>>>>,
        outputs: Vec<Option<F::Output>>,
    }

    // 字段只有 Pin<Box<_>> 与 Vec，整体可安全视为 Unpin
    impl<F: std::future::Future> Unpin for JoinAll<F> {}

    impl<F: std::future::Future> std::future::Future for JoinAll<F> {
        type Output = Vec<F::Output>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let mut all_done = true;
            for index in 0..this.pending.len() {
                if let Some(future) = this.pending[index].as_mut() {
                    match future.as_mut().poll(cx) {
                        Poll::Ready(output) => {
                            this.outputs[index] = Some(output);
                            this.pending[index] = None;
                        }
                        Poll::Pending => all_done = false,
                    }
                }
            }
            if all_done {
                Poll::Ready(
                    this.outputs
                        .iter_mut()
                        .map(|output| output.take().expect("all futures resolved"))
                        .collect(),
                )
            } else {
                Poll::Pending
            }
        }
    }

    let len = futures.len();
    JoinAll {
        pending: futures.into_iter().map(|f| Some(Box::pin(f))).collect(),
        outputs: (0..len).map(|_| None).collect(),
    }
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_guards_db_url() {
        assert_eq!(
            normalize_amll_db_url("", false).as_deref(),
            Some(DEFAULT_AMLL_TTML_DB_URL)
        );
        // 无占位符的 base URL 自动补路径
        assert_eq!(
            normalize_amll_db_url("https://amlldb.bikonoo.com/", false).as_deref(),
            Some("https://amlldb.bikonoo.com/{dir}/{id}.ttml")
        );
        // 带占位符的模板原样保留
        assert_eq!(
            normalize_amll_db_url("https://amlldb.bikonoo.com/ncm-lyrics/%s.ttml", true).as_deref(),
            Some("https://amlldb.bikonoo.com/ncm-lyrics/%s.ttml")
        );
        // 预设模式：只认 bikonoo；GitHub raw / jsDelivr 已下线
        assert!(normalize_amll_db_url(
            "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main",
            false
        )
        .is_none());
        assert!(normalize_amll_db_url(
            "https://cdn.jsdelivr.net/gh/amll-dev/amll-ttml-db@main",
            false
        )
        .is_none());
        assert!(normalize_amll_db_url("http://amlldb.bikonoo.com/x", false).is_none());
        assert!(normalize_amll_db_url("https://amlldb.bikonoo.com@evil.com/x", false).is_none());
        // 自定义模式：公网域放行，内网/IP/明文拒
        assert!(normalize_amll_db_url("https://example.org/db", true).is_some());
        assert!(normalize_amll_db_url("https://127.0.0.1/db", true).is_none());
        assert!(normalize_amll_db_url("https://localhost/db", true).is_none());
        assert!(normalize_amll_db_url("http://amlldb.bikonoo.com", true).is_none());
    }

    #[test]
    fn lookup_url_rejects_path_tricks_and_respects_pinned_dir() {
        let default = DEFAULT_AMLL_TTML_DB_URL;
        assert_eq!(
            lookup_url(default, "ncm-lyrics/123", false).as_deref(),
            Some("https://amlldb.bikonoo.com/ncm-lyrics/123.ttml")
        );
        // 模板写死 ncm-lyrics：QQ 候选跳过
        assert!(lookup_url(default, "qq-lyrics/123", false).is_none());

        let all = normalize_amll_db_url("https://amlldb.bikonoo.com", false).unwrap();
        assert_eq!(
            lookup_url(&all, "qq-lyrics/9", false).as_deref(),
            Some("https://amlldb.bikonoo.com/qq-lyrics/9.ttml")
        );
        assert!(lookup_url(&all, "ncm-lyrics/../../x", false).is_none());
        assert!(lookup_url(&all, "ncm-lyrics/1?x=1", false).is_none());
        assert!(lookup_url(&all, "123", false).is_none());
        assert!(lookup_url(&all, "qq-lyrics/", false).is_none());
    }

    #[tokio::test]
    async fn join_all_preserves_order() {
        type Boxed = std::pin::Pin<Box<dyn std::future::Future<Output = i32>>>;
        let outputs = join_all(vec![
            Box::pin(async { 1 }) as Boxed,
            Box::pin(async {
                tokio::task::yield_now().await;
                2
            }),
            Box::pin(async { 3 }),
        ])
        .await;
        assert_eq!(outputs, vec![1, 2, 3]);
    }

    /// 缓存是进程级的，两个直接操作 CACHE 的测试串行执行，避免互相干扰。
    static CACHE_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn hit(text: &str) -> CachedHit {
        CachedHit {
            lyrics: vec![LyricLine::new(1.0, text)],
            item: None,
        }
    }

    #[test]
    fn cache_hit_expiry_and_negative_entries() {
        let _serial = CACHE_TEST_LOCK.lock();
        // 先清空：淘汰测试留下的「未来时间戳」条目会让本测试的新条目成为最老而被淘汰
        CACHE.lock().clear();
        let now = Instant::now();
        cache_put_at("test:hit", Some(hit("a")), now);
        cache_put_at("test:miss", None, now);
        assert!(cache_get_at("test:unknown", now).is_none());

        // 正向命中：29 分钟仍在，30 分钟过期
        let got = cache_get_at("test:hit", now + Duration::from_secs(29 * 60)).flatten();
        assert_eq!(got.map(|h| h.lyrics[0].text.clone()).as_deref(), Some("a"));
        assert!(cache_get_at("test:hit", now + Duration::from_secs(30 * 60)).is_none());

        // 负缓存：4 分钟内命中 Some(None)，5 分钟后过期
        cache_put_at("test:miss", None, now);
        assert!(matches!(
            cache_get_at("test:miss", now + Duration::from_secs(4 * 60)),
            Some(None)
        ));
        assert!(cache_get_at("test:miss", now + Duration::from_secs(5 * 60)).is_none());
    }

    #[test]
    fn cache_evicts_oldest_when_full() {
        let _serial = CACHE_TEST_LOCK.lock();
        let now = Instant::now();
        // 先清空，保证容量断言不受其它测试影响
        CACHE.lock().clear();
        for i in 0..CACHE_CAPACITY {
            cache_put_at(
                &format!("evict:{i}"),
                Some(hit("x")),
                now + Duration::from_secs(i as u64),
            );
        }
        assert_eq!(CACHE.lock().len(), CACHE_CAPACITY);
        cache_put_at("evict:new", Some(hit("y")), now + Duration::from_secs(1000));
        let cache = CACHE.lock();
        assert_eq!(cache.len(), CACHE_CAPACITY);
        assert!(!cache.contains_key("evict:0"), "最老的一条应被淘汰");
        assert!(cache.contains_key("evict:new"));
        assert!(cache.contains_key("evict:1"));
    }

    #[test]
    fn api_items_deserialize_with_numeric_or_string_id() {
        let raw = r#"{"status":200,"data":{"items":[
            {"id":269710089745311,"filename":"a.ttml","musicNames":["晴天"],"artistNames":["周杰伦"],"albumNames":["叶惠美"],"ncmMusicIds":["1361348080"],"qqMusicIds":["0032UZe62rZk9K"],"appleMusicIds":[],"spotifyIds":[],"isrcs":[]},
            {"id":"42","musicNames":["b"],"artistNames":[],"albumNames":[],"ncmMusicIds":[],"qqMusicIds":[]}
        ],"pagination":{"page":1,"pageSize":5,"total":2,"totalPages":1,"hasMore":false}}}"#;
        let envelope: ApiEnvelope<ApiSearchData> = serde_json::from_str(raw).unwrap();
        assert_eq!(envelope.status, 200);
        let items = envelope.data.unwrap().items;
        assert_eq!(items[0].id, 269710089745311);
        assert_eq!(items[0].ncm_music_ids, vec!["1361348080"]);
        assert_eq!(items[1].id, 42);

        let raw_get = r#"{"status":200,"data":{"id":7,"musicNames":["x"],"lyrics":"<tt/>"}}"#;
        let envelope: ApiEnvelope<ApiGetData> = serde_json::from_str(raw_get).unwrap();
        let data = envelope.data.unwrap();
        assert_eq!(data.item.id, 7);
        assert_eq!(data.lyrics, "<tt/>");
    }

    fn item(id: u64, names: &[&str], artists: &[&str]) -> AmllApiItem {
        AmllApiItem {
            id,
            music_names: names.iter().map(|s| s.to_string()).collect(),
            artist_names: artists.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn api_matching_only_binds_high_confidence() {
        // 标题完全相等（忽略大小写/空白/标点）即 3 分，可绑定
        let picked =
            pick_best_api_item(vec![item(1, &["Hello World!"], &["A"])], "hello world", "");
        assert_eq!(picked.map(|i| i.id), Some(1));
        // 仅包含 = 1 分，不绑定
        assert!(
            pick_best_api_item(vec![item(2, &["晴天 (Live)"], &["周杰伦"])], "晴天", "").is_none()
        );
        // 包含 + 艺术家相等 = 3 分，绑定
        let picked = pick_best_api_item(
            vec![item(3, &["晴天 (Live)"], &["周杰伦"])],
            "晴天",
            "周杰伦",
        );
        assert_eq!(picked.map(|i| i.id), Some(3));
        // 标题不沾边：即使艺术家对也是 0 分
        assert!(
            pick_best_api_item(vec![item(4, &["七里香"], &["周杰伦"])], "晴天", "周杰伦").is_none()
        );
        // 多条取最高分
        let picked = pick_best_api_item(
            vec![
                item(5, &["晴天"], &["别人"]),
                item(6, &["晴天"], &["周杰伦"]),
            ],
            "晴天",
            "周杰伦",
        );
        assert_eq!(picked.map(|i| i.id), Some(6));
    }

    #[test]
    fn lookup_key_validation() {
        assert!(is_valid_lookup_key("ncm-lyrics/123"));
        assert!(is_valid_lookup_key("qq-lyrics/abc_1"));
        assert!(!is_valid_lookup_key("123"));
        assert!(!is_valid_lookup_key("ncm-lyrics/"));
        assert!(!is_valid_lookup_key("ncm-lyrics/../x"));
        assert!(!is_valid_lookup_key("ncm-lyrics/1/2"));
        assert!(!is_valid_lookup_key("a/1?x=1"));
    }

    #[test]
    fn classifies_test_connection_bodies() {
        let url = "https://amlldb.bikonoo.com/ncm-lyrics/1.ttml".to_string();
        assert_eq!(
            classify_ttml_body("  <!DOCTYPE HTML><html>", url.clone()).kind,
            "html"
        );
        assert_eq!(
            classify_ttml_body("<HTML lang=en>", url.clone()).kind,
            "html"
        );
        assert_eq!(
            classify_ttml_body("{\"status\":404}", url.clone()).kind,
            "invalid_xml"
        );
        assert_eq!(
            classify_ttml_body("<root><p/></root>", url.clone()).kind,
            "invalid_xml"
        );
        assert_eq!(
            classify_ttml_body("<ttml/>", url.clone()).kind,
            "invalid_xml"
        );
        assert_eq!(
            classify_ttml_body(
                "<?xml version=\"1.0\"?><tt xmlns=\"x\"><body/></tt>",
                url.clone()
            )
            .kind,
            "unsupported_ttml"
        );
        let ok = classify_ttml_body(
            "<tt xmlns=\"http://www.w3.org/ns/ttml\"><body><div><p begin=\"00:01.000\" end=\"00:02.000\">hi</p></div></body></tt>",
            url.clone(),
        );
        assert_eq!(ok.kind, "ok");
        assert!(ok.ok);
        assert_eq!(ok.lines, 1);
        assert_eq!(ok.url, url);
    }

    #[tokio::test]
    async fn test_connection_rejects_bad_template_without_network() {
        let result = run_amll_ttml_db_test("http://amlldb.bikonoo.com/x", false, None).await;
        assert_eq!(result.kind, "invalid_url");
        assert!(!result.ok);
        // 模板写死了 qq 目录时，默认样例键换 QQ 前缀，但 ncm 样例键会被 lookup_url 拒绝
        let result = run_amll_ttml_db_test(
            "https://amlldb.bikonoo.com/qq-lyrics/%s.ttml",
            false,
            Some("ncm-lyrics/1"),
        )
        .await;
        assert_eq!(result.kind, "invalid_url");
    }
}
