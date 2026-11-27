//! FFmpeg CLI fallback decoder for formats Symphonia cannot handle.

use crate::decoder::{validate_stream_info, Decoder, DecoderError, Packet, StreamInfo};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{
    env,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, Command, Stdio},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

const PACKET_FRAMES: usize = 2048;
/// 收集 stderr 的尾部限额（字节）。ffmpeg 失败时只需最后几条诊断信息。
const STDERR_TAIL_LIMIT: usize = 4096;
/// DEC-01：读线程每次从管道读取的块大小。
const PIPE_CHUNK_BYTES: usize = 64 * 1024;
/// DEC-01：读线程最多预读多少块。写满后它阻塞在通道上、ffmpeg 随之阻塞在写管道上，
/// 背压与此前「解码线程直接读管道」时一样；预读量上限约 1 MiB。
const PIPE_QUEUE_CHUNKS: usize = 16;
/// DEC-01：解码线程等 ffmpeg 输出的最长时间。
///
/// 正常情况下 ffmpeg 解码远快于实时，数据总是现成的；要等的只有进程刚启动、长时间暂停后
/// 恢复（休眠的机械盘 / NAS 要重新起转）和源文件读不动这几种情形。连续这么久一个字节都
/// 没有就判定源不可用：结束进程、向上报错。此前这里是不带时限的阻塞读——源文件在断连的
/// 网络盘上时解码线程永远醒不过来，引擎 5 秒后只能放弃等它，而它名下的 ffmpeg 进程会一直
/// 留到应用退出，反复切到这类文件就越积越多。
///
/// 取 30 秒：比打开时 ffprobe 的 20 秒宽，因为播放中途被判死比打不开更打扰人，要给磁盘
/// 起转留足余量；它只是「卡死」的兜底，不是性能指标。这是**单次等待**的上限，只要有数据
/// 陆续到来就不会触发。
const OUTPUT_STALL_TIMEOUT: Duration = Duration::from_secs(30);
/// R-03：kill 之后等子进程退出、等两个读线程收尾的总时限。
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

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

/// 把一个会阻塞的字节源（ffmpeg 的 stdout 管道）搬到独立线程上读，解码线程带超时地取（DEC-01）。
///
/// 匿名管道的读取没有超时可设，所以由一个专门的线程去阻塞：它把读到的数据经有界通道
/// 交过来，这一侧用 `recv_timeout` 等。源结束时读线程退出、通道断开，这一侧据此识别 EOF。
/// 用完的块经另一条通道还回去复用，稳态下不再分配。
struct PipeReader {
    data: Receiver<io::Result<Vec<u8>>>,
    recycle: Sender<Vec<u8>>,
    /// 正在消费的一块及已读到的位置
    current: Vec<u8>,
    offset: usize,
    /// 源已结束或已出错，不会再有数据
    ended: bool,
    thread: thread::JoinHandle<()>,
}

impl PipeReader {
    fn spawn<R: Read + Send + 'static>(source: R) -> io::Result<Self> {
        let (data_tx, data) = mpsc::sync_channel(PIPE_QUEUE_CHUNKS);
        let (recycle, recycled) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("seraph-ffmpeg-stdout".into())
            .spawn(move || pump_pipe(source, &data_tx, &recycled))?;
        Ok(Self {
            data,
            recycle,
            current: Vec::new(),
            offset: 0,
            ended: false,
            thread,
        })
    }

    /// 换上下一块。`Ok(false)` 表示源已结束；等了 `timeout` 还没有数据返回 `TimedOut`。
    fn next_chunk(&mut self, timeout: Duration) -> io::Result<bool> {
        if self.ended {
            return Ok(false);
        }
        match self.data.recv_timeout(timeout) {
            Ok(Ok(chunk)) => {
                let spent = std::mem::replace(&mut self.current, chunk);
                self.offset = 0;
                // 读线程已经退出时发不出去，丢掉即可
                let _ = self.recycle.send(spent);
                Ok(true)
            }
            Ok(Err(err)) => {
                self.ended = true;
                Err(err)
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.ended = true;
                Ok(false)
            }
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no data arrived on the pipe in time",
            )),
        }
    }

    /// 读满 `buffer`，或读到源结束为止；返回读到的字节数。每次等数据最多等 `timeout`，
    /// 等不到返回 `TimedOut`——此时已经取出的那部分数据作废（调用方随即结束进程）。
    fn read_full(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let mut filled = 0;
        while filled < buffer.len() {
            if self.offset >= self.current.len() {
                if !self.next_chunk(timeout)? {
                    break;
                }
                continue;
            }
            let take = (self.current.len() - self.offset).min(buffer.len() - filled);
            buffer[filled..filled + take]
                .copy_from_slice(&self.current[self.offset..self.offset + take]);
            self.offset += take;
            filled += take;
        }
        Ok(filled)
    }

    /// 读掉并丢弃最多 `bytes` 字节；返回实际丢弃的字节数（源提前结束时会少）。
    fn skip(&mut self, bytes: u64, timeout: Duration) -> io::Result<u64> {
        let mut skipped = 0_u64;
        while skipped < bytes {
            if self.offset >= self.current.len() {
                if !self.next_chunk(timeout)? {
                    break;
                }
                continue;
            }
            let available = (self.current.len() - self.offset) as u64;
            let take = available.min(bytes - skipped);
            self.offset += take as usize;
            skipped += take;
        }
        Ok(skipped)
    }

    /// 放掉接收端——读线程若正阻塞在通道上会立刻返回——并交出线程句柄，由调用方限时等待。
    fn into_thread(self) -> thread::JoinHandle<()> {
        self.thread
    }
}

