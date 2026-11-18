//! 参数均衡器（RBJ biquad 级联）。
//!
//! 每个频段是一个二阶 IIR（biquad），系数按 Robert Bristow-Johnson 的
//! "Audio EQ Cookbook" 公式计算，用 Transposed Direct Form II 求值
//! （数值稳定、每样本只需两个状态寄存器）。逐声道保存独立状态，
//! 频段串联即整条 EQ 响应。
//!
//! DSP-02：系数与状态寄存器一律 `f64`，只在进出时与 `f32` 样本转换。低频高 Q 的
//! 频段在高采样率下极点紧贴单位圆（192 kHz 下 30 Hz、Q 8 时 `1 − |p|` 约 6×10⁻⁵），
//! `f32` 系数的舍入会让实际响应偏离设计值并抬高量化噪声。
//!
//! 位置：运行在解码线程（非实时回调），允许在 settings 变化时重算系数。

use serde::{Deserialize, Serialize};

/// L-6（2026-08-16 审查）：单频段增益与 preamp 的幅度上限（dB），与前端
/// `eqSanitize.ts` 的 EQ_GAIN_LIMIT / EQ_PREAMP_LIMIT 同口径。这是 pub API 的
/// 自卫线——`is_finite` 挡不住 1e30 dB 这类有限极端值。
pub const MAX_EQ_GAIN_DB: f32 = 24.0;

/// 频段数量上限（CMD-01 / EQ4-03）：与前端 `src/lib/eqSanitize.ts` 的 `MAX_EQ_BANDS` 同值。
/// 前端所有入口都会封顶，这里是后端自己的兜底——一旦某条路径漏了消毒，
/// 几千个频段会让解码线程逐样本跑几千级滤波、播放卡死。改这里要同步改前端。
pub const MAX_EQ_BANDS: usize = 64;

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
#[derive(Debug, Clone, Copy, PartialEq)]
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
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
        let sample_rate = f64::from(sample_rate);
        let nyquist = sample_rate * 0.5;
        // 坑 3：频率必须严格小于 nyquist，否则 tan/cos 在边界发散产生 NaN。
        // L-5（2026-08-16 审查）：clamp 上界先兜底到不小于下界——f32::clamp 在
        // min > max 时 panic，这是 pub API，退化采样率不该炸掉调用方。
        let freq_hi = (nyquist - 1.0).max(1.0);
        let freq = f64::from(band.freq).clamp(1.0, freq_hi).max(1.0);
        if freq >= nyquist {
            return Self::identity();
        }
        let q = if band.q.is_finite() && band.q > 1.0e-3 {
            f64::from(band.q)
        } else {
            0.707
        };
        // L-6（2026-08-16 审查）：增益限幅 ±24 dB，与前端 eqSanitize 同口径。
        // 只查 is_finite 挡不住 1e30 dB 这类有限极端值——10^(g/40) 溢出为 Inf
        // 后系数进入滤波器，输出 NaN/Inf 直达 DAC（F32 渲染路径 clamp 传播 NaN）。
        let gain_db = if band.gain.is_finite() {
            f64::from(band.gain.clamp(-MAX_EQ_GAIN_DB, MAX_EQ_GAIN_DB))
        } else {
            0.0
        };

        let a = 10.0_f64.powf(gain_db / 40.0); // sqrt(线性增益)
        let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
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

        if a0.abs() < f64::EPSILON {
            return Self::identity();
        }
        let designed = Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        };
        // L-6 纵深：任何非有限系数一律回退直通，不让 Inf/NaN 进入滤波状态
        //（状态被 NaN 污染后即使换回正常系数也会持续输出 NaN）。
        let finite = [
            designed.b0,
            designed.b1,
            designed.b2,
            designed.a1,
            designed.a2,
        ]
        .iter()
        .all(|c| c.is_finite());
        if finite {
            designed
        } else {
            Self::identity()
        }
    }

    /// 复频响幅值（用于前端曲线；此处仅测试用）。
    #[cfg(test)]
    fn magnitude_db(&self, freq: f32, sample_rate: f32) -> f32 {
        coeffs_magnitude_db(self, freq, sample_rate)
    }

    /// DSP-03：从当前系数走到 `target` 的每帧步进量。
    fn step_towards(&self, target: &Self, frames: f64) -> Self {
        Self {
            b0: (target.b0 - self.b0) / frames,
            b1: (target.b1 - self.b1) / frames,
            b2: (target.b2 - self.b2) / frames,
            a1: (target.a1 - self.a1) / frames,
            a2: (target.a2 - self.a2) / frames,
        }
    }

    #[inline]
    fn advance(&mut self, step: &Self) {
        self.b0 += step.b0;
        self.b1 += step.b1;
        self.b2 += step.b2;
        self.a1 += step.a1;
        self.a2 += step.a2;
    }
}

