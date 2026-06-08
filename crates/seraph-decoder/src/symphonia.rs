//! Symphonia-backed decoder for mainstream PCM-oriented formats.

use crate::decoder::{Decoder, DecoderError, Packet, StreamInfo};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{fs::File, path::Path};
use symphonia::{
    core::{
        audio::{SampleBuffer, SignalSpec},
        codecs::{Decoder as SymphoniaCodecDecoder, DecoderOptions, CODEC_TYPE_NULL},
        errors::Error as SymphoniaError,
        formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
        io::{MediaSourceStream, MediaSourceStreamOptions},
        meta::MetadataOptions,
        probe::Hint,
        units::TimeBase,
    },
    default::{get_codecs, get_probe},
};

const MAX_CONSECUTIVE_DECODE_ERRORS: u32 = 16;

pub struct SymphoniaDecoder {
    info: Option<StreamInfo>,
    format: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn SymphoniaCodecDecoder>>,
    track_id: Option<u32>,
    time_base: Option<TimeBase>,
    sample_rate: u32,
    // L-6: 复用 SampleBuffer 以避免每包重新分配
    sample_buffer: Option<SampleBuffer<f32>>,
    // L-6 修订：缓存对应的 spec/frames，
    // 包间 spec 漂移（VBR、可变声道 mapping）时强制重建，
    // 避免 copy_interleaved_ref 把新数据写进旧布局缓冲。
    buffer_spec: Option<SignalSpec>,
    buffer_frames: u64,
    consecutive_decode_errors: u32,
}

impl SymphoniaDecoder {
    pub fn new() -> Self {
        Self {
            info: None,
            format: None,
            decoder: None,
            track_id: None,
            time_base: None,
            sample_rate: 0,
            sample_buffer: None,
            buffer_spec: None,
            buffer_frames: 0,
            consecutive_decode_errors: 0,
        }
    }
}

impl Default for SymphoniaDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for SymphoniaDecoder {
    fn open(&mut self, path: &Path) -> Result<(), DecoderError> {
        let file = File::open(path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                DecoderError::FileNotFound
            } else {
                DecoderError::Io(err)
            }
        })?;

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
            hint.with_extension(extension);
        }

        let media = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let probed = get_probe()
            .format(
                &hint,
                media,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(map_symphonia_error)?;
        let format = probed.format;

        let codec_params = {
            let default_track = format
                .default_track()
                .filter(|track| track.codec_params.codec != CODEC_TYPE_NULL);
            let track = default_track
                .or_else(|| {
                    format
                        .tracks()
                        .iter()
                        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
                })
                .ok_or_else(|| DecoderError::UnsupportedFormat("no audio track".into()))?;
            let params = track.codec_params.clone();
            self.track_id = Some(track.id);
            params
        };
        let sample_rate = codec_params.sample_rate.unwrap_or(44_100);
        let decoder = get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(map_symphonia_error)?;

        self.info = Some(stream_info_from_codec(&codec_params));
        self.format = Some(format);
        self.decoder = Some(decoder);
        self.time_base = codec_params.time_base;
        self.sample_rate = sample_rate;

        Ok(())
    }

    fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, DecoderError> {
        let track_id = self
            .track_id
            .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?;

        loop {
            let packet = match self
                .format
                .as_mut()
                .ok_or_else(|| DecoderError::Internal("format reader is not open".into()))?
                .next_packet()
            {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder
                        .as_mut()
                        .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?
                        .reset();
                    continue;
                }
                Err(err) => return Err(map_symphonia_error(err)),
            };

            if packet.track_id() != track_id {
                continue;
            }

            let timestamp_seconds =
                timestamp_seconds(self.time_base, packet.ts(), self.sample_rate);
            let decoded = match self
                .decoder
                .as_mut()
                .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?
                .decode(&packet)
            {
                Ok(decoded) => {
                    self.consecutive_decode_errors = 0;
                    decoded
                }
                Err(SymphoniaError::DecodeError(message)) => {
                    // 单包坏不致命，但连续 N 次失败说明流损坏，必须报错避免 CPU 空转。
                    self.consecutive_decode_errors += 1;
                    if self.consecutive_decode_errors >= MAX_CONSECUTIVE_DECODE_ERRORS {
                        return Err(DecoderError::Internal(format!(
                            "too many consecutive decode errors ({} packets): {message}",
                            self.consecutive_decode_errors
                        )));
                    }
                    continue;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder
                        .as_mut()
                        .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?
                        .reset();
                    continue;
                }
                Err(err) => return Err(map_symphonia_error(err)),
            };

            let spec = *decoded.spec();
            let frames = decoded.capacity() as u64;
            // 关键：必须把 spec（采样率 + 声道布局）一起做命中校验。
            // 仅看 capacity 在 VBR/Opus channel-mapping 变化时会把新 spec 数据
            // 写进旧 spec 的缓冲，导致输出错位且无 panic。
            let cache_hit = self
                .buffer_spec
                .map(|cached| cached == spec && self.buffer_frames >= frames)
                .unwrap_or(false);
            if !cache_hit {
                self.sample_buffer = Some(SampleBuffer::<f32>::new(frames, spec));
                self.buffer_spec = Some(spec);
                self.buffer_frames = frames;
            }
            let buffer = self
                .sample_buffer
                .as_mut()
                .expect("sample buffer must exist after rebuild");
            buffer.copy_interleaved_ref(decoded);

            return Ok(Some(Packet {
                samples: buffer.samples().to_vec(),
                timestamp_seconds,
            }));
        }
    }

    fn seek(&mut self, seconds: f64) -> Result<(), DecoderError> {
        let time_base = self
            .time_base
            .ok_or_else(|| DecoderError::UnsupportedFormat("stream is not seekable".into()))?;
        let format = self
            .format
            .as_mut()
            .ok_or_else(|| DecoderError::Internal("format reader is not open".into()))?;
        let target = seconds.max(0.0);
        let seek_ts = time_base.calc_timestamp(target.into());
        let track_id = self
            .track_id
            .ok_or_else(|| DecoderError::Internal("decoder is not open".into()))?;

        format
            .seek(
                SeekMode::Coarse,
                SeekTo::TimeStamp {
                    ts: seek_ts,
                    track_id,
                },
            )
            .map_err(map_symphonia_error)?;
        if let Some(decoder) = self.decoder.as_mut() {
            decoder.reset();
        }
        // L-6: seek 后 spec 可能变（不同 track），保险起见清掉复用缓冲
        self.sample_buffer = None;
        self.buffer_spec = None;
        self.buffer_frames = 0;
        self.consecutive_decode_errors = 0;
        Ok(())
    }
}

