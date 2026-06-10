//! FFmpeg CLI fallback decoder for formats Symphonia cannot handle.

use crate::decoder::{Decoder, DecoderError, Packet, StreamInfo};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{
    env,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    thread,
};

const PACKET_FRAMES: usize = 2048;
/// 收集 stderr 的尾部限额（字节）。ffmpeg 失败时只需最后几条诊断信息。
const STDERR_TAIL_LIMIT: usize = 4096;

/// Windows 上启动子进程时隐藏控制台窗口，避免 cmd 黑窗一闪而过。
/// 0x0800_0000 = CREATE_NO_WINDOW（来自 winbase.h，纯 u32 常量，无 winapi 依赖）。
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

static EXTRA_TOOL_DIRS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

pub struct FfmpegDecoder {
    path: Option<PathBuf>,
    info: Option<StreamInfo>,
    child: Option<Child>,
    stdout: Option<ChildStdout>,
    stderr_tail: Option<Arc<Mutex<Vec<u8>>>>,
    stderr_thread: Option<thread::JoinHandle<()>>,
    frames_read: u64,
    base_seconds: f64,
    sample_scratch: Vec<f32>,
}

impl FfmpegDecoder {
    pub fn new() -> Self {
        Self {
            path: None,
            info: None,
            child: None,
            stdout: None,
            stderr_tail: None,
            stderr_thread: None,
            frames_read: 0,
            base_seconds: 0.0,
            sample_scratch: Vec::new(),
        }
    }

    fn start_process(&mut self, start_seconds: f64) -> Result<(), DecoderError> {
        self.stop_process();
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| DecoderError::Internal("ffmpeg decoder is not open".into()))?;

        let mut command = Command::new(ffmpeg_command_path());
        hide_console_window(&mut command);
        command.arg("-v").arg("error");
        if start_seconds > 0.0 {
            // -ss 放在 -i 之前是 fast seek（关键帧粒度）。对 VBR mp3/AAC
            // 这可能差几十毫秒。准确 seek 需要 -ss 放 -i 之后（重新编码全程），
            // 但代价是数秒延迟，不适合实时拖动进度条。这里牺牲精度换流畅。
            command.arg("-ss").arg(format!("{start_seconds:.6}"));
        }
        command
            .arg("-i")
            .arg(path)
            .arg("-map")
            .arg("0:a:0")
            .arg("-vn")
            .arg("-f")
            .arg("f32le")
            .arg("-acodec")
            .arg("pcm_f32le")
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(map_tool_spawn_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DecoderError::Internal("ffmpeg stdout is unavailable".into()))?;
        let stderr = child.stderr.take();
        let stderr_tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_thread = stderr.map(|mut stderr| {
            let sink = stderr_tail.clone();
            thread::spawn(move || drain_stderr(&mut stderr, sink))
        });
        self.child = Some(child);
        self.stdout = Some(stdout);
        self.stderr_tail = Some(stderr_tail);
        self.stderr_thread = stderr_thread;
        self.frames_read = 0;
        self.base_seconds = start_seconds.max(0.0);
        Ok(())
    }

    fn stop_process(&mut self) {
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
        self.stderr_tail = None;
    }

    /// 检查 ffmpeg 进程是否非正常退出，返回收集到的 stderr 尾部信息（如果有）。
    fn take_stderr_tail(&mut self) -> Option<String> {
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
        let tail = self.stderr_tail.take()?;
        let bytes = tail.lock().ok()?.clone();
        if bytes.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&bytes).trim().to_string())
        }
    }

    /// EOF 时调用：若子进程已退出且 exit_code != 0，把 stderr 尾部当成失败原因返回。
    /// 正常播完返回 None。
    fn detect_crash(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        let status = child.try_wait().ok().flatten()?;
        if status.success() {
            return None;
        }
        self.take_stderr_tail()
            .or_else(|| Some(format!("ffmpeg exited with status {status}")))
    }
}

impl Default for FfmpegDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FfmpegDecoder {
    fn drop(&mut self) {
        self.stop_process();
    }
}

impl Decoder for FfmpegDecoder {
    fn open(&mut self, path: &Path) -> Result<(), DecoderError> {
        let info = probe_with_ffprobe(path)?;
        self.path = Some(path.to_path_buf());
        self.info = Some(info);
        self.start_process(0.0)?;
        Ok(())
    }

    fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, DecoderError> {
        if self.stdout.is_none() {
            self.start_process(self.base_seconds)?;
        }

        let info = self
            .info
            .as_ref()
            .ok_or_else(|| DecoderError::Internal("ffmpeg decoder is not open".into()))?;
        let channels = usize::from(info.channels.0).max(1);
        let frame_bytes = channels * std::mem::size_of::<f32>();
        let bytes_per_packet = PACKET_FRAMES * frame_bytes;
        let mut bytes = vec![0_u8; bytes_per_packet];
        let mut filled = 0;

        while filled < bytes.len() {
            let read = self
                .stdout
                .as_mut()
                .ok_or_else(|| DecoderError::Internal("ffmpeg stdout is unavailable".into()))?
                .read(&mut bytes[filled..])
                .map_err(DecoderError::Io)?;
            if read == 0 {
                break;
            }
            filled += read;
        }

        if filled == 0 {
            // EOF：检查进程退出码区分"正常播完"与"中途崩溃"
            let crash_reason = self.detect_crash();
            self.stop_process();
            if let Some(reason) = crash_reason {
                return Err(DecoderError::Internal(format!("ffmpeg aborted: {reason}")));
            }
            return Ok(None);
        }

        // 关键修复：filled 截断必须按"声道 × 4 字节"对齐，
        // 否则末包字节数不是声道整数倍 → 后续帧串扰。
        let aligned = filled - (filled % frame_bytes);
        if aligned == 0 {
            self.stop_process();
            return Ok(None);
        }
        bytes.truncate(aligned);

        let timestamp_seconds =
            self.base_seconds + self.frames_read as f64 / f64::from(info.sample_rate.0.max(1));
        let sample_count = aligned / std::mem::size_of::<f32>();
        self.sample_scratch.clear();
        self.sample_scratch.reserve(sample_count);
        bytes_to_f32_into(&bytes, &mut self.sample_scratch);
        self.frames_read += (sample_count / channels) as u64;

        Ok(Some(Packet {
            samples: std::mem::take(&mut self.sample_scratch),
            timestamp_seconds,
        }))
    }

    fn seek(&mut self, seconds: f64) -> Result<(), DecoderError> {
        // L-3: 近距向前 seek（<2s）跳过重启 ffmpeg 进程，改为读丢中间样本。
        // 频繁拖动进度条时启动新进程要 ~50–100ms，体验明显卡顿。
        let target = seconds.max(0.0);
        let current = self.base_seconds
            + self
                .info
                .as_ref()
                .map(|info| self.frames_read as f64 / f64::from(info.sample_rate.0.max(1)))
                .unwrap_or(0.0);
        let delta = target - current;
        if self.stdout.is_some() && delta >= 0.0 && delta < 2.0 {
            if let Some(info) = self.info.as_ref() {
                let channels = usize::from(info.channels.0).max(1);
                let frames_to_skip = (delta * f64::from(info.sample_rate.0.max(1))).round() as u64;
                let bytes_to_skip = frames_to_skip
                    .saturating_mul(channels as u64)
                    .saturating_mul(std::mem::size_of::<f32>() as u64);
                if bytes_to_skip == 0 {
                    return Ok(());
                }
                let mut remaining = bytes_to_skip;
                let mut skipped = 0_u64;
                let mut sink = [0_u8; 8192];
                if let Some(stdout) = self.stdout.as_mut() {
                    while remaining > 0 {
                        let chunk = remaining.min(sink.len() as u64) as usize;
                        match stdout.read(&mut sink[..chunk]) {
                            Ok(0) => break,
                            Ok(n) => {
                                remaining = remaining.saturating_sub(n as u64);
                                skipped += n as u64;
                            }
                            Err(err) => return Err(DecoderError::Io(err)),
                        }
                    }
                }
                // L-10：按累计跳过字节一次性换算帧数，避免逐 chunk 整除丢余数。
                self.frames_read += frames_in_byte_run(skipped, channels);
                return Ok(());
            }
        }
        // 远距 / 反向 seek：重启 ffmpeg 进程到目标时间
        self.start_process(target)
    }
}

