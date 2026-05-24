// =============================================================================
//  core/decoder/WavDecoder.h
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

// 解析 RIFF/WAVE、RF64/BWF 和 Sony Wave64 容器。支持:
//   - WAVE_FORMAT_PCM       (0x0001) : 16 / 24-packed / 32 bit
//   - WAVE_FORMAT_IEEE_FLOAT(0x0003) : 32 bit float
//   - WAVE_FORMAT_EXTENSIBLE(0xFFFE) : 通过 SubFormat GUID 区分上面两种
//
// 容器形式:
//   - 标准 RIFF/WAVE (32-bit chunk size, 上限 4GB)
//   - RF64 (BWF):    包含 ds64 chunk,真实 64-bit size 写入此处
//   - Wave64 (Sony): 16-byte GUID chunk ID + 64-bit size, 8-byte 对齐
//
// 暂不支持: A-law/μ-law、ADPCM。
class WavDecoder final : public IDecoder {
public:
    WavDecoder();
    ~WavDecoder() override;

    WavDecoder(const WavDecoder&)            = delete;
    WavDecoder& operator=(const WavDecoder&) = delete;

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
