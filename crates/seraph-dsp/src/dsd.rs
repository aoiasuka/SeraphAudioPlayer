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

            for channel in 0..self.channels {
                output.push(frame[channel]);
                output.push(frame[self.channels + channel]);
                output.push(marker);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DsdToPcmConverter {
    channels: usize,
    decimation_bits: usize,
}

impl DsdToPcmConverter {
    pub fn new(channels: usize, decimation_bits: usize) -> Result<Self, DsdError> {
        if channels == 0 {
            return Err(DsdError::InvalidChannelCount);
        }
        if decimation_bits == 0 || !decimation_bits.is_multiple_of(8) {
            return Err(DsdError::InvalidInputLength);
        }

        Ok(Self {
            channels,
            decimation_bits,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl DsdConverter for DsdToPcmConverter {
    fn mode(&self) -> DsdMode {
        DsdMode::PcmConversion
    }

    fn convert(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), DsdError> {
        let dsd_bytes_per_pcm_frame = self.decimation_bits / 8;
        let input_stride = self.channels * dsd_bytes_per_pcm_frame;
        if !input.len().is_multiple_of(input_stride) {
            return Err(DsdError::InvalidInputLength);
        }

        for frame in input.chunks_exact(input_stride) {
            for channel in 0..self.channels {
                let mut sum = 0_i32;
                for byte_index in 0..dsd_bytes_per_pcm_frame {
                    let byte = frame[byte_index * self.channels + channel];
                    for bit in 0..8 {
                        if byte & (0x80 >> bit) != 0 {
                            sum += 1;
                        } else {
                            sum -= 1;
                        }
                    }
                }

                let scaled = ((sum as f32 / self.decimation_bits as f32) * 8_388_607.0) as i32;
                output.extend_from_slice(&scaled.to_le_bytes()[0..3]);
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

        assert_eq!(
            output,
            vec![0x11, 0x33, 0x05, 0x22, 0x44, 0x05, 0xaa, 0xcc, 0xfa, 0xbb, 0xdd, 0xfa]
        );
    }

    #[test]
    fn converts_dsd_bits_to_pcm24_bytes() {
        let mut converter = DsdToPcmConverter::new(1, 8).unwrap();
        let mut output = Vec::new();
        converter.convert(&[0xff, 0x00], &mut output).unwrap();

        assert_eq!(output.len(), 6);
        assert_eq!(&output[0..3], &[0xff, 0xff, 0x7f]);
        assert_eq!(&output[3..6], &[0x01, 0x00, 0x80]);
    }

    #[test]
    fn native_passthrough_copies_bytes() {
        let mut converter = NativeDsdPassthrough;
        let mut output = Vec::new();
        converter.convert(&[1, 2, 3], &mut output).unwrap();
        assert_eq!(output, vec![1, 2, 3]);
    }
}
