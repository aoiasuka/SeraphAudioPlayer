// =============================================================================
//  core/decoder/WavDecoder.h
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>

namespace apx {

// 解析标准 RIFF/WAVE。支持:
//   - WAVE_FORMAT_PCM       (0x0001) : 16 / 24-packed / 32 bit
//   - WAVE_FORMAT_IEEE_FLOAT(0x0003) : 32 bit float
//   - WAVE_FORMAT_EXTENSIBLE(0xFFFE) : 通过 SubFormat GUID 区分上面两种
//
// 暂不支持: RF64(> 4GB)、A-law/μ-law、ADPCM、Wave64。
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
