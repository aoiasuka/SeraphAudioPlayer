//! 音频解码层。
//!
//! 策略：
//! 1. `SymphoniaDecoder` 处理 FLAC/MP3/WAV/Opus/AAC 等主流格式（Rust 原生）
//! 2. `FfmpegDecoder` 作为 fallback：CUE/ISO/TAK/APE/WV/边缘 VBR 等

pub mod decoder;
pub mod dsd;
pub mod ffmpeg;
pub mod symphonia;

pub use decoder::{open_decoder, probe_stream_info, Decoder, DecoderError, Packet, StreamInfo};
pub use dsd::DsdDecoder;
pub use ffmpeg::{configure_ffmpeg_search_dirs, find_ffmpeg, find_ffprobe, FfmpegDecoder};
pub use symphonia::SymphoniaDecoder;
