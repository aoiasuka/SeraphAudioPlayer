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
    // F-7：seek 返回的 required_ts。coarse seek 落点在目标之前，
    // next_packet 需要把 required_ts 之前的帧丢掉才是样本精确 seek。
    trim_before_ts: Option<u64>,
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
            trim_before_ts: None,
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
        // F-6：启用 gapless，剪掉 MP3/AAC 编码器 delay/padding 引入的首尾静音。
        let format_options = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let probed = get_probe()
            .format(&hint, media, &format_options, &MetadataOptions::default())
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
        self.trim_before_ts = None;

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

            let mut samples = buffer.samples().to_vec();
            let mut effective_ts = packet.ts();
            // F-7：coarse seek 落点在 required_ts 之前，丢掉之前的帧实现样本精确 seek。
            if let Some(required_ts) = self.trim_before_ts {
                let channels = spec.channels.count().max(1);
                // 审2-3：packet.ts()/required_ts 的单位是 time_base tick，
                // 不一定等于采样帧（MP4/MKV 的 time_base ≠ 1/sample_rate）——
                // 切样本前必须显式换算到帧域，否则 seek 落点与裁剪量双错。
                let packet_frame = ts_to_frames(self.time_base, packet.ts(), self.sample_rate);
                let required_frame = ts_to_frames(self.time_base, required_ts, self.sample_rate);
                if trim_samples_before(&mut samples, packet_frame, required_frame, channels) {
                    continue; // 整包都在目标之前
                }
                // 走到这里说明 required_ts 落在包内（或包起点之后），
                // 剩余样本的第一帧对应 max(packet.ts, required_ts)
                effective_ts = effective_ts.max(required_ts);
                self.trim_before_ts = None;
            }
            let timestamp_seconds =
                timestamp_seconds(self.time_base, effective_ts, self.sample_rate);

            return Ok(Some(Packet {
                samples,
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

        let seeked = format
            .seek(
                SeekMode::Coarse,
                SeekTo::TimeStamp {
                    ts: seek_ts,
                    track_id,
                },
            )
            .map_err(map_symphonia_error)?;
        // F-7：保存 required_ts，next_packet 中丢弃其之前的帧（样本精确 seek）
        self.trim_before_ts = Some(seeked.required_ts);
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

/// F-7 的纯逻辑部分：把 `required_frame` 之前的帧从 `samples` 前缀剪掉。
/// 审2-3：参数单位是**采样帧**（调用方负责把 time_base tick 换算为帧）。
/// 返回 true 表示整包都在目标之前（应丢弃并继续读下一包）。
fn trim_samples_before(
    samples: &mut Vec<f32>,
    packet_frame: u64,
    required_frame: u64,
    channels: usize,
) -> bool {
    let channels = channels.max(1);
    let frames = (samples.len() / channels) as u64;
    if packet_frame.saturating_add(frames) <= required_frame {
        samples.clear();
        return true;
    }
    if required_frame > packet_frame {
        let skip = ((required_frame - packet_frame) as usize).saturating_mul(channels);
        samples.drain(..skip.min(samples.len()));
    }
    false
}

/// 审2-3：把 time_base tick 时间戳换算为采样帧数。
/// time_base 为 1/sample_rate 的容器（FLAC/WAV/MP3）换算结果与原值一致；
/// MP4/MKV 等以其它 timescale 计时的容器由此获得正确的帧偏移。
fn ts_to_frames(time_base: Option<TimeBase>, ts: u64, sample_rate: u32) -> u64 {
    (timestamp_seconds(time_base, ts, sample_rate) * f64::from(sample_rate.max(1))).round() as u64
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

    #[test]
    fn trim_drops_whole_packet_before_required_ts() {
        // 包覆盖帧 [100, 104)，目标 104 → 整包丢弃
        let mut samples = vec![0.0_f32; 8]; // 4 帧 × 2 声道
        assert!(trim_samples_before(&mut samples, 100, 104, 2));
        assert!(samples.is_empty());
    }

    #[test]
    fn trim_cuts_prefix_of_straddling_packet() {
        // 包覆盖帧 [100, 104)，目标 102 → 剪掉前 2 帧
        let mut samples: Vec<f32> = (0..8).map(|i| i as f32).collect();
        assert!(!trim_samples_before(&mut samples, 100, 102, 2));
        assert_eq!(samples, vec![4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn trim_noop_when_packet_starts_at_or_after_required_ts() {
        let mut samples: Vec<f32> = (0..8).map(|i| i as f32).collect();
        assert!(!trim_samples_before(&mut samples, 102, 100, 2));
        assert_eq!(samples.len(), 8);
    }

    #[test]
    fn ts_to_frames_converts_non_sample_rate_time_base() {
        // 审2-3：MKV/MP4 等容器的 time_base ≠ 1/sample_rate。
        // tick=毫秒（1/1000）、48kHz：500ms 必须换算为 24000 帧，而不是 500 帧。
        let millis = TimeBase::new(1, 1_000);
        assert_eq!(ts_to_frames(Some(millis), 500, 48_000), 24_000);
        // time_base = 1/sample_rate 的容器（FLAC/WAV/MP3）：换算恒等，行为不回归。
        let native = TimeBase::new(1, 44_100);
        assert_eq!(ts_to_frames(Some(native), 12_345, 44_100), 12_345);
        // 无 time_base：按 ts 即帧处理。
        assert_eq!(ts_to_frames(None, 777, 44_100), 777);
    }

    #[test]
    fn seek_trims_to_sample_accurate_position() {
        // WAV 每包 ts 已知；seek 到非包边界位置后，首包时间戳应精确等于目标
        let path = temp_audio_path("seraph-decoder-seektrim", "wav");
        write_test_wav(&path);

        let mut decoder = SymphoniaDecoder::new();
        decoder.open(&path).expect("open wav");
        let target = 10.0 / 44_100.0; // 第 10 帧
        decoder.seek(target).expect("seek");
        let packet = decoder
            .next_packet()
            .expect("packet result")
            .expect("packet after seek");
        assert!(
            (packet.timestamp_seconds - target).abs() < 0.5 / 44_100.0,
            "seek 后时间戳不精确: {} vs {}",
            packet.timestamp_seconds,
            target
        );

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