/// DSP-03：系数过渡时长。播放中改 EQ 时，系数在这段时间内逐帧线性走到新值，
/// 不再一步硬切（拖滑块的「拉链」噪声、开关频段的咔哒声都来自硬切）。
/// 稳定的二阶极点在 (a1, a2) 平面上构成一个凸三角形，两组稳定系数之间的线性插值
/// 始终落在三角形内，所以过渡途中的滤波器也是稳定的。
const COEFF_RAMP_SECONDS: f32 = 0.02;

fn coeff_ramp_frames(sample_rate: f32) -> u32 {
    ((sample_rate * COEFF_RAMP_SECONDS) as u32).max(1)
}

/// 单声道 biquad 运行状态（TDF-II 两个寄存器）。
#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f64,
    z2: f64,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, coeffs: &BiquadCoeffs, input: f64) -> f64 {
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
    /// 调整滤波级数：保留已有各级的状态，新增的级从零状态起步，多出的尾部丢弃。
    fn resize(&mut self, len: usize) {
        self.states.resize(len, BiquadState::default());
    }

    #[inline]
    fn process(&mut self, coeffs: &[BiquadCoeffs], input: f64) -> f64 {
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
///
/// DSP-03：播放中重新配置时不硬切——`coeffs` / `preamp_gain` 是「当前生效值」，
/// 在 [`COEFF_RAMP_SECONDS`] 内逐帧线性走到 `targets` / `preamp_target`。频段数量
/// 变化时不清状态：增加的滤波级从直通（零状态）渐变到目标，减少的尾部滤波级先渐变
/// 回直通、过渡结束再丢弃，其余各级的状态原样保留。
#[derive(Debug, Clone)]
pub struct Equalizer {
    /// 当前生效的预放大线性增益（过渡期间逐帧变化）。
    preamp_gain: f32,
    preamp_target: f32,
    preamp_step: f32,
    /// 当前生效的系数（过渡期间逐帧变化，长度可能暂时大于 `target_len`）。
    coeffs: Vec<BiquadCoeffs>,
    targets: Vec<BiquadCoeffs>,
    steps: Vec<BiquadCoeffs>,
    /// 过渡结束后保留的滤波级数。
    target_len: usize,
    /// 过渡还剩多少帧；0 = 稳态。
    ramp_remaining: u32,
    channels: Vec<ChannelEq>,
    sample_rate: f32,
    /// 自上次整体重建 / `reset` 以来是否处理过音频。没处理过就没有需要接续的声音，
    /// 配置直接生效（起播、seek 之后的首次配置不该带着一段渐变）。
    primed: bool,
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            preamp_gain: 1.0,
            preamp_target: 1.0,
            preamp_step: 0.0,
            coeffs: Vec::new(),
            targets: Vec::new(),
            steps: Vec::new(),
            target_len: 0,
            ramp_remaining: 0,
            channels: Vec::new(),
            sample_rate: 0.0,
            primed: false,
        }
    }
}

