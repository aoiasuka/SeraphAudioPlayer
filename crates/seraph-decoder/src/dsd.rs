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
const DC_BLOCKER_R: f32 = 0.995; // 截止 ≈ 7 Hz @ 44.1 kHz

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

#[derive(Debug, Clone, Copy)]
enum DsdLayout {
    Dsf { block_size_per_channel: usize },
    Dff,
}

/// 每个容器规定的 byte 内 bit 排列顺序。Boxcar 算法对此不敏感，
/// 但 Hann 加权对 bit 在窗口里的位置敏感，必须按规范取位。
/// - DSF: LSB first（Sony DSD Stream File 规范）
/// - DFF: MSB first（Philips DSDIFF 规范）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitOrder {
    LsbFirst,
    MsbFirst,
}

impl DsdLayout {
    fn bit_order(self) -> BitOrder {
        match self {
            Self::Dsf { .. } => BitOrder::LsbFirst,
            Self::Dff => BitOrder::MsbFirst,
        }
    }
}

pub struct DsdDecoder {
    info: Option<StreamInfo>,
    file: Option<File>,
    layout: Option<DsdLayout>,
    dsd_sample_rate: u32,
    channels: usize,
    data_start: u64,
    data_len: u64,
    data_read: u64,
    pcm_frames_emitted: u64,
    dc_state: Vec<(f32, f32)>, // 每通道一份 (last_input, last_output)
}

