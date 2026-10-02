//! 音频输出层：播放引擎（共享 / WASAPI 独占）、设备枚举与频谱 tap。
//!
//! 引擎的对外入口是 [`PlaybackController`]（命令线程 + 事件总线）。
//! ASIO 仍未实现（`OutputDriver::Asio` 返回 `NotImplemented`）。

pub mod backend;
pub mod device;
pub mod engine;
pub mod spectrum;

pub use backend::BackendError;
pub use device::{list_output_devices, AudioDevice};
pub use engine::{PlaybackController, PlaybackEngine};
pub use spectrum::{SpectrumTap, TapMeta};
