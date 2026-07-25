use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type TrackId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Flac,
    Mp3,
    Wav,
    Aac,
    Alac,
    Opus,
    Dsf,
    Dff,
    Ape,
    Wv,
    Other(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SampleRate(pub u32);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitDepth(pub u16);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Channels(pub u16);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_year: Option<String>,
    pub cover: Option<String>,
    pub format: AudioFormat,
    pub sample_rate: SampleRate,
    pub bit_depth: BitDepth,
    pub channels: Channels,
    pub bitrate_kbps: Option<u32>,
    pub size_bytes: Option<u64>,
    pub path: PathBuf,
    pub duration_seconds: f64,
}

impl AudioFormat {
    /// 展示用大写标签（`{:?}` 会把 `Other("tak")` 打成字面量，展示层不用它）
    pub fn label(&self) -> String {
        match self {
            AudioFormat::Other(ext) => ext.to_uppercase(),
            other => format!("{other:?}").to_uppercase(),
        }
    }
}

impl Track {
    pub fn bitdepth_label(&self) -> String {
        // L-25：44100/1000 的整数除法曾把 44.1k 家族全部显示成 "44kHz"
        // （88.2→88、176.4→176、352.8→352）；非整千采样率保留一位小数。
        let khz = f64::from(self.sample_rate.0) / 1000.0;
        let khz_label = if self.sample_rate.0.is_multiple_of(1000) {
            format!("{}", self.sample_rate.0 / 1000)
        } else {
            format!("{khz:.1}")
        };
        format!(
            "{} {}bit / {khz_label}kHz",
            self.format.label(),
            self.bit_depth.0
        )
    }
}