impl DsdDecoder {
    pub fn new() -> Self {
        Self {
            info: None,
            file: None,
            layout: None,
            dsd_sample_rate: 0,
            channels: 0,
            data_start: 0,
            data_len: 0,
            data_read: 0,
            pcm_frames_emitted: 0,
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
        self.dsd_sample_rate = parsed.dsd_sample_rate;
        self.channels = parsed.channels;
        self.data_start = parsed.data_start;
        self.data_len = parsed.data_len;
        self.data_read = 0;
        self.pcm_frames_emitted = 0;
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

        let order = layout.bit_order();
        let mut samples = match layout {
            DsdLayout::Dsf {
                block_size_per_channel,
            } => decode_dsf_block(&raw, channels, block_size_per_channel, order),
            DsdLayout::Dff => decode_dff_block(&raw, channels, order),
        };

        if samples.is_empty() {
            return Ok(None);
        }

        apply_dc_blocker(&mut samples, channels, &mut self.dc_state);
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
    dsd_sample_rate: u32,
    channels: usize,
    data_start: u64,
    data_len: u64,
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
    let mut data_start = 0_u64;
    let mut data_len = 0_u64;

    while let Some((id, chunk_start, chunk_size)) = read_dsf_chunk_header(file)? {
        let payload_start = chunk_start + 12;
        let payload_len = chunk_size.saturating_sub(12);

        match &id {
            b"fmt " => {
                let mut payload = vec![0_u8; payload_len as usize];
                file.read_exact(&mut payload)?;
                if payload.len() < 40 {
                    return Err(DecoderError::UnsupportedFormat(
                        "invalid DSF fmt chunk".into(),
                    ));
                }
                channels = le_u32(&payload[12..16]) as usize;
                dsd_sample_rate = le_u32(&payload[16..20]);
                sample_count = le_u64(&payload[24..32]);
                block_size_per_channel = le_u32(&payload[32..36]) as usize;
            }
            b"data" => {
                data_start = payload_start;
                data_len = payload_len;
                file.seek(SeekFrom::Start(payload_start + payload_len))?;
            }
            _ => {
                file.seek(SeekFrom::Start(payload_start + payload_len))?;
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

    Ok(ParsedDsd {
        info: dsd_stream_info(dsd_sample_rate, channels, sample_count),
        layout: DsdLayout::Dsf {
            block_size_per_channel,
        },
        dsd_sample_rate,
        channels,
        data_start,
        data_len,
    })
}

fn parse_dff(file: &mut File) -> Result<ParsedDsd, DecoderError> {
    let mut header = [0_u8; 16];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"FRM8" || &header[12..16] != b"DSD " {
        return Err(DecoderError::UnsupportedFormat("invalid DFF header".into()));
    }

    let file_size = be_u64(&header[4..12]);
    let file_end = 12 + file_size;
    let mut channels = 0_usize;
    let mut dsd_sample_rate = 0_u32;
    let mut compression = *b"DSD ";
    let mut data_start = 0_u64;
    let mut data_len = 0_u64;

    while let Some((id, chunk_start, chunk_size)) = read_dff_chunk_header(file, file_end)? {
        let payload_start = chunk_start + 12;

        match &id {
            b"PROP" => {
                let mut payload = vec![0_u8; chunk_size as usize];
                file.read_exact(&mut payload)?;
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
                file.seek(SeekFrom::Start(payload_start + padded(chunk_size)))?;
            }
            b"DST " => {
                return Err(DecoderError::UnsupportedFormat(
                    "DST-compressed DFF requires FFmpeg fallback".into(),
                ));
            }
            _ => {
                file.seek(SeekFrom::Start(payload_start + padded(chunk_size)))?;
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

    let sample_count = (data_len / channels as u64) * 8;
    Ok(ParsedDsd {
        info: dsd_stream_info(dsd_sample_rate, channels, sample_count),
        layout: DsdLayout::Dff,
        dsd_sample_rate,
        channels,
        data_start,
        data_len,
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
        let size = be_u64(&payload[offset + 4..offset + 12]) as usize;
        let data_start = offset + 12;
        let data_end = data_start.saturating_add(size).min(payload.len());
        let data = &payload[data_start..data_end];

        match id {
            b"FS  " if data.len() >= 4 => *dsd_sample_rate = be_u32(&data[0..4]),
            b"CHNL" if data.len() >= 2 => *channels = be_u16(&data[0..2]) as usize,
            b"CMPR" if data.len() >= 4 => compression.copy_from_slice(&data[0..4]),
            _ => {}
        }

        offset = data_start + padded(size as u64) as usize;
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
    let usable_bytes = (raw.len() - last_channel_start).min(block_size_per_channel);
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
fn dsd_64_to_pcm(bytes: &[u8], order: BitOrder) -> f32 {
    let taps = hann_taps();
    let mut acc = 0.0_f32;
    let mut tap = 0_usize;
    for byte in bytes {
        for bit_in_byte in 0..8 {
            let bit = match order {
                BitOrder::LsbFirst => (byte >> bit_in_byte) & 1,
                BitOrder::MsbFirst => (byte >> (7 - bit_in_byte)) & 1,
            };
            // bit=1 -> +1，bit=0 -> -1
            let signed = (bit as i8 * 2 - 1) as f32;
            acc += signed * taps[tap];
            tap += 1;
        }
    }
    acc
}

/// 一阶高通 DC blocker：y[n] = x[n] - x[n-1] + R*y[n-1]
/// 去掉 Σ-Δ 调制器引入的低频直流偏置（典型 ≈ 0.01）。
fn apply_dc_blocker(samples: &mut [f32], channels: usize, state: &mut Vec<(f32, f32)>) {
    let channels = channels.max(1);
    if state.len() < channels {
        state.resize(channels, (0.0, 0.0));
    }
    for (i, sample) in samples.iter_mut().enumerate() {
        let ch = i % channels;
        let (last_in, last_out) = state[ch];
        let current = *sample;
        let filtered = current - last_in + DC_BLOCKER_R * last_out;
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

fn padded(value: u64) -> u64 {
    value + (value & 1)
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
        assert_eq!(packet.samples, vec![1.0, -1.0]);
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
        assert_eq!(packet.samples, vec![1.0, -1.0]);
        assert!(decoder.next_packet().unwrap().is_none());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn decode_dsf_block_truncated_last_channel_does_not_panic() {
        // 复现旧越界 bug：channels=2, block_size=4096，末块组只短读到 5000 字节。
        // 旧实现按完整 block 步长索引第 2 声道 → &raw[6584..6592] 越界 panic。
        let raw = vec![0xaa_u8; 5000];
        let samples = decode_dsf_block(&raw, 2, 4096, BitOrder::LsbFirst);
        // 末声道起点 4096，仅剩 904 字节 → 113 帧 × 2 声道，且不 panic
        assert_eq!(samples.len(), 113 * 2);
    }

    #[test]
    fn decode_dsf_block_drops_block_with_missing_last_channel() {
        // raw 不足以覆盖末声道起点（4096）→ 无完整帧，安全返回空
        let raw = vec![0xaa_u8; 4096];
        assert!(decode_dsf_block(&raw, 2, 4096, BitOrder::LsbFirst).is_empty());
    }

    fn write_test_dsf(path: &Path) {
        let channels = 2_u32;
        let dsd_rate = 2_822_400_u32;
        let sample_count = 64_u64;
        let block_size_per_channel = 8_u32;
        let data_len = channels as u64 * block_size_per_channel as u64;
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
        file.write_all(&1_u32.to_le_bytes()).unwrap();
        file.write_all(&sample_count.to_le_bytes()).unwrap();
        file.write_all(&block_size_per_channel.to_le_bytes())
            .unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();

        file.write_all(b"data").unwrap();
        file.write_all(&(12_u64 + data_len).to_le_bytes()).unwrap();
        file.write_all(&[0xff; 8]).unwrap();
        file.write_all(&[0x00; 8]).unwrap();
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

    fn temp_audio_path(prefix: &str, extension: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
    }
}
