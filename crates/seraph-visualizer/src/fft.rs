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
/// `push_samples` 由分析工作线程推送一批交错 PCM，`latest_frame` 返回最近一次频谱结果。
pub trait Visualizer: Send + Sync {
    fn push_samples(&self, samples: &[f32]) -> Result<(), VisualizerError>;
    fn latest_frame(&self) -> Option<SpectrumFrame>;
    fn fft_size(&self) -> usize;
}

pub struct SimpleVisualizer {
    fft_size: usize,
    bin_count: usize,
    channels: usize,
    sample_rate: u32,
    started_at: Instant,
    work: Mutex<FftWorkspace>,
    latest: Mutex<Option<SpectrumFrame>>,
    fft: Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    /// 中-1：窗相干增益（Σwindow）。加窗后满幅正弦的主 bin 幅度 ≈ A·Σwindow/2，
    /// 用它做归一化分母才能让 0dBFS 满幅信号映射到 1.0。用 fft_size/2 会系统性低 ~6dB。
    window_gain: f32,
}

struct FftWorkspace {
    mono: VecDeque<f32>,
    data: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl std::fmt::Debug for SimpleVisualizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleVisualizer")
            .field("fft_size", &self.fft_size)
            .field("bin_count", &self.bin_count)
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

impl SimpleVisualizer {
    pub fn new(
        fft_size: usize,
        bin_count: usize,
        channels: usize,
        sample_rate: u32,
    ) -> Result<Self, VisualizerError> {
        if fft_size == 0
            || bin_count == 0
            || channels == 0
            || sample_rate == 0
            || bin_count > fft_size / 2
        {
            return Err(VisualizerError::InvalidConfig);
        }

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        let window: Vec<f32> = (0..fft_size).map(|i| hann_window(i, fft_size)).collect();
        // 中-1：相干增益取实际窗系数之和（Hann ≈ N/2），避免硬编码常量与窗函数不一致。
        let window_gain = window.iter().sum::<f32>().max(1.0);
        let scratch_len = fft.get_inplace_scratch_len();

        Ok(Self {
            fft_size,
            bin_count,
            channels,
            sample_rate,
            started_at: Instant::now(),
            work: Mutex::new(FftWorkspace {
                mono: VecDeque::with_capacity(fft_size),
                data: vec![Complex32::default(); fft_size],
                scratch: vec![Complex32::default(); scratch_len],
            }),
            latest: Mutex::new(None),
            fft,
            window,
            window_gain,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn reset(&self) {
        self.work.lock().mono.clear();
        *self.latest.lock() = None;
    }
}

impl Visualizer for SimpleVisualizer {
    fn push_samples(&self, samples: &[f32]) -> Result<(), VisualizerError> {
        if samples.is_empty() {
            return Ok(());
        }

        if samples.len() < self.channels {
            return Ok(());
        }
        // mono 环、加窗输入、FFT scratch 都复用。没有逐帧临时 mono / window Vec。
        let mut work = self.work.lock();
        let FftWorkspace {
            mono,
            data,
            scratch,
        } = &mut *work;
        let mut peak_left = 0.0_f32;
        let mut peak_right = 0.0_f32;
        for frame in samples.chunks_exact(self.channels) {
            peak_left = peak_left.max(frame[0].abs());
            peak_right = peak_right.max(frame.get(1).copied().unwrap_or(frame[0]).abs());
            if mono.len() == self.fft_size {
                mono.pop_front();
            }
            mono.push_back(frame.iter().sum::<f32>() / self.channels as f32);
        }
        if mono.len() < self.fft_size {
            return Ok(());
        }
        for ((target, sample), window) in data.iter_mut().zip(mono.iter()).zip(&self.window) {
            *target = Complex32::new(sample * window, 0.0);
        }
        self.fft.process_with_scratch(data, scratch);

        let mut latest = self.latest.lock();
        let frame = latest.get_or_insert_with(|| SpectrumFrame {
            bins: Vec::with_capacity(self.bin_count),
            peak_left: 0.0,
            peak_right: 0.0,
            timestamp_ms: 0,
        });
        spectrum_bins_from_fft(
            data,
            self.bin_count,
            self.fft_size,
            self.window_gain,
            self.sample_rate,
            &mut frame.bins,
        );
        frame.peak_left = peak_left.min(1.0);
        frame.peak_right = peak_right.min(1.0);
        frame.timestamp_ms = self.started_at.elapsed().as_millis() as u64;
        Ok(())
    }

    fn latest_frame(&self) -> Option<SpectrumFrame> {
        self.latest.lock().clone()
    }

    fn fft_size(&self) -> usize {
        self.fft_size
    }
}

fn hann_window(index: usize, len: usize) -> f32 {
    if len <= 1 {
        return 1.0;
    }

    0.5 - 0.5 * ((2.0 * PI * index as f32) / (len - 1) as f32).cos()
}

/// 把 FFT 输出按 log 频率聚合到 `bin_count` 个频段，便于 UI 直接绘制。
/// `window_gain` 是窗系数之和（Σwindow）：加窗后满幅正弦主 bin 幅度 ≈ A·window_gain/2，
/// 以此为归一化分母，0dBFS 满幅信号才能到 1.0（中-1：修复此前用 fft_size/2 导致的 -6dB 偏差）。
fn spectrum_bins_from_fft(
    fft_output: &[Complex32],
    bin_count: usize,
    fft_size: usize,
    window_gain: f32,
    sample_rate: u32,
    bins: &mut Vec<f32>,
) {
    bins.clear();
    let nyquist = fft_size / 2;
    if nyquist == 0 || bin_count == 0 {
        bins.resize(bin_count, 0.0);
        return;
    }
    // 归一化分母：窗相干增益的一半（≈ fft_size/4）。
    let norm = (window_gain * 0.5).max(1.0);
    // 与前端 binFreq 同一契约：箱中心固定为 20 * 1000^(i/(n-1)) Hz。
    // FFT 索引必须按实际采样率换算，超出 Nyquist 的频段保持空白。
    let log_min = 20.0_f32.ln();
    let log_max = 20_000.0_f32.ln();
    let log_step = (log_max - log_min) / bin_count.saturating_sub(1).max(1) as f32;
    let resolution = sample_rate as f32 / fft_size as f32;
    let nyquist_hz = sample_rate as f32 * 0.5;

    for b in 0..bin_count {
        let center_log = log_min + log_step * b as f32;
        let center_hz = center_log.exp();
        if center_hz > nyquist_hz {
            bins.push(0.0);
            continue;
        }
        let lo_hz = if b == 0 {
            20.0
        } else {
            (center_log - log_step * 0.5).exp()
        };
        let hi_hz = if b + 1 == bin_count {
            20_000.0
        } else {
            (center_log + log_step * 0.5).exp()
        };
        let lo = ((lo_hz / resolution).ceil() as usize).max(1);
        let hi = ((hi_hz.min(nyquist_hz) / resolution).ceil() as usize).min(nyquist + 1);

        let mut max_mag = 0.0_f32;
        for c in fft_output.iter().take(hi).skip(lo) {
            let mag = (c.re * c.re + c.im * c.im).sqrt();
            if mag > max_mag {
                max_mag = mag;
            }
        }
        if lo >= hi {
            // 低频箱窄于一个 FFT 频点时线性插值，避免出现交替空箱。
            let position = center_hz / resolution;
            let left = (position.floor() as usize).min(nyquist);
            let right = (left + 1).min(nyquist);
            let fraction = position - left as f32;
            max_mag =
                fft_output[left].norm() * (1.0 - fraction) + fft_output[right].norm() * fraction;
        }
        // 归一化到 [0, 1]：除以窗相干增益的一半，再 clamp
        bins.push((max_mag / norm).clamp(0.0, 1.0));
    }

    map_bins_to_db_in_place(bins);
}

/// F-14：dB 映射代替逐帧最大值归一。
/// 逐帧归一会抹掉绝对电平（安静段与响段柱高相同）且随最大 bin 抖动闪烁。
/// 把幅度按 20·log10 映射，[-72, 0] dB 线性映射到 [0, 1]。
const SPECTRUM_DB_FLOOR: f32 = -72.0;

fn map_bins_to_db_in_place(bins: &mut [f32]) {
    for bin in bins {
        let db = if *bin > 0.0 {
            20.0 * bin.log10()
        } else {
            SPECTRUM_DB_FLOOR
        };
        *bin = ((db - SPECTRUM_DB_FLOOR) / -SPECTRUM_DB_FLOOR).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_bins_to_db(mut bins: Vec<f32>) -> Vec<f32> {
        map_bins_to_db_in_place(&mut bins);
        bins
    }

    #[test]
    fn bug_audit_11_known_tones_use_the_same_hz_axis_at_every_rate() {
        for sample_rate in [44_100, 96_000, 192_000] {
            for fft_size in [4096, 16_384] {
                for frequency in [100.0_f32, 1000.0, 10_000.0] {
                    // 100 Hz 在高采样率的小窗中不足数个周期，另用大窗核对低频。
                    if frequency == 100.0 && fft_size == 4096 {
                        continue;
                    }
                    let visualizer = SimpleVisualizer::new(fft_size, 96, 1, sample_rate).unwrap();
                    let samples = (0..fft_size)
                        .map(|index| {
                            (2.0 * PI * frequency * index as f32 / sample_rate as f32).sin() * 0.5
                        })
                        .collect::<Vec<_>>();
                    visualizer.push_samples(&samples).unwrap();
                    let bins = visualizer.latest_frame().unwrap().bins;
                    let peak = bins
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .unwrap()
                        .0;
                    let displayed = 20.0_f32 * 1000.0_f32.powf(peak as f32 / 95.0);
                    assert!((displayed - frequency).abs() / frequency < 0.1,
                        "rate={sample_rate}, fft={fft_size}, tone={frequency}, displayed={displayed}");
                }
            }
        }
    }

    #[test]
    fn bug_audit_11_bins_above_nyquist_are_empty() {
        let visualizer = SimpleVisualizer::new(4096, 96, 1, 8_000).unwrap();
        let samples = (0..4096)
            .map(|i| (2.0 * PI * 1000.0 * i as f32 / 8_000.0).sin())
            .collect::<Vec<_>>();
        visualizer.push_samples(&samples).unwrap();
        for (index, value) in visualizer.latest_frame().unwrap().bins.iter().enumerate() {
            if 20.0_f32 * 1000.0_f32.powf(index as f32 / 95.0) > 4000.0 {
                assert_eq!(*value, 0.0);
            }
        }
        assert!(SimpleVisualizer::new(4096, 96, 1, 0).is_err());
    }

    #[test]
    fn converts_interleaved_samples_to_mono_and_peaks() {
        let visualizer = SimpleVisualizer::new(2, 1, 2, 48_000).unwrap();
        visualizer.push_samples(&[0.5, -0.25, -1.0, 0.75]).unwrap();
        assert_eq!(visualizer.work.lock().mono, vec![0.125, -0.125]);
        let frame = visualizer.latest_frame().unwrap();
        assert_eq!(frame.peak_left, 1.0);
        assert_eq!(frame.peak_right, 0.75);
    }

    #[test]
    fn builds_spectrum_frame_after_enough_samples() {
        let visualizer = SimpleVisualizer::new(16, 4, 1, 48_000).unwrap();
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
    fn full_scale_sine_normalizes_near_unity() {
        // 中-1：满幅正弦（对准某个 bin）经加窗 FFT + 窗增益归一后，主 bin 幅度应 ≈ 1.0。
        // 旧实现用 fft_size/2 归一会得到 ≈0.5（低 6dB），0dBFS 永远到不了满格。
        let fft_size = 64;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        let window: Vec<f32> = (0..fft_size).map(|i| hann_window(i, fft_size)).collect();
        let window_gain = window.iter().sum::<f32>().max(1.0);

        // 频率 = 4 个周期/窗，正好对准 bin 4，避免频谱泄漏干扰主 bin 幅度。
        let mut data: Vec<Complex32> = (0..fft_size)
            .map(|i| {
                let s = (2.0 * PI * 4.0 * i as f32 / fft_size as f32).sin();
                Complex32::new(s * window[i], 0.0)
            })
            .collect();
        fft.process(&mut data);

        // 直接取主 bin（4）幅度做归一，不经 log 分箱，验证归一分母正确。
        let mag = (data[4].re * data[4].re + data[4].im * data[4].im).sqrt();
        let normalized = mag / (window_gain * 0.5);
        assert!(
            (normalized - 1.0).abs() < 0.05,
            "满幅正弦主 bin 归一后应≈1.0，实得 {normalized}"
        );
    }

    #[test]
    fn rejects_invalid_config() {
        let err = SimpleVisualizer::new(8, 5, 2, 48_000).unwrap_err();
        assert!(matches!(err, VisualizerError::InvalidConfig));
    }
}
