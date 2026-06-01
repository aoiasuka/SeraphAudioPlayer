//! Lightweight DSD decoder.
//!
//! DSF is converted to interleaved f32 PCM by averaging each 64 DSD bits into
//! one PCM sample. This is intentionally simple and deterministic; the audio
//! engine can later swap this stage for a higher quality FIR decimator without
//! changing the decoder trait.

use crate::decoder::{Decoder, DecoderError, Packet, StreamInfo};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const DSD_TO_PCM_DECIMATION: usize = 64;
const DSD_BYTES_PER_PCM_SAMPLE: usize = DSD_TO_PCM_DECIMATION / 8;
const DFF_PACKET_BYTE_FRAMES: usize = 4096;

#[derive(Debug, Clone, Copy)]
enum DsdLayout {
    Dsf { block_size_per_channel: usize },
    Dff,
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
        let timestamp_seconds = if self.dsd_sample_rate == 0 || channels == 0 {
            0.0
        } else {
            ((self.data_read / channels as u64) * 8) as f64 / self.dsd_sample_rate as f64
        };

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
        let bytes_read = self
            .file
            .as_mut()
            .ok_or_else(|| DecoderError::Internal("file is not open".into()))?
            .read(&mut raw)?;
        if bytes_read == 0 {
            return Ok(None);
        }
        raw.truncate(bytes_read);
        self.data_read += bytes_read as u64;

        let samples = match layout {
            DsdLayout::Dsf {
                block_size_per_channel,
            } => decode_dsf_block(&raw, channels, block_size_per_channel),
            DsdLayout::Dff => decode_dff_block(&raw, channels),
        };

        if samples.is_empty() {
            return Ok(None);
        }

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

fn decode_dsf_block(raw: &[u8], channels: usize, block_size_per_channel: usize) -> Vec<f32> {
    if channels == 0 || block_size_per_channel < DSD_BYTES_PER_PCM_SAMPLE {
        return Vec::new();
    }

    let available_channels = raw.len() / block_size_per_channel;
    let channels = channels.min(available_channels);
    if channels == 0 {
        return Vec::new();
    }

    let pcm_frames = block_size_per_channel / DSD_BYTES_PER_PCM_SAMPLE;
    let mut samples = Vec::with_capacity(pcm_frames * channels);

    for frame in 0..pcm_frames {
        let frame_offset = frame * DSD_BYTES_PER_PCM_SAMPLE;
        for channel in 0..channels {
            let channel_offset = channel * block_size_per_channel + frame_offset;
            samples.push(dsd_64_to_pcm(&raw[channel_offset..channel_offset + 8]));
        }
    }

    samples
}

fn decode_dff_block(raw: &[u8], channels: usize) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }

    let byte_frames = raw.len() / channels;
    let pcm_frames = byte_frames / DSD_BYTES_PER_PCM_SAMPLE;
    let mut samples = Vec::with_capacity(pcm_frames * channels);

    for frame in 0..pcm_frames {
        let byte_frame_offset = frame * DSD_BYTES_PER_PCM_SAMPLE;
        for channel in 0..channels {
            let mut bytes = [0_u8; DSD_BYTES_PER_PCM_SAMPLE];
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = raw[(byte_frame_offset + index) * channels + channel];
            }
            samples.push(dsd_64_to_pcm(&bytes));
        }
    }

    samples
}

fn dsd_64_to_pcm(bytes: &[u8]) -> f32 {
    let ones = bytes.iter().map(|byte| byte.count_ones()).sum::<u32>() as f32;
    ((ones * 2.0) - DSD_TO_PCM_DECIMATION as f32) / DSD_TO_PCM_DECIMATION as f32
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
