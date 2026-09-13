//! 渲染线程 → 可视化的实时安全采样 tap。
//!
//! 写侧在音频渲染回调内运行，必须实时安全：
//! - `try_lock` 拿不到锁立即放弃（丢一个 quantum 的频谱数据，绝不阻塞渲染）；
//! - 环形缓冲预分配，写入只是索引取模 + 赋值，零分配零系统调用。
//!
//! 读侧（Tauri IPC 线程，前端 ~30fps 轮询）短暂持锁把新样本拷出，
//! 交给 `seraph-visualizer` 做 FFT。

use parking_lot::{Mutex, MutexGuard};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// 环形容量：384kHz 双声道约 85ms、48kHz 双声道约 680ms——独占模式高采样率
/// （DoP/352.8k+）流下也足以覆盖前端 30fps 轮询间隔 + 调度抖动，
/// 不至于在两次 drain 之间溢出造成样本断流（断流会让 FFT 窗口出现波形跳变毛刺）。
const TAP_CAPACITY: usize = 64 * 1024;

pub struct SpectrumTap {
    inner: Mutex<TapInner>,
    skipped_callbacks: AtomicU64,
    overwritten_samples: AtomicU64,
    missing_output_frames: AtomicU64,
}

/// 缺样帧包括起播、seek、曲尾等情况，不直接等同于可听断音。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioDiagnostics {
    pub tap_skipped_callbacks: u64,
    pub tap_overwritten_samples: u64,
    pub missing_output_frames: u64,
}

/// drain 时随样本一起带出的流元数据（声学分析需要采样率设计 K 加权滤波器）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapMeta {
    pub channels: usize,
    pub sample_rate: u32,
}

struct TapInner {
    ring: Vec<f32>,
    /// 单调递增的逻辑写位置（对容量取模得物理下标）
    write_pos: u64,
    /// 读侧已消费到的逻辑位置
    read_pos: u64,
    channels: usize,
    sample_rate: u32,
}

impl SpectrumTap {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            skipped_callbacks: AtomicU64::new(0),
            overwritten_samples: AtomicU64::new(0),
            missing_output_frames: AtomicU64::new(0),
            inner: Mutex::new(TapInner {
                ring: vec![0.0; TAP_CAPACITY],
                write_pos: 0,
                read_pos: 0,
                channels: 2,
                sample_rate: 48_000,
            }),
        })
    }

    /// 渲染线程写句柄：try_lock 失败返回 None（放弃本 quantum）。
    pub(crate) fn writer(&self) -> Option<TapWriter<'_>> {
        match self.inner.try_lock() {
            Some(guard) => Some(TapWriter { guard }),
            None => {
                self.skipped_callbacks.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub(crate) fn record_missing_output(&self, frames: usize) {
        if frames > 0 {
            self.missing_output_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
        }
    }

    pub fn diagnostics(&self) -> AudioDiagnostics {
        AudioDiagnostics {
            tap_skipped_callbacks: self.skipped_callbacks.load(Ordering::Relaxed),
            tap_overwritten_samples: self.overwritten_samples.load(Ordering::Relaxed),
            missing_output_frames: self.missing_output_frames.load(Ordering::Relaxed),
        }
    }

    /// 读侧：取出自上次调用以来的新样本（追加到 `out`），返回流元数据。
    /// 溢出时跳过已覆盖的数据，并从下一完整音频帧开始读取。
    pub fn drain(&self, out: &mut Vec<f32>) -> TapMeta {
        let mut inner = self.inner.lock();
        let capacity = inner.ring.len() as u64;
        let channels = inner.channels as u64;
        let oldest = inner.write_pos.saturating_sub(capacity);
        let read = inner.read_pos.max(oldest).div_ceil(channels) * channels;
        self.overwritten_samples
            .fetch_add(read.saturating_sub(inner.read_pos), Ordering::Relaxed);
        // 尾部尚未写满的一帧留给下次 drain，不能把半帧交给分析器。
        let write = inner.write_pos / channels * channels;
        out.reserve(write.saturating_sub(read) as usize);
        for pos in read..write {
            out.push(inner.ring[(pos % capacity) as usize]);
        }
        inner.read_pos = write;
        TapMeta {
            channels: inner.channels,
            sample_rate: inner.sample_rate,
        }
    }
}

pub(crate) struct TapWriter<'a> {
    guard: MutexGuard<'a, TapInner>,
}