impl Equalizer {
    /// 用给定的 preamp（dB）、频段与采样率重建系数。保留声道状态（连续播放不断流）；
    /// 已经在出声时，新系数经一段渐变生效（DSP-03）。
    pub fn configure(
        &mut self,
        preamp_db: f32,
        bands: &[EqBand],
        sample_rate: f32,
        channels: usize,
    ) {
        // L-6：preamp 与频段增益同口径限幅（±24 dB），再算线性增益
        let preamp_db = if preamp_db.is_finite() {
            preamp_db.clamp(-MAX_EQ_GAIN_DB, MAX_EQ_GAIN_DB)
        } else {
            0.0
        };
        let preamp_target = 10.0_f32.powf(preamp_db / 20.0);
        let sample_rate = sample_rate.max(1.0);
        let designed: Vec<BiquadCoeffs> = bands
            .iter()
            .filter(|band| band.enabled)
            .take(MAX_EQ_BANDS)
            .map(|band| BiquadCoeffs::design(band, sample_rate))
            .collect();

        let layout_changed = self.sample_rate != sample_rate || self.channels.len() != channels;
        self.sample_rate = sample_rate;
        if layout_changed || !self.primed {
            // 采样率 / 声道布局变了（旧状态不再适用），或还没出过声：直接生效。
            if layout_changed {
                self.channels = vec![ChannelEq::default(); channels];
                self.primed = false;
            }
            self.preamp_gain = preamp_target;
            self.preamp_target = preamp_target;
            self.target_len = designed.len();
            self.targets = designed;
            self.finish_ramp();
            return;
        }

        if self.preamp_target == preamp_target && self.targets[..self.target_len] == designed[..] {
            // 目标没变（例如只改了 crossfeed）：不重启进行中的过渡。
            return;
        }

        // 过渡期间滤波级数取新旧较大者，缺的一侧按直通补齐。
        let slots = self.coeffs.len().max(designed.len());
        self.coeffs.resize(slots, BiquadCoeffs::identity());
        for channel in &mut self.channels {
            channel.resize(slots);
        }
        self.target_len = designed.len();
        self.targets = designed;
        self.targets.resize(slots, BiquadCoeffs::identity());

        let frames = coeff_ramp_frames(sample_rate);
        self.steps.clear();
        self.steps.extend(
            self.coeffs
                .iter()
                .zip(&self.targets)
                .map(|(current, target)| current.step_towards(target, f64::from(frames))),
        );
        self.preamp_target = preamp_target;
        self.preamp_step = (preamp_target - self.preamp_gain) / frames as f32;
        self.ramp_remaining = frames;
    }

    /// 过渡收尾：精确落到目标值（不留逐帧累加的浮点误差），丢弃已渐变回直通的尾部滤波级。
    fn finish_ramp(&mut self) {
        self.ramp_remaining = 0;
        self.preamp_gain = self.preamp_target;
        self.preamp_step = 0.0;
        self.steps.clear();
        self.targets.truncate(self.target_len);
        self.coeffs.clone_from(&self.targets);
        for channel in &mut self.channels {
            channel.resize(self.target_len);
        }
    }

    #[inline]
    fn advance_ramp(&mut self) {
        if self.ramp_remaining <= 1 {
            self.finish_ramp();
            return;
        }
        self.ramp_remaining -= 1;
        self.preamp_gain += self.preamp_step;
        for (coeffs, step) in self.coeffs.iter_mut().zip(&self.steps) {
            coeffs.advance(step);
        }
    }

    /// 是否为纯直通（无频段且 preamp≈0dB）——上层据此走 zero-copy。
    /// 过渡进行中不算直通：渐变回直通的那段也要处理完，上层才能旁路。
    pub fn is_identity(&self) -> bool {
        self.ramp_remaining == 0
            && self.coeffs.is_empty()
            && (self.preamp_gain - 1.0).abs() < 1.0e-6
    }