fn probe_with_ffprobe(path: &Path) -> Result<StreamInfo, DecoderError> {
    let mut command = Command::new(ffprobe_command_path());
    hide_console_window(&mut command);
    let output = command
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a:0")
        .arg("-show_entries")
        .arg("stream=sample_rate,channels,bits_per_raw_sample,bits_per_sample,duration")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1")
        .arg(path)
        .output()
        .map_err(map_tool_spawn_error)?;

    if !output.status.success() {
        return Err(DecoderError::UnsupportedFormat(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    parse_ffprobe_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ffprobe_output(output: &str) -> Result<StreamInfo, DecoderError> {
    // ffprobe `-of default=noprint_wrappers=1` 输出 stream 段后跟 format 段，
    // 两段都用 `duration=xxx`（没有 section 前缀）。优先取 stream，
    // 再 fallback 到 format 或 TAG:DURATION（FLAC/MKV 容器才有）。
    let mut sample_rate = None;
    let mut channels = None;
    let mut bit_depth = None;
    let mut duration = None;

    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "sample_rate" => sample_rate = value.parse::<u32>().ok(),
            "channels" => channels = value.parse::<u16>().ok(),
            "bits_per_raw_sample" | "bits_per_sample" if value != "N/A" => {
                bit_depth = value.parse::<u16>().ok()
            }
            "duration" | "TAG:DURATION" if duration.is_none() => {
                duration = parse_duration(value);
            }
            _ => {}
        }
    }

    let sample_rate = sample_rate
        .filter(|value| *value > 0)
        .ok_or_else(|| DecoderError::UnsupportedFormat("ffprobe missing sample_rate".into()))?;
    let channels = channels
        .filter(|value| *value > 0)
        .ok_or_else(|| DecoderError::UnsupportedFormat("ffprobe missing channels".into()))?;

    Ok(StreamInfo {
        sample_rate: SampleRate(sample_rate),
        bit_depth: BitDepth(bit_depth.unwrap_or(16)),
        channels: Channels(channels),
        duration_seconds: duration.unwrap_or(0.0),
    })
}

fn parse_duration(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// 把 little-endian f32 字节流写入 output buffer，复用调用方的 Vec 避免分配。
fn bytes_to_f32_into(bytes: &[u8], output: &mut Vec<f32>) {
    for chunk in bytes.chunks_exact(4) {
        output.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
}

/// 把跳读累计的总字节数换算成完整帧数。L-10：必须对**累计**字节做一次整除，
/// 而不是对每个 read chunk 各自整除——多声道 frame_bytes 不整除读缓冲时，
/// 逐 chunk 丢弃余数会让时间戳持续偏小漂移。
fn frames_in_byte_run(byte_count: u64, channels: usize) -> u64 {
    let frame_bytes = (channels.max(1) * std::mem::size_of::<f32>()) as u64;
    byte_count / frame_bytes
}

/// 后台读取 ffmpeg stderr，保留尾部 STDERR_TAIL_LIMIT 字节供失败诊断。
fn drain_stderr(stderr: &mut ChildStderr, sink: Arc<Mutex<Vec<u8>>>) {
    let mut buffer = [0_u8; 1024];
    while let Ok(read) = stderr.read(&mut buffer) {
        if read == 0 {
            break;
        }
        if let Ok(mut tail) = sink.lock() {
            tail.extend_from_slice(&buffer[..read]);
            let overflow = tail.len().saturating_sub(STDERR_TAIL_LIMIT);
            if overflow > 0 {
                tail.drain(..overflow);
            }
        }
    }
}

fn map_tool_spawn_error(err: std::io::Error) -> DecoderError {
    if err.kind() == std::io::ErrorKind::NotFound {
        DecoderError::UnsupportedFormat("ffmpeg/ffprobe executable not found".into())
    } else {
        DecoderError::Io(err)
    }
}

/// Register application-specific directories that may contain ffmpeg tools.
///
/// This lets the Tauri shell provide bundled locations such as
/// `%APPDATA%/.../ffmpeg` while the decoder crate remains UI-runtime agnostic.
pub fn configure_ffmpeg_search_dirs<I, P>(dirs: I)
where
    I: IntoIterator<Item = P>,
    P: Into<PathBuf>,
{
    let mut changed = false;
    {
        let mut stored = EXTRA_TOOL_DIRS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .expect("ffmpeg search dirs mutex poisoned");
        for dir in dirs {
            let dir = dir.into();
            if !stored.iter().any(|existing| existing == &dir) {
                stored.push(dir);
                changed = true;
            }
        }
    }
    if changed {
        invalidate_tool_cache();
    }
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    cached_tool_path("ffmpeg", &FFMPEG_CACHE)
}

pub fn find_ffprobe() -> Option<PathBuf> {
    cached_tool_path("ffprobe", &FFPROBE_CACHE)
}

static FFMPEG_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static FFPROBE_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// 缓存工具路径：每次 open/seek 都会走 ffmpeg/ffprobe，
/// 重复遍历 PATH+candidate dirs+stat 每个候选 5–10 ms，热路径上无意义。
fn cached_tool_path(tool: &str, cache: &OnceLock<Mutex<Option<PathBuf>>>) -> Option<PathBuf> {
    let mutex = cache.get_or_init(|| Mutex::new(None));
    let mut guard = mutex.lock().ok()?;
    if let Some(path) = guard.as_ref() {
        if path.is_file() {
            return Some(path.clone());
        }
    }
    let resolved = find_tool(tool);
    *guard = resolved.clone();
    resolved
}

/// 任何对 EXTRA_TOOL_DIRS 的修改都应失效缓存，否则后注册的目录永远轮不到。
fn invalidate_tool_cache() {
    if let Some(mutex) = FFMPEG_CACHE.get() {
        if let Ok(mut guard) = mutex.lock() {
            *guard = None;
        }
    }
    if let Some(mutex) = FFPROBE_CACHE.get() {
        if let Ok(mut guard) = mutex.lock() {
            *guard = None;
        }
    }
}

fn ffmpeg_command_path() -> PathBuf {
    find_ffmpeg().unwrap_or_else(|| PathBuf::from(tool_exe_name("ffmpeg")))
}

fn ffprobe_command_path() -> PathBuf {
    find_ffprobe().unwrap_or_else(|| PathBuf::from(tool_exe_name("ffprobe")))
}

fn find_tool(tool: &str) -> Option<PathBuf> {
    tool_candidates(tool)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn tool_candidates(tool: &str) -> Vec<PathBuf> {
    let exe_name = tool_exe_name(tool);
    let mut candidates = Vec::new();

    if let Some(path) = tool_env_path(tool) {
        candidates.push(path);
    }

    if let Some(lock) = EXTRA_TOOL_DIRS.get() {
        let dirs = lock.lock().expect("ffmpeg search dirs mutex poisoned");
        candidates.extend(dirs.iter().map(|dir| dir.join(&exe_name)));
    }

    if let Some(dirs) = env::var_os("SERAPH_FFMPEG_DIRS") {
        candidates.extend(env::split_paths(&dirs).map(|dir| dir.join(&exe_name)));
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&exe_name));
            candidates.push(dir.join("ffmpeg").join(&exe_name));
        }
    }

    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|dir| dir.join(&exe_name)));
    }

    dedupe_paths(candidates)
}

