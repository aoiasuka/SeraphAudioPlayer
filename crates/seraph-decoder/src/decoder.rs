use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{fs::File, io::Read, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecoderError {
    #[error("decoder not implemented yet")]
    NotImplemented,
    #[error("file not found")]
    FileNotFound,
    #[error("format not supported: {0}")]
    UnsupportedFormat(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub samples: Vec<f32>,
    pub timestamp_seconds: f64,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub sample_rate: SampleRate,
    pub bit_depth: BitDepth,
    pub channels: Channels,
    pub duration_seconds: f64,
}

/// 解码器 trait。
///
/// 调用方期望：
/// - `open` 完成后 `info()` 可用
/// - `next_packet` 拉一帧；EOF 返回 `Ok(None)`
/// - `seek` 跳到指定秒；下次 `next_packet` 从该位置开始
pub trait Decoder: Send {
    fn open(&mut self, path: &Path) -> Result<(), DecoderError>;
    fn info(&self) -> Option<&StreamInfo>;
    fn next_packet(&mut self) -> Result<Option<Packet>, DecoderError>;
    fn seek(&mut self, seconds: f64) -> Result<(), DecoderError>;
}

pub fn open_decoder(path: &Path) -> Result<Box<dyn Decoder>, DecoderError> {
    if is_dsd_stream(path) {
        let mut decoder = crate::dsd::DsdDecoder::new();
        match decoder.open(path) {
            Ok(()) => return Ok(Box::new(decoder)),
            Err(err) => return open_ffmpeg_fallback(path).or(Err(err)),
        }
    }

    let mut decoder = crate::symphonia::SymphoniaDecoder::new();
    match decoder.open(path) {
        Ok(()) => Ok(Box::new(decoder)),
        Err(err) => open_ffmpeg_fallback(path).or(Err(err)),
    }
}

pub fn probe_stream_info(path: &Path) -> Result<StreamInfo, DecoderError> {
    let decoder = open_decoder(path)?;
    decoder
        .info()
        .cloned()
        .ok_or_else(|| DecoderError::Internal("decoder opened without stream info".into()))
}

fn is_dsd_stream(path: &Path) -> bool {
    match dsd_magic_match(path) {
        Some(is_dsd) => is_dsd,
        None => has_dsd_extension(path),
    }
}

fn dsd_magic_match(path: &Path) -> Option<bool> {
    let Ok(mut file) = File::open(path) else {
        return None;
    };
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic)
        .map(|()| magic == *b"DSD " || magic == *b"FRM8")
        .ok()
}

fn has_dsd_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("dsf") || extension.eq_ignore_ascii_case("dff")
        })
}

fn open_ffmpeg_fallback(path: &Path) -> Result<Box<dyn Decoder>, DecoderError> {
    let mut decoder = crate::ffmpeg::FfmpegDecoder::new();
    decoder.open(path)?;
    Ok(Box::new(decoder))
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
    fn probes_dsd_by_magic_even_with_unknown_extension() {
        let path = temp_audio_path("seraph-dsd-magic", "bin");
        write_test_dsf(&path);

        let info = probe_stream_info(&path).expect("probe dsf by magic");
        assert_eq!(info.sample_rate, SampleRate(44_100));
        assert_eq!(info.channels, Channels(2));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn trusts_magic_over_dsd_extension() {
        let path = temp_audio_path("seraph-not-dsd", "dsf");
        fs::write(&path, b"RIFFnot enough wav data").expect("write fake dsf");

        assert!(!is_dsd_stream(&path));

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

    fn temp_audio_path(prefix: &str, extension: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
    }
}
