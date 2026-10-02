//! 子进程有界等待（R-03 / SEC-03）：运行、终止回收和 stderr 排空分别设期限。
//! 不以无限 `.wait()` / `JoinHandle::join()` 抵消前面的超时保护。

use std::{
    io::{self, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

const MAX_STDERR_BYTES: usize = 16 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const STDERR_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_READERS: usize = 8;
static ACTIVE_READERS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub(crate) enum WaitError {
    Spawn(io::Error),
    Wait(io::Error),
    TimedOut(Duration),
    Cancelled,
    StderrUnavailable,
}

impl std::fmt::Display for WaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "无法启动子进程：{err}"),
            Self::Wait(err) => write!(f, "等待或终止子进程失败：{err}"),
            Self::TimedOut(limit) => write!(f, "子进程超过 {} 秒未结束，已终止", limit.as_secs()),
            Self::Cancelled => write!(f, "已取消"),
            Self::StderrUnavailable => write!(f, "子进程错误输出未能在期限内读取完成"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct BoundedOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stderr: Vec<u8>,
}

/// 后代进程可能继承 stderr 写端。超时读取线程只能脱离，保留名额直至它实际退出，
/// 防止重复调用积累无界线程；正常结束时自动归还名额。
struct ReaderPermit;

impl ReaderPermit {
    fn acquire() -> Result<Self, WaitError> {
        ACTIVE_READERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_READERS).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| WaitError::Spawn(io::Error::other("子进程诊断读取繁忙，请稍后重试")))
    }
}

impl Drop for ReaderPermit {
    fn drop(&mut self) {
        ACTIVE_READERS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// stdout 丢弃、stderr 限量保存但持续排空。结束后最多另等一秒读取 stderr；超时或
/// 取消后 kill，再用 try_wait 最多等待两秒回收。终止失败必须报错，不伪称已终止。
pub(crate) fn run_bounded(
    command: &mut Command,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> Result<BoundedOutput, WaitError> {
    if cancelled() {
        return Err(WaitError::Cancelled);
    }
    let permit = ReaderPermit::acquire()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(WaitError::Spawn)?;
    let Some(stderr) = child.stderr.take() else {
        terminate(&mut child)?;
        return Err(WaitError::StderrUnavailable);
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    if let Err(err) = std::thread::Builder::new()
        .name("seraph-child-stderr".into())
        .spawn(move || {
            let _permit = permit;
            let _ = sender.send(capture_stderr(stderr));
        })
    {
        terminate(&mut child)?;
        return Err(WaitError::Spawn(err));
    }

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(BoundedOutput {
                    status,
                    stderr: finish_reading(receiver, STDERR_TIMEOUT)?,
                });
            }
            Ok(None) => {}
            Err(err) => {
                terminate(&mut child)?;
                return Err(WaitError::Wait(err));
            }
        }
        if cancelled() {
            terminate(&mut child)?;
            return Err(WaitError::Cancelled);
        }
        if started.elapsed() >= timeout {
            terminate(&mut child)?;
            return Err(WaitError::TimedOut(timeout));
        }
        std::thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
    }
}

fn capture_stderr(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(captured),
            Ok(count) => {
                let keep = count.min(MAX_STDERR_BYTES.saturating_sub(captured.len()));
                captured.extend_from_slice(&buffer[..keep]);
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
}

fn finish_reading(
    receiver: Receiver<io::Result<Vec<u8>>>,
    timeout: Duration,
) -> Result<Vec<u8>, WaitError> {
    receiver
        .recv_timeout(timeout)
        .map_err(|_| WaitError::StderrUnavailable)?
        .map_err(WaitError::Wait)
}

fn terminate(child: &mut Child) -> Result<(), WaitError> {
    if child.try_wait().map_err(WaitError::Wait)?.is_some() {
        return Ok(());
    }
    if let Err(err) = child.kill() {
        // kill 与正常退出可能同时发生，再检查一次。
        if child.try_wait().map_err(WaitError::Wait)?.is_none() {
            return Err(WaitError::Wait(err));
        }
    }
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(WaitError::Wait)?.is_some() {
            return Ok(());
        }
        if started.elapsed() >= CLEANUP_TIMEOUT {
            return Err(WaitError::Wait(io::Error::new(
                io::ErrorKind::TimedOut,
                "已请求终止，但子进程仍未退出",
            )));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_reader_deadline_does_not_wait_for_a_held_pipe() {
        let (_sender, receiver) = mpsc::sync_channel(1);
        let started = Instant::now();
        assert!(finish_reading(receiver, Duration::from_millis(20)).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stderr_is_drained_but_only_a_bounded_prefix_is_kept() {
        let bytes = vec![b'x'; MAX_STDERR_BYTES * 8];
        let mut reader = io::Cursor::new(bytes);
        let captured = capture_stderr(&mut reader).unwrap();
        assert_eq!(captured.len(), MAX_STDERR_BYTES);
        assert_eq!(reader.position(), (MAX_STDERR_BYTES * 8) as u64);
    }

    #[cfg(windows)]
    #[test]
    fn completed_process_reports_status_and_stderr() {
        let mut command = Command::new(crate::ipc::path_guard::system32_tool("cmd.exe"));
        command.args(["/D", "/C", "echo oops 1>&2 & exit 3"]);
        let output = run_bounded(&mut command, Duration::from_secs(10), || false).unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&output.stderr).contains("oops"));
    }

    #[cfg(windows)]
    #[test]
    fn hung_process_is_killed_at_the_deadline() {
        let started = Instant::now();
        let mut command = Command::new(crate::ipc::path_guard::system32_tool("ping.exe"));
        command.args(["-n", "30", "127.0.0.1"]);
        let err = run_bounded(&mut command, Duration::from_millis(300), || false).unwrap_err();
        assert!(matches!(err, WaitError::TimedOut(_)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn cancellation_before_spawn_does_not_run_the_command() {
        let mut command = Command::new("not-a-valid-executable");
        let err = run_bounded(&mut command, Duration::from_secs(1), || true).unwrap_err();
        assert!(matches!(err, WaitError::Cancelled));
    }

    #[cfg(windows)]
    #[test]
    fn cancellation_after_spawn_stops_the_process() {
        let started = Instant::now();
        let mut command = Command::new(crate::ipc::path_guard::system32_tool("ping.exe"));
        command.args(["-n", "30", "127.0.0.1"]);
        let err = run_bounded(&mut command, Duration::from_secs(10), || {
            started.elapsed() >= Duration::from_millis(100)
        })
        .unwrap_err();
        assert!(matches!(err, WaitError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
