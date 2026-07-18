//! 参数均衡器（RBJ biquad 级联）。
//!
//! 每个频段是一个二阶 IIR（biquad），系数按 Robert Bristow-Johnson 的
//! "Audio EQ Cookbook" 公式计算，用 Transposed Direct Form II 求值
//! （数值稳定、每样本只需两个状态寄存器）。逐声道保存独立状态，
//! 频段串联即整条 EQ 响应。
//!
//! 位置：运行在解码线程（非实时回调），允许在 settings 变化时重算系数。

use serde::{Deserialize, Serialize};

/// 频段滤波类型。命名与 EqualizerAPO / AutoEq 的 `PK/LSC/HSC/LP/HP` 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BandKind {
    /// 峰化/凹陷（parametric）——最常用，`PK`
    Peaking,
    /// 低频搁架 `LSC`
    LowShelf,
    /// 高频搁架 `HSC`
    HighShelf,
    /// 低通 `LP`
    LowPass,
    /// 高通 `HP`
    HighPass,
}

/// 单个 EQ 频段的用户参数（serde 与前端 / 预设文件对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    pub kind: BandKind,
    /// 中心/转角频率（Hz）
    pub freq: f32,
    /// 增益（dB）——LP/HP 忽略此项
    pub gain: f32,
    /// 品质因数 Q（越大越窄）
    pub q: f32,
    /// 该频段是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            kind: BandKind::Peaking,
            freq: 1_000.0,
            gain: 0.0,
            q: 1.0,
            enabled: true,
        }
    }
}

