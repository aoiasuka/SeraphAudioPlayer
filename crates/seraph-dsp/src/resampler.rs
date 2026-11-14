use thiserror::Error;

/// 每侧保留的 sinc 过零点数。决定过渡带宽度（约为输出奈奎斯特频率的 17%）。
const DEFAULT_ZERO_CROSSINGS: usize = 32;
/// Kaiser 窗的 β：阻带衰减约 87 dB（A ≈ β / 0.1102 + 8.7）。
const KAISER_BETA: f64 = 8.6;
/// 截止频率相对「较低那一侧奈奎斯特频率」的比例。过渡带以它为中心：
/// 通带平直到约 0.875 倍奈奎斯特（44.1 kHz 输出时约 19.3 kHz），
/// 阻带从约 1.045 倍开始，漏过去的混叠只会折回 21 kHz 以上。
const CUTOFF_ROLLOFF: f64 = 0.96;
/// 滤波核查表：每个过零点间隔的采样数（线性插值，误差约 −106 dB）。
const TABLE_STEPS_PER_ZERO: usize = 512;
/// 核半径上限（输入帧）。只在极端降采样比下触及，防止退化参数分配巨表。
const MAX_KERNEL_HALF_FRAMES: usize = 4096;

#[derive(Debug, Error)]
pub enum ResamplerError {
    #[error("invalid input length")]
    InvalidInputLength,
    #[error("invalid channel count")]
    InvalidChannelCount,
    #[error("invalid sample rate")]
    InvalidSampleRate,
    #[error("internal error: {0}")]
    Internal(String),
}

/// 跨包保持 history 的加窗 sinc 重采样器。
///
/// - **降采样时滤波核随降采样比加宽**（AUD-02）：截止频率按 `输出率 / 输入率` 缩放，核半径同步
///   取 `过零点数 / 截止比` 个输入帧。此前核固定 ±16 个输入帧，192 kHz → 48 kHz 时只覆盖
///   4 个过零点，过渡带极宽，高频混叠抑制和通带平坦度都很差。
/// - **Kaiser 窗**（β = 8.6，阻带约 −87 dB）取代 Hann 窗（旁瓣衰减有限）。
/// - **权重每个输出帧只算一次**（AUD-11）：查表得到整组权重，再对各声道做点积；
///   此前在声道循环内部重算 sin / cos，立体声计算量翻倍。
/// - 持有上一包的尾部样本，滤波窗口跨包平滑滑动，包边界没有 click。
pub struct StatefulSincResampler {
    input_rate: u32,
    output_rate: u32,
    channels: usize,
    /// 核半径（输入帧）
    half: usize,
    ratio: f64,
    history: Vec<f32>,
    /// 下一个待输出帧在 history 中的位置（可为分数）。
    next_position: f64,
    /// 半边滤波核查表：`table[i]` 是距离 `i / table_scale` 个输入帧处的核值。
    table: Vec<f32>,
    table_scale: f64,
    /// 当前输出帧的权重（复用，避免每帧分配）
    weights: Vec<f32>,
}

impl StatefulSincResampler {
    pub fn new(input_rate: u32, output_rate: u32, channels: usize) -> Result<Self, ResamplerError> {
        Self::with_zero_crossings(input_rate, output_rate, channels, DEFAULT_ZERO_CROSSINGS)
    }