/// [`PipeReader`] 的读线程：一块一块地读，读到什么交什么。
fn pump_pipe<R: Read>(
    mut source: R,
    data: &SyncSender<io::Result<Vec<u8>>>,
    recycled: &Receiver<Vec<u8>>,
) {
    loop {
        // 不先清空：`resize` 只需要把上一轮没用到的尾部补零，读到多少随后再截断
        let mut chunk = recycled.try_recv().unwrap_or_default();
        chunk.resize(PIPE_CHUNK_BYTES, 0);
        match source.read(&mut chunk) {
            // 源结束：函数返回后发送端被丢弃，接收方看到通道断开
            Ok(0) => return,
            Ok(read) => {
                chunk.truncate(read);
                // 接收方已经放手（解码器停止 / 被丢弃）
                if data.send(Ok(chunk)).is_err() {
                    return;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => {
                let _ = data.send(Err(err));
                return;
            }
        }
    }
}

/// R-03：结束子进程并在 `deadline` 之前回收它。返回它是否确实退出了。
fn terminate_child(child: &mut Child, deadline: Instant) -> bool {
    const POLL: Duration = Duration::from_millis(10);
    if matches!(child.try_wait(), Ok(Some(_))) {
        return true;
    }
    // kill 与正常退出可能同时发生：成败都以随后的 try_wait 为准
    let _ = child.kill();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            _ => return false,
        }
    }
}

/// R-03：在 `deadline` 之前等线程结束并取回它的返回值。到点还没结束就放手（返回 `None`）——
/// 它阻塞在管道读取上，管道另一端关闭后会自己退出，不值得为它钉住调用线程。
fn join_until<T>(handle: thread::JoinHandle<T>, deadline: Instant) -> Option<T> {
    const POLL: Duration = Duration::from_millis(5);
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(POLL);
    }
    handle.join().ok()
}

pub struct FfmpegDecoder {
    path: Option<PathBuf>,
    info: Option<StreamInfo>,
    child: Option<Child>,
    /// ffmpeg 的 stdout，经读线程转交（DEC-01）
    reader: Option<PipeReader>,
    stderr_tail: Option<Arc<Mutex<Vec<u8>>>>,
    stderr_thread: Option<thread::JoinHandle<()>>,
    frames_read: u64,
    base_seconds: f64,
    /// F-10：正常 EOF 后置位，仅 seek/重启进程可清。
    /// 否则轮询式调用方在 EOF 后再调 next_packet 会自动重启进程重播尾段。
    finished: bool,
    /// DEC-01：解码进程卡住被结束后的原因。置位期间 next_packet 一直报这个错——既不当成
    /// 正常播完，也不悄悄从头重启进程；同样只有 seek / 重启进程能清。
    failure: Option<String>,
    /// 等 ffmpeg 输出的时限（测试里会调短）
    stall_timeout: Duration,
    sample_scratch: Vec<f32>,
    /// F-16：每包字节缓冲复用，避免热路径每包 vec! 分配。
    byte_scratch: Vec<u8>,
}

impl FfmpegDecoder {
    pub fn new() -> Self {
        Self {
            path: None,
            info: None,
            child: None,
            reader: None,
            stderr_tail: None,
            stderr_thread: None,
            frames_read: 0,
            base_seconds: 0.0,
            finished: false,
            failure: None,
            stall_timeout: OUTPUT_STALL_TIMEOUT,
            sample_scratch: Vec::new(),
            byte_scratch: Vec::new(),
        }
    }

