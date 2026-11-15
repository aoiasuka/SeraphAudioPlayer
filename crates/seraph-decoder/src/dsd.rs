//! Lightweight DSD decoder.
//!
//! DSF/DFF 经两级抽取转成 interleaved f32 PCM（DSP-01）：
//! 1. 每个 DSD 字节（8 bit）出一个样本的 96 抽头 Kaiser 低通（8:1），按字节查表；
//! 2. 再用一条长 FIR 低通抽取 4:1 或 8:1。
//!
//! 两级都在块之间保留状态。DSD64 输出 88.2 kHz（总 32:1），更高倍率保持 64:1。
//! 旧实现是「每 64 bit 一个不重叠的 Hann 窗」：主瓣 −6 dB 点约 44 kHz，
//! 22–80 kHz 的噪声整形能量几乎不衰减就被 64:1 抽取，混叠回可听频段。
//! 后置一阶 IIR DC blocker 去掉调制器引入的低频偏置。

use crate::decoder::{
    validate_stream_info, Decoder, DecoderError, Packet, StreamInfo, MAX_SUPPORTED_SAMPLE_RATE,
};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::OnceLock,
};

/// 高倍率 DSD（DSD128 及以上）的总抽取比；也是 PCM 率上界反推 DSD 率上界的口径。
const DSD_TO_PCM_DECIMATION: usize = 64;
/// 第一级：每个 DSD 字节出一个样本。
const STAGE1_DECIMATION: usize = 8;
/// 第一级滤波器跨越的字节数（96 抽头）。
const STAGE1_BYTES: usize = 12;
/// 两级共用的 Kaiser β：旁瓣约 −100 dB。
const KAISER_BETA: f64 = 10.06;
/// 第一级截止（相对 DSD 率）。通带到 0.0085·fs（DSD64 下 24 kHz）几乎无滚降，
/// 0.092·fs 起进入阻带——第一级抽取后的镜像最早出现在 fs/8 − fs/64 = 0.109·fs。
const STAGE1_CUTOFF: f64 = 0.059;
/// 第二级每个抽取比对应的抽头数：32·M + 1。
const STAGE2_TAPS_PER_RATIO: usize = 32;
/// 第二级截止（相对输出率）：通带到 0.272·fo（88.2 kHz 输出下 24 kHz），
/// 阻带从输出奈奎斯特 0.5·fo 开始。
const STAGE2_CUTOFF: f64 = 0.386;
/// 输出 PCM 率下限：DSD64 按 32:1 出 88.2 kHz，给 DAC 留出超声过渡带。
const MIN_PCM_RATE: u32 = 88_200;
/// DSD 的「数字静音」码型（01101001）：滤波器历史以它起步，开头不出满幅直流。
const DSD_IDLE_BYTE: u8 = 0x69;
/// seek 对齐粒度（字节 / 声道）：同时是两种第二级抽取比（4、8）的倍数。
const DSD_SEEK_ALIGN_BYTES: usize = 8;
const DFF_PACKET_BYTE_FRAMES: usize = 4096;
/// SACD 0 dB 参考 = 50% 调制深度 → 直接抽取的 PCM 峰值只有 ±0.5（−6 dBFS）。
/// 业界惯例（foobar SACD 插件等）补 +6.02 dB，使 DSD 与 PCM 曲目响度一致。
/// 超出 ±1.0 的瞬时峰值由 render 端已有的 clamp 兜底。
const DSD_GAIN: f32 = 2.0;
/// DC blocker 目标截止频率（Hz）。R 按流的实际 PCM 率换算（见 open）。
const DC_BLOCKER_CUTOFF_HZ: f32 = 2.0;
/// 元数据类 chunk（fmt/PROP）载荷读入上限，防止损坏/恶意文件触发巨型分配。
const MAX_METADATA_CHUNK_LEN: u64 = 1 << 20;

/// 零阶修正贝塞尔函数（Kaiser 窗用）。
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..64 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < sum * 1.0e-18 {
            break;
        }
    }
    sum
}

/// Kaiser 窗 sinc 低通，`cutoff` 为相对采样率的截止（0–0.5），直流增益归一到 1。
fn kaiser_lowpass(taps: usize, cutoff: f64, beta: f64) -> Vec<f64> {
    let center = (taps as f64 - 1.0) / 2.0;
    let denom = bessel_i0(beta);
    let mut out: Vec<f64> = (0..taps)
        .map(|index| {
            let x = index as f64 - center;
            let sinc = if x.abs() < 1.0e-12 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            let ratio = if center > 0.0 { x / center } else { 0.0 };
            let window = bessel_i0(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / denom;
            sinc * window
        })
        .collect();
    let sum: f64 = out.iter().sum();
    for tap in &mut out {
        *tap /= sum;
    }
    out
}

fn stage1_taps() -> &'static [f64] {
    static TAPS: OnceLock<Vec<f64>> = OnceLock::new();
    TAPS.get_or_init(|| kaiser_lowpass(STAGE1_BYTES * 8, STAGE1_CUTOFF, KAISER_BETA))
}

/// 第二级低通：抽取比 4 与 8 各一份，进程内共享。
fn stage2_taps(ratio: usize) -> &'static [f64] {
    static RATIO_4: OnceLock<Vec<f64>> = OnceLock::new();
    static RATIO_8: OnceLock<Vec<f64>> = OnceLock::new();
    let build = |ratio: usize| {
        kaiser_lowpass(
            STAGE2_TAPS_PER_RATIO * ratio + 1,
            STAGE2_CUTOFF / ratio as f64,
            KAISER_BETA,
        )
    };
    match ratio {
        4 => RATIO_4.get_or_init(|| build(4)),
        _ => RATIO_8.get_or_init(|| build(8)),
    }
}

/// 按字节的 LUT：`lut[slot][byte_value]` = 窗口里第 `slot` 个字节（0 = 最早）
/// 的 8 个 bit 对应抽头的部分和。把逐 bit 的 96 次乘加降为 12 次查表相加（F-16）。
type DsdByteLut = [[f64; 256]; STAGE1_BYTES];

fn build_byte_lut(order: BitOrder) -> Box<DsdByteLut> {
    let taps = stage1_taps();
    let mut lut: Box<DsdByteLut> = Box::new([[0.0; 256]; STAGE1_BYTES]);
    for (slot, table) in lut.iter_mut().enumerate() {
        for (value, entry) in table.iter_mut().enumerate() {
            let byte = value as u8;
            let mut acc = 0.0_f64;
            for bit_in_byte in 0..8 {
                let bit = match order {
                    BitOrder::LsbFirst => (byte >> bit_in_byte) & 1,
                    BitOrder::MsbFirst => (byte >> (7 - bit_in_byte)) & 1,
                };
                let signed = f64::from(bit as i8 * 2 - 1);
                acc += signed * taps[slot * 8 + bit_in_byte];
            }
            *entry = acc;
        }
    }
    lut
}

fn byte_lut(order: BitOrder) -> &'static DsdByteLut {
    static LSB: OnceLock<Box<DsdByteLut>> = OnceLock::new();
    static MSB: OnceLock<Box<DsdByteLut>> = OnceLock::new();
    match order {
        BitOrder::LsbFirst => LSB.get_or_init(|| build_byte_lut(BitOrder::LsbFirst)),
        BitOrder::MsbFirst => MSB.get_or_init(|| build_byte_lut(BitOrder::MsbFirst)),
    }
}

