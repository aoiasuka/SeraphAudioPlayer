// =============================================================================
//  core/decoder/MfMediaDecoder.h  (Windows only)
//
//  Windows Media Foundation 通用音频解码器。给 AacDecoder / M4aDecoder 共用。
//  特点:
//    - 无外部依赖,Win10+ 自带 (mfplat / mfreadwrite / mfuuid / Propsys)
//    - 支持 AAC ADTS / MP4 / M4A;理论上也支持 WMA、FLAC (in MP4) 等其它 MF
//      原生格式,但本播放器优先用专门 decoder 处理这些
//    - 输出固定为 Int16 PCM,sample rate / channels 由容器决定
// =============================================================================
#pragma once

#include "core/format/AudioFormat.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

namespace apx {

class MfMediaDecoder {
public:
    MfMediaDecoder();
    ~MfMediaDecoder();

    MfMediaDecoder(const MfMediaDecoder&)            = delete;
    MfMediaDecoder& operator=(const MfMediaDecoder&) = delete;

    bool         open(const std::wstring& path);
    void         close();
    bool         isOpen() const;
    AudioFormat  format() const;
    std::int64_t totalFrames() const;
    std::int64_t currentFrame() const;
    bool         seek(std::int64_t frame);
    // 输出 Int16 PCM。返回字节数(frame_bytes 的整数倍);0 = EOF。
    std::size_t  read(std::uint8_t* dst, std::size_t bytes);
    std::wstring lastError() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