    fn start_process(&mut self, start_seconds: f64) -> Result<(), DecoderError> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| DecoderError::Internal("ffmpeg decoder is not open".into()))?;

        let mut command = Command::new(ffmpeg_command_path()?);
        command.arg("-v").arg("error");
        if start_seconds > 0.0 {
            // -ss 放在 -i 之前是 fast seek（关键帧粒度）。对 VBR mp3/AAC
            // 这可能差几十毫秒。准确 seek 需要 -ss 放 -i 之后（重新编码全程），
            // 但代价是数秒延迟，不适合实时拖动进度条。这里牺牲精度换流畅。
            command.arg("-ss").arg(format!("{start_seconds:.6}"));
        }
        command
            .arg("-i")
            .arg(ffmpeg_input_arg(path))
            .arg("-map")
            .arg("0:a:0")
            .arg("-vn")
            .arg("-f")
            .arg("f32le")
            .arg("-acodec")
            .arg("pcm_f32le")
            .arg("-");
        self.spawn_decoding_process(command, start_seconds)
    }

    /// 启动参数已经配好的解码进程，接上 stdout / stderr 两个读线程。
    fn spawn_decoding_process(
        &mut self,
        mut command: Command,
        start_seconds: f64,
    ) -> Result<(), DecoderError> {
        self.stop_process();
        hide_console_window(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(map_tool_spawn_error)?;
        let reader = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("ffmpeg stdout is unavailable"))
            .and_then(PipeReader::spawn);
        let reader = match reader {
            Ok(reader) => reader,
            Err(err) => {
                // 进程已经起来了，不能把它丢在那儿
                terminate_child(&mut child, Instant::now() + CLEANUP_TIMEOUT);
                return Err(DecoderError::Io(err));
            }
        };
        let stderr_tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_thread = child.stderr.take().and_then(|mut stderr| {
            let sink = stderr_tail.clone();
            thread::Builder::new()
                .name("seraph-ffmpeg-stderr".into())
                .spawn(move || drain_stderr(&mut stderr, sink))
                .ok()
        });
        self.child = Some(child);
        self.reader = Some(reader);
        self.stderr_tail = Some(stderr_tail);
        self.stderr_thread = stderr_thread;
        self.frames_read = 0;
        self.base_seconds = start_seconds.max(0.0);
        self.finished = false;
        self.failure = None;
        Ok(())
    }

    /// 结束解码进程并回收两个读线程。R-03：每一步都有时限（合计 [`CLEANUP_TIMEOUT`]），
    /// 此前 kill 之后是不带时限的 `wait()` 与 `join()`。
    fn stop_process(&mut self) {
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        // 先放掉数据通道的接收端：读线程若正阻塞在通道上会立刻返回
        let stdout_thread = self.reader.take().map(PipeReader::into_thread);
        if let Some(mut child) = self.child.take() {
            if !terminate_child(&mut child, deadline) {
                tracing::warn!(
                    pid = child.id(),
                    "ffmpeg did not exit within {CLEANUP_TIMEOUT:?} after kill"
                );
            }
        }
        // 进程退出后管道写端关闭，两个读线程随即读到结尾；等不到就放手
        for thread in [stdout_thread, self.stderr_thread.take()]
            .into_iter()
            .flatten()
        {
            let _ = join_until(thread, deadline);
        }
        self.stderr_tail = None;
    }

    /// 取走目前收集到的 stderr 尾部（如果有）。调用时进程已经退出，读线程正常情况下
    /// 立刻结束；等不到也不强求（R-03），拿已经收到的部分。
    fn take_stderr_tail(&mut self) -> Option<String> {
        if let Some(thread) = self.stderr_thread.take() {
            let _ = join_until(thread, Instant::now() + CLEANUP_TIMEOUT);
        }
        let tail = self.stderr_tail.take()?;
        let bytes = tail
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if bytes.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&bytes).trim().to_string())
        }
    }

    /// DEC-01：等了 [`Self::stall_timeout`] 仍没有输出。结束进程并记住原因。
    fn fail_stalled(&mut self) -> DecoderError {
        let message = format!(
            "ffmpeg produced no audio data for {:.0} s; the source may be unreachable",
            self.stall_timeout.as_secs_f64()
        );
        tracing::warn!("{message}");
        self.stop_process();
        self.failure = Some(message.clone());
        stalled_error(message)
    }

    /// EOF 时调用：若子进程非正常退出，把 stderr 尾部当成失败原因返回。
    /// 正常播完返回 None。
    /// 审2-7：ffmpeg 关闭 stdout 与进程真正退出之间存在时间窗，单次 try_wait
    /// 在窗口内返回 None 会把中途崩溃误判成正常播完。改为轮询等待退出码，
    /// 上限 500ms：正常 EOF 后 ffmpeg 毫秒级退出，超时视为仍在运行（维持
    /// 原"无崩溃"语义，由随后的 stop_process kill 兜底），不会挂住解码线程。
    fn detect_crash(&mut self) -> Option<String> {
        const EXIT_WAIT_TIMEOUT: Duration = Duration::from_millis(500);
        const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

        let child = self.child.as_mut()?;
        let deadline = Instant::now() + EXIT_WAIT_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    thread::sleep(EXIT_POLL_INTERVAL);
                }
                Err(_) => return None,
            }
        };
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
        // F-11：不再在 open 时 start_process(0.0)——next_packet 已有懒启动，
        // 扫库 probe 场景不必为每个文件白 spawn 一个全量解码进程再立刻 kill。
        self.finished = false;
        Ok(())
    }

    fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, DecoderError> {
        if let Some(message) = &self.failure {
            return Err(stalled_error(message.clone()));
        }
        if self.finished {
            return Ok(None);
        }
        if self.reader.is_none() {
            self.start_process(self.base_seconds)?;
        }

        let info = self
            .info
            .as_ref()
            .ok_or_else(|| DecoderError::Internal("ffmpeg decoder is not open".into()))?;
        let channels = usize::from(info.channels.0).max(1);
        let frame_bytes = channels * std::mem::size_of::<f32>();
        let bytes_per_packet = PACKET_FRAMES * frame_bytes;
        let timestamp_seconds =
            self.base_seconds + self.frames_read as f64 / f64::from(info.sample_rate.0.max(1));
        self.byte_scratch.clear();
        self.byte_scratch.resize(bytes_per_packet, 0);

        let stall_timeout = self.stall_timeout;
        let read = self
            .reader
            .as_mut()
            .ok_or_else(|| DecoderError::Internal("ffmpeg stdout is unavailable".into()))?
            .read_full(&mut self.byte_scratch, stall_timeout);
        let filled = match read {
            Ok(filled) => filled,
            // DEC-01：此前这里是不带时限的阻塞读
            Err(err) if err.kind() == io::ErrorKind::TimedOut => return Err(self.fail_stalled()),
            Err(err) => return Err(DecoderError::Io(err)),
        };

        if filled == 0 {
            // EOF：检查进程退出码区分"正常播完"与"中途崩溃"
            let crash_reason = self.detect_crash();
            self.stop_process();
            self.finished = true;
            if let Some(reason) = crash_reason {
                return Err(DecoderError::Internal(format!("ffmpeg aborted: {reason}")));
            }
            return Ok(None);
        }

        // 关键修复：filled 截断必须按"声道 × 4 字节"对齐，
        // 否则末包字节数不是声道整数倍 → 后续帧串扰。
        let aligned = filled - (filled % frame_bytes);
        if aligned == 0 {
            // F-10：写出半帧后 EOF 同样要走崩溃检测，否则 ffmpeg
            // 半帧崩溃会被当成正常播完。
            let crash_reason = self.detect_crash();
            self.stop_process();
            self.finished = true;
            if let Some(reason) = crash_reason {
                return Err(DecoderError::Internal(format!("ffmpeg aborted: {reason}")));
            }
            return Ok(None);
        }

        let sample_count = aligned / std::mem::size_of::<f32>();
        self.sample_scratch.clear();
        self.sample_scratch.reserve(sample_count);
        bytes_to_f32_into(&self.byte_scratch[..aligned], &mut self.sample_scratch);
        self.frames_read += (sample_count / channels) as u64;

        Ok(Some(Packet {
            samples: std::mem::take(&mut self.sample_scratch),
            timestamp_seconds,
        }))
    }

    fn recycle(&mut self, samples: Vec<f32>) {
        // 只留容量更大的那个：两包大小相近，留哪个都够下一包用
        if samples.capacity() > self.sample_scratch.capacity() {
            self.sample_scratch = samples;
        }
    }

    fn seek(&mut self, seconds: f64) -> Result<(), DecoderError> {
        // F-10：seek 是唯一清除 finished 标志的途径
        self.finished = false;
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
        if self.reader.is_some() && (0.0..2.0).contains(&delta) {
            if let Some(info) = self.info.as_ref() {
                let channels = usize::from(info.channels.0).max(1);
                let frames_to_skip = (delta * f64::from(info.sample_rate.0.max(1))).round() as u64;
                let bytes_to_skip = frames_to_skip
                    .saturating_mul(channels as u64)
                    .saturating_mul(std::mem::size_of::<f32>() as u64);
                if bytes_to_skip == 0 {
                    return Ok(());
                }
                let stall_timeout = self.stall_timeout;
                let skipped = match self
                    .reader
                    .as_mut()
                    .map(|reader| reader.skip(bytes_to_skip, stall_timeout))
                {
                    Some(Ok(skipped)) => skipped,
                    // DEC-01：读丢同样是在等 ffmpeg 输出，同一个时限
                    Some(Err(err)) if err.kind() == io::ErrorKind::TimedOut => {
                        return Err(self.fail_stalled());
                    }
                    Some(Err(err)) => return Err(DecoderError::Io(err)),
                    None => 0,
                };
                // L-10：按累计跳过字节一次性换算帧数，避免逐 chunk 整除丢余数。
                self.frames_read += frames_in_byte_run(skipped, channels);
                return Ok(());
            }
        }
        // 远距 / 反向 seek：重启 ffmpeg 进程到目标时间
        self.start_process(target)
    }
}

