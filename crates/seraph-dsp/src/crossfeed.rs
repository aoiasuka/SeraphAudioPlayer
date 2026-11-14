//! Crossfeed（串扰馈送）——耳机听音舒适化。
//!
//! 立体声录音本为音箱设计：左耳只听左声道、右耳只听右声道，硬声像分离在
//! 耳机上会造成"脑内定位"疲劳。crossfeed 把每个声道经一个低通 + 短延迟后
//! 混入对侧，模拟头部对对侧声音的自然遮蔽（近似 bs2b 的思路，简化为
//! 一阶低通 + 增益混合，零额外延迟以免影响 gapless 与进度对齐）。
//!
//! 仅对立体声（2 声道）生效；其它声道数直通。

use serde::{Deserialize, Serialize};

/// Crossfeed 参数。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossfeedSettings {
    pub enabled: bool,
    /// 混入对侧的强度 0..=1（0.3 ≈ bs2b 默认的适中值）
    pub amount: f32,
    /// 对侧信号低通截止频率（Hz），模拟头部遮蔽，典型 700–2000
    pub cutoff_hz: f32,
}

impl Default for CrossfeedSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            amount: 0.3,
            cutoff_hz: 700.0,
        }
    }
}

/// 一阶低通状态（每声道一个）。
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    z: f32,
}

impl OnePole {
    #[inline]
    fn process(&mut self, alpha: f32, input: f32) -> f32 {
        // y[n] = y[n-1] + alpha * (x[n] - y[n-1])
        self.z += alpha * (input - self.z);
        self.z
    }

    fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// Crossfeed 处理器。
#[derive(Debug, Clone, Default)]
pub struct Crossfeed {
    enabled: bool,
    amount: f32,
    alpha: f32,
    lp_left: OnePole,
    lp_right: OnePole,
    // 混入对侧的量；直达声减去同样多的「本侧低通」，见 process_interleaved
    cross_gain: f32,
}

impl Crossfeed {
    pub fn configure(&mut self, settings: &CrossfeedSettings, sample_rate: f32) {
        self.enabled = settings.enabled;
        // NaN 兜底：NaN.clamp 传播 NaN，会经 cross_gain / alpha 污染整条输出
        let amount = if settings.amount.is_finite() {
            settings.amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.amount = amount;
        // L-5（2026-08-16 审查）：上界先兜底到不小于下界——f32::clamp 在
        // min > max 时 panic（sample_rate < 202 触发）。这是 pub API，
        // 退化采样率不该炸掉调用方。
        let cutoff_hi = (sample_rate.max(2.0) * 0.5 - 1.0).max(100.0);
        let cutoff_raw = if settings.cutoff_hz.is_finite() {
            settings.cutoff_hz
        } else {
            CrossfeedSettings::default().cutoff_hz
        };
        let cutoff = cutoff_raw.clamp(100.0, cutoff_hi);
        // 一阶低通系数：alpha = 1 - exp(-2π fc / fs)
        let x = (-2.0 * std::f32::consts::PI * cutoff / sample_rate.max(1.0)).exp();
        self.alpha = (1.0 - x).clamp(0.0, 1.0);
        self.cross_gain = amount * 0.5;
    }

    pub fn is_active(&self) -> bool {
        self.enabled && self.amount > 1.0e-4
    }

    /// 就地处理立体声交错样本；非立体声或未启用时直接返回。
    pub fn process_interleaved(&mut self, samples: &mut [f32], channels: usize) {
        if !self.is_active() || channels != 2 {
            return;
        }
        for frame in samples.chunks_mut(2) {
            if frame.len() < 2 {
                break;
            }
            let left = frame[0];
            let right = frame[1];
            // 对侧信号先经低通（模拟头部遮蔽高频）
            let low_r = self.lp_right.process(self.alpha, right);
            let low_l = self.lp_left.process(self.alpha, left);
            // EQ4-04：直达声 = 原信号 − g·低通(本侧)，对侧 = g·低通(对侧)。
            // 直达路径因此是一个与对侧低通互补的高架（低频 1−g、高频 1），
            // 左右相同的居中内容（人声）两项正好抵消，总响应全频段平直。
            // 旧写法「直达 ×(1−g) + 对侧低通 ×g」在高频只剩 1−g，居中声像会变暗
            // （默认强度约 −1.4 dB，强度拉满约 −6 dB）。
            frame[0] = left + (low_r - low_l) * self.cross_gain;
            frame[1] = right + (low_l - low_r) * self.cross_gain;
        }
    }

    /// 坑 1：seek 后清零低通状态。
    pub fn reset(&mut self) {
        self.lp_left.reset();
        self.lp_right.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_crossfeed_is_passthrough() {
        let mut cf = Crossfeed::default();
        cf.configure(&CrossfeedSettings::default(), 48_000.0);
        assert!(!cf.is_active());
        let mut samples = vec![0.5, -0.5, 0.2, -0.2];
        let original = samples.clone();
        cf.process_interleaved(&mut samples, 2);
        assert_eq!(samples, original);
    }

    /// L-5：sample_rate < 202 时 clamp 上界会低于下界，修复前直接 panic。
    #[test]
    fn degenerate_sample_rate_does_not_panic() {
        for rate in [0.0, 1.0, 100.0, 201.0] {
            let mut cf = Crossfeed::default();
            cf.configure(
                &CrossfeedSettings {
                    enabled: true,
                    amount: 0.5,
                    cutoff_hz: 700.0,
                },
                rate,
            );
            let mut samples = vec![0.5, -0.5];
            cf.process_interleaved(&mut samples, 2);
            assert!(samples.iter().all(|s| s.is_finite()));
        }
    }

    /// NaN 参数不得把 NaN 灌进增益/滤波系数。
    #[test]
    fn non_finite_settings_fall_back_to_safe_values() {
        let mut cf = Crossfeed::default();
        cf.configure(
            &CrossfeedSettings {
                enabled: true,
                amount: f32::NAN,
                cutoff_hz: f32::INFINITY,
            },
            48_000.0,
        );
        let mut samples = vec![1.0, 0.0, 1.0, 0.0];
        cf.process_interleaved(&mut samples, 2);
        assert!(samples.iter().all(|s| s.is_finite()), "输出必须有限");
    }

    #[test]
    fn active_crossfeed_bleeds_between_channels() {
        let mut cf = Crossfeed::default();
        cf.configure(
            &CrossfeedSettings {
                enabled: true,
                amount: 0.5,
                cutoff_hz: 700.0,
            },
            48_000.0,
        );
        assert!(cf.is_active());
        // 硬左信号：处理后右声道应出现非零能量
        let mut samples = vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        cf.process_interleaved(&mut samples, 2);
        let right_energy: f32 = samples.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
        assert!(right_energy > 0.0, "crossfeed 应把左声道混入右声道");
        assert!(samples.iter().all(|s| s.is_finite()));
    }

    /// EQ4-04：左右相同的居中内容经过 crossfeed 后必须原样输出（任何频率都不变暗）。
    #[test]
    fn centered_content_stays_flat_at_all_frequencies() {
        for amount in [0.3, 1.0] {
            let mut cf = Crossfeed::default();
            cf.configure(
                &CrossfeedSettings {
                    enabled: true,
                    amount,
                    cutoff_hz: 700.0,
                },
                48_000.0,
            );
            // 100 Hz + 12 kHz 的居中信号
            let mut samples = Vec::new();
            for n in 0..4800 {
                let t = n as f32 / 48_000.0;
                let value = 0.4 * (std::f32::consts::TAU * 100.0 * t).sin()
                    + 0.4 * (std::f32::consts::TAU * 12_000.0 * t).sin();
                samples.push(value);
                samples.push(value);
            }
            let original = samples.clone();
            cf.process_interleaved(&mut samples, 2);
            for (out, input) in samples.iter().zip(original.iter()) {
                assert!((out - input).abs() < 1.0e-5, "居中内容应保持不变");
            }
        }
    }

    /// 硬左的高频不混入对侧（头部遮蔽），直达声保持满幅。
    #[test]
    fn hard_panned_high_frequency_keeps_direct_level() {
        let mut cf = Crossfeed::default();
        cf.configure(
            &CrossfeedSettings {
                enabled: true,
                amount: 1.0,
                cutoff_hz: 700.0,
            },
            48_000.0,
        );
        let mut samples = Vec::new();
        for n in 0..4800 {
            let t = n as f32 / 48_000.0;
            samples.push((std::f32::consts::TAU * 12_000.0 * t).sin());
            samples.push(0.0);
        }
        cf.process_interleaved(&mut samples, 2);
        let tail = &samples[4800..];
        let left_peak = tail.iter().step_by(2).fold(0.0_f32, |m, s| m.max(s.abs()));
        let right_peak = tail
            .iter()
            .skip(1)
            .step_by(2)
            .fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(left_peak > 0.9, "直达声高频不应被衰减，实际 {left_peak}");
        assert!(right_peak < 0.1, "高频不应大量混入对侧，实际 {right_peak}");
    }

    #[test]
    fn mono_signal_is_untouched_by_channel_count() {
        let mut cf = Crossfeed::default();
        cf.configure(
            &CrossfeedSettings {
                enabled: true,
                amount: 0.5,
                cutoff_hz: 700.0,
            },
            48_000.0,
        );
        // 非立体声（1 声道）直通
        let mut samples = vec![0.5, 0.6, 0.7];
        let original = samples.clone();
        cf.process_interleaved(&mut samples, 1);
        assert_eq!(samples, original);
    }

    #[test]
    fn settings_serde_roundtrip() {
        let settings = CrossfeedSettings {
            enabled: true,
            amount: 0.42,
            cutoff_hz: 900.0,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: CrossfeedSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings, back);
    }
}
