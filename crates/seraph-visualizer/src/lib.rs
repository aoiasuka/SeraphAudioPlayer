//! 频谱可视化。
//!
//! 设计要点（参见 `初始化.md` 第 3 节）：
//! - FFT 在独立线程跑，结果写入 shared ringbuffer
//! - 前端用 `requestAnimationFrame` 主动拉，避免高频 IPC
//! - 不阻塞音频回调 / 不阻塞 Tauri IPC

pub mod fft;

pub use fft::{SimpleVisualizer, SpectrumFrame, Visualizer, VisualizerError};
