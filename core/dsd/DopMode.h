// =============================================================================
//  core/dsd/DopMode.h
//
//  DoP (DSD over PCM) 标记字节策略。
//  DSF/DFF decoder 都用此枚举来决定写哪种 0xFA/0x05 交替序列。
// =============================================================================
#pragma once

#include <cstdint>

namespace apx {

enum class DopMarkerMode : std::uint8_t {
    // 默认:每个 PCM 帧使用同一 marker (帧间 0xFA <-> 0x05 交替);
    // DoP v1.1 spec 的常见做法,绝大多数现代 DAC 接受。
    PerFrame = 0,
    // 备选:每"样本"(每帧内每通道)各自交替;
    // 个别老 DAC 期望此布局,启用后通道间相位会被错开 1 个 PCM 样本。
    PerSample = 1,
};

} // namespace apx
