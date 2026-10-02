use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

use crate::backend::{BackendError, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// 输出设备身份。REL-03：此前每个设备还调 `supported_output_configs()` 探测采样率 /
/// 位深 / 声道能力，而 IPC 只回传 id 与名称、探测结果直接丢弃——WASAPI 上逐个激活
/// 设备探测很慢，且会唤醒休眠的蓝牙 / USB 设备。能力判定在真正打开流时按文件格式做。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    #[serde(default, rename = "legacyIds")]
    pub legacy_ids: Vec<String>,
}

#[derive(Clone)]
struct CpalOutputDevice {
    index: usize,
    name: String,
    device: cpal::Device,
}

#[cfg(windows)]
#[derive(Clone)]
struct OutputDeviceIdentity {
    id: String,
    name: String,
    is_default: bool,
    legacy_ids: Vec<String>,
    name_occurrence: usize,
}

pub fn list_output_devices() -> Result<Vec<AudioDevice>> {
    platform_list_output_devices()
}

pub(crate) fn resolve_output_device_id(device_id: &str) -> Result<String> {
    platform_resolve_output_device_id(device_id)
}

pub(crate) fn output_device_by_id(device_id: &str) -> Result<cpal::Device> {
    platform_output_device_by_id(device_id)
}

#[cfg(windows)]
fn platform_list_output_devices() -> Result<Vec<AudioDevice>> {
    let cpal_devices = cpal_output_devices()?;
    let identities = windows_output_device_identities(&cpal_devices)?;
    if identities.is_empty() {
        return Err(BackendError::DeviceNotFound);
    }

    Ok(identities
        .into_iter()
        .map(|identity| AudioDevice {
            id: identity.id,
            name: identity.name,
            is_default: identity.is_default,
            legacy_ids: identity.legacy_ids,
        })
        .collect())
}

#[cfg(not(windows))]
fn platform_list_output_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device.name().ok());
    let devices = cpal_output_devices()?;

    let mut output = Vec::new();
    let mut seen = HashMap::<String, usize>::new();
    let mut default_assigned = false;
    for entry in devices {
        let base_id = legacy_hashed_device_id_for(&entry.name);
        let count = seen.entry(base_id.clone()).or_insert(0);
        let id = legacy_hashed_device_id_with_occurrence(&entry.name, *count);
        *count += 1;

        let is_default = !default_assigned && default_name.as_deref() == Some(entry.name.as_str());
        if is_default {
            default_assigned = true;
        }

        output.push(AudioDevice {
            id,
            is_default,
            legacy_ids: vec![legacy_index_device_id(entry.index, &entry.name)],
            name: entry.name,
        });
    }

    if output.is_empty() {
        return Err(BackendError::DeviceNotFound);
    }

    Ok(output)
}

#[cfg(windows)]
fn platform_resolve_output_device_id(device_id: &str) -> Result<String> {
    let cpal_devices = cpal_output_devices()?;
    let identities = windows_output_device_identities(&cpal_devices)?;
    find_windows_identity(&identities, device_id)
        .map(|identity| identity.id.clone())
        .ok_or(BackendError::DeviceNotFound)
}

#[cfg(not(windows))]
fn platform_resolve_output_device_id(device_id: &str) -> Result<String> {
    let devices = cpal_output_devices()?;
    let mut seen = HashMap::<String, usize>::new();
    for entry in devices {
        let base_id = legacy_hashed_device_id_for(&entry.name);
        let count = seen.entry(base_id.clone()).or_insert(0);
        let id = legacy_hashed_device_id_with_occurrence(&entry.name, *count);
        *count += 1;
        if id == device_id || legacy_index_device_id(entry.index, &entry.name) == device_id {
            return Ok(id);
        }
    }

    Err(BackendError::DeviceNotFound)
}

#[cfg(windows)]
fn platform_output_device_by_id(device_id: &str) -> Result<cpal::Device> {
    let cpal_devices = cpal_output_devices()?;
    let identities = windows_output_device_identities(&cpal_devices)?;
    let identity =
        find_windows_identity(&identities, device_id).ok_or(BackendError::DeviceNotFound)?;

    cpal_device_by_name_occurrence(&cpal_devices, &identity.name, identity.name_occurrence)
        .or_else(|| {
            cpal_devices
                .iter()
                .find(|entry| entry.name == identity.name)
        })
        .map(|entry| entry.device.clone())
        .ok_or(BackendError::DeviceNotFound)
}

#[cfg(not(windows))]
fn platform_output_device_by_id(device_id: &str) -> Result<cpal::Device> {
    let devices = cpal_output_devices()?;
    let mut seen = HashMap::<String, usize>::new();
    for entry in devices {
        let base_id = legacy_hashed_device_id_for(&entry.name);
        let count = seen.entry(base_id.clone()).or_insert(0);
        let id = legacy_hashed_device_id_with_occurrence(&entry.name, *count);
        *count += 1;
        if id == device_id || legacy_index_device_id(entry.index, &entry.name) == device_id {
            return Ok(entry.device);
        }
    }

    Err(BackendError::DeviceNotFound)
}

