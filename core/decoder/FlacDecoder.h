// =============================================================================
//  core/decoder/FlacDecoder.h
//
//  封装 dr_flac(单头文件 FLAC 解码器)为 IDecoder。
//
//  位深映射策略(为最大化 DAC 兼容性):
//    源 16-bit FLAC  → Int16              (drflac_read_pcm_frames_s16)
//    源 24-bit FLAC  → Int32, valid=24     (s32 → 算术右移 8 落入低 24 位)
//    源 32-bit FLAC  → Int32, valid=32     (s32 直读)
//    源 20-bit FLAC  → Int32, valid=32     (按 32-bit 容器处理,损失很小)
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class FlacDecoder final : public IDecoder {
public:
    FlacDecoder();
    ~FlacDecoder() override;

    FlacDecoder(const FlacDecoder&)            = delete;
    FlacDecoder& operator=(const FlacDecoder&) = delete;

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
