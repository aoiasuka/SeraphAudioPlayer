//! DSP traits：重采样、DSD 转换、增益、均衡等。
//!
//! 当前只提供 trait 定义，实际算法实现留给后续阶段：
//! - v1: 用 `rubato` 做重采样
//! - v2: 自研 `std::simd + polyphase FIR`

pub mod dsd;
pub mod resampler;

pub use dsd::{DopConverter, DsdConverter, DsdMode, DsdToPcmConverter, NativeDsdPassthrough};
pub use resampler::{
    resample_interleaved_linear, resample_interleaved_sinc, LinearResampler, Resampler,
    ResamplerError, ResamplerQuality, StatefulSincResampler,
};