/// DEC-01：解码进程迟迟没有输出时向上报的错误。
fn stalled_error(message: String) -> DecoderError {
    DecoderError::Io(io::Error::new(io::ErrorKind::TimedOut, message))
}

/// F-11：以 `-` 开头的文件名会被 ffmpeg/ffprobe 当成选项解析，
/// 统一加 `file:` 协议前缀消除歧义。
fn ffmpeg_input_arg(path: &Path) -> std::ffi::OsString {
    use std::ffi::OsString;
    if path.to_string_lossy().starts_with('-') {
        let mut arg = OsString::from("file:");
        arg.push(path.as_os_str());
        arg
    } else {
        path.as_os_str().to_os_string()
    }
}

/// R-03：ffprobe 探测超时。畸形文件能让 ffprobe 在解析阶段挂死，
/// 断连的网络盘也可能长期无响应；`.output()` 是无限等待，扫库时会把
/// 调用线程永久钉住。20 s 对慢速网络盘足够宽松，同时保证有界。
const FFPROBE_TIMEOUT: Duration = Duration::from_secs(20);

fn probe_with_ffprobe(path: &Path) -> Result<StreamInfo, DecoderError> {
    let mut command = Command::new(ffprobe_command_path()?);
    hide_console_window(&mut command);
    let mut child = command
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
        .arg(ffmpeg_input_arg(path))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(map_tool_spawn_error)?;

    // 管道必须在等待退出的同时排空——ffprobe 写满 pipe 缓冲会阻塞在 write 上，
    // 而我们又在等它退出，形成互等死锁。
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let stdout_thread = stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_thread = stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + FFPROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    // R-03：kill 之后的回收同样有界，不再裸 `wait()`
                    terminate_child(&mut child, Instant::now() + CLEANUP_TIMEOUT);
                    return Err(DecoderError::UnsupportedFormat(format!(
                        "ffprobe timed out after {} s",
                        FFPROBE_TIMEOUT.as_secs()
                    )));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(DecoderError::Io(err)),
        }
    };

    // 进程已经退出，管道写端关闭，两个读线程随即读到结尾；等不到就按没有输出处理（R-03）
    let pipes_deadline = Instant::now() + CLEANUP_TIMEOUT;
    let stdout_bytes = stdout_thread
        .and_then(|t| join_until(t, pipes_deadline))
        .unwrap_or_default();
    let stderr_bytes = stderr_thread
        .and_then(|t| join_until(t, pipes_deadline))
        .unwrap_or_default();

    if !status.success() {
        return Err(DecoderError::UnsupportedFormat(
            String::from_utf8_lossy(&stderr_bytes).trim().to_string(),
        ));
    }

    parse_ffprobe_output(&String::from_utf8_lossy(&stdout_bytes))
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
            // 两个字段都可能出现，有损格式通常是 `bits_per_sample=0` 加
            // `bits_per_raw_sample=N/A`。0 与 N/A 都是「没有」，不覆盖已经读到的位深。
            "bits_per_raw_sample" | "bits_per_sample" => {
                if let Some(bits) = value.parse::<u16>().ok().filter(|bits| *bits > 0) {
                    bit_depth = Some(bits);
                }
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

    let info = StreamInfo {
        sample_rate: SampleRate(sample_rate),
        // DEC-03：ffprobe 没给位深时记 0（未知），与 `decoder.rs`「0 表示未知」的约定一致。
        // 此前默认 16：走 ffmpeg 的格式解码输出是 32 位浮点，引擎在独占模式下对「已知
        // ≤16 bit」的源选 16 bit 输出，一个本来不知道位深的源会因此被量化到 16 bit。
        bit_depth: BitDepth(bit_depth.unwrap_or(0)),
        channels: Channels(channels),
        duration_seconds: duration.unwrap_or(0.0),
    };
    // R-01：ffprobe 的输出同样来自不可信文件头，与 symphonia 路径共用同一道闸。
    validate_stream_info(&info)?;
    Ok(info)
}

