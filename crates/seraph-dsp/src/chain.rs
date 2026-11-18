//! DSP 链：把参数 EQ 与 crossfeed 串成一条可配置的后处理链。
//!
//! [`DspSettings`] 是前端 / IPC / 引擎三方共享的配置契约（serde camelCase，
//! 与前端 TS 结构一一对应）。[`DspProcessor`] 持有运行状态（滤波器寄存器），
//! 按 settings 重建系数但保留状态，实现连续播放中的热更新。
//!
//! 处理顺序：preamp+EQ → crossfeed。运行在解码线程（非实时回调）。

use crate::crossfeed::{Crossfeed, CrossfeedSettings};
use crate::eq::{EqBand, Equalizer};
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

/// DSP 链运行处理器：跨 packet 保持滤波器状态。
#[derive(Debug, Default)]
pub struct DspProcessor {
    equalizer: Equalizer,
    crossfeed: Crossfeed,
    sample_rate: f32,
    channels: usize,
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
            self.crossfeed
                .configure(&settings.crossfeed, self.sample_rate);
            self.crossfeed_active = self.crossfeed.is_active();
        } else {
            // DSP-03：关总开关也走过渡——EQ 渐变回直通，过渡走完整条链才零成本旁路。
            // 还没出过声时（起播前）配置直接生效，等同于原先的立即旁路。
            self.equalizer
                .configure(0.0, &[], self.sample_rate, channels);
            self.crossfeed_active = false;
        }
    }

    /// 整条链是否需要处理（否则上层可 zero-copy 跳过）。坑 5。
    /// EQ 一侧按实时状态判断：渐变回直通的过渡没走完之前仍算活跃。
    pub fn is_active(&self) -> bool {
        !self.equalizer.is_identity() || self.crossfeed_active
    }

    /// 就地处理一段交错样本。链不活跃时零成本返回。
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        if !self.equalizer.is_identity() {
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

    /// DSP-03：播放中关掉 DSP 总开关不硬切——输出从处理后的电平渐变回原始电平，
    /// 过渡走完后整条链才旁路，且旁路后逐位直通。
    #[test]
    fn dsp03_disabling_the_chain_mid_playback_ramps_back_to_passthrough() {
        let enabled = DspSettings {
            enabled: true,
            preamp: -12.0,
            ..DspSettings::default()
        };
        let mut proc = DspProcessor::default();
        proc.configure(&enabled, 48_000.0, 1);
        let mut warmup = vec![0.5_f32; 480];
        proc.process(&mut warmup, 1);
        let attenuated = warmup[479];
        assert!((attenuated - 0.5 * 10.0_f32.powf(-12.0 / 20.0)).abs() < 1e-6);

        proc.configure(&DspSettings::default(), 48_000.0, 1);
        assert!(proc.is_active(), "过渡没走完之前不能旁路");
        let mut block = vec![0.5_f32; 4_800];
        proc.process(&mut block, 1);

        // 第一个样本紧挨着关闭前的电平；全程没有大于千分之一满幅的阶跃
        assert!((block[0] - attenuated).abs() < 1e-3);
        let largest_step = block
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(largest_step < 1e-3, "最大阶跃 {largest_step}");
        assert_eq!(block[4_799], 0.5, "过渡结束后逐位直通");
        assert!(!proc.is_active());
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
