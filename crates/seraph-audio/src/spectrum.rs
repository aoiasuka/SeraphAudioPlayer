//! 渲染线程 → 可视化的实时安全采样 tap。
//!
//! 写侧在音频渲染回调内运行，必须实时安全：
//! - `try_lock` 拿不到锁立即放弃（丢一个 quantum 的频谱数据，绝不阻塞渲染）；
//! - 环形缓冲预分配，写入只是索引取模 + 赋值，零分配零系统调用。
//!
//! 读侧（Tauri IPC 线程，前端 ~30fps 轮询）短暂持锁把新样本拷出，
//! 交给 `seraph-visualizer` 做 FFT。

use parking_lot::{Mutex, MutexGuard};
use std::sync::Arc;

/// 环形容量：48kHz 双声道约 170ms——足够覆盖 30fps 轮询间隔的抖动。
const TAP_CAPACITY: usize = 16 * 1024;

pub struct SpectrumTap {
    inner: Mutex<TapInner>,
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
        self.inner.try_lock().map(|guard| TapWriter { guard })
    }

    /// 读侧：取出自上次调用以来的新样本（追加到 `out`），返回流元数据。
    /// 溢出（读得太慢）时自动跳到最近 TAP_CAPACITY 个样本。
    pub fn drain(&self, out: &mut Vec<f32>) -> TapMeta {
        let mut inner = self.inner.lock();
        let capacity = inner.ring.len() as u64;
        if inner.write_pos - inner.read_pos > capacity {
            inner.read_pos = inner.write_pos - capacity;
        }
        let (read, write) = (inner.read_pos, inner.write_pos);
        out.reserve((write - read) as usize);
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
        self.guard.channels = channels.max(1);
    }

    #[inline]
    pub(crate) fn set_sample_rate(&mut self, sample_rate: u32) {
        self.guard.sample_rate = sample_rate.max(1);
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
