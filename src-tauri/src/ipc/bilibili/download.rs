//! 音频流下载（BILI-03：同一地址中断后带 `Range` 续传）。

use super::cancellation::ImportCancellation;
use super::prelude::*;
use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::StatusCode;

pub(crate) async fn download_audio_to_file(
    client: &Client,
    audio_url: &str,
    temp_path: &Path,
    cancellation: &ImportCancellation,
) -> Result<(), String> {
    cancellation.check()?;
    if audio_url.trim().is_empty() {
        return Err("empty bilibili audio url".into());
    }

    // playurl 的 baseUrl/backupUrl 属外部输入：下载不带 Cookie，但仍需白名单限制出站。
    if !is_safe_bilibili_download_url(audio_url) {
        return Err(format!(
            "拒绝从非 B 站 CDN 或非 https 地址下载音频: {audio_url}"
        ));
    }

    download_with_resume(
        client,
        audio_url,
        temp_path,
        cancellation,
        &DOWNLOAD_RESUME_BACKOFF,
    )
    .await
}

/// 一次请求的结局里，「换个时间再试同一地址可能成功」与「重试没有意义」要分开：
/// 前者续传，后者交回调用方换下一个候选地址。
enum AttemptError {
    /// 网络读中断、续传请求没连上、服务端 5xx
    Interrupted(String),
    /// 取消、写盘失败、超出体积上限、地址失效（4xx）
    Fatal(String),
}

/// 跨多次请求累计的下载进度。
#[derive(Default)]
struct DownloadProgress {
    /// 已写进临时文件的字节数，也是下一次续传的起点
    written: u64,
    /// 完整文件的大小（首个响应的 `Content-Length` 或续传响应的 `Content-Range`）
    total: Option<u64>,
    /// 首个响应给的强 ETag 或 Last-Modified，续传时放进 `If-Range`：
    /// 文件在两次请求之间变了，服务端会回 200 整个重发，而不是拼出一个错位的文件。
    validator: Option<HeaderValue>,
}

impl DownloadProgress {
    fn restart(&mut self) {
        *self = Self::default();
    }
}

/// `Content-Range: bytes 起-止/总长` 里的起点与总长（总长未知写作 `*`）。
pub(crate) fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.trim().split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let end = end.trim().parse::<u64>().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        value => Some(value.parse::<u64>().ok()?),
    };
    if total.is_some_and(|total| end >= total) {
        return None;
    }
    Some((start, total))
}

/// 把 `audio_url` 流式写进 `temp_path`。连接中途断开时，保留已写入的部分，
/// 按 `backoff` 等待后带 `Range` 接着下；连续 `backoff.len()` 次没有任何新进展、
/// 或总续传次数超过 [`MAX_DOWNLOAD_RESUMES`] 才放弃。
///
/// 首个请求一个字节都没拿到就失败时不重试——那是地址本身的问题，调用方换下一个候选地址。
/// 出错返回时临时文件可能残留，由调用方删除。
pub(crate) async fn download_with_resume(
    client: &Client,
    audio_url: &str,
    temp_path: &Path,
    cancellation: &ImportCancellation,
    backoff: &[Duration],
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    // L-13：流式写入临时文件，避免把整段 FLAC（数百 MB）整块驻留内存再二次复制。
    let mut file = tokio::fs::File::create(temp_path)
        .await
        .map_err(|err| format!("failed to create bilibili temp file: {err}"))?;
    let mut progress = DownloadProgress::default();
    let mut stalled = 0usize;
    let mut resumes = 0u32;

    let download_result = loop {
        let before = progress.written;
        let reason =
            match download_attempt(client, audio_url, &mut file, &mut progress, cancellation).await
            {
                Ok(()) => break Ok(()),
                Err(AttemptError::Fatal(reason)) => break Err(reason),
                Err(AttemptError::Interrupted(reason)) => reason,
            };
        if let Err(cancelled) = cancellation.check() {
            break Err(cancelled);
        }
        if progress.written == 0 && resumes == 0 {
            break Err(reason);
        }
        if progress.written > before {
            stalled = 0;
        }
        let Some(delay) = backoff.get(stalled).copied() else {
            break Err(format!("{reason}（续传 {stalled} 次仍没有新数据，已放弃）"));
        };
        if resumes >= MAX_DOWNLOAD_RESUMES {
            break Err(format!("{reason}（已续传 {resumes} 次，已放弃）"));
        }
        stalled += 1;
        resumes += 1;
        tracing::info!(
            offset = progress.written,
            attempt = resumes,
            "bilibili 音频下载中断，稍后续传: {reason}"
        );
        if let Err(cancelled) = cancellation
            .run(async {
                tokio::time::sleep(delay).await;
                Ok(())
            })
            .await
        {
            break Err(cancelled);
        }
    };

    // Tokio 文件可能还有内部阻塞写任务；成功或取消都先 flush，再允许调用方删临时文件。
    let flush_result = file
        .flush()
        .await
        .map_err(|err| format!("failed to flush bilibili temp file: {err}"));
    drop(file);
    download_result.and(flush_result)
}

