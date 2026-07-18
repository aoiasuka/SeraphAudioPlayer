//! DSP traits：重采样、DSD 转换、增益、均衡等。
//!
//! 当前只提供 trait 定义，实际算法实现留给后续阶段：
//! - v1: 用 `rubato` 做重采样
//! - v2: 自研 `std::simd + polyphase FIR`

pub mod chain;
pub mod crossfeed;
pub mod dsd;
pub mod eq;
pub mod resampler;

pub use chain::{DspProcessor, DspSettings};
pub use crossfeed::{Crossfeed, CrossfeedSettings};
pub use dsd::{DopConverter, DsdConverter, DsdMode, NativeDsdPassthrough};
pub use eq::{combined_response_db, BandKind, EqBand, Equalizer};
pub use resampler::{
    resample_interleaved_linear, resample_interleaved_sinc, LinearResampler, Resampler,
    ResamplerError, ResamplerQuality, StatefulSincResampler,
};