fn parse_duration(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// 把 little-endian f32 字节流写入 output buffer，复用调用方的 Vec 避免分配。
fn bytes_to_f32_into(bytes: &[u8], output: &mut Vec<f32>) {
    // 尾部不足 4 字节的余数与原 chunks_exact(4) 行为一致：直接丢弃。
    let (chunks, _remainder) = bytes.as_chunks::<4>();
    for chunk in chunks {
        output.push(f32::from_le_bytes(*chunk));
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
            // 持锁区只有 Vec 操作、无 panic 面；即便中毒也取回数据继续，
            // 不让「注册搜索目录」这类边缘操作携带 panic 面（2026-08-16 审查观察项）
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    // 中毒只说明持锁线程 panic 过，缓存值本身仍可用；`.ok()?` 会把它误报成「找不到 ffmpeg」
    let mut guard = mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    for cache in [&FFMPEG_CACHE, &FFPROBE_CACHE] {
        if let Some(mutex) = cache.get() {
            *mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }
}

/// M-1（2026-08-16 审查）：候选目录全部落空时**必须报错**，不得回退裸名。
/// `Command::new("ffmpeg.exe")` 走 CreateProcessW 搜索顺序，其中含**当前工作目录**——
/// 应用经文件关联从「歌曲文件夹」启动时，同目录植入的 ffmpeg.exe 即被执行
/// （CWE-427，与 F-05 的 icacls/explorer 同型）。find_tool 已扫过 PATH，
/// 裸名回退唯一多出来的搜索面就是 cwd/System 目录，正是要消除的攻击面。
fn resolve_tool_or_missing(found: Option<PathBuf>, tool: &str) -> Result<PathBuf, DecoderError> {
    found.ok_or_else(|| {
        DecoderError::UnsupportedFormat(format!(
            "{tool} not found (searched SERAPH_FFMPEG_*, app dirs and PATH); \
             refusing bare-name fallback"
        ))
    })
}

fn ffmpeg_command_path() -> Result<PathBuf, DecoderError> {
    resolve_tool_or_missing(find_ffmpeg(), "ffmpeg")
}

fn ffprobe_command_path() -> Result<PathBuf, DecoderError> {
    resolve_tool_or_missing(find_ffprobe(), "ffprobe")
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
        let dirs = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        candidates.extend(dirs.iter().map(|dir| dir.join(&exe_name)));
    }

    if let Some(dirs) = env::var_os("SERAPH_FFMPEG_DIRS") {
        candidates.extend(search_dir_candidates(&dirs, &exe_name));
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&exe_name));
            candidates.push(dir.join("ffmpeg").join(&exe_name));
        }
    }

    if let Some(path) = env::var_os("PATH") {
        candidates.extend(search_dir_candidates(&path, &exe_name));
    }

    absolute_only(dedupe_paths(candidates))
}

/// 把 `;` 分隔的目录列表展开成候选，只保留绝对目录（SEC-01：相对项与空项
/// 会按当前工作目录解析，重新打开 M-1 的同目录 EXE 种植面）。
fn search_dir_candidates(dirs: &std::ffi::OsStr, exe_name: &str) -> Vec<PathBuf> {
    env::split_paths(dirs)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(exe_name))
        .collect()
}

