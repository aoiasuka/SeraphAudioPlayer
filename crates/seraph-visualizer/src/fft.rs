use parking_lot::Mutex;
use rustfft::{num_complex::Complex32, FftPlanner};
use std::{collections::VecDeque, f32::consts::PI, sync::Arc, time::Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisualizerError {
    #[error("invalid visualizer configuration")]
    InvalidConfig,
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct SpectrumFrame {
    pub bins: Vec<f32>,
    pub peak_left: f32,
    pub peak_right: f32,
    pub timestamp_ms: u64,
}

/// 频谱可视化器 trait。
///
/// `push_samples` 由音频线程推送一小段交错 PCM，`latest_frame` 返回最近一次频谱结果。
pub trait Visualizer: Send + Sync {
    fn push_samples(&self, samples: &[f32]) -> Result<(), VisualizerError>;
    fn latest_frame(&self) -> Option<SpectrumFrame>;
    fn fft_size(&self) -> usize;
}

pub struct SimpleVisualizer {
    fft_size: usize,
    bin_count: usize,
    channels: usize,
    started_at: Instant,
    mono_buffer: Mutex<VecDeque<f32>>,
    latest: Mutex<Option<SpectrumFrame>>,
    fft: Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
}

impl std::fmt::Debug for SimpleVisualizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleVisualizer")
            .field("fft_size", &self.fft_size)
            .field("bin_count", &self.bin_count)
            .field("channels", &self.channels)
            .finish()
    }
}

