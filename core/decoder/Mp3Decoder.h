// =============================================================================
//  core/decoder/Mp3Decoder.h
//
//  封装 dr_mp3 (单头文件 MP3 解码器) 为 IDecoder。
//
//  输出格式:固定为 Int16 PCM (dr_mp3 内部解码默认 16-bit)。
//  采样率与声道数从首帧拿。
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

class Mp3Decoder final : public IDecoder {
public:
    Mp3Decoder();
    ~Mp3Decoder() override;

    Mp3Decoder(const Mp3Decoder&)            = delete;
    Mp3Decoder& operator=(const Mp3Decoder&) = delete;

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