/// 最后一道闸：`SERAPH_FFMPEG_PATH` 等任何来源给出的相对候选一律丢弃。
fn absolute_only(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    candidates
        .into_iter()
        .filter(|candidate| candidate.is_absolute())
        .collect()
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

    /// SEC-01：PATH / SERAPH_FFMPEG_DIRS 的相对项与空项（PATH 以 `;` 结尾很常见，
    /// `split_paths` 会产出空路径）拼出的候选按**当前工作目录**解析，等于把 M-1
    /// 堵掉的「同目录植入 ffmpeg.exe」又放了回来——候选一律只认绝对路径。
    #[cfg(windows)]
    #[test]
    fn search_dirs_only_yield_absolute_candidates() {
        let dirs = std::ffi::OsStr::new(r"C:\tools;;.;bin;");
        assert_eq!(
            search_dir_candidates(dirs, "ffmpeg.exe"),
            vec![PathBuf::from(r"C:\tools\ffmpeg.exe")]
        );
        assert!(absolute_only(vec![PathBuf::from("ffmpeg.exe")]).is_empty());
    }

    #[test]
    fn dash_prefixed_path_gets_file_protocol_prefix() {
        let arg = ffmpeg_input_arg(Path::new("-weird.mp3"));
        assert_eq!(arg.to_string_lossy(), "file:-weird.mp3");
        let arg = ffmpeg_input_arg(Path::new("C:/music/track.mp3"));
        assert_eq!(arg.to_string_lossy(), "C:/music/track.mp3");
    }

    /// M-1：工具缺失必须是明确错误，绝不能落到裸名交给 CreateProcess 隐式搜索
    /// （其搜索顺序含当前工作目录，构成同名 EXE 种植面）。
    #[test]
    fn missing_tool_is_an_error_not_a_bare_name() {
        let err = resolve_tool_or_missing(None, "ffmpeg").unwrap_err();
        assert!(matches!(err, DecoderError::UnsupportedFormat(_)));

        let found = PathBuf::from(r"C:\tools\ffmpeg.exe");
        assert_eq!(
            resolve_tool_or_missing(Some(found.clone()), "ffmpeg").unwrap(),
            found
        );
    }

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
    fn missing_bit_depth_is_reported_as_unknown_not_16() {
        // DEC-03：0 = 未知。引擎在独占模式下对「已知 ≤16 bit」的源选 16 bit 输出，
        // 默认成 16 会把 ffmpeg 解出的 32 位浮点量化掉。
        let depth = |output: &str| parse_ffprobe_output(output).unwrap().bit_depth;
        let base = "sample_rate=48000\nchannels=2\n";
        assert_eq!(depth(base), BitDepth(0), "两个字段都没有");
        // 有损格式的典型输出
        assert_eq!(
            depth(&format!(
                "{base}bits_per_sample=0\nbits_per_raw_sample=N/A\n"
            )),
            BitDepth(0)
        );
        // 0 与 N/A 都是「没有」，不覆盖另一个字段给出的位深，先后顺序无关
        assert_eq!(
            depth(&format!(
                "{base}bits_per_sample=0\nbits_per_raw_sample=24\n"
            )),
            BitDepth(24)
        );
        assert_eq!(
            depth(&format!(
                "{base}bits_per_sample=24\nbits_per_raw_sample=0\n"
            )),
            BitDepth(24)
        );
        assert_eq!(
            depth(&format!(
                "{base}bits_per_sample=16\nbits_per_raw_sample=N/A\n"
            )),
            BitDepth(16)
        );
        // 两个都有且不同（24 位样本装在 32 位容器里）：以后出现的为准，与此前一致
        assert_eq!(
            depth(&format!(
                "{base}bits_per_sample=32\nbits_per_raw_sample=24\n"
            )),
            BitDepth(24)
        );
    }

    /// 测试用字节源：收到什么给什么，发送端全部放手后表现为源结束。
    struct ChannelSource(mpsc::Receiver<Vec<u8>>, Vec<u8>);

    impl Read for ChannelSource {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.1.is_empty() {
                match self.0.recv() {
                    Ok(bytes) => self.1 = bytes,
                    Err(_) => return Ok(0),
                }
            }
            let take = self.1.len().min(buffer.len());
            buffer[..take].copy_from_slice(&self.1[..take]);
            self.1.drain(..take);
            Ok(take)
        }
    }

    fn channel_reader() -> (mpsc::Sender<Vec<u8>>, PipeReader) {
        let (sender, receiver) = mpsc::channel();
        let reader = PipeReader::spawn(ChannelSource(receiver, Vec::new())).unwrap();
        (sender, reader)
    }

    const TEST_WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn pipe_reader_reassembles_bytes_across_chunks_and_reports_the_end() {
        let (sender, mut reader) = channel_reader();
        let bytes: Vec<u8> = (0..=255_u8).cycle().take(1000).collect();
        // 故意按不整齐的大小分段送，考「跨块拼接」
        for piece in bytes.chunks(7) {
            sender.send(piece.to_vec()).unwrap();
        }
        drop(sender);

        let mut first = [0_u8; 300];
        assert_eq!(reader.read_full(&mut first, TEST_WAIT).unwrap(), 300);
        assert_eq!(first[..], bytes[..300]);
        assert_eq!(reader.skip(250, TEST_WAIT).unwrap(), 250);
        // 源只剩 450 字节：读到多少算多少，之后一直是 0
        let mut rest = [0_u8; 600];
        assert_eq!(reader.read_full(&mut rest, TEST_WAIT).unwrap(), 450);
        assert_eq!(rest[..450], bytes[550..]);
        assert_eq!(reader.read_full(&mut rest, TEST_WAIT).unwrap(), 0);
        assert_eq!(reader.skip(10, TEST_WAIT).unwrap(), 0);
        assert!(join_until(reader.into_thread(), Instant::now() + TEST_WAIT).is_some());
    }

    #[test]
    fn pipe_reader_times_out_instead_of_blocking_on_a_silent_source() {
        // DEC-01：源不出数据时（ffmpeg 自己卡在读断连的网络盘上），等待必须有界
        let (sender, mut reader) = channel_reader();
        let mut buffer = [0_u8; 16];
        let started = Instant::now();
        let err = reader
            .read_full(&mut buffer, Duration::from_millis(150))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(started.elapsed() < TEST_WAIT, "{:?}", started.elapsed());
        let err = reader.skip(16, Duration::from_millis(50)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);

        // 超时不是终态：数据后来到了照常能读
        sender.send(vec![7; 16]).unwrap();
        assert_eq!(reader.read_full(&mut buffer, TEST_WAIT).unwrap(), 16);
        assert_eq!(buffer, [7; 16]);
        drop(sender);
        assert_eq!(reader.read_full(&mut buffer, TEST_WAIT).unwrap(), 0);
    }

    #[test]
    fn pipe_reader_thread_stops_when_the_reader_is_dropped_with_data_pending() {
        // 源一直有数据、没人取：读线程把有界通道写满后阻塞在发送上（这就是背压）。
        // 放手之后它必须立刻退出，不能一直占着线程和管道。
        let reader = PipeReader::spawn(io::repeat(0x5a)).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(!reader.thread.is_finished(), "预读有上限，读线程此时应在等");
        assert!(join_until(reader.into_thread(), Instant::now() + TEST_WAIT).is_some());
    }

    /// 测试用的系统工具路径（解码 crate 里没有、也不该有按名字找系统工具的生产代码）。
    #[cfg(windows)]
    fn system_tool(name: &str) -> PathBuf {
        let root = env::var_os("SystemRoot").expect("SystemRoot");
        let path = Path::new(&root).join("System32").join(name);
        assert!(path.is_file(), "{}", path.display());
        path
    }

    /// 一个活着但不往 stdout 写满一包数据的进程：`ping` 每秒才输出一行。
    #[cfg(windows)]
    fn slow_command() -> Command {
        let mut command = Command::new(system_tool("ping.exe"));
        command.args(["-n", "60", "127.0.0.1"]);
        command
    }

    #[cfg(windows)]
    fn stalled_decoder() -> FfmpegDecoder {
        let mut decoder = FfmpegDecoder::new();
        decoder.info = Some(StreamInfo {
            sample_rate: SampleRate(48_000),
            bit_depth: BitDepth(0),
            channels: Channels(2),
            duration_seconds: 0.0,
        });
        decoder.stall_timeout = Duration::from_millis(300);
        decoder.spawn_decoding_process(slow_command(), 0.0).unwrap();
        decoder
    }

    #[cfg(windows)]
    #[test]
    fn stalled_decoding_process_is_killed_and_reported() {
        // DEC-01：此前 next_packet 会一直阻塞在管道读取上，进程留到应用退出
        let mut decoder = stalled_decoder();
        let started = Instant::now();
        let err = decoder.next_packet().unwrap_err();
        assert!(
            matches!(&err, DecoderError::Io(io) if io.kind() == io::ErrorKind::TimedOut),
            "{err}"
        );
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(started.elapsed() < TEST_WAIT, "{:?}", started.elapsed());
        assert!(decoder.child.is_none() && decoder.reader.is_none());
        assert!(decoder.stderr_thread.is_none());

        // 既不当成正常播完，也不悄悄重启进程重来：之后一直是同一个错误
        let again = decoder.next_packet().unwrap_err();
        assert_eq!(again.to_string(), err.to_string());
        assert!(decoder.child.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn near_forward_seek_on_a_stalled_process_is_bounded_too() {
        // 近距离向前跳转是「读丢中间的样本」，同样在等进程输出
        let mut decoder = stalled_decoder();
        let started = Instant::now();
        let err = decoder.seek(1.5).unwrap_err();
        assert!(
            matches!(&err, DecoderError::Io(io) if io.kind() == io::ErrorKind::TimedOut),
            "{err}"
        );
        assert!(started.elapsed() < TEST_WAIT, "{:?}", started.elapsed());
        assert!(decoder.child.is_none() && decoder.reader.is_none());
        assert!(decoder.next_packet().is_err());
    }

    #[cfg(windows)]
    #[test]
    fn stopping_a_live_process_reaps_it_within_the_cleanup_deadline() {
        // R-03：kill 之后的回收与读线程收尾都有界（此前是裸 wait / join）
        let mut child = slow_command()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(child.try_wait().unwrap().is_none(), "进程应当还活着");
        assert!(terminate_child(&mut child, Instant::now() + TEST_WAIT));
        assert!(child.try_wait().unwrap().is_some(), "已经被回收");
        // 对已经退出的进程再调一次是无害的
        assert!(terminate_child(&mut child, Instant::now()));

        let mut decoder = stalled_decoder();
        let started = Instant::now();
        decoder.stop_process();
        assert!(started.elapsed() < CLEANUP_TIMEOUT + Duration::from_secs(1));
        assert!(decoder.child.is_none() && decoder.reader.is_none());
        // 放弃一个正卡着的解码器（引擎超时后丢弃它）也不会挂住
        let started = Instant::now();
        drop(stalled_decoder());
        assert!(started.elapsed() < CLEANUP_TIMEOUT + Duration::from_secs(1));
    }

    /// 一段「样本」数据落成临时文件，由 `cmd /C type` 原样写到 stdout——充当一个瞬间
    /// 解完码的 ffmpeg。每个样本的 4 个字节都是可打印 ASCII，`type` 不会对它做任何转换。
    #[cfg(windows)]
    struct PcmFixture {
        dir: PathBuf,
        samples: Vec<f32>,
    }

    #[cfg(windows)]
    impl PcmFixture {
        const FILE: &'static str = "pcm.bin";

        /// `trailing` 是追加在整帧数据之后的零头字节（不足一帧，解码器应当丢弃）。
        fn new(case: &str, frames: usize, channels: usize, trailing: &[u8]) -> Self {
            let dir =
                env::temp_dir().join(format!("seraph-ffmpeg-pipe-{}-{case}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let samples: Vec<f32> = (0..frames * channels)
                .map(|index| {
                    f32::from_le_bytes([
                        b'A' + (index % 26) as u8,
                        b'a' + (index / 26 % 26) as u8,
                        b'0' + (index / 676 % 10) as u8,
                        b'K',
                    ])
                })
                .collect();
            let mut bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            bytes.extend_from_slice(trailing);
            std::fs::write(dir.join(Self::FILE), bytes).unwrap();
            Self { dir, samples }
        }

        /// `type pcm.bin`，可在后面接别的命令（`& exit 3`）。工作目录设到夹具目录，
        /// 命令行里只出现不含空格的相对文件名。
        fn command(&self, suffix: &str) -> Command {
            let mut command = Command::new(system_tool("cmd.exe"));
            command
                .current_dir(&self.dir)
                .args(["/D", "/C"])
                .arg(format!("type {}{suffix}", Self::FILE));
            command
        }

        fn decoder(&self, channels: u16, suffix: &str) -> FfmpegDecoder {
            let mut decoder = FfmpegDecoder::new();
            decoder.info = Some(StreamInfo {
                sample_rate: SampleRate(48_000),
                bit_depth: BitDepth(0),
                channels: Channels(channels),
                duration_seconds: 0.0,
            });
            decoder.stall_timeout = TEST_WAIT;
            decoder
                .spawn_decoding_process(self.command(suffix), 0.0)
                .unwrap();
            decoder
        }
    }

    #[cfg(windows)]
    impl Drop for PcmFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[cfg(windows)]
    #[test]
    fn process_output_is_decoded_byte_for_byte_and_ends_cleanly() {
        // DEC-01 改了取数据的路径（读线程 + 通道），这里用真实子进程和真实管道核对：
        // 样本一个不多、一个不少、顺序不变，时间戳连续，正常退出识别为播完。
        let channels = 2_usize;
        let frames = PACKET_FRAMES * 24 + 848;
        let fixture = PcmFixture::new("clean", frames, channels, b"xyz");
        let mut decoder = fixture.decoder(channels as u16, "");

        let mut decoded = Vec::new();
        let mut packets = 0;
        while let Some(packet) = decoder.next_packet().unwrap() {
            assert_eq!(packet.samples.len() % channels, 0, "只交整帧");
            let expected_time = (decoded.len() / channels) as f64 / 48_000.0;
            assert!((packet.timestamp_seconds - expected_time).abs() < 1e-9);
            decoded.extend_from_slice(&packet.samples);
            packets += 1;
            decoder.recycle(packet.samples);
        }
        assert_eq!(packets, 25, "24 个整包加一个不满的尾包");
        assert_eq!(
            decoded.len(),
            fixture.samples.len(),
            "末尾 3 个零头字节被丢弃"
        );
        assert!(decoded == fixture.samples, "解出的样本与写入的逐个相同");
        // F-10：播完之后不重启进程重播
        assert!(decoder.next_packet().unwrap().is_none());
        assert!(decoder.child.is_none() && decoder.reader.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn near_forward_seek_discards_exactly_the_skipped_frames() {
        let channels = 2_usize;
        let fixture = PcmFixture::new("skip", PACKET_FRAMES * 40, channels, b"");
        let mut decoder = fixture.decoder(channels as u16, "");
        let first = decoder.next_packet().unwrap().unwrap();
        assert_eq!(
            first.samples[..],
            fixture.samples[..PACKET_FRAMES * channels]
        );

        // 当前位置是第 2048 帧；跳到 1 秒处（第 48000 帧）在 2 秒以内，走读丢而不是重启进程
        decoder.seek(1.0).unwrap();
        assert!(decoder.child.is_some(), "没有重启进程");
        let next = decoder.next_packet().unwrap().unwrap();
        assert!((next.timestamp_seconds - 1.0).abs() < 1e-9);
        let from = 48_000 * channels;
        assert_eq!(
            next.samples[..],
            fixture.samples[from..from + PACKET_FRAMES * channels]
        );

        // 跳过的量超出剩余数据：读到结尾为止，之后按正常播完处理
        decoder.seek(next.timestamp_seconds + 1.9).unwrap();
        assert!(decoder.next_packet().unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn process_that_fails_after_writing_output_is_reported_as_aborted() {
        // 数据照常交付，结尾按退出码识别为中途失败，而不是当成正常播完
        let channels = 2_usize;
        let fixture = PcmFixture::new("crash", PACKET_FRAMES * 3, channels, b"");
        let mut decoder = fixture.decoder(channels as u16, " & exit 3");
        let mut decoded = Vec::new();
        let err = loop {
            match decoder.next_packet() {
                Ok(Some(packet)) => decoded.extend_from_slice(&packet.samples),
                Ok(None) => panic!("非零退出码必须报错"),
                Err(err) => break err,
            }
        };
        assert!(decoded == fixture.samples, "失败之前的数据完整交付");
        assert!(
            matches!(&err, DecoderError::Internal(message) if message.contains("ffmpeg aborted")),
            "{err}"
        );
        assert!(decoder.next_packet().unwrap().is_none(), "之后不再重启");
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