#[cfg(windows)]
fn windows_output_device_identities(
    cpal_devices: &[CpalOutputDevice],
) -> Result<Vec<OutputDeviceIdentity>> {
    use wasapi::Direction;

    let _ = wasapi::initialize_mta();
    let enumerator =
        wasapi::DeviceEnumerator::new().map_err(|err| BackendError::Internal(err.to_string()))?;
    let default_id = enumerator
        .get_default_device(&Direction::Render)
        .ok()
        .and_then(|device| device.get_id().ok());
    let collection = enumerator
        .get_device_collection(&Direction::Render)
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
    let count = collection
        .get_nbr_devices()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

    let mut identities = Vec::new();
    let mut seen_names = HashMap::<String, usize>::new();
    for index in 0..count {
        let device = collection
            .get_device_at_index(index)
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let id = device
            .get_id()
            .map_err(|err| BackendError::DeviceLost(err.to_string()))?;
        let name = device
            .get_friendlyname()
            .unwrap_or_else(|_| format!("Output Device {}", index + 1));
        let occurrence = {
            let count = seen_names.entry(name.clone()).or_insert(0);
            let occurrence = *count;
            *count += 1;
            occurrence
        };
        let cpal_index = cpal_device_by_name_occurrence(cpal_devices, &name, occurrence)
            .map(|entry| entry.index)
            .unwrap_or(index as usize);

        identities.push(OutputDeviceIdentity {
            is_default: default_id.as_deref() == Some(id.as_str()),
            legacy_ids: legacy_device_ids(&name, occurrence, cpal_index),
            id,
            name,
            name_occurrence: occurrence,
        });
    }

    Ok(identities)
}

#[cfg(windows)]
fn find_windows_identity<'a>(
    identities: &'a [OutputDeviceIdentity],
    device_id: &str,
) -> Option<&'a OutputDeviceIdentity> {
    identities
        .iter()
        .find(|identity| exact_device_id_matches(identity, device_id))
        .or_else(|| {
            let slug = legacy_index_device_id_slug(device_id)?;
            let mut matches = identities
                .iter()
                .filter(|identity| sanitize_device_id(&identity.name) == slug);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
}

#[cfg(windows)]
fn exact_device_id_matches(identity: &OutputDeviceIdentity, device_id: &str) -> bool {
    identity.id == device_id || identity.legacy_ids.iter().any(|id| id == device_id)
}

fn cpal_output_devices() -> Result<Vec<CpalOutputDevice>> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|err| BackendError::DeviceLost(err.to_string()))?;

    Ok(devices
        .enumerate()
        .map(|(index, device)| {
            let name = device
                .name()
                .unwrap_or_else(|_| format!("Output Device {}", index + 1));
            CpalOutputDevice {
                index,
                name,
                device,
            }
        })
        .collect())
}

fn cpal_device_by_name_occurrence<'a>(
    devices: &'a [CpalOutputDevice],
    name: &str,
    occurrence: usize,
) -> Option<&'a CpalOutputDevice> {
    devices
        .iter()
        .filter(|entry| entry.name == name)
        .nth(occurrence)
}

fn legacy_device_ids(name: &str, occurrence: usize, enum_index: usize) -> Vec<String> {
    let mut ids = vec![
        legacy_hashed_device_id_with_occurrence(name, occurrence),
        legacy_index_device_id(enum_index, name),
    ];
    ids.sort();
    ids.dedup();
    ids
}

fn legacy_hashed_device_id_with_occurrence(name: &str, occurrence: usize) -> String {
    let base_id = legacy_hashed_device_id_for(name);
    if occurrence == 0 {
        base_id
    } else {
        format!("{base_id}-{occurrence}")
    }
}

fn legacy_hashed_device_id_for(name: &str) -> String {
    let sanitized = sanitize_device_id(name);
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let hash = hasher.finish();
    format!("cpal:{hash:016x}:{sanitized}")
}

fn legacy_index_device_id(index: usize, name: &str) -> String {
    format!("cpal:{index}:{}", sanitize_device_id(name))
}

fn legacy_index_device_id_slug(device_id: &str) -> Option<&str> {
    let rest = device_id.strip_prefix("cpal:")?;
    let (index, slug) = rest.split_once(':')?;
    (!index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit()) && !slug.is_empty())
        .then_some(slug)
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
    fn keeps_legacy_device_ids_for_migration() {
        assert_eq!(sanitize_device_id("USB DAC (WASAPI)"), "usb-dac-wasapi");
        assert_eq!(
            legacy_index_device_id(2, "USB DAC (WASAPI)"),
            "cpal:2:usb-dac-wasapi"
        );
        assert_eq!(
            legacy_index_device_id_slug("cpal:2:usb-dac-wasapi"),
            Some("usb-dac-wasapi")
        );
        assert_eq!(legacy_index_device_id_slug("cpal:abc:usb-dac"), None);

        let id_a = legacy_hashed_device_id_for("USB DAC (WASAPI)");
        let id_b = legacy_hashed_device_id_for("USB DAC (WASAPI)");
        assert_eq!(id_a, id_b);
        assert_ne!(
            legacy_hashed_device_id_for("USB DAC (WASAPI)"),
            legacy_hashed_device_id_for("Built-in Output")
        );
        assert!(id_a.starts_with("cpal:"));
        assert!(id_a.ends_with(":usb-dac-wasapi"));
        assert_eq!(
            legacy_hashed_device_id_with_occurrence("USB DAC (WASAPI)", 1),
            format!("{id_a}-1")
        );
    }
}
