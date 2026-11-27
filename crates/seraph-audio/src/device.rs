use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// 枚举到的一个输出设备。设备句柄的类型是参数：生产代码里是 `cpal::Device`，
/// 测试里换成普通值，匹配逻辑不依赖真实硬件就能测。
#[derive(Clone)]
struct OutputEntry<D> {
    index: usize,
    name: String,
    device: D,
}

type CpalOutputDevice = OutputEntry<cpal::Device>;

/// 一次枚举解析出来的输出设备：规范 ID（Windows 上是 WASAPI 端点 ID）加对应的 cpal 设备。
#[derive(Clone)]
pub(crate) struct ResolvedOutputDevice {
    pub(crate) id: String,
    pub(crate) device: cpal::Device,
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

/// 把前端给的设备 ID（当前的端点 ID，或旧版本存下来的 ID）解析成规范 ID 和设备。
///
/// DEV-01：此前这是两个函数（`resolve_output_device_id` 与 `output_device_by_id`），各自都把
/// cpal 设备和 WASAPI 端点完整枚举一遍，而引擎总是成对调用——切换一次设备要枚举 3～5 轮。
/// 有蓝牙或休眠的 USB 设备时枚举本来就慢。现在一次枚举同时给出两样。
pub(crate) fn resolve_output_device(device_id: &str) -> Result<ResolvedOutputDevice> {
    platform_resolve_output_device(device_id)
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
fn platform_resolve_output_device(device_id: &str) -> Result<ResolvedOutputDevice> {
    let cpal_devices = cpal_output_devices()?;
    let identities = windows_output_device_identities(&cpal_devices)?;
    let (id, device) = resolve_among(&identities, &cpal_devices, device_id)?;
    Ok(ResolvedOutputDevice { id, device })
}

/// 在已经枚举好的两份清单里找 `device_id` 对应的设备：先定位 WASAPI 端点（按端点 ID、
/// 旧版 ID，或旧版「序号 + 名称」ID 里唯一对得上的名称），再按「名称 + 同名出现次序」
/// 找到 cpal 那边的同一个设备；次序对不上时退而取第一个同名的。
#[cfg(windows)]
fn resolve_among<D: Clone>(
    identities: &[OutputDeviceIdentity],
    devices: &[OutputEntry<D>],
    device_id: &str,
) -> Result<(String, D)> {
    let identity =
        find_windows_identity(identities, device_id).ok_or(BackendError::DeviceNotFound)?;
    cpal_device_by_name_occurrence(devices, &identity.name, identity.name_occurrence)
        .or_else(|| devices.iter().find(|entry| entry.name == identity.name))
        .map(|entry| (identity.id.clone(), entry.device.clone()))
        .ok_or(BackendError::DeviceNotFound)
}

#[cfg(not(windows))]
fn platform_resolve_output_device(device_id: &str) -> Result<ResolvedOutputDevice> {
    let devices = cpal_output_devices()?;
    let mut seen = HashMap::<String, usize>::new();
    for entry in devices {
        let base_id = legacy_hashed_device_id_for(&entry.name);
        let count = seen.entry(base_id.clone()).or_insert(0);
        let id = legacy_hashed_device_id_with_occurrence(&entry.name, *count);
        *count += 1;
        if id == device_id || legacy_index_device_id(entry.index, &entry.name) == device_id {
            return Ok(ResolvedOutputDevice {
                id,
                device: entry.device,
            });
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
            OutputEntry {
                index,
                name,
                device,
            }
        })
        .collect())
}

fn cpal_device_by_name_occurrence<'a, D>(
    devices: &'a [OutputEntry<D>],
    name: &str,
    occurrence: usize,
) -> Option<&'a OutputEntry<D>> {
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
    // LIB-01：旧版本把这个 ID 存进了前端设置，算法必须固定才能继续认出来
    let hash = seraph_core::stable_hash::hash_str(name);
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
        // LIB-01：旧设置里存的就是这个值，哈希算法变了它也不能变
        assert_eq!(id_a, "cpal:a6c87d21c470e6a0:usb-dac-wasapi");
        assert_eq!(
            legacy_hashed_device_id_with_occurrence("USB DAC (WASAPI)", 1),
            format!("{id_a}-1")
        );
    }

    #[cfg(windows)]
    fn identity(
        id: &str,
        name: &str,
        occurrence: usize,
        enum_index: usize,
    ) -> OutputDeviceIdentity {
        OutputDeviceIdentity {
            id: id.into(),
            name: name.into(),
            is_default: false,
            legacy_ids: legacy_device_ids(name, occurrence, enum_index),
            name_occurrence: occurrence,
        }
    }

    #[cfg(windows)]
    fn entry(index: usize, name: &str, device: &'static str) -> OutputEntry<&'static str> {
        OutputEntry {
            index,
            name: name.into(),
            device,
        }
    }

    /// DEV-01：一次查找同时给出规范 ID 与设备。清单是手写的，不碰真实硬件。
    #[cfg(windows)]
    #[test]
    fn one_lookup_yields_the_canonical_id_and_the_matching_device() {
        let identities = [
            identity("{ep-speakers}", "Speakers", 0, 0),
            identity("{ep-dac-a}", "USB DAC", 0, 1),
            identity("{ep-dac-b}", "USB DAC", 1, 2),
        ];
        // cpal 那边的枚举顺序与 WASAPI 不同
        let devices = [
            entry(0, "USB DAC", "dac-first"),
            entry(1, "Speakers", "speakers"),
            entry(2, "USB DAC", "dac-second"),
        ];
        let resolve = |id: &str| resolve_among(&identities, &devices, id).ok();

        // 端点 ID
        assert_eq!(
            resolve("{ep-speakers}"),
            Some(("{ep-speakers}".into(), "speakers"))
        );
        // 两台同名设备按「同名出现次序」各自对应
        assert_eq!(
            resolve("{ep-dac-a}"),
            Some(("{ep-dac-a}".into(), "dac-first"))
        );
        assert_eq!(
            resolve("{ep-dac-b}"),
            Some(("{ep-dac-b}".into(), "dac-second"))
        );
        // 旧版本存下来的 ID 解析成现在的端点 ID
        let legacy_hashed = legacy_hashed_device_id_with_occurrence("USB DAC", 1);
        assert_eq!(
            resolve(&legacy_hashed),
            Some(("{ep-dac-b}".into(), "dac-second"))
        );
        assert_eq!(
            resolve(&legacy_index_device_id(0, "Speakers")),
            Some(("{ep-speakers}".into(), "speakers"))
        );
        // 旧版「序号 + 名称」ID 的序号已经对不上：名称唯一时仍能认出来，同名的有歧义不猜
        assert_eq!(
            resolve("cpal:9:speakers"),
            Some(("{ep-speakers}".into(), "speakers"))
        );
        assert_eq!(resolve("cpal:9:usb-dac"), None);
        assert_eq!(resolve("{ep-gone}"), None);
        assert_eq!(resolve(""), None);
    }

    #[cfg(windows)]
    #[test]
    fn lookup_falls_back_to_the_first_same_named_device_and_fails_without_one() {
        // WASAPI 报了第二台同名设备，cpal 只枚举到一台：退而取那一台
        let identities = [identity("{ep-dac-b}", "USB DAC", 1, 5)];
        let devices = [entry(0, "USB DAC", "only-dac")];
        assert_eq!(
            resolve_among(&identities, &devices, "{ep-dac-b}").ok(),
            Some(("{ep-dac-b}".into(), "only-dac"))
        );
        // cpal 那边完全没有这个名字：整个解析失败，而不是只给出 ID
        let devices = [entry(0, "Headphones", "headphones")];
        assert!(matches!(
            resolve_among(&identities, &devices, "{ep-dac-b}"),
            Err(BackendError::DeviceNotFound)
        ));
    }

    /// 本机有输出设备时的实机核对：列出来的每个设备都能按自己的 ID 解析回同一个 ID。
    /// 没有音频设备的环境（CI）里枚举会失败，直接跳过。
    #[test]
    fn listed_devices_resolve_back_to_their_own_ids() {
        let Ok(devices) = list_output_devices() else {
            eprintln!("skipping: no output devices in this environment");
            return;
        };
        for device in devices {
            match resolve_output_device(&device.id) {
                Ok(resolved) => assert_eq!(resolved.id, device.id, "{}", device.name),
                // 两次枚举之间设备被拔掉是可能的，其它错误不该出现
                Err(BackendError::DeviceNotFound) => {}
                Err(err) => panic!("{}: {err}", device.name),
            }
        }
    }
}
