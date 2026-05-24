// =============================================================================
//  core/decoder/AacDecoder.cpp
//
//  AAC (ADTS) 解码:走 Windows Media Foundation Source Reader,无需第三方库。
//  Win10+ 自带的 MFT 能解 raw ADTS 与 LATM (后者较罕见,容器内的 AAC 由
//  M4aDecoder 处理同一 MF 路径)。
// =============================================================================
#include "AacDecoder.h"
#include "MfMediaDecoder.h"

namespace apx {

struct AacDecoder::Impl {
    MfMediaDecoder mf;
};

AacDecoder::AacDecoder()  : d_(std::make_unique<Impl>()) {}
AacDecoder::~AacDecoder() = default;

bool         AacDecoder::open(const std::wstring& path) { return d_->mf.open(path); }
void         AacDecoder::close()                       { d_->mf.close(); }
bool         AacDecoder::isOpen()       const            { return d_->mf.isOpen(); }
AudioFormat  AacDecoder::format()       const            { return d_->mf.format(); }
std::int64_t AacDecoder::totalFrames()  const            { return d_->mf.totalFrames(); }
std::int64_t AacDecoder::currentFrame() const            { return d_->mf.currentFrame(); }
bool         AacDecoder::seek(std::int64_t frame)        { return d_->mf.seek(frame); }
std::size_t  AacDecoder::read(std::uint8_t* dst, std::size_t bytes) { return d_->mf.read(dst, bytes); }
std::wstring AacDecoder::lastError()    const            { return d_->mf.lastError(); }

} // namespace apx
