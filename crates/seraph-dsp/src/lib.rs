//! 播放链路上的信号处理：重采样（`resampler`）、参数均衡（`eq`）、
//! 耳机串扰馈送（`crossfeed`），以及把后两者串起来的处理链（`chain`）。
//!
//! DSD 的解码与抽取在 `seraph-decoder`；DoP / 原生 DSD 输出尚未实现，这里不留占位。

pub mod chain;
pub mod crossfeed;
pub mod eq;
pub mod resampler;

pub use chain::{DspProcessor, DspSettings};
pub use crossfeed::{Crossfeed, CrossfeedSettings};
pub use eq::{combined_response_db, BandKind, EqBand, Equalizer};
pub use resampler::{ResamplerError, StatefulSincResampler};
