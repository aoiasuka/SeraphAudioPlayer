//! FFmpeg CLI fallback decoder for formats Symphonia cannot handle.

use crate::decoder::{Decoder, DecoderError, Packet, StreamInfo};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
};

const PACKET_FRAMES: usize = 2048;

pub struct FfmpegDecoder {
    path: Option<PathBuf>,
    info: Option<StreamInfo>,
    child: Option<Child>,
    stdout: Option<ChildStdout>,
    frames_read: u64,
    base_seconds: f64,
}

impl FfmpegDecoder {
    pub fn new() -> Self {
        Self {
            path: None,
            info: None,
            child: None,
            stdout: None,
            frames_read: 0,
            base_seconds: 0.0,
        }
    }

    fn start_process(&mut self, start_seconds: f64) -> Result<(), DecoderError> {
        self.stop_process();
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| DecoderError::Internal("ffmpeg decoder is not open".into()))?;

        let mut command = Command::new("ffmpeg");
        command.arg("-v").arg("error");
        if start_seconds > 0.0 {
            command.arg("-ss").arg(format!("{start_seconds:.6}"));
        }
        command
            .arg("-i")
            .arg(path)
            .arg("-map")
            .arg("0:a:0")
            .arg("-vn")
            .arg("-f")
            .arg("f32le")
            .arg("-acodec")
            .arg("pcm_f32le")
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command.spawn().map_err(map_tool_spawn_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DecoderError::Internal("ffmpeg stdout is unavailable".into()))?;
        self.child = Some(child);
        self.stdout = Some(stdout);
        self.frames_read = 0;
        self.base_seconds = start_seconds.max(0.0);
        Ok(())
    }

    fn stop_process(&mut self) {
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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
        self.start_process(0.0)?;
        Ok(())
    }

    fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, DecoderError> {
        if self.stdout.is_none() {
            self.start_process(self.base_seconds)?;
        }

        let info = self
            .info
            .as_ref()
            .ok_or_else(|| DecoderError::Internal("ffmpeg decoder is not open".into()))?;
        let channels = usize::from(info.channels.0).max(1);
        let bytes_per_packet = PACKET_FRAMES * channels * std::mem::size_of::<f32>();
        let mut bytes = vec![0_u8; bytes_per_packet];
        let mut filled = 0;

        while filled < bytes.len() {
            let read = self
                .stdout
                .as_mut()
                .ok_or_else(|| DecoderError::Internal("ffmpeg stdout is unavailable".into()))?
                .read(&mut bytes[filled..])
                .map_err(DecoderError::Io)?;
            if read == 0 {
                break;
            }
            filled += read;
        }

        if filled == 0 {
            self.stop_process();
            return Ok(None);
        }

        bytes.truncate(filled - (filled % 4));
        let timestamp_seconds =
            self.base_seconds + self.frames_read as f64 / f64::from(info.sample_rate.0.max(1));
        let samples = bytes_to_f32_samples(&bytes);
        self.frames_read += (samples.len() / channels) as u64;

        Ok(Some(Packet {
            samples,
            timestamp_seconds,
        }))
    }

    fn seek(&mut self, seconds: f64) -> Result<(), DecoderError> {
        self.start_process(seconds.max(0.0))
    }
}

fn probe_with_ffprobe(path: &Path) -> Result<StreamInfo, DecoderError> {
    let output = Command::new("ffprobe")
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
        .arg(path)
        .output()
        .map_err(map_tool_spawn_error)?;

    if !output.status.success() {
        return Err(DecoderError::UnsupportedFormat(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    parse_ffprobe_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ffprobe_output(output: &str) -> Result<StreamInfo, DecoderError> {
    let mut sample_rate = None;
    let mut channels = None;
    let mut bit_depth = None;
    let mut stream_duration = None;
    let mut format_duration = None;

    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "sample_rate" => sample_rate = value.parse::<u32>().ok(),
            "channels" => channels = value.parse::<u16>().ok(),
            "bits_per_raw_sample" | "bits_per_sample" if value != "N/A" => {
                bit_depth = value.parse::<u16>().ok()
            }
            "duration" if stream_duration.is_none() => {
                stream_duration = parse_duration(value);
            }
            "TAG:DURATION" if stream_duration.is_none() => {
                stream_duration = parse_duration(value);
            }
            _ => {
                if key.trim() == "format.duration" {
                    format_duration = parse_duration(value);
                }
            }
        }
    }

    let sample_rate = sample_rate
        .filter(|value| *value > 0)
        .ok_or_else(|| DecoderError::UnsupportedFormat("ffprobe missing sample_rate".into()))?;
    let channels = channels
        .filter(|value| *value > 0)
        .ok_or_else(|| DecoderError::UnsupportedFormat("ffprobe missing channels".into()))?;

    Ok(StreamInfo {
        sample_rate: SampleRate(sample_rate),
        bit_depth: BitDepth(bit_depth.unwrap_or(16)),
        channels: Channels(channels),
        duration_seconds: stream_duration.or(format_duration).unwrap_or(0.0),
    })
}

fn parse_duration(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn bytes_to_f32_samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn map_tool_spawn_error(err: std::io::Error) -> DecoderError {
    if err.kind() == std::io::ErrorKind::NotFound {
        DecoderError::UnsupportedFormat("ffmpeg/ffprobe executable not found".into())
    } else {
        DecoderError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn converts_f32le_bytes_to_samples() {
        let bytes = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
        let samples = bytes_to_f32_samples(&bytes);

        assert_eq!(samples, vec![0.25, -0.5]);
    }
}