/// 丢弃已下载的部分，下一次从头请求。
async fn restart_download(
    file: &mut tokio::fs::File,
    progress: &mut DownloadProgress,
) -> Result<(), AttemptError> {
    use tokio::io::AsyncSeekExt;

    progress.restart();
    file.set_len(0)
        .await
        .map_err(|err| AttemptError::Fatal(format!("failed to reset bilibili temp file: {err}")))?;
    file.seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(|err| AttemptError::Fatal(format!("failed to reset bilibili temp file: {err}")))?;
    Ok(())
}

fn status_error(response: reqwest::Response) -> AttemptError {
    let server_error = response.status().is_server_error();
    let reason = match response.error_for_status() {
        Err(err) => format!("bilibili audio download failed: {err}"),
        Ok(response) => format!(
            "bilibili audio download failed: unexpected HTTP status {}",
            response.status()
        ),
    };
    if server_error {
        AttemptError::Interrupted(reason)
    } else {
        AttemptError::Fatal(reason)
    }
}

async fn download_attempt(
    client: &Client,
    audio_url: &str,
    file: &mut tokio::fs::File,
    progress: &mut DownloadProgress,
    cancellation: &ImportCancellation,
) -> Result<(), AttemptError> {
    use tokio::io::AsyncWriteExt;

    let resume_from = progress.written;
    let mut request = client.get(audio_url);
    if resume_from > 0 {
        request = request.header(RANGE, format!("bytes={resume_from}-"));
        if let Some(validator) = &progress.validator {
            request = request.header(IF_RANGE, validator.clone());
        }
    }
    let mut response = cancellation
        .run(async { Ok(request.send().await) })
        .await
        .map_err(AttemptError::Fatal)?
        .map_err(|err| {
            AttemptError::Interrupted(format!("failed to download bilibili audio: {err}"))
        })?;

    let status = response.status();
    if resume_from > 0 && status == StatusCode::PARTIAL_CONTENT {
        let range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range);
        let consistent = range.is_some_and(|(start, total)| {
            start == resume_from
                && match (total, progress.total) {
                    (Some(now), Some(before)) => now == before,
                    _ => true,
                }
        });
        if !consistent {
            // 这份响应体接不上已有的部分，不能写进文件；整个重来。
            restart_download(file, progress).await?;
            return Err(AttemptError::Interrupted(
                "服务器返回的续传区间与本地进度不一致，改为从头下载".into(),
            ));
        }
        if progress.total.is_none() {
            progress.total = range.and_then(|(_, total)| total);
        }
    } else if resume_from > 0 && status == StatusCode::RANGE_NOT_SATISFIABLE {
        restart_download(file, progress).await?;
        return Err(AttemptError::Interrupted(
            "服务器不接受续传区间，改为从头下载".into(),
        ));
    } else if status.is_success() && status != StatusCode::PARTIAL_CONTENT {
        // 首个请求；或续传时服务端忽略了 Range、文件已变（If-Range 不匹配）——
        // 响应体是完整文件，已写的部分作废。
        if resume_from > 0 {
            restart_download(file, progress).await?;
        }
        progress.total = response.content_length();
        progress.validator = response
            .headers()
            .get(ETAG)
            .filter(|value| !value.as_bytes().starts_with(b"W/"))
            .or_else(|| response.headers().get(LAST_MODIFIED))
            .cloned();
    } else {
        return Err(status_error(response));
    }

    // 提前检查声明的总长；超出上限直接拒绝，避免下载到一半才发现。
    if let Some(total) = progress.total {
        if total > MAX_AUDIO_DOWNLOAD_BYTES {
            return Err(AttemptError::Fatal(format!(
                "bilibili audio too large: declared {total} bytes (limit {MAX_AUDIO_DOWNLOAD_BYTES})"
            )));
        }
    }

    loop {
        let chunk = cancellation
            .run(async { Ok(response.chunk().await) })
            .await
            .map_err(AttemptError::Fatal)?
            .map_err(|err| AttemptError::Interrupted(format!("network read error: {err}")))?;
        let Some(chunk) = chunk else {
            break;
        };
        if progress.written.saturating_add(chunk.len() as u64) > MAX_AUDIO_DOWNLOAD_BYTES {
            return Err(AttemptError::Fatal(format!(
                "bilibili audio exceeded {MAX_AUDIO_DOWNLOAD_BYTES} bytes; aborted"
            )));
        }
        // 一次写入完成后再检查取消，保证关闭/删除临时文件时没有遗留在途磁盘写。
        file.write_all(&chunk).await.map_err(|err| {
            AttemptError::Fatal(format!("failed to write bilibili temp file: {err}"))
        })?;
        // 只把确实写进文件的字节计入进度——它就是下一次续传的起点。
        progress.written += chunk.len() as u64;
    }

    if progress.written == 0 {
        return Err(AttemptError::Fatal(
            "downloaded bilibili audio is empty".into(),
        ));
    }
    match progress.total {
        Some(total) if progress.written < total => Err(AttemptError::Interrupted(format!(
            "bilibili audio download ended early: {} of {total} bytes",
            progress.written
        ))),
        Some(total) if progress.written > total => Err(AttemptError::Fatal(format!(
            "bilibili audio download size mismatch: got {} bytes, expected {total}",
            progress.written
        ))),
        _ => cancellation.check().map_err(AttemptError::Fatal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// 一份应答：`declared_len` 大于 `body` 的长度时，连接会在响应体中途断开。
    struct Reply {
        status: &'static str,
        headers: Vec<String>,
        declared_len: usize,
        body: Vec<u8>,
    }

    impl Reply {
        fn full(body: &[u8]) -> Self {
            Self {
                status: "200 OK",
                headers: Vec::new(),
                declared_len: body.len(),
                body: body.to_vec(),
            }
        }

        /// 声明完整长度，但只发出前 `sent` 个字节就断开
        fn cut(body: &[u8], sent: usize) -> Self {
            Self {
                status: "200 OK",
                headers: Vec::new(),
                declared_len: body.len(),
                body: body[..sent].to_vec(),
            }
        }

        /// 206：从 `start` 起的剩余部分
        fn partial(body: &[u8], start: usize) -> Self {
            Self {
                status: "206 Partial Content",
                headers: vec![format!(
                    "Content-Range: bytes {start}-{}/{}",
                    body.len() - 1,
                    body.len()
                )],
                declared_len: body.len() - start,
                body: body[start..].to_vec(),
            }
        }

        fn with_header(mut self, header: &str) -> Self {
            self.headers.push(header.to_string());
            self
        }

        fn status(mut self, status: &'static str) -> Self {
            self.status = status;
            self
        }
    }

    fn read_request_head(stream: &mut TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => break,
            }
        }
        String::from_utf8_lossy(&head).to_string()
    }

    /// 请求头里某个字段的值（不区分大小写）
    fn header_value(head: &str, name: &str) -> Option<String> {
        head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    fn range_start(head: &str) -> Option<usize> {
        header_value(head, "range")?
            .strip_prefix("bytes=")?
            .strip_suffix('-')?
            .parse()
            .ok()
    }

    /// 本机一次性 HTTP 服务：按顺序对每个连接调用 `script`（参数是第几个请求与请求头），
    /// 发完应答即断开。返回地址与收到的全部请求头。
    fn serve(
        requests: usize,
        script: impl Fn(usize, &str) -> Reply + Send + 'static,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/audio.m4s", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_server = Arc::clone(&seen);
        std::thread::spawn(move || {
            for index in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let head = read_request_head(&mut stream);
                let reply = script(index, &head);
                seen_by_server.lock().unwrap().push(head);
                let mut response = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    reply.status, reply.declared_len
                );
                for header in &reply.headers {
                    response.push_str(header);
                    response.push_str("\r\n");
                }
                response.push_str("\r\n");
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&reply.body);
                let _ = stream.flush();
                let _ = stream.shutdown(Shutdown::Both);
            }
        });
        (url, seen)
    }

    fn test_client() -> Client {
        Client::builder().no_proxy().build().unwrap()
    }

    fn temp_file(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "seraph-bili03-{label}-{}-{}.download",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn sample_body() -> Vec<u8> {
        (0..200_000u32).map(|value| (value % 251) as u8).collect()
    }

    const FAST: [Duration; 3] = [Duration::from_millis(1); 3];

    async fn download(url: &str, path: &Path, backoff: &[Duration]) -> Result<(), String> {
        download_with_resume(
            &test_client(),
            url,
            path,
            &ImportCancellation::default(),
            backoff,
        )
        .await
    }

    #[test]
    fn bili03_content_range_parsing() {
        assert_eq!(
            parse_content_range("bytes 100-199/200"),
            Some((100, Some(200)))
        );
        assert_eq!(parse_content_range(" bytes 0-0/* "), Some((0, None)));
        // 残缺、倒置、越过总长、非字节单位一律不认
        for bad in [
            "",
            "bytes */200",
            "bytes 200-100/300",
            "bytes 100-200/200",
            "items 0-1/2",
            "bytes 1-2",
            "bytes a-b/c",
        ] {
            assert_eq!(parse_content_range(bad), None, "{bad}");
        }
    }

    /// 连接中途断开后带 `Range` 接着下，已下载的部分不重来，拼出来的文件与原文件一致。
    #[tokio::test]
    async fn bili03_interrupted_download_resumes_with_range() {
        let body = sample_body();
        let served = body.clone();
        let (url, seen) = serve(3, move |index, head| match index {
            0 => Reply::cut(&served, 60_000).with_header("ETag: \"v1\""),
            // 第二次也断一回：续传可以连续发生
            1 => {
                let start = range_start(head).expect("resume request carries Range");
                let mut reply = Reply::partial(&served, start);
                reply.body.truncate(50_000);
                reply
            }
            _ => Reply::partial(&served, range_start(head).expect("Range")),
        });
        let path = temp_file("resume");

        download(&url, &path, &FAST)
            .await
            .expect("download resumes");

        assert_eq!(std::fs::read(&path).unwrap(), body);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(range_start(&seen[0]), None, "首个请求不带 Range");
        let first = range_start(&seen[1]).unwrap();
        let second = range_start(&seen[2]).unwrap();
        assert!(first > 0 && first <= 60_000, "从已写入的位置续传: {first}");
        assert!(second > first, "第二次续传接在第一次之后: {second}");
        // 首个响应给了强 ETag：续传必须带 If-Range，文件变了才不会拼错
        assert_eq!(
            header_value(&seen[1], "if-range").as_deref(),
            Some("\"v1\"")
        );
        let _ = std::fs::remove_file(&path);
    }

    /// 服务端忽略 Range（或文件已变）回 200 整个重发：已写的部分作废，不能接在后面。
    #[tokio::test]
    async fn bili03_full_response_to_a_range_request_restarts_from_zero() {
        let body = sample_body();
        let changed: Vec<u8> = body.iter().map(|byte| byte.wrapping_add(1)).collect();
        let (first, second) = (body.clone(), changed.clone());
        let (url, seen) = serve(2, move |index, _| match index {
            0 => Reply::cut(&first, 70_000),
            _ => Reply::full(&second),
        });
        let path = temp_file("restart");

        download(&url, &path, &FAST)
            .await
            .expect("download restarts");

        assert_eq!(std::fs::read(&path).unwrap(), changed);
        assert!(range_start(&seen.lock().unwrap()[1]).is_some());
        let _ = std::fs::remove_file(&path);
    }

    /// 续传响应的区间接不上本地进度（起点不对、总长变了）或服务端回 416：
    /// 这份响应体不写进文件，改为从头下载。
    #[tokio::test]
    async fn bili03_mismatched_or_unsatisfiable_range_restarts_from_zero() {
        for case in ["wrong-start", "wrong-total", "416"] {
            let body = sample_body();
            let served = body.clone();
            let (url, seen) = serve(3, move |index, head| match index {
                0 => Reply::cut(&served, 40_000),
                1 => {
                    let start = range_start(head).expect("Range");
                    match case {
                        "wrong-start" => Reply::partial(&served, start + 10),
                        "wrong-total" => {
                            let mut longer = served.clone();
                            longer.extend_from_slice(b"tail");
                            Reply::partial(&longer, start)
                        }
                        _ => Reply::full(b"").status("416 Range Not Satisfiable"),
                    }
                }
                _ => Reply::full(&served),
            });
            let path = temp_file(case);

            download(&url, &path, &FAST)
                .await
                .unwrap_or_else(|err| panic!("{case}: {err}"));

            assert_eq!(std::fs::read(&path).unwrap(), body, "{case}");
            let seen = seen.lock().unwrap();
            assert_eq!(seen.len(), 3, "{case}");
            assert_eq!(range_start(&seen[2]), None, "{case}: 重来的请求不带 Range");
            let _ = std::fs::remove_file(&path);
        }
    }

    /// 一直没有新数据：退避次数用完就放弃，不无限重试。
    #[tokio::test]
    async fn bili03_gives_up_after_consecutive_stalled_resumes() {
        let body = sample_body();
        let served = body.clone();
        // 首个请求给一部分，之后每次续传都是 503
        let (url, seen) = serve(1 + FAST.len(), move |index, _| match index {
            0 => Reply::cut(&served, 30_000),
            _ => Reply::full(b"").status("503 Service Unavailable"),
        });
        let path = temp_file("stalled");

        let err = download(&url, &path, &FAST).await.unwrap_err();

        assert!(err.contains("已放弃"), "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1 + FAST.len());
        let _ = std::fs::remove_file(&path);
    }

    /// 首个请求就被拒绝（地址失效）或一个字节都没拿到：不重试，交回调用方换地址；
    /// 续传途中遇到 4xx 同样立即放弃这个地址。
    #[tokio::test]
    async fn bili03_address_level_failures_are_not_retried() {
        let (url, seen) = serve(1, |_, _| Reply::full(b"").status("403 Forbidden"));
        let path = temp_file("forbidden");
        let err = download(&url, &path, &FAST).await.unwrap_err();
        assert!(err.contains("403"), "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        let _ = std::fs::remove_file(&path);

        let (url, seen) = serve(1, |_, _| Reply::cut(b"abcdef", 0));
        let path = temp_file("empty");
        assert!(download(&url, &path, &FAST).await.is_err());
        assert_eq!(seen.lock().unwrap().len(), 1, "零进度的首个请求不续传");
        let _ = std::fs::remove_file(&path);

        let body = sample_body();
        let served = body.clone();
        let (url, seen) = serve(2, move |index, _| match index {
            0 => Reply::cut(&served, 30_000),
            _ => Reply::full(b"").status("404 Not Found"),
        });
        let path = temp_file("expired");
        let err = download(&url, &path, &FAST).await.unwrap_err();
        assert!(err.contains("404"), "{err}");
        assert_eq!(seen.lock().unwrap().len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    /// 等待续传期间取消：立刻返回，不再发请求。
    #[tokio::test]
    async fn bili03_cancellation_during_backoff_stops_the_download() {
        let body = sample_body();
        let served = body.clone();
        let (url, seen) = serve(2, move |index, _| match index {
            0 => Reply::cut(&served, 30_000),
            _ => Reply::full(&served),
        });
        let path = temp_file("cancel");
        let cancellation = ImportCancellation::default();
        let token = cancellation.clone();
        let client = test_client();
        let target = path.clone();
        let job = tokio::spawn(async move {
            download_with_resume(&client, &url, &target, &token, &[Duration::from_secs(30)]).await
        });
        // 等首个请求结束、进入退避
        for _ in 0..200 {
            if seen.lock().unwrap().len() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancellation.cancel();

        let result = tokio::time::timeout(Duration::from_secs(5), job)
            .await
            .expect("cancellation interrupts the backoff")
            .unwrap();
        assert!(result.is_err());
        assert_eq!(seen.lock().unwrap().len(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