fn stream_info_from_codec(params: &symphonia::core::codecs::CodecParameters) -> StreamInfo {
    let sample_rate = params.sample_rate.unwrap_or(44_100);
    let duration_seconds = params
        .n_frames
        .map(|frames| frames as f64 / sample_rate as f64)
        .unwrap_or(0.0);

    StreamInfo {
        sample_rate: SampleRate(sample_rate),
        bit_depth: BitDepth(params.bits_per_sample.unwrap_or(16).min(u16::MAX as u32) as u16),
        channels: Channels(
            params
                .channels
                .map(|channels| channels.count() as u16)
                .unwrap_or(2),
        ),
        duration_seconds,
    }
}

fn timestamp_seconds(time_base: Option<TimeBase>, ts: u64, sample_rate: u32) -> f64 {
    if let Some(time_base) = time_base {
        let time = time_base.calc_time(ts);
        return time.seconds as f64 + time.frac;
    }

    if sample_rate == 0 {
        0.0
    } else {
        ts as f64 / sample_rate as f64
    }
}

fn map_symphonia_error(err: SymphoniaError) -> DecoderError {
    match err {
        SymphoniaError::IoError(err) => DecoderError::Io(err),
        SymphoniaError::Unsupported(message) => {
            DecoderError::UnsupportedFormat(message.to_string())
        }
        SymphoniaError::DecodeError(message) => DecoderError::Internal(message.to_string()),
        SymphoniaError::SeekError(message) => DecoderError::Internal(format!("{message:?}")),
        SymphoniaError::LimitError(message) => DecoderError::Internal(message.to_string()),
        SymphoniaError::ResetRequired => DecoderError::Internal("decoder reset required".into()),
    }
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
    fn decodes_pcm_wav_packets() {
        let path = temp_audio_path("seraph-decoder", "wav");
        write_test_wav(&path);

        let mut decoder = SymphoniaDecoder::new();
        decoder.open(&path).expect("open wav");
        let info = decoder.info().expect("stream info");
        assert_eq!(info.sample_rate, SampleRate(44_100));
        assert_eq!(info.bit_depth, BitDepth(16));
        assert_eq!(info.channels, Channels(2));

        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("first packet");
        assert!(!packet.samples.is_empty());
        assert!(packet.timestamp_seconds >= 0.0);

        let _ = fs::remove_file(path);
    }

    fn write_test_wav(path: &Path) {
        let sample_rate = 44_100_u32;
        let channels = 2_u16;
        let bits_per_sample = 16_u16;
        let frames = 32_u32;
        let block_align = channels * bits_per_sample / 8;
        let byte_rate = sample_rate * block_align as u32;
        let data_len = frames * block_align as u32;

        let mut file = File::create(path).expect("create wav");
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
            let value = ((frame as i16) * 64).to_le_bytes();
            file.write_all(&value).unwrap();
            file.write_all(&value).unwrap();
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
