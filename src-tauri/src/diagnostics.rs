//! 有界队列异步写本地日志；正式版也启用，最多保留当前文件和三个轮转文件。

use crossbeam_channel::{bounded, Sender};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::LazyLock,
};
use tauri::{AppHandle, Manager};
use tracing_subscriber::{fmt::MakeWriter, EnvFilter};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 16 * 1024;
const BACKUPS: usize = 3;

#[derive(Clone)]
struct LogSink(Option<Sender<Vec<u8>>>);

struct LogRecord {
    sender: Option<Sender<Vec<u8>>>,
    bytes: Vec<u8>,
}

impl Write for LogRecord {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let keep = bytes
            .len()
            .min(MAX_RECORD_BYTES.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..keep]);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogRecord {
    fn drop(&mut self) {
        if !self.bytes.ends_with(b"\n") {
            self.bytes.push(b'\n');
        }
        if let Some(sender) = &self.sender {
            // 慢盘或队列已满时丢弃诊断记录，不反压业务线程。
            let _ = sender.try_send(std::mem::take(&mut self.bytes));
        } else {
            let _ = io::stderr().lock().write_all(&redact_record(&self.bytes));
        }
    }
}

impl<'a> MakeWriter<'a> for LogSink {
    type Writer = LogRecord;
    fn make_writer(&'a self) -> Self::Writer {
        LogRecord {
            sender: self.0.clone(),
            bytes: Vec::with_capacity(256),
        }
    }
}

/// 诊断只保留 HTTP 地址的主机/路径；敏感字段后面的整行保守移除，
/// 兼容 Cookie 多段值、Authorization 空格及 Debug/JSON 引号形式。
fn redact_record(bytes: &[u8]) -> Vec<u8> {
    static SECRET: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?im)\b(authorization|proxy-authorization|cookie(?:_?header)?|set-cookie|sessdata|bili_jct|(?:access_?|refresh_?|csrf_?)?token|csrf|password|qrcode_?key|client_secret)\b[ \t"'\\]*[:=][^\r\n]*"#).expect("valid secret field pattern")
    });
    static URL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)https?://[^\s"'<>]+"#).expect("valid URL pattern")
    });
    let text = String::from_utf8_lossy(bytes);
    let fields = SECRET.replace_all(&text, "$1=[redacted]");
    URL.replace_all(&fields, |captures: &regex::Captures<'_>| {
        let matched = captures.get(0).expect("whole URL match").as_str();
        let token = matched.trim_end_matches([')', ']', '}', ',', ';']);
        let suffix = &matched[token.len()..];
        match reqwest::Url::parse(token) {
            Ok(mut url) => {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                if url.query().is_some() {
                    url.set_query(Some("redacted"));
                }
                if url.fragment().is_some() {
                    url.set_fragment(Some("redacted"));
                }
                format!("{url}{suffix}")
            }
            Err(_) => format!("[redacted-url]{suffix}"),
        }
    })
    .into_owned()
    .into_bytes()
}

struct RotatingFile {
    root: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}

impl RotatingFile {
    fn open(root: &Path, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("seraph.log"))?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            root: root.to_path_buf(),
            file: Some(file),
            bytes,
            limit,
        })
    }

    fn write_record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.bytes > 0 && self.bytes + bytes.len() as u64 > self.limit {
            // Windows rename 前关闭句柄；只操作固定日志文件名。
            self.file.take();
            let oldest = self.root.join(format!("seraph.{BACKUPS}.log"));
            if oldest.exists() {
                fs::remove_file(oldest)?;
            }
            for index in (1..BACKUPS).rev() {
                let from = self.root.join(format!("seraph.{index}.log"));
                if from.exists() {
                    fs::rename(from, self.root.join(format!("seraph.{}.log", index + 1)))?;
                }
            }
            fs::rename(self.root.join("seraph.log"), self.root.join("seraph.1.log"))?;
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.root.join("seraph.log"))?,
            );
            self.bytes = 0;
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("日志文件不可用"))?
            .write_all(bytes)?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
}

fn make_sink(root: &Path) -> io::Result<LogSink> {
    let mut file = Some(RotatingFile::open(root, MAX_FILE_BYTES)?);
    let (sender, receiver) = bounded::<Vec<u8>>(256);
    std::thread::Builder::new()
        .name("seraph-log".into())
        .spawn(move || {
            while let Ok(bytes) = receiver.recv() {
                let bytes = redact_record(&bytes);
                let _ = io::stderr().lock().write_all(&bytes);
                if let Some(Err(error)) = file.as_mut().map(|file| file.write_record(&bytes)) {
                    eprintln!("诊断日志写入失败：{error}");
                    file = None;
                }
            }
        })?;
    Ok(LogSink(Some(sender)))
}

pub(crate) fn init(app: &AppHandle) {
    let filter =
        || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("seraph=info,warn"));
    let sink = app
        .path()
        .app_log_dir()
        .map_err(|error| io::Error::other(error.to_string()))
        .and_then(|root| make_sink(&root));
    let (sink, error) = match sink {
        Ok(sink) => (sink, None),
        Err(error) => (LogSink(None), Some(error)),
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_ansi(false)
        .with_writer(sink)
        .try_init();
    if let Some(error) = error {
        tracing::warn!("无法创建本地诊断日志：{error}");
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "播放器启动");
}

