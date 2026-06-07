use seraph_core::types::{BitDepth, Channels, SampleRate};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::hash::{Hash, Hasher};

use crate::backend::{BackendError, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait},
    SampleFormat,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShareMode {
    Exclusive,
    Shared,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub capabilities: DeviceCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    pub sample_rates: Vec<SampleRate>,
    pub bit_depths: Vec<BitDepth>,
    pub max_channels: Channels,
    pub supports_exclusive: bool,
    pub supports_dsd_dop: bool,
    pub supports_dsd_native: bool,
}

pub fn list_output_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device.name().ok());
    let devices = host
        .output_devices()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

    let mut output = Vec::new();
    let mut id_dedupe = std::collections::HashMap::<String, usize>::new();
    // default 设备只标记一次：第一个名字匹配的设备
    let mut default_assigned = false;
    for (index, device) in devices.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| format!("Output Device {}", index + 1));
        let capabilities = capabilities_from_device(&device);
        let mut id = device_id_for(&name);
        // 极少数情况下两个设备同名 sanitize 后相同；附加序号避免撞 key
        let count = id_dedupe.entry(id.clone()).or_insert(0);
        if *count > 0 {
            id = format!("{id}-{count}");
        }
        *count += 1;
        let is_default = !default_assigned && default_name.as_deref() == Some(name.as_str());
        if is_default {
            default_assigned = true;
        }
        output.push(AudioDevice {
            id,
            is_default,
            name,
            capabilities,
        });
    }

    if output.is_empty() {
        return Err(BackendError::DeviceNotFound);
    }

    Ok(output)
}

pub(crate) fn output_device_by_id(device_id: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

    let mut seen = std::collections::HashMap::<String, usize>::new();
    for (index, device) in devices.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| format!("Output Device {}", index + 1));
        let base_id = device_id_for(&name);
        let count = seen.entry(base_id.clone()).or_insert(0);
        let candidate_id = if *count == 0 {
            base_id.clone()
        } else {
            format!("{base_id}-{count}")
        };
        *count += 1;
        if candidate_id == device_id {
            return Ok(device);
        }
    }

    Err(BackendError::DeviceNotFound)
}

fn capabilities_from_device(device: &cpal::Device) -> DeviceCapabilities {
    let mut sample_rates = BTreeSet::new();
    let mut bit_depths = BTreeSet::new();
    let mut max_channels = 2_u16;

    if let Ok(configs) = device.supported_output_configs() {
        for config in configs {
            max_channels = max_channels.max(config.channels());
            for rate in
                sample_rates_from_range(config.min_sample_rate().0, config.max_sample_rate().0)
            {
                sample_rates.insert(rate);
            }
            if let Some(bits) = bit_depth_from_sample_format(config.sample_format()) {
                bit_depths.insert(bits);
            }
        }
    }

    DeviceCapabilities {
        sample_rates: sample_rates.into_iter().map(SampleRate).collect(),
        bit_depths: bit_depths.into_iter().map(BitDepth).collect(),
        max_channels: Channels(max_channels),
        supports_exclusive: cfg!(windows),
        supports_dsd_dop: false,
        supports_dsd_native: false,
    }
}

fn sample_rates_from_range(min: u32, max: u32) -> Vec<u32> {
    const COMMON: [u32; 9] = [
        44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 768_000,
    ];
    let mut rates: Vec<u32> = COMMON
        .into_iter()
        .filter(|rate| (min..=max).contains(rate))
        .collect();

    if rates.is_empty() {
        rates.push(min);
        if max != min {
            rates.push(max);
        }
    }

    rates
}

fn bit_depth_from_sample_format(format: SampleFormat) -> Option<u16> {
    match format {
        SampleFormat::I8 | SampleFormat::U8 => Some(8),
        SampleFormat::I16 | SampleFormat::U16 => Some(16),
        SampleFormat::I32 | SampleFormat::U32 | SampleFormat::F32 => Some(32),
        SampleFormat::I64 | SampleFormat::U64 | SampleFormat::F64 => Some(64),
        _ => None,
    }
}

fn device_id_for(name: &str) -> String {
    // 旧实现 `cpal:{enum-index}:{name}` 在设备增删后枚举顺序变化时
    // 会让持久化的 device_id 失效。改用 name hash 作为稳定主键。
    let sanitized = sanitize_device_id(name);
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish();
    format!("cpal:{hash:016x}:{sanitized}")
}

fn sanitize_device_id(name: &str) -> String {
    let mut id = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch.to_ascii_lowercase());
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }

    id.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_common_rates_inside_device_range() {
        assert_eq!(
            sample_rates_from_range(44_100, 96_000),
            vec![44_100, 48_000, 88_200, 96_000]
        );
    }

    #[test]
    fn falls_back_to_min_max_when_common_rates_are_absent() {
        assert_eq!(sample_rates_from_range(8_000, 16_000), vec![8_000, 16_000]);
    }

    #[test]
    fn sanitizes_device_names_for_stable_ids() {
        assert_eq!(sanitize_device_id("USB DAC (WASAPI)"), "usb-dac-wasapi");
        // 同一个 name 永远产出相同 id
        let id_a = device_id_for("USB DAC (WASAPI)");
        let id_b = device_id_for("USB DAC (WASAPI)");
        assert_eq!(id_a, id_b);
        // 不同 name 不同 id
        assert_ne!(
            device_id_for("USB DAC (WASAPI)"),
            device_id_for("Built-in Output")
        );
        // id 形如 cpal:<hex>:<sanitized>
        assert!(id_a.starts_with("cpal:"));
        assert!(id_a.ends_with(":usb-dac-wasapi"));
    }
}
