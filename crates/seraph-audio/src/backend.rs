use crate::device::{AudioDevice, ShareMode};
use seraph_core::types::{BitDepth, Channels, SampleRate};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend not implemented yet")]
    NotImplemented,
    #[error("audio device not found")]
    DeviceNotFound,
    #[error("device lost: {0}")]
    DeviceLost(String),
    #[error("exclusive mode unavailable")]
    ExclusiveModeUnavailable,
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, BackendError>;

/// 音频后端 trait。
///
/// 任何具体实现（WASAPI、ASIO、CoreAudio…）都通过本 trait 暴露统一接口。
/// 实现应当：
/// - `play / pause` 立即返回，真正的工作在内部线程
/// - `submit` 用于把已解码的 PCM 帧推入后端的内部缓冲（推荐用 rtrb）
/// - `current_format` 反映"实际打开"的格式（独占模式可能与请求不同）
pub trait AudioBackend: Send + Sync {
    fn list_devices(&self) -> Result<Vec<AudioDevice>>;
    fn open(
        &mut self,
        device: &AudioDevice,
        share_mode: ShareMode,
        sample_rate: SampleRate,
        bit_depth: BitDepth,
        channels: Channels,
    ) -> Result<()>;
    fn close(&mut self) -> Result<()>;
    fn play(&mut self) -> Result<()>;
    fn pause(&mut self) -> Result<()>;
    fn submit(&mut self, samples: &[f32]) -> Result<usize>;
    fn current_device(&self) -> Option<&AudioDevice>;
    fn current_format(&self) -> Option<(SampleRate, BitDepth, Channels)>;
}