/// 单条前端日志的最大字符数（按 char 截断，不切碎中文）。
const MAX_FRONTEND_MESSAGE_CHARS: usize = 2000;
const MAX_FRONTEND_SCOPE_CHARS: usize = 64;
/// 每个窗口在一个窗口期内最多接收的前端日志条数，超出丢弃（防刷屏把真实记录轮转掉）。
const FRONTEND_LOG_BURST: usize = 30;
const FRONTEND_LOG_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// 换行类字符 → 空格、按 char 截断：前端文本不可信（可能含外部歌词 / 元数据），
/// 不能在日志里伪造出额外的行。凭据字段与 URL 查询串由写入线程统一脱敏。
fn sanitize_frontend_log(scope: &str, message: &str) -> (String, String) {
    fn single_line(text: &str, limit: usize) -> String {
        let mut out = String::with_capacity(text.len().min(limit * 3));
        for (count, ch) in text.chars().enumerate() {
            if count == limit {
                out.push('…');
                break;
            }
            out.push(match ch {
                '\n' | '\r' | '\u{2028}' | '\u{2029}' | '\u{85}' => ' ',
                ch if ch.is_control() => ' ',
                ch => ch,
            });
        }
        out
    }
    (
        single_line(scope, MAX_FRONTEND_SCOPE_CHARS),
        single_line(message, MAX_FRONTEND_MESSAGE_CHARS),
    )
}

#[derive(Default)]
struct FrontendLogLimiter {
    windows: std::collections::HashMap<String, (std::time::Instant, usize)>,
}

impl FrontendLogLimiter {
    fn allow(&mut self, window: &str, now: std::time::Instant) -> bool {
        // 窗口 label 只有 main / taskbar-lyrics（命令白名单已拒绝其它 label），表不会增长
        let entry = self.windows.entry(window.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= FRONTEND_LOG_WINDOW {
            *entry = (now, 0);
        }
        if entry.1 >= FRONTEND_LOG_BURST {
            return false;
        }
        entry.1 += 1;
        true
    }
}

static FRONTEND_LIMITER: LazyLock<parking_lot::Mutex<FrontendLogLimiter>> =
    LazyLock::new(Default::default);

/// REL-06：前端（发布版没有 devtools）的警告、未捕获异常写进同一份诊断日志。
/// 限长、单行化、按窗口限频；两个窗口都可调用（歌词条的 IPC 失败此前被 `.catch` 吞掉）。
#[tauri::command]
pub(crate) fn log_frontend(
    window: tauri::WebviewWindow,
    level: String,
    scope: String,
    message: String,
) {
    let label = window.label().to_string();
    if !FRONTEND_LIMITER
        .lock()
        .allow(&label, std::time::Instant::now())
    {
        return;
    }
    let (scope, message) = sanitize_frontend_log(&scope, &message);
    if level == "error" {
        tracing::error!(target: "seraph::frontend", window = %label, scope = %scope, "{message}");
    } else {
        tracing::warn!(target: "seraph::frontend", window = %label, scope = %scope, "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_log_is_single_line_bounded_and_labelled() {
        // REL-06：前端日志进同一份诊断文件，不得用换行伪造其它日志行，长度有上限
        let message = format!("第一行\n伪造 INFO 行\r\u{2028}{}", "x".repeat(5000));
        let (scope, text) = sanitize_frontend_log("sync\nscope", &message);
        assert_eq!(scope, "sync scope");
        assert!(!text.contains(['\n', '\r', '\u{2028}']));
        assert!(text.starts_with("第一行 伪造 INFO 行"));
        assert!(text.chars().count() <= MAX_FRONTEND_MESSAGE_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn frontend_log_limiter_caps_bursts_per_window() {
        let mut limiter = FrontendLogLimiter::default();
        let start = std::time::Instant::now();
        let accepted = (0..100).filter(|_| limiter.allow("main", start)).count();
        assert_eq!(accepted, FRONTEND_LOG_BURST);
        // 另一个窗口有独立配额；窗口期过后恢复
        assert!(limiter.allow("taskbar-lyrics", start));
        assert!(limiter.allow("main", start + FRONTEND_LOG_WINDOW));
    }

    #[test]
    fn diagnostics_redact_credentials_and_http_query_strings() {
        let input = concat!(
            "下载失败 https://user:pass@example.com/audio?id=private#fragment)\n",
            "WARN Cookie: SESSDATA=private-cookie; bili_jct=private-csrf\n",
            "WARN Authorization = Bearer private-bearer\n",
            "WARN {\"refreshToken\": \"private-refresh\", \"stage\": 1}\n",
            "WARN playback failed track_id=local-1\n",
        );
        let clean = String::from_utf8(redact_record(input.as_bytes())).unwrap();
        assert!(clean.contains("https://example.com/audio?redacted#redacted)"));
        assert!(!clean.contains("private") && !clean.contains("user:pass"));
        assert!(clean.contains("Cookie=[redacted]"));
        assert!(clean.contains("Authorization=[redacted]"));
        assert!(clean.contains("refreshToken=[redacted]"));
        assert!(clean.contains("playback failed track_id=local-1"));
        assert_eq!(clean.lines().count(), input.lines().count());
    }

    #[test]
    fn rotation_preserves_recent_records_with_bounded_file_count() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("seraph-log-test-{unique}"));
        let mut file = RotatingFile::open(&root, 8).unwrap();
        for index in 0..10 {
            file.write_record(format!("line {index}\n").as_bytes())
                .unwrap();
        }
        drop(file);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 4);
        assert_eq!(
            fs::read_to_string(root.join("seraph.log")).unwrap(),
            "line 9\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("seraph.3.log")).unwrap(),
            "line 6\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
