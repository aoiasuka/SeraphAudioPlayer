//! DSP 链：把参数 EQ 与 crossfeed 串成一条可配置的后处理链。
//!
//! [`DspSettings`] 是前端 / IPC / 引擎三方共享的配置契约（serde camelCase，
//! 与前端 TS 结构一一对应）。[`DspProcessor`] 持有运行状态（滤波器寄存器），
//! 按 settings 重建系数但保留状态，实现连续播放中的热更新。
//!
//! 处理顺序：preamp+EQ → crossfeed。运行在解码线程（非实时回调）。

use crate::crossfeed::{Crossfeed, CrossfeedSettings};
use crate::eq::{combined_response_db, EqBand, Equalizer};
use serde::{Deserialize, Serialize};

/// DSP 链的完整配置。持久化于前端，经 IPC 下发到引擎。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DspSettings {
    /// 总开关：false 时整条链零成本直通。
    pub enabled: bool,
    /// 预放大（dB）——大幅提升多段增益后防止削波的总衰减。
    pub preamp: f32,
    /// EQ 频段（顺序即级联顺序）。
    pub bands: Vec<EqBand>,
    /// Crossfeed 设置。
    pub crossfeed: CrossfeedSettings,
    /// 坑 6：EQ/DSP 是否对 DSD（已解码为 PCM）曲目生效。默认关——
    /// DSD 听众通常追求"原汁"，默认不加处理，由用户显式开启。
    pub apply_to_dsd: bool,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp: 0.0,
            bands: Vec::new(),
            crossfeed: CrossfeedSettings::default(),
            apply_to_dsd: false,
        }
    }
}

impl DspSettings {
    /// 计算整条 EQ 在给定频点的合成响应（dB），供曲线预览。
    pub fn response_db(&self, freq: f32, sample_rate: f32) -> f32 {
        combined_response_db(self.preamp, &self.bands, freq, sample_rate)
    }
}

/// DSP 链运行处理器：跨 packet 保持滤波器状态。
#[derive(Debug, Default)]
pub struct DspProcessor {
    equalizer: Equalizer,
    crossfeed: Crossfeed,
    sample_rate: f32,
    channels: usize,
    eq_active: bool,
    crossfeed_active: bool,
}

impl DspProcessor {
    /// 按 settings 重建系数（保留滤波器状态）。sample_rate/channels 变化时重置声道布局。
    pub fn configure(&mut self, settings: &DspSettings, sample_rate: f32, channels: usize) {
        self.sample_rate = sample_rate.max(1.0);
        self.channels = channels;

        if settings.enabled {
            self.equalizer
                .configure(settings.preamp, &settings.bands, self.sample_rate, channels);
            self.eq_active = !self.equalizer.is_identity();
            self.crossfeed
                .configure(&settings.crossfeed, self.sample_rate);
            self.crossfeed_active = self.crossfeed.is_active();
        } else {
            self.eq_active = false;
            self.crossfeed_active = false;
        }
    }

    /// 整条链是否需要处理（否则上层可 zero-copy 跳过）。坑 5。
    pub fn is_active(&self) -> bool {
        self.eq_active || self.crossfeed_active
    }

    /// 就地处理一段交错样本。链不活跃时零成本返回。
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        if self.eq_active {
            self.equalizer.process_interleaved(samples, channels);
        }
        if self.crossfeed_active {
            self.crossfeed.process_interleaved(samples, channels);
        }
    }

    /// 坑 1：seek 后清零全链滤波器状态。
    pub fn reset(&mut self) {
        self.equalizer.reset();
        self.crossfeed.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::BandKind;

    #[test]
    fn default_settings_disabled_and_inactive() {
        let settings = DspSettings::default();
        let mut proc = DspProcessor::default();
        proc.configure(&settings, 48_000.0, 2);
        assert!(!proc.is_active());

        let mut samples = vec![0.1, -0.1, 0.2, -0.2];
        let original = samples.clone();
        proc.process(&mut samples, 2);
        assert_eq!(samples, original, "禁用链必须 bit-exact 直通");
    }

    #[test]
    fn enabled_but_flat_is_inactive() {
        // enabled=true 但 EQ 空、preamp=0、crossfeed 关 → 仍然不活跃
        let settings = DspSettings {
            enabled: true,
            ..DspSettings::default()
        };
        let mut proc = DspProcessor::default();
        proc.configure(&settings, 48_000.0, 2);
        assert!(!proc.is_active());
    }

    #[test]
    fn active_eq_changes_signal() {
        let settings = DspSettings {
            enabled: true,
            preamp: 0.0,
            bands: vec![EqBand {
                kind: BandKind::Peaking,
                freq: 1_000.0,
                gain: 6.0,
                q: 1.0,
                enabled: true,
            }],
            ..DspSettings::default()
        };
        let mut proc = DspProcessor::default();
        proc.configure(&settings, 48_000.0, 2);
        assert!(proc.is_active());

        let mut samples: Vec<f32> = (0..512)
            .flat_map(|i| {
                let s = (i as f32 * 0.1).sin() * 0.4;
                [s, s]
            })
            .collect();
        let original = samples.clone();
        proc.process(&mut samples, 2);
        assert_ne!(samples, original);
        assert!(samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn settings_serde_roundtrip_camel_case() {
        let settings = DspSettings {
            enabled: true,
            preamp: -3.0,
            bands: vec![EqBand::default()],
            crossfeed: CrossfeedSettings {
                enabled: true,
                amount: 0.3,
                cutoff_hz: 700.0,
            },
            apply_to_dsd: true,
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("applyToDsd"), "字段应为 camelCase");
        assert!(json.contains("cutoffHz"));
        let back: DspSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings, back);
    }

    #[test]
    fn reset_between_seeks_keeps_output_deterministic() {
        let settings = DspSettings {
            enabled: true,
            bands: vec![EqBand {
                kind: BandKind::Peaking,
                freq: 500.0,
                gain: 9.0,
                q: 3.0,
                enabled: true,
            }],
            ..DspSettings::default()
        };
        let mut proc = DspProcessor::default();
        proc.configure(&settings, 48_000.0, 1);
        let mut a = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        proc.process(&mut a, 1);
        proc.reset();
        let mut b = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        proc.process(&mut b, 1);
        for (x, y) in a.iter().zip(b.iter()) {
            assert!((x - y).abs() < 1e-9);
        }
    }
}
