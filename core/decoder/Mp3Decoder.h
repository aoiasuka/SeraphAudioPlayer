// =============================================================================
//  core/decoder/Mp3Decoder.h
//
//  封装 dr_mp3 (单头文件 MP3 解码器) 为 IDecoder。
//
//  输出格式:默认 Int16 PCM;通过 setOutputFloat32(true) 切换为 Float32。
//  Float32 路径精度更高 (dr_mp3 内部就是浮点),适合需要后端 DSP 的场景;
//  16-bit 路径有些 DAC 在独占模式下偏好。
//  采样率与声道数从首帧拿。
//
//  注意:setOutputFloat32 必须在 open() 之前调用,否则被忽略。
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

    // 在 open() 之前调用;true → Float32 输出,false → Int16 输出。默认 false。
    void setOutputFloat32(bool on);
    bool outputFloat32() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
