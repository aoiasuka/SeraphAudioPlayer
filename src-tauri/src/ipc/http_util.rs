//! 外部 HTTP 响应的公共读取防线(安全审查 S-03)。
//!
//! 所有出站请求的响应体一律经 capped 增量读取落地——HTTP 响应体天然无界,
//! 上游被攻破/CDN 异常返回超大响应时,`Response::bytes()` / `Response::json()`
//! 会把全部字节读进内存。新增网络调用时禁止直接 `.bytes()` / `.json()`,
//! 统一走本模块的 [`read_bytes_capped`] / [`read_json_capped`]。

use serde::de::DeserializeOwned;

/// 外部 JSON 响应体默认上限。正常 API 响应(搜索/歌词/元数据)在几十 KB 量级,
/// 8 MB 已远超合理上限,仅作为异常响应的止损线。
pub(crate) const MAX_EXTERNAL_JSON_BYTES: u64 = 8 * 1024 * 1024;

/// 增量读取响应体,超出上限即中止并报错;不信任 Content-Length。
/// 避免恶意服务器或异常重定向把进程内存撑爆。
pub(crate) async fn read_bytes_capped(
    mut response: reqwest::Response,
    cap: u64,
) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("network read error: {err}"))?
    {
        if (buf.len() as u64).saturating_add(chunk.len() as u64) > cap {
            return Err(format!("response body exceeded {cap} bytes; aborted"));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// capped 读取 + JSON 反序列化,替代无界的 `Response::json()`。
pub(crate) async fn read_json_capped<T: DeserializeOwned>(
    response: reqwest::Response,
    cap: u64,
) -> Result<T, String> {
    let bytes = read_bytes_capped(response, cap).await?;
    serde_json::from_slice(&bytes).map_err(|err| format!("failed to parse json: {err}"))
}

/// 跟随重定向的最大跳数。B 站 CDN 正常只有 1~2 跳。
const MAX_REDIRECT_HOPS: usize = 5;

/// F-01/F-04/F-07:**逐跳**复验重定向目标的 URL 白名单策略。
///
/// 只在首个 URL 上做白名单不够——reqwest 默认会跟随最多 10 跳,首跳之后的每一跳
/// 都是一次真实出站请求,不拦就是盲 SSRF 探针(可探内网/localhost 端口),
/// 且最终响应体会被当作可信内容落盘(音频缓存直接交给解码器执行链)。
/// 只复验**最终** host(`resolve_bvid` 的 S-15 口径)也只挡住结果、挡不住探测本身,
/// 所以这里逐跳拦截,并且不合规直接 `error` 而非 `stop`——
/// `stop` 会把 3xx 响应原样交回调用方,反而被误当成正常响应处理。
///
/// 新增「跟随重定向 + 白名单」的出站请求一律走这里,别只校验首个 URL。
pub(crate) fn guarded_redirect_policy(allow: fn(&str) -> bool) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECT_HOPS {
            return attempt.error("too many redirects");
        }
        if allow(attempt.url().as_str()) {
            attempt.follow()
        } else {
            let rejected = format!("redirect target is not on the allowlist: {}", attempt.url());
            attempt.error(rejected)
        }
    })
}
