//! 声学分析引擎（声学分析页数据源）。
//!
//! 输入：渲染线程 tap 旁路出来的交错 PCM（drain 批次）。
//! 输出：五个仪表面板所需的全部标量与数组：
//! - K 加权响度 M / S / I / LRA（ITU-R BS.1770-4 / EBU R128 语义，
//!   400ms / 3s 窗，100ms hop，绝对 -70 LUFS + 相对 -10 LU 门限积分）
//! - 真峰估计（Catmull-Rom 4x 内插近似，非全规格多相 FIR，标注 ≈）
//! - 每声道瞬时 Peak / RMS（弹道学交给前端）
//! - 立体声相关度与抽取散点（声场仪）
//!
//! 仅分析前两个声道（面板均为立体声语义）；单声道复制为双声道。

use std::collections::VecDeque;
use std::f64::consts::PI;

/// 100ms hop：BS.1770 的 400ms 门限块 = 4 hop，短期 3s = 30 hop。
const HOPS_MOMENTARY: usize = 4;
const HOPS_SHORT_TERM: usize = 30;
/// 短期响度每 10 hop（1s）记一次样本，供 LRA 分布统计。
const HOPS_PER_LRA_SAMPLE: usize = 10;
/// 门限块历史上限（100ms 一块 ≈ 5.5 小时），超出丢最旧，积分退化为"最近窗口"。
const MAX_GATING_BLOCKS: usize = 200_000;
const MAX_LRA_SAMPLES: usize = 20_000;
/// 声场散点最多保留的样本对数。
const MAX_SCATTER_PAIRS: usize = 160;
/// 绝对门限 -70 LUFS 对应的块能量。
const ABSOLUTE_GATE_LUFS: f64 = -70.0;

/// 转置直接 II 型双二阶，f64 状态保证低频高 Q 滤波器数值稳定。
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// BS.1770 K 加权第一级：高架滤波（头部声学补偿）。
/// 参数取规格的模拟原型，经 RBJ 双线性设计适配任意采样率。
fn k_weight_shelf(sample_rate: f64) -> Biquad {
    let f0 = 1_681.974_450_955_533;
    let gain_db = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * f0 / sample_rate;
    let (sin_w0, cos_w0) = w0.sin_cos();
    let alpha = sin_w0 / (2.0 * q);
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
    Biquad {
        b0: (a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha)) / a0,
        b1: (-2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0,
        b2: (a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha)) / a0,
        a1: (2.0 * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0,
        a2: ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0,
        z1: 0.0,
        z2: 0.0,
    }
}

/// BS.1770 K 加权第二级：高通（RLB 加权）。
fn k_weight_highpass(sample_rate: f64) -> Biquad {
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let w0 = 2.0 * PI * f0 / sample_rate;
    let (sin_w0, cos_w0) = w0.sin_cos();
    let alpha = sin_w0 / (2.0 * q);

    let a0 = 1.0 + alpha;
    Biquad {
        b0: ((1.0 + cos_w0) / 2.0) / a0,
        b1: (-(1.0 + cos_w0)) / a0,
        b2: ((1.0 + cos_w0) / 2.0) / a0,
        a1: (-2.0 * cos_w0) / a0,
        a2: (1.0 - alpha) / a0,
        z1: 0.0,
        z2: 0.0,
    }
}

#[inline]
fn energy_to_lufs(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.max(1.0e-12).log10()
}

#[inline]
fn lufs_to_energy(lufs: f64) -> f64 {
    10.0_f64.powf((lufs + 0.691) / 10.0)
}

/// 快照：一次 IPC 轮询带走的全部面板数据。`None` = 数据还不足（前端显示 --）。
#[derive(Debug, Clone, Default)]
pub struct AnalysisSnapshot {
    pub momentary_lufs: Option<f32>,
    pub short_term_lufs: Option<f32>,
    pub integrated_lufs: Option<f32>,
    pub loudness_range_lu: Option<f32>,
    /// Catmull-Rom 4x 内插近似 dBTP
    pub true_peak_db: Option<f32>,
    pub true_peak_max_db: Option<f32>,
    /// 线性幅度（0..1+），弹道学在前端做
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
    pub correlation: f32,
    /// 交错 L,R 抽取样本对（声场散点），最多 [`MAX_SCATTER_PAIRS`] 对
    pub scatter: Vec<f32>,
}

