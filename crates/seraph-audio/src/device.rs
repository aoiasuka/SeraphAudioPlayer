use seraph_core::types::{BitDepth, Channels, SampleRate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    for (index, device) in devices.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| format!("Output Device {}", index + 1));
        let capabilities = capabilities_from_device(&device);
        output.push(AudioDevice {
            id: device_id_for(index, &name),
            is_default: default_name.as_deref() == Some(name.as_str()),
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

    for (index, device) in devices.enumerate() {
        let name = device
            .name()
            .unwrap_or_else(|_| format!("Output Device {}", index + 1));
        if device_id_for(index, &name) == device_id {
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

fn device_id_for(index: usize, name: &str) -> String {
    format!("cpal:{index}:{}", sanitize_device_id(name))
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
        assert_eq!(
            device_id_for(2, "USB DAC (WASAPI)"),
            "cpal:2:usb-dac-wasapi"
        );
    }
}