    pub fn with_zero_crossings(
        input_rate: u32,
        output_rate: u32,
        channels: usize,
        zero_crossings: usize,
    ) -> Result<Self, ResamplerError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ResamplerError::InvalidSampleRate);
        }
        if channels == 0 {
            return Err(ResamplerError::InvalidChannelCount);
        }
        let zero_crossings = zero_crossings.max(2);
        // 降采样比（≤ 1）：核按它的倒数加宽，才能在新的奈奎斯特频率之下截止
        let rate_ratio = (output_rate as f64 / input_rate as f64).min(1.0);
        let half = ((zero_crossings as f64 / rate_ratio).ceil() as usize)
            .clamp(zero_crossings, MAX_KERNEL_HALF_FRAMES);
        let cutoff = rate_ratio * CUTOFF_ROLLOFF;

        // 查表分辨率按输入帧计：过零点间隔是 1/rate_ratio 个输入帧
        let table_scale = (TABLE_STEPS_PER_ZERO as f64 * rate_ratio).max(8.0);
        let table_len = (half as f64 * table_scale).ceil() as usize + 2;
        let half_f = half as f64;
        let window_norm = bessel_i0(KAISER_BETA);
        let table = (0..table_len)
            .map(|index| {
                let distance = index as f64 / table_scale;
                if distance >= half_f {
                    return 0.0;
                }
                let x = distance / half_f;
                let window = bessel_i0(KAISER_BETA * (1.0 - x * x).sqrt()) / window_norm;
                (cutoff * sinc(cutoff * distance) * window) as f32
            })
            .collect();

        let mut resampler = Self {
            input_rate,
            output_rate,
            channels,
            half,
            ratio: input_rate as f64 / output_rate as f64,
            history: Vec::with_capacity(channels * half * 4),
            next_position: half as f64,
            table,
            table_scale,
            weights: Vec::with_capacity(half * 2 + 1),
        };
        resampler.prime_history();
        Ok(resampler)
    }

    /// 中-2：向 history 头部预填充 `half` 帧零，作为第一个真实输入帧的左侧滤波上下文。
    /// 否则输入前 `half` 帧只当上下文、永不作为输出中心——每个会话开头永久丢掉这些帧，
    /// gapless 连播接缝处可闻。与曲尾冲洗（喂零冲出尾部）对称。
    fn prime_history(&mut self) {
        if self.input_rate == self.output_rate {
            return;
        }
        self.history.clear();
        self.history.resize(self.channels * self.half, 0.0_f32);
        self.next_position = self.half as f64;
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

    /// 曲尾需要补多少帧零，才能把仍卡在滤波窗口里的尾部真实样本全部冲出来。
    /// 同采样率直通模式没有内部状态，返回 0。
    pub fn flush_input_frames(&self) -> usize {
        if self.input_rate == self.output_rate {
            0
        } else {
            self.half + 1
        }
    }

    pub fn reset(&mut self) {
        // 中-2：reset 后同样预填充左侧上下文，seek 之后的第一段也不丢首帧。
        self.prime_history();
    }

    #[inline]
    fn kernel(&self, distance: f64) -> f32 {
        let position = distance.abs() * self.table_scale;
        let index = position as usize;
        if index + 1 >= self.table.len() {
            return 0.0;
        }
        let frac = (position - index as f64) as f32;
        let a = self.table[index];
        a + (self.table[index + 1] - a) * frac
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

        let channels = self.channels;
        let history_frames = self.history.len() / channels;
        let half = self.half as isize;
        let half_f = self.half as f64;
        let mut weights = std::mem::take(&mut self.weights);
        // 必须 next_position + half 严格小于 history_frames 才能采样（右侧窗口完整）
        while self.next_position + half_f < history_frames as f64 {
            let center = self.next_position.floor() as isize;
            let first = (center - half).max(0);
            let last = (center + half).min(history_frames as isize - 1);

            // 权重只算一次，所有声道共用
            weights.clear();
            let mut weight_sum = 0.0_f64;
            for source in first..=last {
                let weight = self.kernel(self.next_position - source as f64);
                weight_sum += weight as f64;
                weights.push(weight);
            }
            let normalize = if weight_sum.abs() > 1.0e-9 {
                1.0 / weight_sum
            } else {
                0.0
            };

            let window = &self.history[first as usize * channels..(last as usize + 1) * channels];
            for channel in 0..channels {
                let mut sum = 0.0_f64;
                for (frame, weight) in window.chunks_exact(channels).zip(weights.iter()) {
                    sum += frame[channel] as f64 * *weight as f64;
                }
                out.push((sum * normalize) as f32);
            }
            self.next_position += self.ratio;
        }
        self.weights = weights;

        // 裁剪 history：只保留下一个输出帧的窗口左端之后的部分，避免无限增长。
        let drop_frames = (self.next_position.floor() as isize - half).max(0) as usize;
        let drop_frames = drop_frames.min(history_frames);
        if drop_frames > 0 {
            self.history.drain(..drop_frames * channels);
            self.next_position -= drop_frames as f64;
        }

        Ok(())
    }

    /// F-13：曲尾冲洗——喂 [`Self::flush_input_frames`] 帧零样本，把仍卡在滤波窗口里的
    /// 尾部真实样本冲出来。不调用则每曲结尾永远丢约半个核宽的帧。
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<(), ResamplerError> {
        let frames = self.flush_input_frames();
        if frames == 0 {
            return Ok(());
        }
        let zeros = vec![0.0_f32; frames * self.channels];
        self.process(&zeros, out)
    }
}

