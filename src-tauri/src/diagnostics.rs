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

#[cfg(test)]
mod tests {
    use super::*;

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