/// 声学分析引擎：喂样本、出快照。
#[derive(Debug)]
pub struct AnalysisEngine {
    sample_rate: u32,
    channels: usize,
    /// 前两声道各一条 K 加权链
    filters: [[Biquad; 2]; 2],
    hop_len: usize,
    /// 当前 hop 内累计的 K 加权平方和（声道求和）与帧计数
    hop_energy_accum: f64,
    hop_frames: usize,
    /// 最近 hop 的均方能量（声道求和），容量 = 短期窗 30 个
    recent_hops: VecDeque<f64>,
    /// 门限块能量（400ms 块 @ 100ms hop），积分用
    gating_blocks: Vec<f64>,
    /// 短期响度采样（1 个/秒），LRA 用
    lra_samples: Vec<f64>,
    hops_since_lra_sample: usize,
    /// 真峰内插的跨批次延续窗（前两声道各 3 个历史样本）
    tp_history: [[f32; 3]; 2],
    true_peak_linear: Option<f32>,
    true_peak_max_linear: Option<f32>,
    /// 最近一批的电平与立体声量
    peak: [f32; 2],
    rms: [f32; 2],
    correlation: f32,
    scatter: Vec<f32>,
}

impl AnalysisEngine {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let sample_rate = sample_rate.max(8_000);
        let fs = f64::from(sample_rate);
        let chain = [k_weight_shelf(fs), k_weight_highpass(fs)];
        Self {
            sample_rate,
            channels: channels.max(1),
            filters: [chain, chain],
            hop_len: (sample_rate as usize / 10).max(1),
            hop_energy_accum: 0.0,
            hop_frames: 0,
            recent_hops: VecDeque::with_capacity(HOPS_SHORT_TERM),
            gating_blocks: Vec::new(),
            lra_samples: Vec::new(),
            hops_since_lra_sample: 0,
            tp_history: [[0.0; 3]; 2],
            true_peak_linear: None,
            true_peak_max_linear: None,
            peak: [0.0; 2],
            rms: [0.0; 2],
            correlation: 0.0,
            scatter: Vec::new(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// 换曲目：清空积分 / LRA / 真峰会话最大值与滑窗，滤波器结构保留。
    pub fn reset_session(&mut self) {
        for chain in &mut self.filters {
            for stage in chain {
                stage.reset();
            }
        }
        self.hop_energy_accum = 0.0;
        self.hop_frames = 0;
        self.recent_hops.clear();
        self.gating_blocks.clear();
        self.lra_samples.clear();
        self.hops_since_lra_sample = 0;
        self.tp_history = [[0.0; 3]; 2];
        self.true_peak_linear = None;
        self.true_peak_max_linear = None;
        self.peak = [0.0; 2];
        self.rms = [0.0; 2];
        self.correlation = 0.0;
        self.scatter.clear();
    }

    /// 喂一批交错样本（渲染 tap drain 的结果）。
    pub fn push(&mut self, interleaved: &[f32]) {
        let channels = self.channels;
        let frames = interleaved.len() / channels;
        if frames == 0 {
            return;
        }

        let mut peak = [0.0_f32; 2];
        let mut sq_sum = [0.0_f64; 2];
        let mut cross_sum = 0.0_f64;
        let mut block_tp = 0.0_f32;

        for frame in 0..frames {
            let offset = frame * channels;
            let left = interleaved[offset];
            let right = if channels > 1 {
                interleaved[offset + 1]
            } else {
                left
            };
            let pair = [left, right];

            for (ch, &sample) in pair.iter().enumerate() {
                let abs = sample.abs();
                if abs > peak[ch] {
                    peak[ch] = abs;
                }
                sq_sum[ch] += f64::from(sample) * f64::from(sample);

                // 真峰：Catmull-Rom 在最近 4 样本窗内查 3 个内插点
                let [p0, p1, p2] = self.tp_history[ch];
                block_tp = block_tp.max(abs).max(catmull_rom_peak(p0, p1, p2, sample));
                self.tp_history[ch] = [p1, p2, sample];

                // K 加权能量（响度）
                let mut weighted = f64::from(sample);
                for stage in &mut self.filters[ch] {
                    weighted = stage.process(weighted);
                }
                self.hop_energy_accum += weighted * weighted;
            }
            cross_sum += f64::from(left) * f64::from(right);

            self.hop_frames += 1;
            if self.hop_frames >= self.hop_len {
                self.finish_hop();
            }
        }

        // 电平 / 相关度 / 真峰（本批口径）
        self.peak = peak;
        self.rms = [
            (sq_sum[0] / frames as f64).sqrt() as f32,
            (sq_sum[1] / frames as f64).sqrt() as f32,
        ];
        let denom = (sq_sum[0] * sq_sum[1]).sqrt();
        self.correlation = if denom > 1.0e-12 {
            (cross_sum / denom).clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        self.true_peak_linear = Some(block_tp);
        self.true_peak_max_linear = Some(self.true_peak_max_linear.unwrap_or(0.0).max(block_tp));

        // 声场散点：等距抽取 ≤160 对
        let stride = frames.div_ceil(MAX_SCATTER_PAIRS).max(1);
        self.scatter.clear();
        let mut frame = 0;
        while frame < frames {
            let offset = frame * channels;
            let left = interleaved[offset];
            let right = if channels > 1 {
                interleaved[offset + 1]
            } else {
                left
            };
            self.scatter.push(left);
            self.scatter.push(right);
            frame += stride;
        }
    }

    /// 一个 100ms hop 结束：结算均方能量，推进 M/S/I/LRA 的窗与历史。
    fn finish_hop(&mut self) {
        // 均方能量按"逐声道均方后求和"口径：Σ样本² / 帧数（双声道已在累计时求和）
        let hop_energy = self.hop_energy_accum / self.hop_frames as f64;
        self.hop_energy_accum = 0.0;
        self.hop_frames = 0;

        if self.recent_hops.len() >= HOPS_SHORT_TERM {
            self.recent_hops.pop_front();
        }
        self.recent_hops.push_back(hop_energy);

        // 门限块（400ms = 最近 4 hop 的均值，75% 重叠）
        if self.recent_hops.len() >= HOPS_MOMENTARY {
            let block: f64 = self
                .recent_hops
                .iter()
                .rev()
                .take(HOPS_MOMENTARY)
                .sum::<f64>()
                / HOPS_MOMENTARY as f64;
            if self.gating_blocks.len() >= MAX_GATING_BLOCKS {
                self.gating_blocks.remove(0);
            }
            self.gating_blocks.push(block);
        }

        // LRA 短期采样：每 1s 记一次短期响度
        self.hops_since_lra_sample += 1;
        if self.hops_since_lra_sample >= HOPS_PER_LRA_SAMPLE
            && self.recent_hops.len() >= HOPS_SHORT_TERM
        {
            self.hops_since_lra_sample = 0;
            let short_term = self.recent_hops.iter().sum::<f64>() / self.recent_hops.len() as f64;
            if self.lra_samples.len() >= MAX_LRA_SAMPLES {
                self.lra_samples.remove(0);
            }
            self.lra_samples.push(short_term);
        }
    }

    fn momentary(&self) -> Option<f64> {
        if self.recent_hops.len() < HOPS_MOMENTARY {
            return None;
        }
        let energy = self
            .recent_hops
            .iter()
            .rev()
            .take(HOPS_MOMENTARY)
            .sum::<f64>()
            / HOPS_MOMENTARY as f64;
        Some(energy_to_lufs(energy))
    }

    fn short_term(&self) -> Option<f64> {
        if self.recent_hops.is_empty() {
            return None;
        }
        let energy = self.recent_hops.iter().sum::<f64>() / self.recent_hops.len() as f64;
        Some(energy_to_lufs(energy))
    }

    /// 门限积分响度（BS.1770-4）：绝对 -70 LUFS 门 → 相对 -10 LU 门 → 均值。
    fn integrated(&self) -> Option<f64> {
        let abs_gate = lufs_to_energy(ABSOLUTE_GATE_LUFS);
        let above_abs: Vec<f64> = self
            .gating_blocks
            .iter()
            .copied()
            .filter(|energy| *energy > abs_gate)
            .collect();
        if above_abs.is_empty() {
            return None;
        }
        let mean_abs = above_abs.iter().sum::<f64>() / above_abs.len() as f64;
        let rel_gate = lufs_to_energy(energy_to_lufs(mean_abs) - 10.0);
        let gated: Vec<f64> = above_abs
            .into_iter()
            .filter(|energy| *energy > rel_gate)
            .collect();
        if gated.is_empty() {
            return None;
        }
        Some(energy_to_lufs(
            gated.iter().sum::<f64>() / gated.len() as f64,
        ))
    }

    /// 响度范围 LRA（EBU Tech 3342）：短期分布，绝对 -70 门 + 相对 -20 门，P95 - P10。
    fn loudness_range(&self) -> Option<f64> {
        let abs_gate = lufs_to_energy(ABSOLUTE_GATE_LUFS);
        let above_abs: Vec<f64> = self
            .lra_samples
            .iter()
            .copied()
            .filter(|energy| *energy > abs_gate)
            .collect();
        if above_abs.len() < 2 {
            return None;
        }
        let mean_abs = above_abs.iter().sum::<f64>() / above_abs.len() as f64;
        let rel_gate = lufs_to_energy(energy_to_lufs(mean_abs) - 20.0);
        let mut gated: Vec<f64> = above_abs
            .into_iter()
            .filter(|energy| *energy > rel_gate)
            .collect();
        if gated.len() < 2 {
            return None;
        }
        gated.sort_by(|a, b| a.partial_cmp(b).expect("finite energies"));
        let p10 = percentile(&gated, 0.10);
        let p95 = percentile(&gated, 0.95);
        Some(energy_to_lufs(p95) - energy_to_lufs(p10))
    }

    pub fn snapshot(&self) -> AnalysisSnapshot {
        let to_db = |linear: f32| 20.0 * linear.max(1.0e-7).log10();
        AnalysisSnapshot {
            momentary_lufs: self.momentary().map(|value| value as f32),
            short_term_lufs: self.short_term().map(|value| value as f32),
            integrated_lufs: self.integrated().map(|value| value as f32),
            loudness_range_lu: self.loudness_range().map(|value| value as f32),
            true_peak_db: self.true_peak_linear.map(to_db),
            true_peak_max_db: self.true_peak_max_linear.map(to_db),
            peak_left: self.peak[0],
            peak_right: self.peak[1],
            rms_left: self.rms[0],
            rms_right: self.rms[1],
            correlation: self.correlation,
            scatter: self.scatter.clone(),
        }
    }
}

/// 已排序切片的线性插值分位数。
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let position = fraction * (sorted.len() - 1) as f64;
    let index = position.floor() as usize;
    let next = (index + 1).min(sorted.len() - 1);
    let weight = position - index as f64;
    sorted[index] * (1.0 - weight) + sorted[next] * weight
}

/// Catmull-Rom 样条在 p1..p2 段内 3 个四分点的最大绝对值（真峰内插近似）。
fn catmull_rom_peak(p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let (p0, p1, p2, p3) = (f64::from(p0), f64::from(p1), f64::from(p2), f64::from(p3));
    let mut max_abs = 0.0_f64;
    for t in [0.25_f64, 0.5, 0.75] {
        let t2 = t * t;
        let t3 = t2 * t;
        let value = 0.5
            * ((2.0 * p1)
                + (-p0 + p2) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
        max_abs = max_abs.max(value.abs());
    }
    max_abs as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: u32 = 48_000;

    fn stereo_sine(freq: f64, amplitude: f64, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(FS)) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for n in 0..frames {
            let value = (amplitude * (2.0 * PI * freq * n as f64 / f64::from(FS)).sin()) as f32;
            samples.push(value);
            samples.push(value);
        }
        samples
    }

    #[test]
    fn ebu_reference_sine_reads_minus_23_lufs() {
        // EBU Tech 3341 测例 1：997Hz、-23dBFS 立体声正弦 → M/S/I 均为 -23 LUFS。
        let mut engine = AnalysisEngine::new(FS, 2);
        let amplitude = 10.0_f64.powf(-23.0 / 20.0);
        engine.push(&stereo_sine(997.0, amplitude, 5.0));

        let snapshot = engine.snapshot();
        let momentary = snapshot.momentary_lufs.expect("momentary after 5s");
        let short_term = snapshot.short_term_lufs.expect("short-term after 5s");
        let integrated = snapshot.integrated_lufs.expect("integrated after 5s");
        assert!((momentary - -23.0).abs() < 0.5, "M={momentary}");
        assert!((short_term - -23.0).abs() < 0.5, "S={short_term}");
        assert!((integrated - -23.0).abs() < 0.5, "I={integrated}");
    }

    #[test]
    fn integrated_gating_ignores_silence() {
        // 响段之后接长静音：门限积分不应被静音拉低。
        let mut engine = AnalysisEngine::new(FS, 2);
        let amplitude = 10.0_f64.powf(-23.0 / 20.0);
        engine.push(&stereo_sine(997.0, amplitude, 4.0));
        let loud_only = engine.snapshot().integrated_lufs.expect("integrated");

        engine.push(&stereo_sine(997.0, 0.0, 8.0));
        let with_silence = engine.snapshot().integrated_lufs.expect("integrated");
        assert!(
            (with_silence - loud_only).abs() < 0.3,
            "静音后积分漂移过大: {loud_only} -> {with_silence}"
        );
    }

    #[test]
    fn true_peak_catches_intersample_overshoot() {
        // fs/4 正弦相位偏 45°：采样点最大 0.707，真峰应显著高于采样峰。
        let mut engine = AnalysisEngine::new(FS, 2);
        let frames = FS as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for n in 0..frames {
            let value = (0.9 * (PI / 2.0 * n as f64 + PI / 4.0).sin()) as f32;
            samples.push(value);
            samples.push(value);
        }
        engine.push(&samples);

        let snapshot = engine.snapshot();
        let sample_peak = snapshot.peak_left;
        let true_peak_db = snapshot.true_peak_db.expect("true peak");
        let sample_peak_db = 20.0 * sample_peak.log10();
        assert!((sample_peak - 0.9 * 0.707_f32).abs() < 0.01);
        assert!(
            true_peak_db > sample_peak_db + 1.0,
            "真峰 {true_peak_db} 应明显高于采样峰 {sample_peak_db}"
        );
    }

    #[test]
    fn correlation_reads_plus_one_for_mono_and_minus_one_for_inverted() {
        let mut engine = AnalysisEngine::new(FS, 2);
        engine.push(&stereo_sine(440.0, 0.5, 0.2));
        assert!(engine.snapshot().correlation > 0.99);

        let mut inverted = AnalysisEngine::new(FS, 2);
        let frames = (0.2 * f64::from(FS)) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for n in 0..frames {
            let value = (0.5 * (2.0 * PI * 440.0 * n as f64 / f64::from(FS)).sin()) as f32;
            samples.push(value);
            samples.push(-value);
        }
        inverted.push(&samples);
        assert!(inverted.snapshot().correlation < -0.99);
    }

    #[test]
    fn loudness_range_spreads_between_quiet_and_loud_passages() {
        // 前 12s 安静（-33dBFS）+ 后 12s 响（-13dBFS）→ LRA 应接近 20 LU。
        let mut engine = AnalysisEngine::new(FS, 2);
        engine.push(&stereo_sine(997.0, 10.0_f64.powf(-33.0 / 20.0), 12.0));
        engine.push(&stereo_sine(997.0, 10.0_f64.powf(-13.0 / 20.0), 12.0));

        let lra = engine.snapshot().loudness_range_lu.expect("LRA after 24s");
        assert!(
            (f64::from(lra) - 20.0).abs() < 3.0,
            "两段相差 20dB 的信号 LRA 应≈20 LU，实得 {lra}"
        );
    }

    #[test]
    fn reset_session_clears_integrated_and_true_peak_max() {
        let mut engine = AnalysisEngine::new(FS, 2);
        engine.push(&stereo_sine(997.0, 0.3, 2.0));
        assert!(engine.snapshot().true_peak_max_db.is_some());

        engine.reset_session();
        let snapshot = engine.snapshot();
        assert!(snapshot.integrated_lufs.is_none());
        assert!(snapshot.true_peak_max_db.is_none());
        assert!(snapshot.momentary_lufs.is_none());
    }

    #[test]
    fn mono_input_duplicates_into_both_channels() {
        let mut engine = AnalysisEngine::new(FS, 1);
        let frames = 4_800;
        let samples: Vec<f32> = (0..frames)
            .map(|n| (0.4 * (2.0 * PI * 440.0 * n as f64 / f64::from(FS)).sin()) as f32)
            .collect();
        engine.push(&samples);

        let snapshot = engine.snapshot();
        assert!((snapshot.peak_left - snapshot.peak_right).abs() < 1.0e-6);
        assert!(snapshot.correlation > 0.99);
        assert_eq!(snapshot.scatter.len() % 2, 0);
        assert!(!snapshot.scatter.is_empty());
    }
}