impl SimpleVisualizer {
    pub fn new(
        fft_size: usize,
        bin_count: usize,
        channels: usize,
    ) -> Result<Self, VisualizerError> {
        if fft_size == 0 || bin_count == 0 || channels == 0 || bin_count > fft_size / 2 {
            return Err(VisualizerError::InvalidConfig);
        }

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        let window = (0..fft_size).map(|i| hann_window(i, fft_size)).collect();

        Ok(Self {
            fft_size,
            bin_count,
            channels,
            started_at: Instant::now(),
            mono_buffer: Mutex::new(VecDeque::with_capacity(fft_size)),
            latest: Mutex::new(None),
            fft,
            window,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl Visualizer for SimpleVisualizer {
    fn push_samples(&self, samples: &[f32]) -> Result<(), VisualizerError> {
        if samples.is_empty() {
            return Ok(());
        }

        let (mono, peak_left, peak_right) = interleaved_to_mono(samples, self.channels);
        if mono.is_empty() {
            return Ok(());
        }

        // L-16: 单次加锁批量 extend，避免每个 sample 抢锁。
        let buffer_snapshot = {
            let mut buffer = self.mono_buffer.lock();
            // 把 mono 整体推入，再裁掉超出 fft_size 的旧样本
            buffer.extend(mono.iter().copied());
            let excess = buffer.len().saturating_sub(self.fft_size);
            for _ in 0..excess {
                buffer.pop_front();
            }
            if buffer.len() < self.fft_size {
                return Ok(());
            }
            // 复制出来后立刻释放锁，再去跑 FFT
            buffer.iter().copied().collect::<Vec<f32>>()
        };

        // L-1: 用 rustfft 计算频谱（O(N log N) 而非旧的 O(N²) 朴素 DFT）。
        let mut data: Vec<Complex32> = buffer_snapshot
            .iter()
            .zip(self.window.iter())
            .map(|(sample, win)| Complex32::new(sample * win, 0.0))
            .collect();
        self.fft.process(&mut data);

        let bins = spectrum_bins_from_fft(&data, self.bin_count, self.fft_size);
        *self.latest.lock() = Some(SpectrumFrame {
            bins,
            peak_left,
            peak_right,
            timestamp_ms: self.started_at.elapsed().as_millis() as u64,
        });
        Ok(())
    }

    fn latest_frame(&self) -> Option<SpectrumFrame> {
        self.latest.lock().clone()
    }

    fn fft_size(&self) -> usize {
        self.fft_size
    }
}

fn interleaved_to_mono(samples: &[f32], channels: usize) -> (Vec<f32>, f32, f32) {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    let mut peak_left = 0.0_f32;
    let mut peak_right = 0.0_f32;

    for frame in 0..frames {
        let offset = frame * channels;
        let frame_samples = &samples[offset..offset + channels];
        peak_left = peak_left.max(frame_samples[0].abs());
        peak_right = peak_right.max(
            frame_samples
                .get(1)
                .copied()
                .unwrap_or(frame_samples[0])
                .abs(),
        );
        mono.push(frame_samples.iter().sum::<f32>() / channels as f32);
    }

    (mono, peak_left.min(1.0), peak_right.min(1.0))
}

fn hann_window(index: usize, len: usize) -> f32 {
    if len <= 1 {
        return 1.0;
    }

    0.5 - 0.5 * ((2.0 * PI * index as f32) / (len - 1) as f32).cos()
}

/// 把 FFT 输出按 log 频率聚合到 `bin_count` 个频段，便于 UI 直接绘制。
fn spectrum_bins_from_fft(fft_output: &[Complex32], bin_count: usize, fft_size: usize) -> Vec<f32> {
    let nyquist = fft_size / 2;
    if nyquist == 0 || bin_count == 0 {
        return vec![0.0; bin_count];
    }
    // log 间隔分箱：低频区分辨率高，高频区聚合，符合人耳感受。
    let min_bin = 1.0_f32; // 跳过 DC
    let max_bin = nyquist as f32;
    let log_min = min_bin.ln();
    let log_max = max_bin.ln();
    let log_step = (log_max - log_min) / bin_count as f32;
    let mut bins = Vec::with_capacity(bin_count);

    for b in 0..bin_count {
        let lo_log = log_min + log_step * b as f32;
        let hi_log = log_min + log_step * (b + 1) as f32;
        let lo = lo_log.exp().floor() as usize;
        let hi = hi_log.exp().ceil() as usize;
        let lo = lo.max(1).min(nyquist);
        let hi = hi.max(lo + 1).min(nyquist + 1);

        let mut max_mag = 0.0_f32;
        for c in fft_output.iter().take(hi).skip(lo) {
            let mag = (c.re * c.re + c.im * c.im).sqrt();
            if mag > max_mag {
                max_mag = mag;
            }
        }
        // 归一化到 [0, 1]：除以 fft_size / 2，再 clamp
        bins.push((max_mag / (fft_size as f32 * 0.5)).clamp(0.0, 1.0));
    }

    map_bins_to_db(bins)
}

/// F-14：dB 映射代替逐帧最大值归一。
/// 逐帧归一会抹掉绝对电平（安静段与响段柱高相同）且随最大 bin 抖动闪烁。
/// 把幅度按 20·log10 映射，[-72, 0] dB 线性映射到 [0, 1]。
const SPECTRUM_DB_FLOOR: f32 = -72.0;

fn map_bins_to_db(mut bins: Vec<f32>) -> Vec<f32> {
    for bin in &mut bins {
        let db = if *bin > 0.0 {
            20.0 * bin.log10()
        } else {
            SPECTRUM_DB_FLOOR
        };
        *bin = ((db - SPECTRUM_DB_FLOOR) / -SPECTRUM_DB_FLOOR).clamp(0.0, 1.0);
    }
    bins
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_interleaved_samples_to_mono_and_peaks() {
        let (mono, left, right) = interleaved_to_mono(&[0.5, -0.25, -1.0, 0.75], 2);

        assert_eq!(mono, vec![0.125, -0.125]);
        assert_eq!(left, 1.0);
        assert_eq!(right, 0.75);
    }

    #[test]
    fn builds_spectrum_frame_after_enough_samples() {
        let visualizer = SimpleVisualizer::new(16, 4, 1).unwrap();
        let samples: Vec<f32> = (0..16)
            .map(|index| (2.0 * PI * index as f32 / 16.0).sin())
            .collect();

        visualizer.push_samples(&samples).unwrap();
        let frame = visualizer.latest_frame().expect("spectrum frame");

        assert_eq!(frame.bins.len(), 4);
        assert!(frame.bins.iter().any(|value| *value > 0.5));
        assert!(frame.peak_left > 0.9);
        assert!(frame.peak_right > 0.9);
    }

    #[test]
    fn db_mapping_preserves_absolute_level() {
        // F-14：0 dBFS → 1.0；-36 dB → 0.5；地板以下 → 0；绝对电平差异必须保留
        let mapped = map_bins_to_db(vec![1.0, 10.0_f32.powf(-36.0 / 20.0), 1.0e-6, 0.0]);
        assert!((mapped[0] - 1.0).abs() < 1.0e-4);
        assert!((mapped[1] - 0.5).abs() < 1.0e-4);
        assert_eq!(mapped[2], 0.0); // -120 dB 低于 -72 dB 地板
        assert_eq!(mapped[3], 0.0);

        // 响 10 倍的信号柱高必须更高（旧逐帧归一会把两者都拉到 1.0）
        let loud = map_bins_to_db(vec![0.5]);
        let quiet = map_bins_to_db(vec![0.05]);
        assert!(loud[0] > quiet[0]);
    }

    #[test]
    fn rejects_invalid_config() {
        let err = SimpleVisualizer::new(8, 5, 2).unwrap_err();
        assert!(matches!(err, VisualizerError::InvalidConfig));
    }
}
