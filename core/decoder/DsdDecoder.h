// =============================================================================
//  core/decoder/DsdDecoder.h
//
//  DSD 解码器:输入 DSF / DFF 文件,输出 DoP (DSD over PCM 24-bit) 帧给
//  WASAPI 独占,DAC 端自动识别 0xFA/0x05 marker 字节后,把伪 PCM 还原为
//  原始 1-bit DSD 流播放。
//
//  当前实现:
//    - 支持 DSF (Sony 格式)。DFF 后续添加。
//    - 支持 DSD64 / DSD128 / DSD256 (任意倍数,只要 sampleFreq % 64 == 0)
//    - 输出格式: Int24Packed,采样率 = DSD_rate / 16
//        - DSD64  → 176400 Hz 24-bit
//        - DSD128 → 352800 Hz 24-bit
//        - DSD256 → 705600 Hz 24-bit
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class DsdDecoder final : public IDecoder {
public:
    DsdDecoder();
    ~DsdDecoder() override;

    DsdDecoder(const DsdDecoder&)            = delete;
    DsdDecoder& operator=(const DsdDecoder&) = delete;

    bool         open(const std::wstring& path) override;
    void         close() override;
    bool         isOpen() const override;
    AudioFormat  format() const override;
    std::int64_t totalFrames() const override;
    std::int64_t currentFrame() const override;
    bool         seek(std::int64_t frame) override;
    std::size_t  read(std::uint8_t* dst, std::size_t bytes) override;
    std::wstring lastError() const override;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
