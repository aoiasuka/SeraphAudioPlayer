// =============================================================================
//  core/decoder/DffDecoder.h
//
//  DFF (DSDIFF, Philips) -> DoP 解码器。
//
//  与 DsdDecoder (DSF) 区别:
//    - DFF 数据以 frame-interleaved 方式存储:每帧 N_channels 字节,
//      每字节包含一通道连续 8 个 DSD bits,bit 顺序为 MSB-first (时间方向)
//    - 不需要 reverseBits,直接按字节复制即可
//
//  输出格式与 DsdDecoder 相同:Int24Packed,sample_rate = DSD_rate / 16
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"
#include "core/dsd/DopMode.h"

#include <memory>

namespace apx {

class DffDecoder final : public IDecoder {
public:
    DffDecoder();
    ~DffDecoder() override;

    DffDecoder(const DffDecoder&)            = delete;
    DffDecoder& operator=(const DffDecoder&) = delete;

    bool         open(const std::wstring& path) override;
    void         close() override;
    bool         isOpen() const override;
    AudioFormat  format() const override;
    std::int64_t totalFrames() const override;
    std::int64_t currentFrame() const override;
    bool         seek(std::int64_t frame) override;
    std::size_t  read(std::uint8_t* dst, std::size_t bytes) override;
    std::wstring lastError() const override;

    void          setMarkerMode(DopMarkerMode mode);
    DopMarkerMode markerMode() const;
    void setDopMarkerMode(DopMarkerMode mode) override { setMarkerMode(mode); }
    bool setNativeDsd(bool native) override;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
