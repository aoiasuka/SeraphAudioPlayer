// =============================================================================
//  core/decoder/OpusDecoder.h
//
//  Opus 解码 (.opus 文件,Ogg 容器内的 Opus)。通过编译宏 APX_HAVE_OPUS 切换。
//  集成路线:opusfile + libopus (xiph 官方,BSD 许可)。
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class OpusDecoder final : public IDecoder {
public:
    OpusDecoder();
    ~OpusDecoder() override;

    OpusDecoder(const OpusDecoder&)            = delete;
    OpusDecoder& operator=(const OpusDecoder&) = delete;

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
