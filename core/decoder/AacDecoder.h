// =============================================================================
//  core/decoder/AacDecoder.h
//
//  AAC ADTS / LATM 流解码器接口。具体实现需要第三方库 (fdk-aac 等),
//  通过编译宏 APX_HAVE_AAC 切换:
//    - APX_HAVE_AAC=1 → 真实实现 (cpp 中编写 fdk_aac wrapper)
//    - 未定义        → 桩实现,open() 返回 false 提示未编入支持
//
//  使用与 Mp3Decoder/VorbisDecoder 同构。
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class AacDecoder final : public IDecoder {
public:
    AacDecoder();
    ~AacDecoder() override;

    AacDecoder(const AacDecoder&)            = delete;
    AacDecoder& operator=(const AacDecoder&) = delete;

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
