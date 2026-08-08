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
