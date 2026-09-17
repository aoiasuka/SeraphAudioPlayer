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

use super::prelude::*;
use crate::ipc::url_guard::{is_public_https_url, is_safe_amll_ttml_url};

/// 默认 DB 地址（社区镜像 amlldb.bikonoo.com，网易云目录模板）。
pub(crate) const DEFAULT_AMLL_TTML_DB_URL: &str = "https://amlldb.bikonoo.com/ncm-lyrics/%s.ttml";

/// 每次搜索最多试取多少个候选的 TTML（每个 = 一次 GET，绝大多数 404）。
const MAX_TTML_LOOKUPS: usize = 6;

pub(crate) const AMLL_SOURCE_LABEL: &str = "AMLL TTML DB";

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

fn lookup_url(template: &str, key: &str, custom: bool) -> Option<String> {
    // key 形如 `ncm-lyrics/123`：只允许 [目录]/[id] 两段、字符集受限，
    // 防止候选 ID 里混入 `..` 或查询串拼出白名单外的路径
    let (dir, id) = key.split_once('/')?;
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    };
    if !valid(dir) || !valid(id) {
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
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = read_bytes_capped(response, super::ttml::MAX_TTML_BYTES)
        .await
        .ok()?;
    let text = decode_lyric_bytes(&bytes);
    let lyrics = super::ttml::parse_ttml_lyrics(&text);
    (!lyrics.is_empty()).then_some(lyrics)
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
}
