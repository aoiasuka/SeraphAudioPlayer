//! Seraph core types, events and state machine.
//!
//! 这个 crate 不依赖任何具体的音频后端 / 解码器实现，
//! 只提供共享的领域类型 + 事件总线 + 播放状态机。

pub mod bus;
pub mod event;
pub mod state;
pub mod types;

pub use bus::EventBus;
pub use event::PlayerEvent;
pub use state::PlayerState;
pub use types::{AudioFormat, BitDepth, Channels, SampleRate, Track, TrackId};
