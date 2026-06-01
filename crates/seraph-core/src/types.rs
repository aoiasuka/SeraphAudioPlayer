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

impl Track {
    pub fn bitdepth_label(&self) -> String {
        format!(
            "{:?} {}bit / {}kHz",
            self.format,
            self.bit_depth.0,
            self.sample_rate.0 / 1000
        )
    }
}
