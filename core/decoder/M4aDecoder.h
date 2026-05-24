// =============================================================================
//  core/decoder/M4aDecoder.h
//
//  M4A / MP4 容器内 AAC 解码器。具体实现需要 MP4 容器解析 (minimp4 等)
//  + AAC 解码 (fdk-aac);通过编译宏 APX_HAVE_M4A 切换。
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class M4aDecoder final : public IDecoder {
public:
    M4aDecoder();
    ~M4aDecoder() override;

    M4aDecoder(const M4aDecoder&)            = delete;
    M4aDecoder& operator=(const M4aDecoder&) = delete;

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
