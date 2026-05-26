// =============================================================================
//  core/dsp/FormatConverter.h
//
//  在不同 AudioFormat 之间做实时转换 (供共享模式回退使用):
//    - 位深: Int16 / Int24Packed / Int32 / Float32 互转 (无 dither, 无 noise shape)
//    - 采样率: windowed-sinc 多相 FIR (PolyphaseResampler) — 比线性插值好得多;
//      旧的线性插值路径仍可通过 useHighQuality(false) 选择,作为低开销 fallback
//    - 通道: 必须相同;不做 up/down-mix
//
//  设计目标:替代实在搞不出独占模式时的"播不了" → 至少能听见。
//  在 Hi-Fi 场景,UI 应明示"已降级到共享模式"。
//
//  线程模型:一个 Converter 只能由一个线程持有调用 (内部相位 / 历史样本是状态)。
// =============================================================================
#pragma once

#include "core/format/AudioFormat.h"
#include "core/dsp/PolyphaseResampler.h"

#include <cstddef>
#include <cstdint>
#include <vector>

namespace apx {

class FormatConverter {
public:
    FormatConverter();
    ~FormatConverter();

    // 设置 src/dst 格式。channels 必须一致,否则返回 false。
    // 可重复调用(重置状态)。
    bool configure(const AudioFormat& src, const AudioFormat& dst);

    bool needsConversion() const noexcept { return active_; }

    // 高质量重采样开关。true (默认) → PolyphaseResampler;false → 线性插值。
    // 必须在 configure() 之前设置,否则当前会话不变。
    void setHighQuality(bool on) { high_quality_ = on; }
    bool highQuality() const     { return high_quality_; }

    // 量化 dither(仅在 dst 是 Int16 且 src 比 Int16 宽时生效)。
    // 默认 true。TPDF 噪声 + 一阶 noise shaping,显著降低低位区谐波失真。
    void setDither(bool on) { dither_ = on; }
    bool dither() const     { return dither_; }

    // 把 src 中的 src_frames 帧转换成 dst 帧,写到 dst。
    // 返回实际写入的"目的帧数"。
    // dst_capacity_frames 限制写入上限;source 多余的样本会被丢弃。
    std::size_t process(const std::uint8_t* src, std::size_t src_frames,
                        std::uint8_t*       dst, std::size_t dst_capacity_frames);

    void reset();

    const AudioFormat& src() const { return src_; }
    const AudioFormat& dst() const { return dst_; }

private:
    bool        active_       = false;
    bool        high_quality_ = true;
    bool        dither_       = true;
    AudioFormat src_{};
    AudioFormat dst_{};
    double      ratio_ = 1.0;          // dst_rate / src_rate
    // 线性插值路径状态
    double      phase_ = 0.0;
    // 高质量路径:多相 FIR
    PolyphaseResampler resampler_;
    // 临时浮点 buffer (避免 process 内每次分配)
    std::vector<float> src_f_;
    std::vector<float> dst_f_;
    // 同采样率 & 线性插值路径的逐帧 buffer——避免实时路径里 std::vector 临时分配
    std::vector<float> frame_buf_;   // 同采样率路径
    std::vector<float> linear_a_;    // 线性插值 a
    std::vector<float> linear_b_;    // 线性插值 b
    std::vector<float> linear_out_;  // 线性插值 out
    // Dither + 一阶 noise shaping 状态:每通道保留上一帧 quantization error
    std::vector<float> dither_err_;
    std::uint32_t      rng_state_ = 0x12345678u;     // xorshift32 状态

    void storeFrameWithDither(std::uint8_t* dst, const float* in_f);
};

} // namespace apx