    /// 就地处理交错样本。`channels` 必须与 configure 时一致。
    pub fn process_interleaved(&mut self, samples: &mut [f32], channels: usize) {
        if channels == 0 || self.channels.len() != channels {
            return;
        }
        if !samples.is_empty() {
            self.primed = true;
        }
        let mut frames = samples.chunks_mut(channels);
        // 过渡段：每帧先推进一步系数再处理
        while self.ramp_remaining > 0 {
            let Some(frame) = frames.next() else {
                return;
            };
            self.advance_ramp();
            self.process_frame(frame);
        }
        for frame in frames {
            self.process_frame(frame);
        }
    }

    #[inline]
    fn process_frame(&mut self, frame: &mut [f32]) {
        let identity = self.coeffs.is_empty();
        for (channel_index, sample) in frame.iter_mut().enumerate() {
            // 没有频段时只做预放大，保持单精度（0 dB 时逐位直通）；
            // 有频段时整条级联在 f64 里算，末尾才舍回 f32。
            *sample = if identity {
                *sample * self.preamp_gain
            } else {
                let scaled = f64::from(*sample) * f64::from(self.preamp_gain);
                self.channels[channel_index].process(&self.coeffs, scaled) as f32
            };
        }
    }

    /// 坑 1：seek 后必须清零所有 biquad 状态，否则跳转点残留历史样本渗出杂音。
    /// 进行中的过渡直接落到目标——跳转点本身就是不连续的，没有需要接续的声音。
    pub fn reset(&mut self) {
        self.finish_ramp();
        for channel in &mut self.channels {
            channel.reset();
        }
        self.primed = false;
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
    let w = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sample_rate.max(1.0));
    let (cos1, sin1) = (w.cos(), w.sin());
    let (cos2, sin2) = ((2.0 * w).cos(), (2.0 * w).sin());
    let num_re = coeffs.b0 + coeffs.b1 * cos1 + coeffs.b2 * cos2;
    let num_im = -(coeffs.b1 * sin1 + coeffs.b2 * sin2);
    let den_re = 1.0 + coeffs.a1 * cos1 + coeffs.a2 * cos2;
    let den_im = -(coeffs.a1 * sin1 + coeffs.a2 * sin2);
    let num = (num_re * num_re + num_im * num_im).sqrt();
    let den = (den_re * den_re + den_im * den_im).sqrt();
    (20.0 * (num / den.max(1.0e-12)).log10()) as f32
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

    /// L-5：退化采样率（nyquist - 1 < 1）不得触发 f32::clamp 的 min>max panic。
    #[test]
    fn degenerate_sample_rate_does_not_panic() {
        for rate in [0.0, 1.0, 2.0, 3.9] {
            let _ = BiquadCoeffs::design(&peaking(1_000.0, 6.0, 1.0), rate);
            let mut eq = Equalizer::default();
            eq.configure(0.0, &[peaking(1_000.0, 6.0, 1.0)], rate, 2);
            let mut samples = vec![0.5, -0.5];
            eq.process_interleaved(&mut samples, 2);
            assert!(samples.iter().all(|s| s.is_finite()));
        }
    }

    /// L-6：极端有限增益必须被限幅到 ±24 dB，系数恒为有限值。
    #[test]
    fn extreme_gain_is_clamped_to_finite_coeffs() {
        let coeffs = BiquadCoeffs::design(&peaking(1_000.0, 1.0e30, 1.0), 48_000.0);
        for c in [coeffs.b0, coeffs.b1, coeffs.b2, coeffs.a1, coeffs.a2] {
            assert!(c.is_finite(), "系数必须有限，实际 {c}");
        }
        // 限幅后与显式 +24 dB 完全一致
        let reference = BiquadCoeffs::design(&peaking(1_000.0, MAX_EQ_GAIN_DB, 1.0), 48_000.0);
        assert_eq!(coeffs, reference);
    }