fn sinc(value: f64) -> f64 {
    let x = std::f64::consts::PI * value;
    if x.abs() < 1.0e-8 {
        1.0
    } else {
        x.sin() / x
    }
}

/// 第一类零阶修正贝塞尔函数（Kaiser 窗用），幂级数展开。
fn bessel_i0(x: f64) -> f64 {
    let quarter = x * x / 4.0;
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    for k in 1..64 {
        term *= quarter / (k as f64 * k as f64);
        sum += term;
        if term < sum * 1.0e-16 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| (std::f64::consts::TAU * freq * n as f64 / rate as f64).sin() as f32)
            .collect()
    }

    /// 跳过头尾的建立 / 衰减段后的 RMS。
    fn steady_rms(samples: &[f32]) -> f64 {
        let skip = samples.len() / 4;
        let body = &samples[skip..samples.len() - skip];
        (body.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / body.len() as f64).sqrt()
    }

    fn resample_mono(input_rate: u32, output_rate: u32, input: &[f32]) -> Vec<f32> {
        let mut resampler = StatefulSincResampler::new(input_rate, output_rate, 1).unwrap();
        let mut out = Vec::new();
        resampler.process(input, &mut out).unwrap();
        resampler.flush(&mut out).unwrap();
        out
    }

    fn db(ratio: f64) -> f64 {
        20.0 * ratio.max(1.0e-12).log10()
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
    fn rejects_partial_frames_and_bad_parameters() {
        let mut resampler = StatefulSincResampler::new(44_100, 48_000, 2).unwrap();
        let mut output = Vec::new();
        let err = resampler
            .process(&[0.0, 1.0, 0.5], &mut output)
            .unwrap_err();
        assert!(matches!(err, ResamplerError::InvalidInputLength));
        assert!(StatefulSincResampler::new(0, 48_000, 2).is_err());
        assert!(StatefulSincResampler::new(48_000, 48_000, 0).is_err());
    }

    /// AUD-02：192 kHz → 48 kHz 时，输出奈奎斯特（24 kHz）以上的内容必须被滤掉，
    /// 否则会混叠回可听频段。旧实现（±16 输入帧、Hann 窗）在这里只有十几 dB 的抑制。
    #[test]
    fn downsampling_rejects_content_above_output_nyquist() {
        let reference = steady_rms(&sine(192_000, 30_000.0, 96_000));
        for freq in [30_000.0, 40_000.0, 60_000.0, 90_000.0] {
            let out = resample_mono(192_000, 48_000, &sine(192_000, freq, 96_000));
            let attenuation = db(steady_rms(&out) / reference);
            assert!(
                attenuation < -70.0,
                "{freq} Hz 应被抑制 70 dB 以上，实际 {attenuation:.1} dB"
            );
        }
    }

    #[test]
    fn downsampling_keeps_passband_flat() {
        for freq in [100.0, 1_000.0, 10_000.0, 19_000.0] {
            let input = sine(192_000, freq, 96_000);
            let out = resample_mono(192_000, 48_000, &input);
            let gain = db(steady_rms(&out) / steady_rms(&input));
            assert!(
                gain.abs() < 0.1,
                "{freq} Hz 通带增益应平直，实际 {gain:.3} dB"
            );
        }
    }

    #[test]
    fn upsampling_keeps_passband_flat_and_length() {
        for freq in [100.0, 1_000.0, 10_000.0, 18_000.0] {
            let input = sine(44_100, freq, 44_100);
            let out = resample_mono(44_100, 48_000, &input);
            let gain = db(steady_rms(&out) / steady_rms(&input));
            assert!(
                gain.abs() < 0.1,
                "{freq} Hz 通带增益应平直，实际 {gain:.3} dB"
            );
            // 首帧对齐、尾部冲干净：输出长度就是按比例换算的长度（±2 帧）
            assert!((out.len() as i64 - 48_000).abs() <= 2, "长度 {}", out.len());
        }
    }

    /// 首个输出帧对准首个输入帧：开头不丢帧。升采样时滤波核接近冲激，
    /// 首帧取到的就是第一个输入样本本身（左侧预填的零只贡献很小的权重）。
    #[test]
    fn first_output_frame_is_centered_on_first_input_frame() {
        let out = resample_mono(44_100, 48_000, &vec![1.0_f32; 4_410]);
        assert!(out[0] > 0.9, "首帧应落在第一个输入样本上，实际 {}", out[0]);
        let middle = out[out.len() / 2];
        assert!((middle - 1.0).abs() < 1.0e-3, "稳态 {middle}");
    }

    #[test]
    fn stereo_channels_are_resampled_independently() {
        let left = sine(96_000, 1_000.0, 9_600);
        let mut interleaved = Vec::with_capacity(left.len() * 2);
        for sample in &left {
            interleaved.push(*sample);
            interleaved.push(-*sample * 0.5);
        }
        let mut resampler = StatefulSincResampler::new(96_000, 48_000, 2).unwrap();
        let mut out = Vec::new();
        resampler.process(&interleaved, &mut out).unwrap();
        assert!(out.len().is_multiple_of(2));
        for frame in out.as_chunks::<2>().0 {
            assert!((frame[1] + frame[0] * 0.5).abs() < 1.0e-4);
        }
    }

    #[test]
    fn flush_emits_buffered_tail_samples() {
        let mut resampler = StatefulSincResampler::with_zero_crossings(8, 4, 1, 4).unwrap();
        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.2).sin()).collect();
        let mut out = Vec::new();
        resampler.process(&input, &mut out).unwrap();
        let before_flush = out.len();
        resampler.flush(&mut out).unwrap();
        assert!(out.len() > before_flush, "flush 应冲出卡在窗口里的尾部样本");
        assert!(out.iter().all(|s| s.is_finite()));

        // 同采样率直通模式下 flush 不产生任何输出
        let mut passthrough = StatefulSincResampler::new(48_000, 48_000, 2).unwrap();
        let mut out2 = Vec::new();
        passthrough.flush(&mut out2).unwrap();
        assert!(out2.is_empty());
        assert_eq!(passthrough.flush_input_frames(), 0);
    }

    #[test]
    fn stateful_resampler_continuous_across_packets() {
        // 把一段 sine 切成几块喂进去，输出应与一次性喂进去的结果一致
        let input: Vec<f32> = (0..2_000).map(|i| (i as f32 * 0.1).sin()).collect();

        let mut full_resampler = StatefulSincResampler::new(44_100, 48_000, 1).unwrap();
        let mut full_out = Vec::new();
        full_resampler.process(&input, &mut full_out).unwrap();
        full_resampler.flush(&mut full_out).unwrap();

        let mut chunked_resampler = StatefulSincResampler::new(44_100, 48_000, 1).unwrap();
        let mut chunked_out = Vec::new();
        for chunk in input.chunks(137) {
            chunked_resampler.process(chunk, &mut chunked_out).unwrap();
        }
        chunked_resampler.flush(&mut chunked_out).unwrap();

        assert_eq!(full_out.len(), chunked_out.len());
        for (a, b) in full_out.iter().zip(chunked_out.iter()) {
            assert!(
                (a - b).abs() < 1.0e-5,
                "包边界差异过大 (跨包 sinc 不连续): {a} vs {b}"
            );
        }
    }

    #[test]
    fn history_stays_bounded_over_long_streams() {
        let mut resampler = StatefulSincResampler::new(192_000, 44_100, 2).unwrap();
        let mut out = Vec::new();
        let packet = vec![0.25_f32; 4_096 * 2];
        for _ in 0..200 {
            out.clear();
            resampler.process(&packet, &mut out).unwrap();
        }
        let frames = resampler.history.len() / 2;
        assert!(frames <= resampler.half * 2 + 8, "history 残留 {frames} 帧");
    }

    #[test]
    fn extreme_ratio_kernel_is_capped() {
        let resampler = StatefulSincResampler::new(768_000, 1_000, 1).unwrap();
        assert_eq!(resampler.half, MAX_KERNEL_HALF_FRAMES);
        assert!(resampler.table.len() < 1 << 20);
    }
}
