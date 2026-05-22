// =============================================================================
//  core/decoder/VorbisDecoder.h
//
//  封装 stb_vorbis (单文件 OGG Vorbis 解码器) 为 IDecoder。
//
//  输出格式:固定 Int16 PCM。stb_vorbis 内部解码默认输出 short。
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class VorbisDecoder final : public IDecoder {
public:
    VorbisDecoder();
    ~VorbisDecoder() override;

    VorbisDecoder(const VorbisDecoder&)            = delete;
    VorbisDecoder& operator=(const VorbisDecoder&) = delete;

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
