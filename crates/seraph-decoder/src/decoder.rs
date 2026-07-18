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
            Err(primary) => {
                return open_ffmpeg_fallback(path)
                    .map_err(|fallback| merge_open_errors("dsd", primary, fallback));
            }
        }
    }

    let mut decoder = crate::symphonia::SymphoniaDecoder::new();
    match decoder.open(path) {
        Ok(()) => Ok(Box::new(decoder)),
        Err(primary) => open_ffmpeg_fallback(path)
            .map_err(|fallback| merge_open_errors("symphonia", primary, fallback)),
    }
}

/// 把主解码器错误 + ffmpeg fallback 错误合并成一条 message，
/// 避免静默吞掉真正的失败原因（典型场景：fallback 报"ffmpeg not found"
/// 但日志里只看到 Symphonia 的 "unknown format"）。
/// F-17：FileNotFound 原样透传，让上层能区分"文件被移动"与"格式不支持"。
fn merge_open_errors(
    primary_name: &str,
    primary: DecoderError,
    fallback: DecoderError,
) -> DecoderError {
    if matches!(primary, DecoderError::FileNotFound) {
        return DecoderError::FileNotFound;
    }
    DecoderError::UnsupportedFormat(format!(
        "{primary_name}: {primary}; ffmpeg fallback: {fallback}"
    ))
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

/// 公开的 DSD 判定：供上层（DSP 链的「EQ 是否对 DSD 生效」开关）判断当前曲目是否为 DSD。
/// 与 `open_decoder` 用同一套魔数优先 / 扩展名兜底的逻辑，判定口径一致。
pub fn is_dsd_file(path: &Path) -> bool {
    is_dsd_stream(path)
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
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn missing_file_reports_file_not_found() {
        // F-17：文件不存在时不得被合并成 UnsupportedFormat
        let path = temp_audio_path("seraph-decoder-missing", "flac");
        let err = match open_decoder(&path) {
            Err(err) => err,
            Ok(_) => panic!("opening a missing file must fail"),
        };
        assert!(matches!(err, DecoderError::FileNotFound), "{err}");
    }

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

    #[test]
    fn probes_24_bit_wav_fixture() {
        let path = temp_audio_path("seraph-decoder-24bit", "wav");
        write_test_wav_24(&path);

        let info = probe_stream_info(&path).expect("probe 24-bit wav");
        assert_eq!(info.sample_rate, SampleRate(96_000));
        assert_eq!(info.bit_depth, BitDepth(24));
        assert_eq!(info.channels, Channels(2));
        assert!(info.duration_seconds > 0.0);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn seek_returns_packets_near_requested_time() {
        let path = temp_audio_path("seraph-decoder-seek", "wav");
        write_test_seek_wav(&path);

        let mut decoder = open_decoder(&path).expect("open seek fixture");
        decoder.seek(0.5).expect("seek fixture");
        let packet = decoder
            .next_packet()
            .expect("packet after seek")
            .expect("packet exists after seek");

        assert!(
            (0.45..=0.55).contains(&packet.timestamp_seconds),
            "unexpected timestamp after seek: {}",
            packet.timestamp_seconds
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn decodes_generated_compressed_fixtures_when_ffmpeg_is_available() {
        let Some(ffmpeg) = crate::ffmpeg::find_ffmpeg() else {
            eprintln!("skipping compressed fixture decode test: ffmpeg not found");
            return;
        };

        let formats: [(&str, &[&str]); 4] = [
            ("flac", &[]),
            ("mp3", &["-codec:a", "libmp3lame"]),
            ("m4a", &["-codec:a", "aac"]),
            ("opus", &["-codec:a", "libopus"]),
        ];
        let mut decoded_count = 0;

        for (extension, codec_args) in formats {
            let path = temp_audio_path("seraph-decoder-compressed", extension);
            if !generate_sine_fixture(&ffmpeg, &path, codec_args) {
                eprintln!("skipping {extension} fixture: ffmpeg could not generate it");
                let _ = fs::remove_file(&path);
                continue;
            }

            let mut decoder = open_decoder(&path).expect("open generated compressed fixture");
            let info = decoder.info().expect("stream info");
            assert_eq!(info.channels, Channels(2));
            assert!(info.sample_rate.0 > 0);

            let packet = decoder
                .next_packet()
                .expect("decode generated compressed fixture")
                .expect("first packet");
            assert!(!packet.samples.is_empty());
            decoded_count += 1;

            let _ = fs::remove_file(path);
        }

        assert!(
            decoded_count > 0,
            "ffmpeg was found but no compressed fixtures could be generated"
        );
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

    fn write_test_wav_24(path: &Path) {
        let sample_rate = 96_000_u32;
        let channels = 2_u16;
        let bits_per_sample = 24_u16;
        let frames = 96_u32;
        let block_align = channels * 3;
        let byte_rate = sample_rate * block_align as u32;
        let data_len = frames * block_align as u32;

        let mut file = File::create(path).expect("create 24-bit wav");
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&sample_rate.to_le_bytes()).unwrap();
        file.write_all(&byte_rate.to_le_bytes()).unwrap();
        file.write_all(&block_align.to_le_bytes()).unwrap();
        file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_len.to_le_bytes()).unwrap();

        for frame in 0..frames {
            let value = ((frame as i32) * 4096).to_le_bytes();
            file.write_all(&value[0..3]).unwrap();
            file.write_all(&value[0..3]).unwrap();
        }
    }

    fn write_test_seek_wav(path: &Path) {
        let sample_rate = 44_100_u32;
        let channels = 2_u16;
        let bits_per_sample = 16_u16;
        let frames = sample_rate;
        let block_align = channels * bits_per_sample / 8;
        let byte_rate = sample_rate * block_align as u32;
        let data_len = frames * block_align as u32;

        let mut file = File::create(path).expect("create seek wav");
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&sample_rate.to_le_bytes()).unwrap();
        file.write_all(&byte_rate.to_le_bytes()).unwrap();
        file.write_all(&block_align.to_le_bytes()).unwrap();
        file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_len.to_le_bytes()).unwrap();

        for frame in 0..frames {
            let value = (((frame % 512) as i16) * 32).to_le_bytes();
            file.write_all(&value).unwrap();
            file.write_all(&value).unwrap();
        }
    }

    fn generate_sine_fixture(ffmpeg: &Path, path: &Path, codec_args: &[&str]) -> bool {
        let mut command = Command::new(ffmpeg);
        command
            .arg("-y")
            .arg("-v")
            .arg("error")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("sine=frequency=440:duration=0.25")
            .arg("-ac")
            .arg("2")
            .arg("-ar")
            .arg("48000");
        command.args(codec_args);
        command.arg(path);

        command.status().is_ok_and(|status| status.success())
    }

    fn temp_audio_path(prefix: &str, extension: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
    }
}
