//! Lightweight DSD decoder.
//!
//! DSF/DFF 被以 64:1 抽取为 interleaved f32 PCM。
//! 抗混叠：相对原来的 boxcar (popcount/64) 取均值，
//! 改为 Hann 加权积分 + 后置一阶 IIR DC blocker，
//! 旁瓣从 -13 dB 抑制到 ≈ -30 dB，
//! 同时去掉 DSD 调制器引入的低频偏置。

use crate::decoder::{Decoder, DecoderError, Packet, StreamInfo};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::OnceLock,
};

const DSD_TO_PCM_DECIMATION: usize = 64;
const DSD_BYTES_PER_PCM_SAMPLE: usize = DSD_TO_PCM_DECIMATION / 8;
const DFF_PACKET_BYTE_FRAMES: usize = 4096;
/// SACD 0 dB 参考 = 50% 调制深度 → 直接抽取的 PCM 峰值只有 ±0.5（−6 dBFS）。
/// 业界惯例（foobar SACD 插件等）补 +6.02 dB，使 DSD 与 PCM 曲目响度一致。
/// 超出 ±1.0 的瞬时峰值由 render 端已有的 clamp 兜底。
const DSD_GAIN: f32 = 2.0;
/// DC blocker 目标截止频率（Hz）。R 按流的实际 PCM 率换算（见 open）。
const DC_BLOCKER_CUTOFF_HZ: f32 = 2.0;
/// 元数据类 chunk（fmt/PROP）载荷读入上限，防止损坏/恶意文件触发巨型分配。
const MAX_METADATA_CHUNK_LEN: u64 = 1 << 20;

/// 64-tap Hann 加权窗，预计算一次。
fn hann_taps() -> &'static [f32; DSD_TO_PCM_DECIMATION] {
    static TAPS: OnceLock<[f32; DSD_TO_PCM_DECIMATION]> = OnceLock::new();
    TAPS.get_or_init(|| {
        let mut taps = [0.0_f32; DSD_TO_PCM_DECIMATION];
        let mut sum = 0.0_f32;
        for (i, slot) in taps.iter_mut().enumerate() {
            let theta = std::f32::consts::PI * (i as f32 + 0.5) / DSD_TO_PCM_DECIMATION as f32;
            let value = 0.5 - 0.5 * (2.0 * theta).cos();
            *slot = value;
            sum += value;
        }
        // 归一化使全 1 输入得到 +1，全 0 输入得到 -1
        for slot in taps.iter_mut() {
            *slot /= sum;
        }
        taps
    })
}

/// 按字节的 LUT：`lut[byte_pos][byte_value]` = 该字节 8 个 bit 对应 tap 的部分和。
/// 把逐 bit 的 64 次乘加降为 8 次查表相加（F-16）。
type DsdByteLut = [[f32; 256]; DSD_BYTES_PER_PCM_SAMPLE];