/// TDF-II biquad 系数（a0 已归一化为 1）。
#[derive(Debug, Clone, Copy)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    /// 直通（单位增益）系数。
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// 按 RBJ Audio EQ Cookbook 计算系数。
    /// 频率钳制到 (0, nyquist)，Q 与增益做兜底，避免除零 / NaN 渗入音频。
    fn design(band: &EqBand, sample_rate: f32) -> Self {
        let nyquist = sample_rate * 0.5;
        // 坑 3：频率必须严格小于 nyquist，否则 tan/cos 在边界发散产生 NaN。
        let freq = band.freq.clamp(1.0, nyquist - 1.0).max(1.0);
        if freq >= nyquist {
            return Self::identity();
        }
        let q = if band.q.is_finite() && band.q > 1.0e-3 {
            band.q
        } else {
            0.707
        };
        let gain_db = if band.gain.is_finite() {
            band.gain
        } else {
            0.0
        };

        let a = 10.0_f32.powf(gain_db / 40.0); // sqrt(线性增益)
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match band.kind {
            BandKind::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos_w0,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos_w0,
                1.0 - alpha / a,
            ),
            BandKind::LowShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
                    (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
                )
            }
            BandKind::HighShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
                    (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
                    (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
                )
            }
            BandKind::LowPass => {
                let b1 = 1.0 - cos_w0;
                (
                    b1 / 2.0,
                    b1,
                    b1 / 2.0,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
            BandKind::HighPass => {
                let b1 = -(1.0 + cos_w0);
                (
                    (1.0 + cos_w0) / 2.0,
                    b1,
                    (1.0 + cos_w0) / 2.0,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
        };

        if a0.abs() < f32::EPSILON {
            return Self::identity();
        }
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// 复频响幅值（用于前端曲线；此处仅测试用）。
    #[cfg(test)]
    fn magnitude_db(&self, freq: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let (cos1, sin1) = (w.cos(), w.sin());
        let (cos2, sin2) = ((2.0 * w).cos(), (2.0 * w).sin());
        // 分子 b0 + b1 z^-1 + b2 z^-2
        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = -(self.b1 * sin1 + self.b2 * sin2);
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = -(self.a1 * sin1 + self.a2 * sin2);
        let num = (num_re * num_re + num_im * num_im).sqrt();
        let den = (den_re * den_re + den_im * den_im).sqrt();
        20.0 * (num / den.max(1.0e-12)).log10()
    }
}

/// 单声道 biquad 运行状态（TDF-II 两个寄存器）。
#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, coeffs: &BiquadCoeffs, input: f32) -> f32 {
        // Transposed Direct Form II
        let output = coeffs.b0 * input + self.z1;
        self.z1 = coeffs.b1 * input - coeffs.a1 * output + self.z2;
        self.z2 = coeffs.b2 * input - coeffs.a2 * output;
        output
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// 一条声道上的 EQ：多个频段 biquad 级联。
#[derive(Debug, Clone, Default)]
struct ChannelEq {
    states: Vec<BiquadState>,
}

impl ChannelEq {
    fn ensure_len(&mut self, len: usize) {
        if self.states.len() != len {
            self.states = vec![BiquadState::default(); len];
        }
    }

    #[inline]
    fn process(&mut self, coeffs: &[BiquadCoeffs], input: f32) -> f32 {
        let mut sample = input;
        for (state, coeff) in self.states.iter_mut().zip(coeffs.iter()) {
            sample = state.process(coeff, sample);
        }
        sample
    }

    fn reset(&mut self) {
        for state in &mut self.states {
            state.reset();
        }
    }
}

/// 参数均衡器：预放大 + 若干频段，逐声道独立状态。
#[derive(Debug, Clone, Default)]
pub struct Equalizer {
    preamp_gain: f32,
    coeffs: Vec<BiquadCoeffs>,
    channels: Vec<ChannelEq>,
    sample_rate: f32,
}

impl Equalizer {
    /// 用给定的 preamp（dB）、频段与采样率重建系数。保留声道状态（连续播放不断流）。
    pub fn configure(
        &mut self,
        preamp_db: f32,
        bands: &[EqBand],
        sample_rate: f32,
        channels: usize,
    ) {
        let preamp_db = if preamp_db.is_finite() {
            preamp_db
        } else {
            0.0
        };
        self.preamp_gain = 10.0_f32.powf(preamp_db / 20.0);
        self.sample_rate = sample_rate.max(1.0);
        self.coeffs = bands
            .iter()
            .filter(|band| band.enabled)
            .map(|band| BiquadCoeffs::design(band, self.sample_rate))
            .collect();

        if self.channels.len() != channels {
            self.channels = vec![ChannelEq::default(); channels];
        }
        for channel in &mut self.channels {
            channel.ensure_len(self.coeffs.len());
        }
    }

    /// 是否为纯直通（无频段且 preamp≈0dB）——上层据此走 zero-copy。
    pub fn is_identity(&self) -> bool {
        self.coeffs.is_empty() && (self.preamp_gain - 1.0).abs() < 1.0e-6
    }

    /// 就地处理交错样本。`channels` 必须与 configure 时一致。
    pub fn process_interleaved(&mut self, samples: &mut [f32], channels: usize) {
        if channels == 0 || self.channels.len() != channels {
            return;
        }
        let identity = self.coeffs.is_empty();
        for frame in samples.chunks_mut(channels) {
            for (channel_index, sample) in frame.iter_mut().enumerate() {
                let mut value = *sample * self.preamp_gain;
                if !identity {
                    value = self.channels[channel_index].process(&self.coeffs, value);
                }
                *sample = value;
            }
        }
    }

    /// 坑 1：seek 后必须清零所有 biquad 状态，否则跳转点残留历史样本渗出杂音。
    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
    }
}

/// 计算一组频段在指定频率上的合成幅频响应（dB），供前端曲线复用同一套系数逻辑。
/// 独立于运行状态，纯函数。
pub fn combined_response_db(preamp_db: f32, bands: &[EqBand], freq: f32, sample_rate: f32) -> f32 {
    let mut total = if preamp_db.is_finite() {
        preamp_db
    } else {
        0.0
    };
    for band in bands.iter().filter(|band| band.enabled) {
        let coeffs = BiquadCoeffs::design(band, sample_rate);
        total += coeffs_magnitude_db(&coeffs, freq, sample_rate);
    }
    total
}

fn coeffs_magnitude_db(coeffs: &BiquadCoeffs, freq: f32, sample_rate: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * freq / sample_rate.max(1.0);
    let (cos1, sin1) = (w.cos(), w.sin());
    let (cos2, sin2) = ((2.0 * w).cos(), (2.0 * w).sin());
    let num_re = coeffs.b0 + coeffs.b1 * cos1 + coeffs.b2 * cos2;
    let num_im = -(coeffs.b1 * sin1 + coeffs.b2 * sin2);
    let den_re = 1.0 + coeffs.a1 * cos1 + coeffs.a2 * cos2;
    let den_im = -(coeffs.a1 * sin1 + coeffs.a2 * sin2);
    let num = (num_re * num_re + num_im * num_im).sqrt();
    let den = (den_re * den_re + den_im * den_im).sqrt();
    20.0 * (num / den.max(1.0e-12)).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peaking(freq: f32, gain: f32, q: f32) -> EqBand {
        EqBand {
            kind: BandKind::Peaking,
            freq,
            gain,
            q,
            enabled: true,
        }
    }

    #[test]
    fn flat_eq_is_identity_passthrough() {
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[], 48_000.0, 2);
        assert!(eq.is_identity());

        let mut samples = vec![0.1, -0.2, 0.3, -0.4, 0.5, -0.6];
        let original = samples.clone();
        eq.process_interleaved(&mut samples, 2);
        assert_eq!(samples, original, "空 EQ 必须 bit-exact 直通");
    }

    #[test]
    fn preamp_only_scales_linearly() {
        let mut eq = Equalizer::default();
        eq.configure(-6.0, &[], 48_000.0, 1);
        assert!(!eq.is_identity());
        let mut samples = vec![1.0, 1.0];
        eq.process_interleaved(&mut samples, 1);
        let expected = 10.0_f32.powf(-6.0 / 20.0);
        assert!((samples[0] - expected).abs() < 1e-6);
    }

    #[test]
    fn peaking_boost_raises_center_magnitude() {
        // +6dB @ 1kHz 的峰化滤波器在中心频率响应应接近 +6dB
        let coeffs = BiquadCoeffs::design(&peaking(1_000.0, 6.0, 1.0), 48_000.0);
        let mag = coeffs.magnitude_db(1_000.0, 48_000.0);
        assert!((mag - 6.0).abs() < 0.5, "中心增益应≈6dB，实际 {mag}");
        // 远端（20Hz）几乎不受影响
        let low = coeffs.magnitude_db(20.0, 48_000.0);
        assert!(low.abs() < 0.5, "远端应接近 0dB，实际 {low}");
    }

    #[test]
    fn stays_finite_at_nyquist_edge() {
        // 坑 3：频率贴 nyquist 不得产生 NaN/Inf
        let mut eq = Equalizer::default();
        eq.configure(
            0.0,
            &[peaking(23_999.0, 9.0, 4.0), peaking(24_500.0, 9.0, 4.0)],
            48_000.0,
            2,
        );
        let mut samples = vec![0.5_f32; 256];
        eq.process_interleaved(&mut samples, 2);
        assert!(samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn reset_clears_filter_memory() {
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[peaking(1_000.0, 9.0, 2.0)], 48_000.0, 1);
        let mut impulse = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        eq.process_interleaved(&mut impulse, 1);
        eq.reset();
        // reset 后同样的冲激应得到完全相同的响应（状态已清零）
        let mut impulse2 = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        eq.process_interleaved(&mut impulse2, 1);
        for (a, b) in impulse.iter().zip(impulse2.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn remains_stable_over_long_signal() {
        // 高增益窄带不应发散
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[peaking(100.0, 12.0, 8.0)], 44_100.0, 2);
        let mut samples: Vec<f32> = (0..44_100)
            .map(|i| (i as f32 * 0.05).sin() * 0.5)
            .flat_map(|s| [s, s])
            .collect();
        eq.process_interleaved(&mut samples, 2);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() < 50.0));
    }

    #[test]
    fn band_serde_roundtrip() {
        let band = peaking(2_500.0, -3.5, 1.4);
        let json = serde_json::to_string(&band).unwrap();
        let back: EqBand = serde_json::from_str(&json).unwrap();
        assert_eq!(band, back);
    }

    #[test]
    fn combined_response_matches_single_band() {
        let bands = [peaking(1_000.0, 6.0, 1.0)];
        let combined = combined_response_db(0.0, &bands, 1_000.0, 48_000.0);
        assert!((combined - 6.0).abs() < 0.5);
    }
}
