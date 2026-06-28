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
    if !input.len().is_multiple_of(channels) {
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

/// 跨包保持 history 的 sinc 重采样器。
///
/// 旧的 `resample_interleaved_sinc` 是无状态的：每个 Packet 独立处理，
/// 包边界用 clamp 复制边界样本 → 周期性 click。该结构持有上一包的尾部样本，
/// 让 sinc 滤波器的窗口可以跨包平滑滑动，与 rubato 的 fast-fixed-in/out 策略一致。
pub struct StatefulSincResampler {
    input_rate: u32,
    output_rate: u32,
    channels: usize,
    radius: usize,
    ratio: f64,
    cutoff: f64,
    history: Vec<f32>,
    // 下一个待输出的 frame 在 history 中的位置（可为分数）。
    // history 头部对应"全局 input frame 0"位置；
    // 每次处理后从 history 砍掉已经不再被 sinc 窗口需要的样本并相应调整。
    next_position: f64,
}

impl StatefulSincResampler {
    pub fn new(input_rate: u32, output_rate: u32, channels: usize) -> Result<Self, ResamplerError> {
        Self::with_radius(input_rate, output_rate, channels, DEFAULT_SINC_RADIUS)
    }

    pub fn with_radius(
        input_rate: u32,
        output_rate: u32,
        channels: usize,
        radius: usize,
    ) -> Result<Self, ResamplerError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ResamplerError::InvalidSampleRate);
        }
        if channels == 0 {
            return Err(ResamplerError::InvalidChannelCount);
        }
        let radius = radius.max(2);
        Ok(Self {
            input_rate,
            output_rate,
            channels,
            radius,
            ratio: input_rate as f64 / output_rate as f64,
            cutoff: (output_rate as f64 / input_rate as f64).min(1.0),
            history: Vec::with_capacity(channels * radius * 4),
            next_position: radius as f64, // 第一个输出从 history[radius] 处取
        })
    }

    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.next_position = self.radius as f64;
    }

    /// 处理一段输入并追加输出到 `out`。
    /// 同采样率时是 zero-copy 直通；不同时执行有状态 sinc。
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), ResamplerError> {
        if input.is_empty() {
            return Ok(());
        }
        if !input.len().is_multiple_of(self.channels) {
            return Err(ResamplerError::InvalidInputLength);
        }
        if self.input_rate == self.output_rate {
            out.extend_from_slice(input);
            return Ok(());
        }

        self.history.extend_from_slice(input);

        let history_frames = self.history.len() / self.channels;
        let radius = self.radius;
        let radius_f = radius as f64;
        // 必须 next_position + radius 严格小于 history_frames 才能采样
        // （等于 history_frames-1 也行，但保留一帧缓冲让插值边界更干净）
        while self.next_position + radius_f < history_frames as f64 {
            let center = self.next_position.floor() as isize;
            for channel in 0..self.channels {
                let mut weighted_sum = 0.0_f64;
                let mut weight_sum = 0.0_f64;
                for tap in -(radius as isize)..=(radius as isize) {
                    let source = center + tap;
                    if source < 0 || source >= history_frames as isize {
                        continue;
                    }
                    let distance = self.next_position - source as f64;
                    let window = hann_window(distance, radius_f);
                    if window <= 0.0 {
                        continue;
                    }
                    let weight = self.cutoff * sinc(self.cutoff * distance) * window;
                    let sample = self.history[(source as usize * self.channels) + channel] as f64;
                    weighted_sum += sample * weight;
                    weight_sum += weight;
                }
                let value = if weight_sum.abs() > f64::EPSILON {
                    weighted_sum / weight_sum
                } else {
                    self.history[(center.max(0) as usize * self.channels) + channel] as f64
                };
                out.push(value as f32);
            }
            self.next_position += self.ratio;
        }

        // 裁剪 history：保留最后 (radius * 2 + 1) frames 给下一轮的 sinc 窗口；
        // 多余的前部丢弃并把 next_position 往前移对应偏移，避免无限增长。
        let keep_frames = (radius * 2 + 1).max(self.ratio.ceil() as usize + radius);
        let history_frames_now = self.history.len() / self.channels;
        if history_frames_now > keep_frames {
            let drop_frames = history_frames_now - keep_frames;
            let drop_samples = drop_frames * self.channels;
            self.history.drain(..drop_samples);
            self.next_position -= drop_frames as f64;
        }

        Ok(())
    }
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
    if !input.len().is_multiple_of(channels) {
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

    #[test]
    fn stateful_resampler_passthrough_when_rates_match() {
        let mut resampler = StatefulSincResampler::new(48_000, 48_000, 2).unwrap();
        let mut output = Vec::new();
        resampler
            .process(&[0.0, 0.1, 0.2, 0.3], &mut output)
            .unwrap();
        assert_eq!(output, vec![0.0, 0.1, 0.2, 0.3]);
    }

    #[test]
    fn stateful_resampler_continuous_across_packets() {
        // 把一段 sine 切成 4 块喂进去，输出应该 ~= 一次性喂进去的结果
        let input_len = 200;
        let mut input = Vec::with_capacity(input_len);
        for i in 0..input_len {
            input.push((i as f32 * 0.1).sin());
        }

        // 一次性处理
        let mut full_resampler = StatefulSincResampler::with_radius(8, 4, 1, 4).unwrap();
        let mut full_out = Vec::new();
        full_resampler.process(&input, &mut full_out).unwrap();
        // 再喂个 padding 让最后一段也能输出
        full_resampler
            .process(&vec![0.0; 64], &mut full_out)
            .unwrap();

        // 分块处理
        let mut chunked_resampler = StatefulSincResampler::with_radius(8, 4, 1, 4).unwrap();
        let mut chunked_out = Vec::new();
        for chunk in input.chunks(50) {
            chunked_resampler.process(chunk, &mut chunked_out).unwrap();
        }
        chunked_resampler
            .process(&vec![0.0; 64], &mut chunked_out)
            .unwrap();

        assert_eq!(full_out.len(), chunked_out.len());
        for (a, b) in full_out.iter().zip(chunked_out.iter()) {
            assert!(
                (a - b).abs() < 1.0e-5,
                "包边界差异过大 (跨包 sinc 不连续): {a} vs {b}"
            );
        }
    }
}