fn build_byte_lut(order: BitOrder) -> Box<DsdByteLut> {
    let taps = hann_taps();
    let mut lut: Box<DsdByteLut> = Box::new([[0.0; 256]; DSD_BYTES_PER_PCM_SAMPLE]);
    for (byte_pos, table) in lut.iter_mut().enumerate() {
        for (value, slot) in table.iter_mut().enumerate() {
            let byte = value as u8;
            let mut acc = 0.0_f32;
            for bit_in_byte in 0..8 {
                let bit = match order {
                    BitOrder::LsbFirst => (byte >> bit_in_byte) & 1,
                    BitOrder::MsbFirst => (byte >> (7 - bit_in_byte)) & 1,
                };
                let signed = (bit as i8 * 2 - 1) as f32;
                acc += signed * taps[byte_pos * 8 + bit_in_byte];
            }
            *slot = acc;
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
        let pcm_rate = (parsed.dsd_sample_rate / DSD_TO_PCM_DECIMATION as u32).max(1) as f32;
        self.dc_r = (1.0 - 2.0 * std::f32::consts::PI * DC_BLOCKER_CUTOFF_HZ / pcm_rate)
            .clamp(0.995, 0.999_99);
        self.dc_state = vec![(0.0, 0.0); parsed.channels];
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
        let pcm_rate = (self.dsd_sample_rate as usize / DSD_TO_PCM_DECIMATION).max(1) as u64;
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

        let order = self.bit_order;
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
                decode_dsf_block(&raw, channels, block_size_per_channel, valid_bytes, order)
            }
            DsdLayout::Dff => decode_dff_block(&raw, channels, order),
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
        let mut byte_offset = match layout {
            DsdLayout::Dsf {
                block_size_per_channel,
            } => {
                let block_size = block_size_per_channel as u64;
                let pcm_aligned_byte = byte_frame - (byte_frame % DSD_BYTES_PER_PCM_SAMPLE as u64);
                let block_index = pcm_aligned_byte / block_size;
                block_index * block_size * channels
            }
            DsdLayout::Dff => {
                let aligned = byte_frame - (byte_frame % DSD_BYTES_PER_PCM_SAMPLE as u64);
                aligned * channels
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
        // PCM 帧计数对齐到 seek 位置（按 byte_offset 反推）
        let pcm_rate = (self.dsd_sample_rate as u64 / DSD_TO_PCM_DECIMATION as u64).max(1);
        self.pcm_frames_emitted = (seconds.max(0.0) * pcm_rate as f64) as u64;
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
fn validate_dsd_header(
    channels: usize,
    dsd_sample_rate: u32,
    block_size_per_channel: Option<usize>,
) -> Result<(), DecoderError> {
    if !(1..=32).contains(&channels)
        || !(64_000..=100_000_000).contains(&dsd_sample_rate)
        || block_size_per_channel.is_some_and(|size| !(8..=(1 << 20)).contains(&size))
    {
        return Err(DecoderError::UnsupportedFormat(
            "implausible DSD header fields".into(),
        ));
    }
    Ok(())
}

/// F-4：chunk 偏移推进必须 checked 且严格单调递增，
/// 否则 u64 回绕导致向后 seek → chunk 循环重复解析 → 死循环。
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

    while let Some((id, chunk_start, chunk_size)) = read_dsf_chunk_header(file)? {
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

    while let Some((id, chunk_start, chunk_size)) = read_dff_chunk_header(file, file_end)? {
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

fn decode_dsf_block(
    raw: &[u8],
    channels: usize,
    block_size_per_channel: usize,
    valid_bytes_per_channel: usize,
    order: BitOrder,
) -> Vec<f32> {
    if channels == 0 || block_size_per_channel < DSD_BYTES_PER_PCM_SAMPLE {
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
    let pcm_frames = usable_bytes / DSD_BYTES_PER_PCM_SAMPLE;
    if pcm_frames == 0 {
        return Vec::new();
    }
    let mut samples = Vec::with_capacity(pcm_frames * channels);

    for frame in 0..pcm_frames {
        let frame_offset = frame * DSD_BYTES_PER_PCM_SAMPLE;
        for channel in 0..channels {
            let channel_offset = channel * block_size_per_channel + frame_offset;
            samples.push(dsd_64_to_pcm(
                &raw[channel_offset..channel_offset + DSD_BYTES_PER_PCM_SAMPLE],
                order,
            ));
        }
    }

    samples
}

fn decode_dff_block(raw: &[u8], channels: usize, order: BitOrder) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }

    let byte_frames = raw.len() / channels;
    let pcm_frames = byte_frames / DSD_BYTES_PER_PCM_SAMPLE;
    if pcm_frames == 0 {
        return Vec::new();
    }
    let mut samples = Vec::with_capacity(pcm_frames * channels);

    for frame in 0..pcm_frames {
        let byte_frame_offset = frame * DSD_BYTES_PER_PCM_SAMPLE;
        for channel in 0..channels {
            let mut bytes = [0_u8; DSD_BYTES_PER_PCM_SAMPLE];
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = raw[(byte_frame_offset + index) * channels + channel];
            }
            samples.push(dsd_64_to_pcm(&bytes, order));
        }
    }

    samples
}

/// 把 64 bit DSD 抽取成 1 个 PCM 样本，Hann 加权抑制高频混叠。
/// 按字节 LUT（8 次查表相加）代替逐 bit 64 次乘加（F-16），
/// 并施加 +6.02 dB 增益补偿（F-5）。
fn dsd_64_to_pcm(bytes: &[u8], order: BitOrder) -> f32 {
    let lut = byte_lut(order);
    let mut acc = 0.0_f32;
    for (byte_pos, byte) in bytes.iter().enumerate() {
        acc += lut[byte_pos][*byte as usize];
    }
    acc * DSD_GAIN
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
    let pcm_rate = (dsd_sample_rate / DSD_TO_PCM_DECIMATION as u32).max(1);
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
        assert_eq!(info.sample_rate, SampleRate(44_100));
        assert_eq!(info.bit_depth, BitDepth(24));
        assert_eq!(info.channels, Channels(2));

        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        // F-5：+6.02 dB 增益补偿后，全 1 → +2.0，全 0 → -2.0
        assert_eq!(packet.samples.len(), 2);
        assert!((packet.samples[0] - 2.0).abs() < 1.0e-4);
        assert!((packet.samples[1] + 2.0).abs() < 1.0e-4);
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
        assert_eq!(info.sample_rate, SampleRate(44_100));
        assert_eq!(info.channels, Channels(2));

        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        assert_eq!(packet.samples.len(), 2);
        assert!((packet.samples[0] - 2.0).abs() < 1.0e-4);
        assert!((packet.samples[1] + 2.0).abs() < 1.0e-4);
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
        // 只应产出 1 个 PCM 帧（2 声道 = 2 样本），填充部分被截断
        assert_eq!(packet.samples.len(), 2);
        assert!((packet.samples[0] - 2.0).abs() < 1.0e-4);
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
        // F-12：bits_per_sample=8 → MSB first。ch0 数据只有第一个字节是 0x01，
        // LSB-first 下 +1 位落在 tap 0，MSB-first 下落在 tap 7 —— Hann 权重不同。
        // （若用 0x01 重复 8 次，两种位序的 tap 集合互为 Hann 对称镜像、结果相同，
        // 无法区分位序。）
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
        let expected = dsd_64_to_pcm(&ch0, BitOrder::MsbFirst);
        let unexpected = dsd_64_to_pcm(&ch0, BitOrder::LsbFirst);
        assert!((packet.samples[0] - expected).abs() < 1.0e-5);
        assert!((expected - unexpected).abs() > 1.0e-3, "位序应产生不同结果");
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
        assert!((packet.samples[0] - 2.0).abs() < 1.0e-4);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn byte_lut_matches_bitwise_reference() {
        // LUT 解码必须与逐 bit 参考实现一致
        let taps = hann_taps();
        for order in [BitOrder::LsbFirst, BitOrder::MsbFirst] {
            let bytes: [u8; 8] = [0x69, 0xaa, 0x01, 0x80, 0xff, 0x00, 0x5a, 0xc3];
            let mut reference = 0.0_f32;
            for (byte_pos, byte) in bytes.iter().enumerate() {
                for bit_in_byte in 0..8 {
                    let bit = match order {
                        BitOrder::LsbFirst => (byte >> bit_in_byte) & 1,
                        BitOrder::MsbFirst => (byte >> (7 - bit_in_byte)) & 1,
                    };
                    reference += (bit as i8 * 2 - 1) as f32 * taps[byte_pos * 8 + bit_in_byte];
                }
            }
            let lut_value = dsd_64_to_pcm(&bytes, order);
            assert!(
                (lut_value - reference * DSD_GAIN).abs() < 1.0e-5,
                "{order:?}: {lut_value} vs {reference}"
            );
        }
    }

    #[test]
    fn decode_dsf_block_truncated_last_channel_does_not_panic() {
        // 复现旧越界 bug：channels=2, block_size=4096，末块组只短读到 5000 字节。
        // 旧实现按完整 block 步长索引第 2 声道 → &raw[6584..6592] 越界 panic。
        let raw = vec![0xaa_u8; 5000];
        let samples = decode_dsf_block(&raw, 2, 4096, usize::MAX, BitOrder::LsbFirst);
        // 末声道起点 4096，仅剩 904 字节 → 113 帧 × 2 声道，且不 panic
        assert_eq!(samples.len(), 113 * 2);
    }

    #[test]
    fn decode_dsf_block_drops_block_with_missing_last_channel() {
        // raw 不足以覆盖末声道起点（4096）→ 无完整帧，安全返回空
        let raw = vec![0xaa_u8; 4096];
        assert!(decode_dsf_block(&raw, 2, 4096, usize::MAX, BitOrder::LsbFirst).is_empty());
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
