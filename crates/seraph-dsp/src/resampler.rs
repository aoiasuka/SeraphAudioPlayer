use thiserror::Error;

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
}
