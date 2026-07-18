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
    // 直接声与混入声的能量归一化，避免整体响度抬升
    direct_gain: f32,
    cross_gain: f32,
}

impl Crossfeed {
    pub fn configure(&mut self, settings: &CrossfeedSettings, sample_rate: f32) {
        self.enabled = settings.enabled;
        let amount = settings.amount.clamp(0.0, 1.0);
        self.amount = amount;
        let cutoff = settings
            .cutoff_hz
            .clamp(100.0, sample_rate.max(2.0) * 0.5 - 1.0);
        // 一阶低通系数：alpha = 1 - exp(-2π fc / fs)
        let x = (-2.0 * std::f32::consts::PI * cutoff / sample_rate.max(1.0)).exp();
        self.alpha = (1.0 - x).clamp(0.0, 1.0);
        // 归一化：direct + cross = 1，保持单位增益
        self.cross_gain = amount * 0.5;
        self.direct_gain = 1.0 - self.cross_gain;
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
            let cross_l = self.lp_right.process(self.alpha, right);
            let cross_r = self.lp_left.process(self.alpha, left);
            frame[0] = left * self.direct_gain + cross_l * self.cross_gain;
            frame[1] = right * self.direct_gain + cross_r * self.cross_gain;
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
