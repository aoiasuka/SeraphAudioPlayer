//! WASAPI Exclusive 模式占位实现。
//!
//! TODO:
//! - 接入 `windows` crate（Win32_Media_Audio + Win32_System_Com）
//! - 实现 IMMDeviceEnumerator 枚举设备
//! - IAudioClient::Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, ...)
//! - 处理 AUDCLNT_E_UNSUPPORTED_FORMAT 退到 SUPPORTED 表
//! - Device Lost 状态机：监听 IMMNotificationClient
//! - DSD DoP：未来模式；当前播放链路使用 DSD -> PCM Conversion

use crate::backend::{AudioBackend, BackendError, Result};
use crate::device::{AudioDevice, ShareMode};
use seraph_core::types::{BitDepth, Channels, SampleRate};

pub struct WasapiExclusive {
    current_device: Option<AudioDevice>,
    current_format: Option<(SampleRate, BitDepth, Channels)>,
}

impl WasapiExclusive {
    pub fn new() -> Self {
        Self {
            current_device: None,
            current_format: None,
        }
    }
}

impl Default for WasapiExclusive {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for WasapiExclusive {
    fn list_devices(&self) -> Result<Vec<AudioDevice>> {
        Err(BackendError::NotImplemented)
    }

    fn open(
        &mut self,
        _device: &AudioDevice,
        _share_mode: ShareMode,
        _sample_rate: SampleRate,
        _bit_depth: BitDepth,
        _channels: Channels,
    ) -> Result<()> {
        Err(BackendError::NotImplemented)
    }

    fn close(&mut self) -> Result<()> {
        Err(BackendError::NotImplemented)
    }

    fn play(&mut self) -> Result<()> {
        Err(BackendError::NotImplemented)
    }

    fn pause(&mut self) -> Result<()> {
        Err(BackendError::NotImplemented)
    }

    fn submit(&mut self, _samples: &[f32]) -> Result<usize> {
        Err(BackendError::NotImplemented)
    }

    fn current_device(&self) -> Option<&AudioDevice> {
        self.current_device.as_ref()
    }

    fn current_format(&self) -> Option<(SampleRate, BitDepth, Channels)> {
        self.current_format
    }
}
