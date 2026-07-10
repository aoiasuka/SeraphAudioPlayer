use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsdMode {
    /// DSD over PCM —— 在 PCM 流里塞 DSD 标记字节，给 WASAPI 喂
    DoP,
    /// 原生 DSD —— ASIO 路径专用
    Native,
    /// fallback：DSD → PCM 转换后输出
    PcmConversion,
}

#[derive(Debug, Error)]
pub enum DsdError {
    #[error("DSD converter not implemented yet")]
    NotImplemented,
    #[error("mode {0:?} unsupported")]
    UnsupportedMode(DsdMode),
    #[error("invalid channel count")]
    InvalidChannelCount,
    #[error("invalid input length")]
    InvalidInputLength,
}

/// DSD 转换器 trait。
///
/// 输入原始 DSD 数据流，输出按 [`DsdMode`] 决定的目标格式。
pub trait DsdConverter: Send {
    fn mode(&self) -> DsdMode;
    fn convert(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), DsdError>;
}

#[derive(Debug, Clone)]
pub struct NativeDsdPassthrough;

impl DsdConverter for NativeDsdPassthrough {
    fn mode(&self) -> DsdMode {
        DsdMode::Native
    }

    fn convert(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), DsdError> {
        output.extend_from_slice(input);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DopConverter {
    channels: usize,
    marker: bool,
}

impl DopConverter {
    pub fn new(channels: usize) -> Result<Self, DsdError> {
        if channels == 0 {
            return Err(DsdError::InvalidChannelCount);
        }

        Ok(Self {
            channels,
            marker: false,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl DsdConverter for DopConverter {
    fn mode(&self) -> DsdMode {
        DsdMode::DoP
    }

    fn convert(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), DsdError> {
        let bytes_per_dop_frame = self.channels * 2;
        if !input.len().is_multiple_of(bytes_per_dop_frame) {
            return Err(DsdError::InvalidInputLength);
        }

        for frame in input.chunks_exact(bytes_per_dop_frame) {
            let marker = if self.marker { 0xfa } else { 0x05 };
            self.marker = !self.marker;

            // DoP 1.1：24-bit LE 样本中 bits 15..8 = 较早的 DSD 字节、
            // bits 7..0 = 较晚的字节 → 内存序为 [later, earlier, marker]。
            // （F-9：原实现两个数据字节对调，接真 DoP DAC 会输出满带宽噪声。）
            for channel in 0..self.channels {
                output.push(frame[self.channels + channel]);
                output.push(frame[channel]);
                output.push(marker);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_dop_frames_with_alternating_markers() {
        let mut converter = DopConverter::new(2).unwrap();
        let mut output = Vec::new();
        converter
            .convert(
                &[0x11, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd],
                &mut output,
            )
            .unwrap();

        // F-9：DoP 1.1 内存序 [较晚字节, 较早字节, marker]
        assert_eq!(
            output,
            vec![0x33, 0x11, 0x05, 0x44, 0x22, 0x05, 0xcc, 0xaa, 0xfa, 0xdd, 0xbb, 0xfa]
        );
    }

    #[test]
    fn native_passthrough_copies_bytes() {
        let mut converter = NativeDsdPassthrough;
        let mut output = Vec::new();
        converter.convert(&[1, 2, 3], &mut output).unwrap();
        assert_eq!(output, vec![1, 2, 3]);
    }
}