    /// L-6：preamp 与频段增益同口径限幅。
    #[test]
    fn extreme_preamp_is_clamped() {
        let mut eq = Equalizer::default();
        eq.configure(1.0e30, &[], 48_000.0, 1);
        let mut samples = vec![1.0];
        eq.process_interleaved(&mut samples, 1);
        let expected = 10.0_f32.powf(MAX_EQ_GAIN_DB / 20.0);
        assert!(
            (samples[0] - expected).abs() < 1e-2,
            "preamp 应被钳到 +24dB（≈{expected}），实际输出 {}",
            samples[0]
        );
    }

    /// CMD-01：后端自己给频段数量封顶，不依赖前端消毒。
    #[test]
    fn band_count_is_capped() {
        let bands = vec![peaking(1_000.0, 1.0, 1.0); MAX_EQ_BANDS * 10];
        let mut eq = Equalizer::default();
        eq.configure(0.0, &bands, 48_000.0, 2);
        assert_eq!(eq.coeffs.len(), MAX_EQ_BANDS);
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

    /// DSP-02：192 kHz 下 30 Hz、Q 8、+12 dB 的峰化——实测中心增益要贴合设计值。
    /// 单精度系数在这里的偏差是 0.1 dB 量级，双精度应在 0.01 dB 以内。
    #[test]
    fn dsp02_low_frequency_high_q_band_matches_design_at_high_sample_rate() {
        let rate = 192_000.0_f32;
        let band = peaking(30.0, 12.0, 8.0);
        let coeffs = BiquadCoeffs::design(&band, rate);
        // 极点半径紧贴单位圆：f32 表示不了 a2 与 1 的差的有效数字
        assert!(coeffs.a2 < 1.0 && 1.0 - coeffs.a2 < 1.0e-3);
        assert_ne!(coeffs.a2, f64::from(coeffs.a2 as f32));

        let mut eq = Equalizer::default();
        eq.configure(0.0, &[band], rate, 1);
        // 30 Hz 正弦跑 8 秒，量最后 1 秒（整 30 个周期）的 RMS
        let total = 192_000 * 8;
        let mut samples: Vec<f32> = (0..total)
            .map(|n| {
                (0.1 * (2.0 * std::f64::consts::PI * 30.0 * n as f64 / 192_000.0).sin()) as f32
            })
            .collect();
        eq.process_interleaved(&mut samples, 1);
        let tail = &samples[total - 192_000..];
        let rms =
            (tail.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        let measured_db = 20.0 * (rms / (0.1 / std::f64::consts::SQRT_2)).log10();
        assert!(
            (measured_db - 12.0).abs() < 0.01,
            "measured {measured_db} dB"
        );
    }

    fn sine(freq: f64, rate: f64, start: usize, frames: usize) -> Vec<f32> {
        (start..start + frames)
            .map(|n| (0.25 * (2.0 * std::f64::consts::PI * freq * n as f64 / rate).sin()) as f32)
            .collect()
    }

    /// DSP-03：播放中改增益，系数在约 20 ms 内逐帧走到新值，不是一步硬切；
    /// 走完后精确等于目标系数。
    #[test]
    fn dsp03_reconfigure_while_playing_ramps_coefficients() {
        let rate = 48_000.0_f32;
        let before = peaking(1_000.0, 0.0, 1.0);
        let after = peaking(1_000.0, 12.0, 1.0);
        let old = BiquadCoeffs::design(&before, rate);
        let target = BiquadCoeffs::design(&after, rate);
        let frames = coeff_ramp_frames(rate) as usize;
        assert_eq!(frames, 960);

        let mut eq = Equalizer::default();
        eq.configure(0.0, &[before], rate, 1);
        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 0, 480), 1);
        eq.configure(0.0, &[after], rate, 1);
        assert_eq!(eq.coeffs[0], old, "重新配置的瞬间系数还没动");
        assert!(!eq.is_identity());

        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 480, frames / 2), 1);
        let halfway = eq.coeffs[0].b0;
        let expected = (old.b0 + target.b0) / 2.0;
        assert!(
            (halfway - expected).abs() < 1e-9,
            "走到一半时系数应在中点：{halfway} vs {expected}"
        );

        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 960, frames / 2), 1);
        assert_eq!(eq.coeffs[0], target, "过渡结束后精确落在目标系数");
        assert_eq!(eq.ramp_remaining, 0);
    }

    /// DSP-03：预放大同样渐变——直流输入下输出没有阶跃（硬切时是 0.375 的一步）。
    #[test]
    fn dsp03_preamp_change_has_no_step() {
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[peaking(1_000.0, 0.0, 1.0)], 48_000.0, 1);
        let mut warmup = vec![0.5_f32; 480];
        eq.process_interleaved(&mut warmup, 1);
        eq.configure(-12.0, &[peaking(1_000.0, 0.0, 1.0)], 48_000.0, 1);
        let mut block = vec![0.5_f32; 2_400];
        eq.process_interleaved(&mut block, 1);

        let mut previous = warmup[479];
        let mut largest_step = 0.0_f32;
        for sample in &block {
            largest_step = largest_step.max((sample - previous).abs());
            previous = *sample;
        }
        assert!(largest_step < 1e-3, "最大阶跃 {largest_step}");
        let settled = 0.5 * 10.0_f32.powf(-12.0 / 20.0);
        assert!((block[2_399] - settled).abs() < 1e-6);
    }

    /// DSP-03：增加一个频段时，已有频段的滤波状态原样保留——新频段从直通起步，
    /// 所以重新配置后的头几个样本与「什么都没改」的参照输出几乎一样。
    /// （此前频段数量一变就把所有状态清零，谐振中的低频高 Q 频段会咔哒一声。）
    #[test]
    fn dsp03_adding_a_band_keeps_existing_filter_state() {
        let rate = 48_000.0_f32;
        let resonant = peaking(100.0, 12.0, 8.0);
        let mut eq = Equalizer::default();
        let mut reference = Equalizer::default();
        eq.configure(0.0, &[resonant], rate, 1);
        reference.configure(0.0, &[resonant], rate, 1);
        let mut warmup = sine(100.0, 48_000.0, 0, 48_000);
        let mut warmup_reference = warmup.clone();
        eq.process_interleaved(&mut warmup, 1);
        reference.process_interleaved(&mut warmup_reference, 1);

        eq.configure(0.0, &[resonant, peaking(4_000.0, 6.0, 1.0)], rate, 1);
        let mut head = sine(100.0, 48_000.0, 48_000, 8);
        let mut head_reference = head.clone();
        eq.process_interleaved(&mut head, 1);
        reference.process_interleaved(&mut head_reference, 1);
        for (actual, expected) in head.iter().zip(&head_reference) {
            assert!(
                (actual - expected).abs() < 1e-3,
                "状态被清零了：{actual} vs {expected}"
            );
        }
        assert_eq!(eq.coeffs.len(), 2);
    }

    /// DSP-03：去掉频段时，多出来的尾部滤波级先渐变回直通，过渡结束才丢弃；
    /// 全部去掉后回到纯直通（上层可以旁路）。
    #[test]
    fn dsp03_removing_bands_ramps_them_out_then_drops_them() {
        let rate = 48_000.0_f32;
        let bands = [peaking(100.0, 9.0, 4.0), peaking(4_000.0, 6.0, 1.0)];
        let mut eq = Equalizer::default();
        eq.configure(0.0, &bands, rate, 2);
        let mut warmup: Vec<f32> = sine(100.0, 48_000.0, 0, 4_800)
            .into_iter()
            .flat_map(|sample| [sample, sample])
            .collect();
        eq.process_interleaved(&mut warmup, 2);

        eq.configure(0.0, &[], rate, 2);
        assert_eq!(eq.coeffs.len(), 2, "过渡期间滤波级还在");
        assert!(!eq.is_identity());
        let mut block: Vec<f32> = sine(100.0, 48_000.0, 4_800, 2_400)
            .into_iter()
            .flat_map(|sample| [sample, sample])
            .collect();
        let dry = block.clone();
        let mut previous = warmup[warmup.len() - 2];
        eq.process_interleaved(&mut block, 2);
        for frame in block.chunks(2) {
            // 100 Hz、幅度 0.25 的正弦相邻样本最多差 0.0033；留一倍余量给渐变本身
            assert!((frame[0] - previous).abs() < 0.02, "输出出现阶跃");
            previous = frame[0];
        }
        assert!(eq.is_identity());
        assert!(eq.coeffs.is_empty());
        assert_eq!(block[block.len() - 2..], dry[dry.len() - 2..]);
    }

    /// DSP-03：过渡途中再改一次，从当前值接着走（不跳回起点也不跳到旧目标）。
    #[test]
    fn dsp03_retarget_mid_ramp_continues_from_current_coefficients() {
        let rate = 48_000.0_f32;
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[peaking(1_000.0, 0.0, 1.0)], rate, 1);
        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 0, 64), 1);
        eq.configure(0.0, &[peaking(1_000.0, 12.0, 1.0)], rate, 1);
        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 64, 300), 1);
        let mid = eq.coeffs[0];

        eq.configure(0.0, &[peaking(1_000.0, -6.0, 1.0)], rate, 1);
        assert_eq!(eq.coeffs[0], mid);
        // 目标没变的重复配置不重启过渡
        let remaining = eq.ramp_remaining;
        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 364, 100), 1);
        eq.configure(0.0, &[peaking(1_000.0, -6.0, 1.0)], rate, 1);
        assert_eq!(eq.ramp_remaining, remaining - 100);

        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 464, 960), 1);
        assert_eq!(
            eq.coeffs[0],
            BiquadCoeffs::design(&peaking(1_000.0, -6.0, 1.0), rate)
        );
    }

    /// DSP-03：极端参数之间的过渡全程稳定（稳定区是凸的，线性插值不会越界）。
    #[test]
    fn dsp03_ramp_between_extreme_bands_stays_bounded() {
        let rate = 192_000.0_f32;
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[peaking(30.0, 24.0, 8.0)], rate, 1);
        eq.process_interleaved(&mut sine(30.0, 192_000.0, 0, 19_200), 1);
        let mut start = 19_200;
        for band in [
            peaking(18_000.0, -24.0, 0.3),
            EqBand {
                kind: BandKind::LowShelf,
                ..peaking(40.0, 24.0, 0.5)
            },
            EqBand {
                kind: BandKind::HighPass,
                ..peaking(20.0, 0.0, 8.0)
            },
            peaking(30.0, 24.0, 8.0),
        ] {
            eq.configure(0.0, &[band], rate, 1);
            let mut block = sine(30.0, 192_000.0, start, 9_600);
            start += 9_600;
            eq.process_interleaved(&mut block, 1);
            assert!(block
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() < 50.0));
        }
    }

    /// DSP-03：seek（`reset`）让进行中的过渡直接落到目标；之后的首次配置也直接生效。
    #[test]
    fn dsp03_reset_finishes_a_pending_ramp() {
        let rate = 48_000.0_f32;
        let mut eq = Equalizer::default();
        eq.configure(0.0, &[peaking(1_000.0, 0.0, 1.0)], rate, 1);
        eq.process_interleaved(&mut sine(1_000.0, 48_000.0, 0, 64), 1);
        eq.configure(-6.0, &[peaking(1_000.0, 12.0, 1.0)], rate, 1);
        assert!(eq.ramp_remaining > 0);
        eq.reset();
        assert_eq!(eq.ramp_remaining, 0);
        assert_eq!(
            eq.coeffs[0],
            BiquadCoeffs::design(&peaking(1_000.0, 12.0, 1.0), rate)
        );

        eq.configure(0.0, &[peaking(2_000.0, 3.0, 1.0)], rate, 1);
        assert_eq!(eq.ramp_remaining, 0, "seek 之后还没出声，配置直接生效");
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
