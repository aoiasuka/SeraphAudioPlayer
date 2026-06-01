use parking_lot::Mutex;
use std::{collections::VecDeque, f32::consts::PI, time::Instant};
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

#[derive(Debug)]
pub struct SimpleVisualizer {
    fft_size: usize,
    bin_count: usize,
    channels: usize,
    started_at: Instant,
    mono_buffer: Mutex<VecDeque<f32>>,
    latest: Mutex<Option<SpectrumFrame>>,
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

        Ok(Self {
            fft_size,
            bin_count,
            channels,
            started_at: Instant::now(),
            mono_buffer: Mutex::new(VecDeque::with_capacity(fft_size)),
            latest: Mutex::new(None),
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

        let mut buffer = self.mono_buffer.lock();
        for sample in mono {
            if buffer.len() == self.fft_size {
                buffer.pop_front();
            }
            buffer.push_back(sample);
        }

        if buffer.len() < self.fft_size {
            return Ok(());
        }

        let windowed: Vec<f32> = buffer
            .iter()
            .enumerate()
            .map(|(index, sample)| sample * hann_window(index, self.fft_size))
            .collect();
        drop(buffer);

        let bins = spectrum_bins(&windowed, self.bin_count);
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

fn spectrum_bins(samples: &[f32], bin_count: usize) -> Vec<f32> {
    let len = samples.len();
    let mut bins = Vec::with_capacity(bin_count);
    for bin in 0..bin_count {
        let frequency_bin = bin + 1;
        let mut re = 0.0_f32;
        let mut im = 0.0_f32;
        for (index, sample) in samples.iter().enumerate() {
            let phase = 2.0 * PI * frequency_bin as f32 * index as f32 / len as f32;
            re += sample * phase.cos();
            im -= sample * phase.sin();
        }

        let magnitude = (re.mul_add(re, im * im).sqrt() / len as f32).min(1.0);
        bins.push(magnitude);
    }

    normalize_bins(bins)
}

fn normalize_bins(mut bins: Vec<f32>) -> Vec<f32> {
    let max = bins
        .iter()
        .copied()
        .fold(0.0_f32, |acc, value| acc.max(value));
    if max <= f32::EPSILON {
        return bins;
    }

    for bin in &mut bins {
        *bin = (*bin / max).clamp(0.0, 1.0);
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
    fn rejects_invalid_config() {
        let err = SimpleVisualizer::new(8, 5, 2).unwrap_err();
        assert!(matches!(err, VisualizerError::InvalidConfig));
    }
}