/// 总抽取比：输出率不低于 88.2 kHz 时用 64:1，否则（DSD64）用 32:1。
fn pcm_decimation(dsd_sample_rate: u32) -> usize {
    if dsd_sample_rate / DSD_TO_PCM_DECIMATION as u32 >= MIN_PCM_RATE {
        DSD_TO_PCM_DECIMATION
    } else {
        DSD_TO_PCM_DECIMATION / 2
    }
}

fn pcm_rate_of(dsd_sample_rate: u32) -> u32 {
    (dsd_sample_rate / pcm_decimation(dsd_sample_rate) as u32).max(1)
}

/// 单声道的两级抽取器。两级的延迟线都用「双写」环形缓冲，取窗口时是一段连续切片。
struct ChannelDecimator {
    lut: &'static DsdByteLut,
    bytes: [u8; STAGE1_BYTES * 2],
    byte_pos: usize,
    taps: &'static [f64],
    line: Vec<f64>,
    line_pos: usize,
    ratio: usize,
    phase: usize,
}

impl ChannelDecimator {
    fn new(order: BitOrder, ratio: usize) -> Self {
        let taps = stage2_taps(ratio);
        Self {
            lut: byte_lut(order),
            bytes: [DSD_IDLE_BYTE; STAGE1_BYTES * 2],
            byte_pos: 0,
            taps,
            line: vec![0.0; taps.len() * 2],
            line_pos: 0,
            ratio,
            phase: 0,
        }
    }

    fn reset(&mut self) {
        self.bytes = [DSD_IDLE_BYTE; STAGE1_BYTES * 2];
        self.byte_pos = 0;
        self.line.fill(0.0);
        self.line_pos = 0;
        self.phase = 0;
    }

    /// 喂入一个 DSD 字节；每 `ratio` 个字节产出一个 PCM 样本（已含 +6.02 dB 补偿）。
    #[inline]
    fn push(&mut self, byte: u8) -> Option<f32> {
        self.bytes[self.byte_pos] = byte;
        self.bytes[self.byte_pos + STAGE1_BYTES] = byte;
        self.byte_pos = (self.byte_pos + 1) % STAGE1_BYTES;
        let window = &self.bytes[self.byte_pos..self.byte_pos + STAGE1_BYTES];
        let mut stage1 = 0.0_f64;
        for (table, value) in self.lut.iter().zip(window) {
            stage1 += table[*value as usize];
        }

        let len = self.taps.len();
        self.line[self.line_pos] = stage1;
        self.line[self.line_pos + len] = stage1;
        self.line_pos = (self.line_pos + 1) % len;
        self.phase += 1;
        if self.phase < self.ratio {
            return None;
        }
        self.phase = 0;
        let history = &self.line[self.line_pos..self.line_pos + len];
        let acc: f64 = history.iter().zip(self.taps).map(|(x, h)| x * h).sum();
        Some((acc * f64::from(DSD_GAIN)) as f32)
    }
}

#[derive(Debug, Clone, Copy)]
enum DsdLayout {
    Dsf { block_size_per_channel: usize },
    Dff,
}

/// 每个容器规定的 byte 内 bit 排列顺序。Boxcar 算法对此不敏感，
/// 但 Hann 加权对 bit 在窗口里的位置敏感，必须按规范取位。
/// - DSF: 通常 LSB first（fmt 的 bits_per_sample=1）；bits_per_sample=8 变体为 MSB first
/// - DFF: MSB first（Philips DSDIFF 规范）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitOrder {
    LsbFirst,
    MsbFirst,
}

pub struct DsdDecoder {
    info: Option<StreamInfo>,
    file: Option<File>,
    layout: Option<DsdLayout>,
    bit_order: BitOrder,
    dsd_sample_rate: u32,
    channels: usize,
    data_start: u64,
    data_len: u64,
    data_read: u64,
    /// 每声道的有效音频字节数（F-1）：DSF 末块 block 对齐的零填充不属于音频，
    /// 解码到此为止，否则填充被解码成满幅负直流 → 曲尾爆音。
    audio_bytes_per_channel: u64,
    pcm_frames_emitted: u64,
    dc_r: f32,
    dc_state: Vec<(f32, f32)>, // 每通道一份 (last_input, last_output)
    /// 每声道一份两级抽取器（块之间保留滤波状态）。
    decimators: Vec<ChannelDecimator>,
}

impl DsdDecoder {
    pub fn new() -> Self {
        Self {
            info: None,
            file: None,
            layout: None,
            bit_order: BitOrder::LsbFirst,
            dsd_sample_rate: 0,
            channels: 0,
            data_start: 0,
            data_len: 0,
            data_read: 0,
            audio_bytes_per_channel: 0,
            pcm_frames_emitted: 0,
            dc_r: 0.995,
            dc_state: Vec::new(),
            decimators: Vec::new(),
        }
    }
}

