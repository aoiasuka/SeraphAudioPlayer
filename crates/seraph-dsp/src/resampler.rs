use thiserror::Error;

const DEFAULT_SINC_RADIUS: usize = 16;

#[derive(Debug, Error)]
pub enum ResamplerError {
    #[error("resampler not implemented yet")]
    NotImplemented,
    #[error("invalid input length")]
    InvalidInputLength,
    #[error("invalid channel count")]
    InvalidChannelCount,
    #[error("invalid sample rate")]
    InvalidSampleRate,
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResamplerQuality {
    Linear,
    SincLow,
    SincMedium,
    SincHigh,
    SincVeryHigh,
}

/// 重采样器 trait。
///
/// 输入若干 f32 采样（按通道交错或按通道分离由实现决定），
/// 输出按目标采样率重采的结果。
pub trait Resampler: Send {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<(), ResamplerError>;
    fn reset(&mut self);
    fn input_rate(&self) -> u32;
    fn output_rate(&self) -> u32;
}

#[derive(Debug, Clone)]
pub struct LinearResampler {
    input_rate: u32,
    output_rate: u32,
    channels: usize,
}

impl LinearResampler {
    pub fn new(input_rate: u32, output_rate: u32, channels: usize) -> Result<Self, ResamplerError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ResamplerError::InvalidSampleRate);
        }
        if channels == 0 {
            return Err(ResamplerError::InvalidChannelCount);
        }

        Ok(Self {
            input_rate,
            output_rate,
            channels,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl Resampler for LinearResampler {
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<(), ResamplerError> {
        resample_interleaved_linear(
            input,
            self.channels,
            self.input_rate,
            self.output_rate,
            output,
        )
    }

    fn reset(&mut self) {}

    fn input_rate(&self) -> u32 {
        self.input_rate
    }

    fn output_rate(&self) -> u32 {
        self.output_rate
    }
}

pub fn resample_interleaved_linear(
    input: &[f32],
    channels: usize,
    input_rate: u32,
    output_rate: u32,
    output: &mut Vec<f32>,
) -> Result<(), ResamplerError> {
    if channels == 0 {
        return Err(ResamplerError::InvalidChannelCount);
    }
    if input_rate == 0 || output_rate == 0 {
        return Err(ResamplerError::InvalidSampleRate);
    }
    if input.len() % channels != 0 {
        return Err(ResamplerError::InvalidInputLength);
    }
    if input.is_empty() {
        return Ok(());
    }
    if input_rate == output_rate {
        output.extend_from_slice(input);
        return Ok(());
    }

    let input_frames = input.len() / channels;
    let output_frames =
        ((input_frames as u64 * output_rate as u64) / input_rate as u64).max(1) as usize;
    let ratio = input_rate as f64 / output_rate as f64;
    output.reserve(output_frames * channels);

    for frame in 0..output_frames {
        let position = frame as f64 * ratio;
        let base = position.floor() as usize;
        let next = (base + 1).min(input_frames - 1);
        let frac = (position - base as f64) as f32;

        for channel in 0..channels {
            let a = input[(base.min(input_frames - 1) * channels) + channel];
            let b = input[(next * channels) + channel];
            output.push(a + (b - a) * frac);
        }
    }

    Ok(())
}

pub fn resample_interleaved_sinc(
    input: &[f32],
    channels: usize,
    input_rate: u32,
    output_rate: u32,
    output: &mut Vec<f32>,
) -> Result<(), ResamplerError> {
    resample_interleaved_sinc_with_radius(
        input,
        channels,
        input_rate,
        output_rate,
        DEFAULT_SINC_RADIUS,
        output,
    )
}

pub fn resample_interleaved_sinc_with_radius(
    input: &[f32],
    channels: usize,
    input_rate: u32,
    output_rate: u32,
    radius: usize,
    output: &mut Vec<f32>,
) -> Result<(), ResamplerError> {
    if channels == 0 {
        return Err(ResamplerError::InvalidChannelCount);
    }
    if input_rate == 0 || output_rate == 0 {
        return Err(ResamplerError::InvalidSampleRate);
    }
    if input.len() % channels != 0 {
        return Err(ResamplerError::InvalidInputLength);
    }
    if input.is_empty() {
        return Ok(());
    }
    if input_rate == output_rate {
        output.extend_from_slice(input);
        return Ok(());
    }

    let input_frames = input.len() / channels;
    let output_frames =
        ((input_frames as u64 * output_rate as u64).div_ceil(input_rate as u64)).max(1) as usize;
    let ratio = input_rate as f64 / output_rate as f64;
    let cutoff = (output_rate as f64 / input_rate as f64).min(1.0);
    let radius = radius.max(2);
    output.reserve(output_frames * channels);

    for output_frame in 0..output_frames {
        let position = output_frame as f64 * ratio;
        let center = position.floor() as isize;

        for channel in 0..channels {
            let mut weighted_sum = 0.0_f64;
            let mut weight_sum = 0.0_f64;

            for tap in -(radius as isize)..=(radius as isize) {
                let source_frame = center + tap;
                let clamped_frame = source_frame.clamp(0, input_frames.saturating_sub(1) as isize);
                let distance = position - source_frame as f64;
                let window = hann_window(distance, radius as f64);
                if window <= 0.0 {
                    continue;
                }

                let weight = cutoff * sinc(cutoff * distance) * window;
                let sample = input[(clamped_frame as usize * channels) + channel] as f64;
                weighted_sum += sample * weight;
                weight_sum += weight;
            }

            let value = if weight_sum.abs() > f64::EPSILON {
                weighted_sum / weight_sum
            } else {
                let frame = center.clamp(0, input_frames.saturating_sub(1) as isize) as usize;
                input[(frame * channels) + channel] as f64
            };
            output.push(value as f32);
        }
    }

    Ok(())
}

fn sinc(value: f64) -> f64 {
    let x = std::f64::consts::PI * value;
    if x.abs() < 1.0e-8 {
        1.0
    } else {
        x.sin() / x
    }
}

fn hann_window(distance: f64, radius: f64) -> f64 {
    let normalized = distance.abs() / radius;
    if normalized >= 1.0 {
        0.0
    } else {
        0.5 + 0.5 * (std::f64::consts::PI * normalized).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_when_rates_match() {
        let mut resampler = LinearResampler::new(44_100, 44_100, 2).unwrap();
        let mut output = Vec::new();
        resampler
            .process(&[0.0, 0.1, 0.2, 0.3], &mut output)
            .unwrap();
        assert_eq!(output, vec![0.0, 0.1, 0.2, 0.3]);
    }

    #[test]
    fn resamples_interleaved_linear() {
        let mut output = Vec::new();
        resample_interleaved_linear(&[0.0, 1.0, 0.0, -1.0], 1, 4, 2, &mut output).unwrap();
        assert_eq!(output.len(), 2);
        assert!((output[0] - 0.0).abs() < 0.001);
        assert!((output[1] - 0.0).abs() < 0.001);
    }

    #[test]
    fn rejects_partial_frames() {
        let mut output = Vec::new();
        let err = resample_interleaved_linear(&[0.0, 1.0, 0.5], 2, 44_100, 48_000, &mut output)
            .unwrap_err();
        assert!(matches!(err, ResamplerError::InvalidInputLength));
    }

    #[test]
    fn resamples_interleaved_with_windowed_sinc() {
        let mut output = Vec::new();
        resample_interleaved_sinc(&[0.0, 1.0, 0.0, -1.0], 1, 4, 2, &mut output).unwrap();

        assert_eq!(output.len(), 2);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn sinc_rejects_partial_frames() {
        let mut output = Vec::new();
        let err = resample_interleaved_sinc(&[0.0, 1.0, 0.5], 2, 44_100, 48_000, &mut output)
            .unwrap_err();
        assert!(matches!(err, ResamplerError::InvalidInputLength));
    }
}
