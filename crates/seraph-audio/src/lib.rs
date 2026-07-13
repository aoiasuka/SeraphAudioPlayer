//! 音频后端抽象。
//!
//! 定义 [`AudioBackend`] trait 让上层与具体平台解耦：
//! - Windows: WASAPI Exclusive / WASAPI Shared / ASIO
//! - macOS:   CoreAudio
//! - Linux:   ALSA / PipeWire

pub mod backend;
pub mod device;
pub mod engine;
pub mod spectrum;
pub mod wasapi;

pub use backend::{AudioBackend, BackendError};
pub use device::{list_output_devices, AudioDevice, DeviceCapabilities, ShareMode};
pub use engine::{PlaybackController, PlaybackEngine};
pub use spectrum::SpectrumTap;
pub use wasapi::WasapiExclusive;