impl Default for DsdDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for DsdDecoder {
    fn open(&mut self, path: &Path) -> Result<(), DecoderError> {
        let mut file = File::open(path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                DecoderError::FileNotFound
            } else {
                DecoderError::Io(err)
            }
        })?;

        let mut magic = [0_u8; 4];
        file.read_exact(&mut magic)?;
        file.seek(SeekFrom::Start(0))?;

        let parsed = match &magic {
            b"DSD " => parse_dsf(&mut file)?,
            b"FRM8" => parse_dff(&mut file)?,
            _ => {
                return Err(DecoderError::UnsupportedFormat(
                    "not a DSF/DFF DSD stream".into(),
                ));
            }
        };

        file.seek(SeekFrom::Start(parsed.data_start))?;
        // R-01：与 symphonia/ffmpeg 路径共用同一道边界闸（DSD 头已过
        // validate_dsd_header，这里复验抽取后的 PCM 口径）。
        validate_stream_info(&parsed.info)?;
        self.info = Some(parsed.info);
        self.file = Some(file);
        self.layout = Some(parsed.layout);
        self.bit_order = parsed.bit_order;
        self.dsd_sample_rate = parsed.dsd_sample_rate;
        self.channels = parsed.channels;
        self.data_start = parsed.data_start;
        self.data_len = parsed.data_len;
        self.data_read = 0;
        self.audio_bytes_per_channel = parsed.audio_bytes_per_channel;
        self.pcm_frames_emitted = 0;
        // F-2：一阶 DC blocker 的 −3 dB 点 ≈ fs·(1−R)/(2π)。
        // R 必须按流的实际 PCM 率换算（DSD128/256 的 PCM 率翻倍），
        // 否则截止频率随倍率翻倍，系统性削薄 sub-bass。目标 ~2 Hz。
        let pcm_rate = pcm_rate_of(parsed.dsd_sample_rate) as f32;
        self.dc_r = (1.0 - 2.0 * std::f32::consts::PI * DC_BLOCKER_CUTOFF_HZ / pcm_rate)
            .clamp(0.995, 0.999_99);
        self.dc_state = vec![(0.0, 0.0); parsed.channels];
        let ratio = pcm_decimation(parsed.dsd_sample_rate) / STAGE1_DECIMATION;
        self.decimators = (0..parsed.channels)
            .map(|_| ChannelDecimator::new(parsed.bit_order, ratio))
            .collect();
        Ok(())
    }

    fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, DecoderError> {
        if self.data_read >= self.data_len {
            return Ok(None);
        }

        let layout = self
            .layout
            .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?;
        let channels = self.channels;
        let pcm_rate = u64::from(pcm_rate_of(self.dsd_sample_rate));
        // 用已发出的 PCM 帧数算时戳，比从 byte 累计反推稳定（不会被块尾对齐截断）
        let timestamp_seconds = self.pcm_frames_emitted as f64 / pcm_rate as f64;

        let remaining = (self.data_len - self.data_read) as usize;
        let read_len = match layout {
            DsdLayout::Dsf {
                block_size_per_channel,
            } => remaining.min(block_size_per_channel * channels),
            DsdLayout::Dff => remaining.min(DFF_PACKET_BYTE_FRAMES * channels),
        };
        if read_len == 0 {
            return Ok(None);
        }

        let mut raw = vec![0_u8; read_len];
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| DecoderError::Internal("file is not open".into()))?;
        // 单次 read 不保证读满 read_len（Read trait 允许短读）；循环读满直到 EOF，
        // 配合 decode_dsf_block 的越界保护，彻底消除截断/损坏 DSF 的 panic 与错位。
        let mut filled = 0usize;
        while filled < read_len {
            match file.read(&mut raw[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        if filled == 0 {
            return Ok(None);
        }
        raw.truncate(filled);
        self.data_read += filled as u64;

        let mut samples = match layout {
            DsdLayout::Dsf {
                block_size_per_channel,
            } => {
                // F-1：DSF data chunk 按 block 对齐，末块用 0x00 填充；
                // 填充解码为满幅负直流 → 曲尾爆音。把可解码范围钳制到
                // fmt.sample_count 折算出的有效音频字节数。
                let consumed_per_channel =
                    (self.data_read - filled as u64) / self.channels.max(1) as u64;
                let valid_per_channel = self
                    .audio_bytes_per_channel
                    .saturating_sub(consumed_per_channel);
                if valid_per_channel == 0 {
                    return Ok(None);
                }
                let valid_bytes = (valid_per_channel.min(block_size_per_channel as u64)) as usize;
                decode_dsf_block(
                    &raw,
                    &mut self.decimators,
                    block_size_per_channel,
                    valid_bytes,
                )
            }
            DsdLayout::Dff => decode_dff_block(&raw, &mut self.decimators),
        };

        if samples.is_empty() {
            return Ok(None);
        }

        apply_dc_blocker(&mut samples, channels, &mut self.dc_state, self.dc_r);
        self.pcm_frames_emitted += (samples.len() / channels.max(1)) as u64;

        Ok(Some(Packet {
            samples,
            timestamp_seconds,
        }))
    }

    fn seek(&mut self, seconds: f64) -> Result<(), DecoderError> {
        let layout = self
            .layout
            .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?;
        let channels = self.channels as u64;
        if self.dsd_sample_rate == 0 || channels == 0 {
            return Ok(());
        }

        let target_dsd_frames = (seconds.max(0.0) * self.dsd_sample_rate as f64) as u64;
        let byte_frame = target_dsd_frames / 8;
        // R-02：seek 偏移的乘法一律 saturating——release 下回绕后虽被下方
        // min(data_len) 钳住行为无害，但 debug 构建会直接 panic。
        let mut byte_offset = match layout {
            DsdLayout::Dsf {
                block_size_per_channel,
            } => {
                let block_size = block_size_per_channel as u64;
                let pcm_aligned_byte = byte_frame - (byte_frame % DSD_SEEK_ALIGN_BYTES as u64);
                let block_index = pcm_aligned_byte / block_size;
                block_index
                    .saturating_mul(block_size)
                    .saturating_mul(channels)
            }
            DsdLayout::Dff => {
                let aligned = byte_frame - (byte_frame % DSD_SEEK_ALIGN_BYTES as u64);
                aligned.saturating_mul(channels)
            }
        };

        byte_offset = byte_offset.min(self.data_len);
        self.file
            .as_mut()
            .ok_or_else(|| DecoderError::Internal("file is not open".into()))?
            .seek(SeekFrom::Start(self.data_start + byte_offset))?;
        self.data_read = byte_offset;
        // 重建 DC blocker 状态，避免 seek 后边界泄漏
        for state in self.dc_state.iter_mut() {
            *state = (0.0, 0.0);
        }
        // 抽取滤波器的历史属于 seek 之前的位置，同样清掉
        for decimator in self.decimators.iter_mut() {
            decimator.reset();
        }
        // L-26：PCM 帧计数按实际落点（对齐后的 byte_offset）反推。DSF 的读取
        // 位置向下对齐到 block 边界（DSD64 典型 block ≈11.6ms 音频），此前按
        // 请求秒数回设会让时间戳比实际音频恒定超前最多一个 block，进度显示、
        // 歌词对位持续偏移到曲尾/下次 seek。
        let bytes_per_channel = byte_offset / channels.max(1);
        self.pcm_frames_emitted =
            bytes_per_channel * 8 / pcm_decimation(self.dsd_sample_rate) as u64;
        Ok(())
    }
}

struct ParsedDsd {
    info: StreamInfo,
    layout: DsdLayout,
    bit_order: BitOrder,
    dsd_sample_rate: u32,
    channels: usize,
    data_start: u64,
    data_len: u64,
    audio_bytes_per_channel: u64,
}

/// F-4：容器头字段合理性校验，防止损坏/恶意文件触发巨型分配（OOM abort）。
///
/// R-01：DSD 率上界原为 100 MHz，抽取 64:1 后 PCM 率仍高达 1.56 MHz——超过引擎
/// 支持的 768 kHz，会一路流到环形缓冲容量计算。改为按 PCM 上界反推：
/// 768 kHz × 64 = 49.152 MHz，正好覆盖到 DSD1024（22.5792 MHz 的 2 倍）。
/// （只有 PCM 率低于 88.2 kHz 的流才用 32:1，离上界很远。）
const MAX_DSD_SAMPLE_RATE: u32 = MAX_SUPPORTED_SAMPLE_RATE * DSD_TO_PCM_DECIMATION as u32;

fn validate_dsd_header(
    channels: usize,
    dsd_sample_rate: u32,
    block_size_per_channel: Option<usize>,
) -> Result<(), DecoderError> {
    if !(1..=32).contains(&channels)
        || !(64_000..=MAX_DSD_SAMPLE_RATE).contains(&dsd_sample_rate)
        || block_size_per_channel.is_some_and(|size| !(8..=(1 << 20)).contains(&size))
        // 流式抽取要求块大小是 seek 对齐粒度的倍数（规范值 4096）
        || block_size_per_channel.is_some_and(|size| size % DSD_SEEK_ALIGN_BYTES != 0)
    {
        return Err(DecoderError::UnsupportedFormat(
            "implausible DSD header fields".into(),
        ));
    }
    Ok(())
}

/// F-4：chunk 偏移推进必须 checked 且严格单调递增，
/// 否则 u64 回绕导致向后 seek → chunk 循环重复解析 → 死循环。
/// 容器块遍历次数上限（DEC4-01），与标签解析（`dsd_tags.rs` / `wav_id3.rs`）同值。
/// 块大小全为 0 的构造文件每次只前进 12 字节，1 GB 文件要循环约九千万次「定位 + 读取」，
/// 而这段代码在打开文件时跑在引擎命令线程 / 导入线程上。
const MAX_CONTAINER_CHUNKS: usize = 4096;

fn too_many_chunks() -> DecoderError {
    DecoderError::UnsupportedFormat("too many container chunks".into())
}

fn advance_offset(chunk_start: u64, next: Option<u64>) -> Result<u64, DecoderError> {
    next.filter(|n| *n > chunk_start)
        .ok_or_else(|| DecoderError::UnsupportedFormat("corrupt chunk size".into()))
}

fn parse_dsf(file: &mut File) -> Result<ParsedDsd, DecoderError> {
    let mut header = [0_u8; 28];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"DSD " {
        return Err(DecoderError::UnsupportedFormat("invalid DSF header".into()));
    }

    let first_chunk_size = le_u64(&header[4..12]);
    file.seek(SeekFrom::Start(first_chunk_size))?;

    let mut channels = 0_usize;
    let mut dsd_sample_rate = 0_u32;
    let mut sample_count = 0_u64;
    let mut block_size_per_channel = 0_usize;
    let mut bit_order = BitOrder::LsbFirst;
    let mut data_start = 0_u64;
    let mut data_len = 0_u64;

    let mut visited = 0_usize;
    while let Some((id, chunk_start, chunk_size)) = read_dsf_chunk_header(file)? {
        visited += 1;
        if visited > MAX_CONTAINER_CHUNKS {
            return Err(too_many_chunks());
        }
        let payload_start = advance_offset(chunk_start, chunk_start.checked_add(12))?;
        let payload_len = chunk_size.saturating_sub(12);
        let next_chunk = advance_offset(chunk_start, payload_start.checked_add(payload_len))?;

        match &id {
            b"fmt " => {
                if payload_len > MAX_METADATA_CHUNK_LEN {
                    return Err(DecoderError::UnsupportedFormat(
                        "implausible DSF fmt chunk size".into(),
                    ));
                }
                let mut payload = vec![0_u8; payload_len as usize];
                file.read_exact(&mut payload)?;
                if payload.len() < 40 {
                    return Err(DecoderError::UnsupportedFormat(
                        "invalid DSF fmt chunk".into(),
                    ));
                }
                channels = le_u32(&payload[12..16]) as usize;
                dsd_sample_rate = le_u32(&payload[16..20]);
                // F-12：DSF fmt 的 bits_per_sample 字段（偏移 20）：1 = LSB first（常见），
                // 8 = MSB first（罕见变体）。忽略该字段会把 8-bit 变体解成噪声。
                bit_order = match le_u32(&payload[20..24]) {
                    1 => BitOrder::LsbFirst,
                    8 => BitOrder::MsbFirst,
                    other => {
                        return Err(DecoderError::UnsupportedFormat(format!(
                            "unsupported DSF bits-per-sample: {other}"
                        )));
                    }
                };
                sample_count = le_u64(&payload[24..32]);
                block_size_per_channel = le_u32(&payload[32..36]) as usize;
                validate_dsd_header(channels, dsd_sample_rate, Some(block_size_per_channel))?;
                if sample_count == 0 {
                    return Err(DecoderError::UnsupportedFormat(
                        "DSF sample count is zero".into(),
                    ));
                }
            }
            b"data" => {
                data_start = payload_start;
                data_len = payload_len;
                file.seek(SeekFrom::Start(next_chunk))?;
            }
            _ => {
                file.seek(SeekFrom::Start(next_chunk))?;
            }
        }

        if channels > 0 && dsd_sample_rate > 0 && block_size_per_channel > 0 && data_len > 0 {
            break;
        }
    }

    if channels == 0 || dsd_sample_rate == 0 || block_size_per_channel == 0 || data_len == 0 {
        return Err(DecoderError::UnsupportedFormat(
            "incomplete DSF stream".into(),
        ));
    }

    // 审2：data chunk 声明长度与文件真实大小交叉校验——截断/损坏文件的
    // chunk size 可能虚大，时长与进度会按虚值严重失真（流式读取本身安全）。
    let file_len = file.metadata().map(|meta| meta.len()).unwrap_or(u64::MAX);
    let data_len = data_len.min(file_len.saturating_sub(data_start));
    let decodable_samples = (data_len / channels.max(1) as u64).saturating_mul(8);
    let sample_count = sample_count.min(decodable_samples.max(1));

    // F-1：有效音频字节数（每声道），不含末块的 block 对齐零填充；
    // 与 data chunk 实际容量取 min，防 sample_count 虚大。
    let audio_bytes_per_channel = sample_count
        .div_ceil(8)
        .min(data_len / channels.max(1) as u64);

    Ok(ParsedDsd {
        info: dsd_stream_info(dsd_sample_rate, channels, sample_count),
        layout: DsdLayout::Dsf {
            block_size_per_channel,
        },
        bit_order,
        dsd_sample_rate,
        channels,
        data_start,
        data_len,
        audio_bytes_per_channel,
    })
}

fn parse_dff(file: &mut File) -> Result<ParsedDsd, DecoderError> {
    let mut header = [0_u8; 16];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"FRM8" || &header[12..16] != b"DSD " {
        return Err(DecoderError::UnsupportedFormat("invalid DFF header".into()));
    }

    let file_size = be_u64(&header[4..12]);
    let file_end = 12_u64.saturating_add(file_size);
    let mut channels = 0_usize;
    let mut dsd_sample_rate = 0_u32;
    let mut compression = *b"DSD ";
    let mut data_start = 0_u64;
    let mut data_len = 0_u64;

    let mut visited = 0_usize;
    while let Some((id, chunk_start, chunk_size)) = read_dff_chunk_header(file, file_end)? {
        visited += 1;
        if visited > MAX_CONTAINER_CHUNKS {
            return Err(too_many_chunks());
        }
        let payload_start = advance_offset(chunk_start, chunk_start.checked_add(12))?;
        let padded_size = chunk_size
            .checked_add(chunk_size & 1)
            .ok_or_else(|| DecoderError::UnsupportedFormat("corrupt chunk size".into()))?;
        let next_chunk = advance_offset(chunk_start, payload_start.checked_add(padded_size))?;

        match &id {
            b"PROP" => {
                if chunk_size > MAX_METADATA_CHUNK_LEN {
                    return Err(DecoderError::UnsupportedFormat(
                        "implausible DFF PROP chunk size".into(),
                    ));
                }
                let mut payload = vec![0_u8; chunk_size as usize];
                file.read_exact(&mut payload)?;
                // F-12：奇数长度 chunk 后有 1 字节 pad，必须跳过，
                // 否则后续 chunk 头错位解析。
                file.seek(SeekFrom::Start(next_chunk))?;
                parse_dff_prop(
                    &payload,
                    &mut channels,
                    &mut dsd_sample_rate,
                    &mut compression,
                );
            }
            b"DSD " => {
                data_start = payload_start;
                data_len = chunk_size;
                file.seek(SeekFrom::Start(next_chunk))?;
            }
            b"DST " => {
                return Err(DecoderError::UnsupportedFormat(
                    "DST-compressed DFF requires FFmpeg fallback".into(),
                ));
            }
            _ => {
                file.seek(SeekFrom::Start(next_chunk))?;
            }
        }
    }

    if compression != *b"DSD " {
        return Err(DecoderError::UnsupportedFormat(
            "compressed DFF requires FFmpeg fallback".into(),
        ));
    }
    if channels == 0 || dsd_sample_rate == 0 || data_len == 0 {
        return Err(DecoderError::UnsupportedFormat(
            "incomplete DFF stream".into(),
        ));
    }
    validate_dsd_header(channels, dsd_sample_rate, None)?;

    // 审2：DSD chunk 声明长度与文件真实大小交叉校验（同 DSF 端说明）。
    let file_len = file.metadata().map(|meta| meta.len()).unwrap_or(u64::MAX);
    let data_len = data_len.min(file_len.saturating_sub(data_start));

    let sample_count = (data_len / channels as u64) * 8;
    Ok(ParsedDsd {
        info: dsd_stream_info(dsd_sample_rate, channels, sample_count),
        layout: DsdLayout::Dff,
        bit_order: BitOrder::MsbFirst,
        dsd_sample_rate,
        channels,
        data_start,
        data_len,
        audio_bytes_per_channel: data_len / channels as u64,
    })
}

fn parse_dff_prop(
    payload: &[u8],
    channels: &mut usize,
    dsd_sample_rate: &mut u32,
    compression: &mut [u8; 4],
) {
    if payload.len() < 4 || &payload[0..4] != b"SND " {
        return;
    }

    let mut offset = 4_usize;
    while offset + 12 <= payload.len() {
        let id = &payload[offset..offset + 4];
        // size 上限钳到 payload 长度，防止 u64→usize 巨值让偏移推进回绕
        let size = be_u64(&payload[offset + 4..offset + 12]).min(payload.len() as u64) as usize;
        let data_start = offset + 12;
        let data_end = data_start.saturating_add(size).min(payload.len());
        let data = &payload[data_start..data_end];

        match id {
            b"FS  " if data.len() >= 4 => *dsd_sample_rate = be_u32(&data[0..4]),
            b"CHNL" if data.len() >= 2 => *channels = be_u16(&data[0..2]) as usize,
            b"CMPR" if data.len() >= 4 => compression.copy_from_slice(&data[0..4]),
            _ => {}
        }

        offset = data_start + size + (size & 1);
    }
}

fn read_dsf_chunk_header(file: &mut File) -> Result<Option<([u8; 4], u64, u64)>, DecoderError> {
    let chunk_start = file.stream_position()?;
    let mut header = [0_u8; 12];
    match file.read_exact(&mut header) {
        Ok(()) => Ok(Some((
            [header[0], header[1], header[2], header[3]],
            chunk_start,
            le_u64(&header[4..12]),
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(err) => Err(DecoderError::Io(err)),
    }
}

fn read_dff_chunk_header(
    file: &mut File,
    file_end: u64,
) -> Result<Option<([u8; 4], u64, u64)>, DecoderError> {
    let chunk_start = file.stream_position()?;
    if chunk_start + 12 > file_end {
        return Ok(None);
    }

    let mut header = [0_u8; 12];
    match file.read_exact(&mut header) {
        Ok(()) => Ok(Some((
            [header[0], header[1], header[2], header[3]],
            chunk_start,
            be_u64(&header[4..12]),
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(err) => Err(DecoderError::Io(err)),
    }
}

/// 把各声道的输出交错写入；所有声道喂入的字节数相同、相位一致，产出帧数必然相等。
fn interleave(per_channel: &[Vec<f32>]) -> Vec<f32> {
    let frames = per_channel.iter().map(Vec::len).min().unwrap_or(0);
    let mut samples = Vec::with_capacity(frames * per_channel.len());
    for frame in 0..frames {
        for channel in per_channel {
            samples.push(channel[frame]);
        }
    }
    samples
}

fn decode_dsf_block(
    raw: &[u8],
    decimators: &mut [ChannelDecimator],
    block_size_per_channel: usize,
    valid_bytes_per_channel: usize,
) -> Vec<f32> {
    let channels = decimators.len();
    if channels == 0 || block_size_per_channel == 0 {
        return Vec::new();
    }

    // DSF 数据按「每声道一整块 block_size_per_channel 字节」交错存放。
    // 末块组可能因单次 read 短读 / 文件损坏而不足 channels*block_size：
    // 必须以偏移最大的末声道(channel = channels-1)能覆盖的字节数来限制帧数，
    // 否则按完整 block 步长索引第 2+ 声道会 slice 越界 panic（原 bug）。
    let last_channel_start = (channels - 1) * block_size_per_channel;
    if raw.len() <= last_channel_start {
        // 末声道数据完全缺失，无法解出任何完整帧
        return Vec::new();
    }
    // F-1：再钳到有效音频字节（不含末块 block 对齐的零填充）
    let usable_bytes = (raw.len() - last_channel_start)
        .min(block_size_per_channel)
        .min(valid_bytes_per_channel);

    let per_channel: Vec<Vec<f32>> = decimators
        .iter_mut()
        .enumerate()
        .map(|(channel, decimator)| {
            let start = channel * block_size_per_channel;
            raw[start..start + usable_bytes]
                .iter()
                .filter_map(|byte| decimator.push(*byte))
                .collect()
        })
        .collect();
    interleave(&per_channel)
}

fn decode_dff_block(raw: &[u8], decimators: &mut [ChannelDecimator]) -> Vec<f32> {
    let channels = decimators.len();
    if channels == 0 {
        return Vec::new();
    }

    let byte_frames = raw.len() / channels;
    let mut samples = Vec::with_capacity(byte_frames / 4 * channels + channels);
    // DFF 按字节在声道间交错；各声道相位一致，同一个字节帧要么都出样本要么都不出。
    for frame in raw[..byte_frames * channels].chunks_exact(channels) {
        for (byte, decimator) in frame.iter().zip(decimators.iter_mut()) {
            if let Some(sample) = decimator.push(*byte) {
                samples.push(sample);
            }
        }
    }
    samples
}

/// 一阶高通 DC blocker：y[n] = x[n] - x[n-1] + R*y[n-1]
/// 去掉 Σ-Δ 调制器引入的低频直流偏置（典型 ≈ 0.01）。
/// R 由 open 时按实际 PCM 率算出（目标截止 ~2 Hz，F-2）。
fn apply_dc_blocker(samples: &mut [f32], channels: usize, state: &mut Vec<(f32, f32)>, r: f32) {
    let channels = channels.max(1);
    if state.len() < channels {
        state.resize(channels, (0.0, 0.0));
    }
    for (i, sample) in samples.iter_mut().enumerate() {
        let ch = i % channels;
        let (last_in, last_out) = state[ch];
        let current = *sample;
        let filtered = current - last_in + r * last_out;
        state[ch] = (current, filtered);
        *sample = filtered;
    }
}

fn dsd_stream_info(dsd_sample_rate: u32, channels: usize, sample_count: u64) -> StreamInfo {
    let pcm_rate = pcm_rate_of(dsd_sample_rate);
    StreamInfo {
        sample_rate: SampleRate(pcm_rate),
        bit_depth: BitDepth(24),
        channels: Channels(channels.min(u16::MAX as usize) as u16),
        duration_seconds: if dsd_sample_rate == 0 {
            0.0
        } else {
            sample_count as f64 / dsd_sample_rate as f64
        },
    }
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("u32 slice"))
}

fn le_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().expect("u64 slice"))
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().expect("u16 slice"))
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("u32 slice"))
}

fn be_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("u64 slice"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn decodes_minimal_dsf_packet() {
        let path = temp_audio_path("seraph-dsd", "dsf");
        write_test_dsf(&path);

        let mut decoder = DsdDecoder::new();
        decoder.open(&path).expect("open dsf");
        let info = decoder.info().expect("stream info");
        // DSP-01：DSD64 按 32:1 输出 88.2 kHz
        assert_eq!(info.sample_rate, SampleRate(88_200));
        assert_eq!(info.bit_depth, BitDepth(24));
        assert_eq!(info.channels, Channels(2));

        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        // 每声道 8 字节 → 2 帧；滤波器还在建立，只核对帧数与极性（ch0 全 1、ch1 全 0）
        assert_eq!(packet.samples.len(), 4);
        assert!(packet.samples.iter().all(|sample| sample.is_finite()));
        assert!(packet.samples[2] > packet.samples[3]);
        assert!(decoder.next_packet().unwrap().is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn decodes_minimal_dff_packet() {
        let path = temp_audio_path("seraph-dsdiff", "dff");
        write_test_dff(&path);

        let mut decoder = DsdDecoder::new();
        decoder.open(&path).expect("open dff");
        let info = decoder.info().expect("stream info");
        assert_eq!(info.sample_rate, SampleRate(88_200));
        assert_eq!(info.channels, Channels(2));

        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        assert_eq!(packet.samples.len(), 4);
        assert!(packet.samples.iter().all(|sample| sample.is_finite()));
        assert!(packet.samples[2] > packet.samples[3]);
        assert!(decoder.next_packet().unwrap().is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn dsf_trailing_block_padding_is_not_decoded() {
        // F-1：block 16 字节/声道，sample_count=64 → 每声道只有 8 字节有效音频，
        // 末尾 8 字节是 block 对齐的 0x00 填充。旧实现把填充解码成满幅负直流爆音。
        let path = temp_audio_path("seraph-dsd-pad", "dsf");
        write_test_dsf_padded(&path);

        let mut decoder = DsdDecoder::new();
        decoder.open(&path).expect("open dsf");

        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        // 每声道只有 8 字节有效 → 2 个 PCM 帧（2 声道 = 4 样本），填充部分被截断
        assert_eq!(packet.samples.len(), 4);
        assert!(decoder.next_packet().unwrap().is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_implausible_dsf_header_fields() {
        // F-4：channels 巨值 → 拒绝，而不是 vec![...; 42 亿] OOM abort
        let path = temp_audio_path("seraph-dsd-badch", "dsf");
        write_test_dsf_with(&path, 0xffff_ffff, 2_822_400, 8);
        let err = DsdDecoder::new().open(&path).unwrap_err();
        assert!(matches!(err, DecoderError::UnsupportedFormat(_)), "{err}");
        let _ = fs::remove_file(path);

        // block_size 巨值 → 拒绝
        let path = temp_audio_path("seraph-dsd-badblk", "dsf");
        write_test_dsf_with(&path, 2, 2_822_400, 0x7fff_ffff);
        let err = DsdDecoder::new().open(&path).unwrap_err();
        assert!(matches!(err, DecoderError::UnsupportedFormat(_)), "{err}");
        let _ = fs::remove_file(path);

        // dsd_sample_rate 离谱 → 拒绝
        let path = temp_audio_path("seraph-dsd-badrate", "dsf");
        write_test_dsf_with(&path, 2, 7, 8);
        let err = DsdDecoder::new().open(&path).unwrap_err();
        assert!(matches!(err, DecoderError::UnsupportedFormat(_)), "{err}");
        let _ = fs::remove_file(path);
    }

    /// DEC4-01：块大小全为 0 的文件每次只前进 12 字节，遍历必须有次数上限。
    #[test]
    fn rejects_dsf_with_endless_zero_sized_chunks() {
        let path = temp_audio_path("seraph-dsd-zero-chunks", "dsf");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"DSD ");
        bytes.extend_from_slice(&28_u64.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        for _ in 0..(MAX_CONTAINER_CHUNKS + 64) {
            bytes.extend_from_slice(b"junk");
            bytes.extend_from_slice(&0_u64.to_le_bytes());
        }
        fs::write(&path, bytes).expect("write dsf");

        let err = DsdDecoder::new().open(&path).unwrap_err();
        let _ = fs::remove_file(&path);
        assert!(
            matches!(&err, DecoderError::UnsupportedFormat(message) if message.contains("too many")),
            "{err}"
        );
    }

    #[test]
    fn rejects_overflowing_dsf_chunk_size() {
        // F-4：chunk size 接近 u64::MAX，偏移推进回绕 → 旧实现死循环/回退 seek
        let path = temp_audio_path("seraph-dsd-overflow", "dsf");
        let mut file = File::create(&path).expect("create dsf");
        file.write_all(b"DSD ").unwrap();
        file.write_all(&28_u64.to_le_bytes()).unwrap();
        file.write_all(&1000_u64.to_le_bytes()).unwrap();
        file.write_all(&0_u64.to_le_bytes()).unwrap();
        // 伪 chunk：size = u64::MAX → payload_start + payload_len 溢出
        file.write_all(b"junk").unwrap();
        file.write_all(&u64::MAX.to_le_bytes()).unwrap();
        drop(file);

        let err = DsdDecoder::new().open(&path).unwrap_err();
        assert!(matches!(err, DecoderError::UnsupportedFormat(_)), "{err}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn dsf_msb_first_variant_uses_msb_bit_order() {
        // F-12：bits_per_sample=8 → MSB first。ch0 数据只有第一个字节是 0x01：
        // 两种位序下这个 +1 位落在不同的抽头上，解码结果不同。
        let path = temp_audio_path("seraph-dsd-msb", "dsf");
        let mut ch0 = [0x00_u8; 8];
        ch0[0] = 0x01;
        write_test_dsf_bits(&path, 8, &ch0);
        let mut decoder = DsdDecoder::new();
        decoder.open(&path).expect("open msb dsf");
        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        // DC blocker 对首个样本是直通（初始状态为 0），可直接与抽取器对比
        let first = |order| {
            let mut decimator = ChannelDecimator::new(order, 4);
            ch0.iter().find_map(|byte| decimator.push(*byte)).unwrap()
        };
        let expected = first(BitOrder::MsbFirst);
        let unexpected = first(BitOrder::LsbFirst);
        assert_eq!(packet.samples[0], expected);
        assert_ne!(expected, unexpected, "位序应产生不同结果");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn dff_skips_pad_byte_after_odd_length_prop() {
        // F-12：奇数长度 PROP 后有 1 字节 pad；不跳过会导致后续 DSD chunk 头错位。
        let path = temp_audio_path("seraph-dsdiff-oddprop", "dff");
        write_test_dff_odd_prop(&path);

        let mut decoder = DsdDecoder::new();
        decoder.open(&path).expect("open dff with odd PROP");
        let info = decoder.info().expect("stream info");
        assert_eq!(info.channels, Channels(2));
        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        assert_eq!(packet.samples.len(), 4);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn byte_lut_matches_bitwise_reference() {
        // LUT 解码必须与逐 bit 参考实现一致
        let taps = stage1_taps();
        for order in [BitOrder::LsbFirst, BitOrder::MsbFirst] {
            let bytes: [u8; STAGE1_BYTES] = [
                0x69, 0xaa, 0x01, 0x80, 0xff, 0x00, 0x5a, 0xc3, 0x17, 0xe8, 0x3c, 0x96,
            ];
            let mut reference = 0.0_f64;
            for (slot, byte) in bytes.iter().enumerate() {
                for bit_in_byte in 0..8 {
                    let bit = match order {
                        BitOrder::LsbFirst => (byte >> bit_in_byte) & 1,
                        BitOrder::MsbFirst => (byte >> (7 - bit_in_byte)) & 1,
                    };
                    reference += f64::from(bit as i8 * 2 - 1) * taps[slot * 8 + bit_in_byte];
                }
            }
            let lut = byte_lut(order);
            let lut_value: f64 = bytes
                .iter()
                .enumerate()
                .map(|(slot, byte)| lut[slot][*byte as usize])
                .sum();
            assert!(
                (lut_value - reference).abs() < 1.0e-12,
                "{order:?}: {lut_value} vs {reference}"
            );
        }
    }

    fn decimators(channels: usize) -> Vec<ChannelDecimator> {
        (0..channels)
            .map(|_| ChannelDecimator::new(BitOrder::LsbFirst, 4))
            .collect()
    }

    #[test]
    fn decode_dsf_block_truncated_last_channel_does_not_panic() {
        // 复现旧越界 bug：channels=2, block_size=4096，末块组只短读到 5000 字节。
        // 旧实现按完整 block 步长索引第 2 声道 → &raw[6584..6592] 越界 panic。
        let raw = vec![0xaa_u8; 5000];
        let samples = decode_dsf_block(&raw, &mut decimators(2), 4096, usize::MAX);
        // 末声道起点 4096，仅剩 904 字节 → 226 帧 × 2 声道，且不 panic
        assert_eq!(samples.len(), 226 * 2);
    }

    #[test]
    fn decode_dsf_block_drops_block_with_missing_last_channel() {
        // raw 不足以覆盖末声道起点（4096）→ 无完整帧，安全返回空
        let raw = vec![0xaa_u8; 4096];
        assert!(decode_dsf_block(&raw, &mut decimators(2), 4096, usize::MAX).is_empty());
    }

    /// 滤波器在归一化频率 `f`（相对其采样率）处的幅度响应。
    fn magnitude(taps: &[f64], f: f64) -> f64 {
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for (index, tap) in taps.iter().enumerate() {
            let phase = 2.0 * std::f64::consts::PI * f * index as f64;
            re += tap * phase.cos();
            im -= tap * phase.sin();
        }
        re.hypot(im)
    }

    /// DSP-01 回归闸：两级级联后，通带平坦，且所有会折叠进输出频带的频率
    /// （输出奈奎斯特以上直到 DSD 奈奎斯特）抑制 > 90 dB。旧的 64 抽头 Hann 窗
    /// 在 30–80 kHz 只有 0–10 dB 的衰减。
    #[test]
    fn dsp01_cascade_is_flat_in_band_and_rejects_everything_that_aliases() {
        for (dsd_rate, ratio) in [(2_822_400.0_f64, 4_usize), (5_644_800.0, 8)] {
            let stage1 = stage1_taps();
            let stage2 = stage2_taps(ratio);
            let stage2_rate = dsd_rate / STAGE1_DECIMATION as f64;
            let output_nyquist = stage2_rate / ratio as f64 / 2.0;
            let response = |hz: f64| {
                // 第二级工作在 fs/8，其响应对 fs/8 周期重复
                magnitude(stage1, hz / dsd_rate) * magnitude(stage2, hz / stage2_rate)
            };

            let mut hz = 0.0;
            while hz <= 20_000.0 {
                let db = 20.0 * response(hz).log10();
                assert!(db.abs() < 0.05, "passband {hz} Hz: {db} dB");
                hz += 250.0;
            }
            let mut hz = output_nyquist;
            let mut worst = f64::NEG_INFINITY;
            while hz <= dsd_rate / 2.0 {
                worst = worst.max(20.0 * response(hz).max(1.0e-12).log10());
                hz += 500.0;
            }
            assert!(worst < -90.0, "DSD {dsd_rate}: worst stopband {worst} dB");
        }
    }

    /// 用一阶 Σ-Δ 调制器把正弦调成 DSD64 位流，解码后幅度应为输入 × DSD_GAIN，
    /// 且跨块连续（状态保留）——整段一次解码与分块解码结果一致。
    #[test]
    fn dsp01_decodes_modulated_sine_with_expected_level_and_block_continuity() {
        let dsd_rate = 2_822_400.0_f64;
        let amplitude = 0.25_f64;
        let bytes: Vec<u8> = {
            let mut integrator = 0.0_f64;
            (0..40_000)
                .map(|byte_index| {
                    let mut byte = 0_u8;
                    for bit in 0..8 {
                        let n = (byte_index * 8 + bit) as f64;
                        let input =
                            amplitude * (2.0 * std::f64::consts::PI * 1_000.0 * n / dsd_rate).sin();
                        let output = if integrator >= 0.0 { 1.0 } else { -1.0 };
                        integrator += input - output;
                        if output > 0.0 {
                            byte |= 1 << bit; // LSB first
                        }
                    }
                    byte
                })
                .collect()
        };

        let mut whole = ChannelDecimator::new(BitOrder::LsbFirst, 4);
        let all: Vec<f32> = bytes.iter().filter_map(|byte| whole.push(*byte)).collect();
        assert_eq!(all.len(), bytes.len() / 4);

        let mut chunked = ChannelDecimator::new(BitOrder::LsbFirst, 4);
        let pieces: Vec<f32> = bytes
            .chunks(4097) // 故意不是抽取比的倍数
            .flat_map(|chunk| {
                chunk
                    .iter()
                    .filter_map(|byte| chunked.push(*byte))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(all, pieces);

        // 跳过滤波器建立段，按 1 kHz 整周期（88.2 帧/周期 → 取 441 帧 = 5 周期的整数倍）量 RMS
        let steady = &all[1_000..1_000 + 4_410];
        let rms = (steady
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / steady.len() as f64)
            .sqrt();
        let expected = amplitude * f64::from(DSD_GAIN) / std::f64::consts::SQRT_2;
        assert!(
            (rms / expected - 1.0).abs() < 0.02,
            "rms {rms} vs expected {expected}"
        );
    }

    #[test]
    fn dsp01_rate_selection_keeps_output_at_or_above_88k2() {
        assert_eq!(pcm_rate_of(2_822_400), 88_200); // DSD64
        assert_eq!(pcm_rate_of(3_072_000), 96_000); // DSD64 (48k 系)
        assert_eq!(pcm_rate_of(5_644_800), 88_200); // DSD128
        assert_eq!(pcm_rate_of(11_289_600), 176_400); // DSD256
        assert_eq!(pcm_rate_of(22_579_200), 352_800); // DSD512
        assert!(pcm_rate_of(MAX_DSD_SAMPLE_RATE) <= MAX_SUPPORTED_SAMPLE_RATE);
    }

    fn write_dsf_fixture(
        path: &Path,
        channels: u32,
        dsd_rate: u32,
        bits_per_sample: u32,
        sample_count: u64,
        block_size_per_channel: u32,
        data: &[u8],
    ) {
        let data_len = data.len() as u64;
        let file_size = 28_u64 + 52 + 12 + data_len;

        let mut file = File::create(path).expect("create dsf");
        file.write_all(b"DSD ").unwrap();
        file.write_all(&28_u64.to_le_bytes()).unwrap();
        file.write_all(&file_size.to_le_bytes()).unwrap();
        file.write_all(&0_u64.to_le_bytes()).unwrap();

        file.write_all(b"fmt ").unwrap();
        file.write_all(&52_u64.to_le_bytes()).unwrap();
        file.write_all(&1_u32.to_le_bytes()).unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();
        file.write_all(&2_u32.to_le_bytes()).unwrap();
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&dsd_rate.to_le_bytes()).unwrap();
        file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        file.write_all(&sample_count.to_le_bytes()).unwrap();
        file.write_all(&block_size_per_channel.to_le_bytes())
            .unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();

        file.write_all(b"data").unwrap();
        file.write_all(&(12_u64 + data_len).to_le_bytes()).unwrap();
        file.write_all(data).unwrap();
    }

    fn write_test_dsf(path: &Path) {
        let mut data = vec![0xff_u8; 8];
        data.extend_from_slice(&[0x00; 8]);
        write_dsf_fixture(path, 2, 2_822_400, 1, 64, 8, &data);
    }

    fn write_test_dsf_padded(path: &Path) {
        // block 16 字节/声道；sample_count=64 → 每声道仅前 8 字节有效，
        // 后 8 字节为 block 对齐零填充。
        let mut data = Vec::new();
        data.extend_from_slice(&[0xff; 8]); // ch0 有效
        data.extend_from_slice(&[0x00; 8]); // ch0 填充
        data.extend_from_slice(&[0x00; 8]); // ch1 有效
        data.extend_from_slice(&[0x00; 8]); // ch1 填充
        write_dsf_fixture(path, 2, 2_822_400, 1, 64, 16, &data);
    }

    fn write_test_dsf_with(path: &Path, channels: u32, dsd_rate: u32, block_size: u32) {
        write_dsf_fixture(path, channels, dsd_rate, 1, 64, block_size, &[0xff; 16]);
    }

    fn write_test_dsf_bits(path: &Path, bits_per_sample: u32, first_channel_block: &[u8; 8]) {
        let mut data = first_channel_block.to_vec();
        data.extend_from_slice(&[0x00; 8]);
        write_dsf_fixture(path, 2, 2_822_400, bits_per_sample, 64, 8, &data);
    }

    fn write_test_dff(path: &Path) {
        let dsd_rate = 2_822_400_u32;
        let data_len = 16_u64;
        let chnl_size = 2_u64;
        let prop_size = 4_u64 + (12 + 4) + (12 + chnl_size) + (12 + 4);
        let frm8_size = 4_u64 + (12 + prop_size) + (12 + data_len);

        let mut file = File::create(path).expect("create dff");
        file.write_all(b"FRM8").unwrap();
        file.write_all(&frm8_size.to_be_bytes()).unwrap();
        file.write_all(b"DSD ").unwrap();

        file.write_all(b"PROP").unwrap();
        file.write_all(&prop_size.to_be_bytes()).unwrap();
        file.write_all(b"SND ").unwrap();

        file.write_all(b"FS  ").unwrap();
        file.write_all(&4_u64.to_be_bytes()).unwrap();
        file.write_all(&dsd_rate.to_be_bytes()).unwrap();

        file.write_all(b"CHNL").unwrap();
        file.write_all(&chnl_size.to_be_bytes()).unwrap();
        file.write_all(&2_u16.to_be_bytes()).unwrap();

        file.write_all(b"CMPR").unwrap();
        file.write_all(&4_u64.to_be_bytes()).unwrap();
        file.write_all(b"DSD ").unwrap();

        file.write_all(b"DSD ").unwrap();
        file.write_all(&data_len.to_be_bytes()).unwrap();
        for _ in 0..8 {
            file.write_all(&[0xff, 0x00]).unwrap();
        }
    }

    fn write_test_dff_odd_prop(path: &Path) {
        // PROP 总长为奇数（含一个 size=1 的 TITL 局部 chunk），
        // 其后必须有 1 字节 pad，否则 DSD chunk 头错位。
        let dsd_rate = 2_822_400_u32;
        let data_len = 16_u64;
        let chnl_size = 2_u64;
        let prop_size = 4_u64 + (12 + 4) + (12 + chnl_size) + (12 + 4) + (12 + 1); // 63，奇数
        let frm8_size = 4_u64 + (12 + prop_size + 1) + (12 + data_len);

        let mut file = File::create(path).expect("create dff");
        file.write_all(b"FRM8").unwrap();
        file.write_all(&frm8_size.to_be_bytes()).unwrap();
        file.write_all(b"DSD ").unwrap();

        file.write_all(b"PROP").unwrap();
        file.write_all(&prop_size.to_be_bytes()).unwrap();
        file.write_all(b"SND ").unwrap();

        file.write_all(b"FS  ").unwrap();
        file.write_all(&4_u64.to_be_bytes()).unwrap();
        file.write_all(&dsd_rate.to_be_bytes()).unwrap();

        file.write_all(b"CHNL").unwrap();
        file.write_all(&chnl_size.to_be_bytes()).unwrap();
        file.write_all(&2_u16.to_be_bytes()).unwrap();

        file.write_all(b"CMPR").unwrap();
        file.write_all(&4_u64.to_be_bytes()).unwrap();
        file.write_all(b"DSD ").unwrap();

        file.write_all(b"TITL").unwrap();
        file.write_all(&1_u64.to_be_bytes()).unwrap();
        file.write_all(b"x").unwrap();

        // PROP 长度为奇数 → 1 字节 pad
        file.write_all(&[0_u8]).unwrap();

        file.write_all(b"DSD ").unwrap();
        file.write_all(&data_len.to_be_bytes()).unwrap();
        for _ in 0..8 {
            file.write_all(&[0xff, 0x00]).unwrap();
        }
    }

    fn temp_audio_path(prefix: &str, extension: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
    }
}