impl TapWriter<'_> {
    #[inline]
    pub(crate) fn set_channels(&mut self, channels: usize) {
        let channels = channels.max(1);
        if self.guard.channels != channels {
            self.guard.read_pos = 0;
            self.guard.write_pos = 0;
            self.guard.channels = channels;
        }
    }

    #[inline]
    pub(crate) fn set_sample_rate(&mut self, sample_rate: u32) {
        let sample_rate = sample_rate.max(1);
        if self.guard.sample_rate != sample_rate {
            self.guard.read_pos = 0;
            self.guard.write_pos = 0;
            self.guard.sample_rate = sample_rate;
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, value: f32) {
        let capacity = self.guard.ring.len() as u64;
        let index = (self.guard.write_pos % capacity) as usize;
        self.guard.ring[index] = value;
        self.guard.write_pos += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_accumulate_overflow_and_contention_without_double_counting() {
        let tap = SpectrumTap::new();
        {
            let mut writer = tap.writer().unwrap();
            assert!(tap.writer().is_none(), "实时写侧不能等待已被占用的锁");
            for _ in 0..TAP_CAPACITY + 4 {
                writer.push(0.25);
            }
        }
        let mut samples = Vec::new();
        tap.drain(&mut samples);
        assert_eq!(tap.diagnostics().tap_skipped_callbacks, 1);
        assert_eq!(tap.diagnostics().tap_overwritten_samples, 4);
        samples.clear();
        tap.drain(&mut samples);
        assert!(samples.is_empty());
        assert_eq!(tap.diagnostics().tap_overwritten_samples, 4);
    }

    #[test]
    fn bug_audit_12_repeated_overflows_keep_channel_alignment() {
        for channels in [1, 2, 6, 8] {
            let tap = SpectrumTap::new();
            for _ in 0..3 {
                {
                    let mut writer = tap.writer().unwrap();
                    writer.set_channels(channels);
                    writer.set_sample_rate(192_000);
                    for _ in 0..(TAP_CAPACITY / channels + 50) {
                        for channel in 0..channels {
                            writer.push(channel as f32 / 10.0);
                        }
                    }
                }
                let mut samples = Vec::new();
                let meta = tap.drain(&mut samples);
                assert_eq!(meta.channels, channels);
                assert!(!samples.is_empty() && samples.len().is_multiple_of(channels));
                assert!(samples.len() <= TAP_CAPACITY);
                for (index, sample) in samples.iter().enumerate() {
                    assert_eq!(*sample, (index % channels) as f32 / 10.0);
                }
            }
        }
    }

    #[test]
    fn bug_audit_12_partial_frame_waits_for_remaining_channels() {
        let tap = SpectrumTap::new();
        {
            let mut writer = tap.writer().unwrap();
            writer.set_channels(6);
            writer.push(0.0);
            writer.push(1.0);
        }
        let mut samples = Vec::new();
        tap.drain(&mut samples);
        assert!(samples.is_empty());
        {
            let mut writer = tap.writer().unwrap();
            for channel in 2..6 {
                writer.push(channel as f32);
            }
        }
        tap.drain(&mut samples);
        assert_eq!(samples, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn bug_audit_12_format_change_discards_samples_from_previous_stream() {
        let tap = SpectrumTap::new();
        {
            let mut writer = tap.writer().unwrap();
            writer.push(0.9);
            writer.push(0.8);
            writer.set_channels(6);
            writer.set_sample_rate(96_000);
            for channel in 0..6 {
                writer.push(channel as f32 / 10.0);
            }
        }
        let mut samples = Vec::new();
        let meta = tap.drain(&mut samples);
        assert_eq!(
            meta,
            TapMeta {
                channels: 6,
                sample_rate: 96_000
            }
        );
        assert_eq!(samples, vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5]);
    }

    #[test]
    fn drain_returns_pushed_samples_in_order() {
        let tap = SpectrumTap::new();
        {
            let mut writer = tap.writer().expect("uncontended lock");
            writer.set_channels(2);
            writer.set_sample_rate(44_100);
            for i in 0..8 {
                writer.push(i as f32);
            }
        }

        let mut out = Vec::new();
        let meta = tap.drain(&mut out);
        assert_eq!(meta.channels, 2);
        assert_eq!(meta.sample_rate, 44_100);
        assert_eq!(out, (0..8).map(|i| i as f32).collect::<Vec<_>>());

        // 再次 drain 没有新数据
        out.clear();
        tap.drain(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn overflow_keeps_most_recent_capacity_window() {
        let tap = SpectrumTap::new();
        {
            let mut writer = tap.writer().expect("uncontended lock");
            for i in 0..(TAP_CAPACITY * 2 + 10) {
                writer.push(i as f32);
            }
        }

        let mut out = Vec::new();
        tap.drain(&mut out);
        assert_eq!(out.len(), TAP_CAPACITY);
        assert_eq!(out[0], (TAP_CAPACITY + 10) as f32);
        assert_eq!(*out.last().unwrap(), (TAP_CAPACITY * 2 + 9) as f32);
    }
}
