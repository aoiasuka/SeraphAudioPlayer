// =============================================================================
//  core/format/AudioFormat.h
//
//  音频流格式描述。覆盖 PCM 与"PCM-packed DSD over PCM" 两类需求。
//  本结构体不依赖任何 Windows / 第三方头文件,可作为模块间的纯数据 DTO。
// =============================================================================
#pragma once

#include <cstdint>
#include <string>

namespace apx {

// PCM 采样数据类型。
enum class SampleType : std::uint8_t {
    Int16,          // 16-bit signed PCM (little-endian)
    Int24Packed,    // 24-bit signed PCM,3 字节紧凑排布
    Int32,          // 32-bit container;若 valid_bits < 32(如 24-bit-in-32),右对齐
    Float32,        // IEEE 754 单精度
    DsdLsb8,        // 原生 DSD:每字节装 8 个 1-bit 样本,LSB 在时间上靠前
                    // (与 DSF 文件 storage 一致)。bits_per_sample 应为 8。
                    // 仅可经 WASAPI KSDATAFORMAT_SUBTYPE_DSD 或 ASIO native DSD
                    // 路径输出;走 DoP 时 decoder 输出 Int24Packed,不用这个枚举。
};

// AudioFormat
//   不可变值类型。比较与哈希按所有字段计算。
//
//   常见组合:
//     - CD : pcm16(44100, 2)
//     - Hi-Res 24-bit FLAC : pcm24in32(96000, 2)
//     - DAC 通用浮点 : float32(192000, 2)
struct AudioFormat {
    std::uint32_t sample_rate     = 0;   // Hz,如 44100 / 96000 / 192000
    std::uint16_t channels        = 0;   // 1, 2, 4, 6, 8 ...
    std::uint16_t bits_per_sample = 0;   // 容器位宽:16 / 24 / 32
    std::uint16_t valid_bits      = 0;   // 有效位数,<= bits_per_sample
    SampleType    sample_type     = SampleType::Int16;
    std::uint32_t channel_mask    = 0;   // Windows SPEAKER_* 位掩码,0 表示按通道数推导

    // 基本校验
    bool valid() const noexcept;

    // 单样本字节数(container)
    std::uint32_t bytes_per_sample() const noexcept { return bits_per_sample / 8u; }

    // 单帧字节数 = channels * bytes_per_sample
    std::uint32_t frame_bytes() const noexcept {
        return static_cast<std::uint32_t>(channels) * bytes_per_sample();
    }

    // 每秒字节数(用于估算缓冲区容量)
    std::uint32_t bytes_per_second() const noexcept {
        return sample_rate * frame_bytes();
    }

    bool operator==(const AudioFormat& o) const noexcept;
    bool operator!=(const AudioFormat& o) const noexcept { return !(*this == o); }

    // 调试用文本
    std::wstring to_wstring() const;

    // ------- 工厂(覆盖最常见 90% 场景) -------
    static AudioFormat pcm16     (std::uint32_t sr, std::uint16_t ch);
    static AudioFormat pcm24in32 (std::uint32_t sr, std::uint16_t ch); // 24-bit-in-32 container
    static AudioFormat pcm24     (std::uint32_t sr, std::uint16_t ch); // 24-bit packed (3-byte)
    static AudioFormat int32     (std::uint32_t sr, std::uint16_t ch);
    static AudioFormat float32   (std::uint32_t sr, std::uint16_t ch);
};

// 默认通道掩码(channel_mask == 0 时使用):
//   mono → FRONT_CENTER
//   stereo → FRONT_LEFT | FRONT_RIGHT
//   其它 → 0(交给 WASAPI 默认)
std::uint32_t default_channel_mask(std::uint16_t channels) noexcept;

} // namespace apx