fn tool_env_path(tool: &str) -> Option<PathBuf> {
    let key = match tool {
        "ffmpeg" => "SERAPH_FFMPEG_PATH",
        "ffprobe" => "SERAPH_FFPROBE_PATH",
        _ => return None,
    };
    env::var_os(key).map(PathBuf::from)
}

fn tool_exe_name(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    }
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::with_capacity(paths.len());
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_stream_info() {
        let info = parse_ffprobe_output(
            "sample_rate=96000\nchannels=2\nbits_per_raw_sample=24\nduration=12.5\n",
        )
        .unwrap();

        assert_eq!(info.sample_rate, SampleRate(96_000));
        assert_eq!(info.channels, Channels(2));
        assert_eq!(info.bit_depth, BitDepth(24));
        assert!((info.duration_seconds - 12.5).abs() < 0.001);
    }

    #[test]
    fn converts_f32le_bytes_to_samples() {
        let bytes = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
        let mut samples = Vec::new();
        bytes_to_f32_into(&bytes, &mut samples);

        assert_eq!(samples, vec![0.25, -0.5]);
    }

    #[test]
    fn frames_in_byte_run_accumulates_without_per_chunk_truncation() {
        // 6 声道 f32：frame_bytes = 24，8192 % 24 = 8。三个 8192 chunk 的余数
        // 累计到 24 = 整一帧。逐 chunk 整除会少算这一帧。
        let channels = 6;
        let total = 8192_u64 * 3; // 24576
        assert_eq!(frames_in_byte_run(total, channels), 24576 / 24); // 1024
        let per_chunk_sum = (8192 / 24) * 3; // 旧实现：1023
        assert_eq!(per_chunk_sum, 1023);
        assert_eq!(frames_in_byte_run(total, channels), 1024);
    }
}
