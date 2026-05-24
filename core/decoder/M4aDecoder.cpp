// =============================================================================
//  core/decoder/M4aDecoder.cpp
//
//  M4A / MP4 容器:走 Windows Media Foundation Source Reader。
//  MF 解 MP4 容器 + AAC payload 是开箱即用的 (Win10+);
//  ALAC/AC3/E-AC3 等也能解,但解码出来一律 Int16 PCM。
// =============================================================================
#include "M4aDecoder.h"
#include "MfMediaDecoder.h"

namespace apx {

struct M4aDecoder::Impl {
    MfMediaDecoder mf;
};

M4aDecoder::M4aDecoder()  : d_(std::make_unique<Impl>()) {}
M4aDecoder::~M4aDecoder() = default;

bool         M4aDecoder::open(const std::wstring& path) { return d_->mf.open(path); }
void         M4aDecoder::close()                       { d_->mf.close(); }
bool         M4aDecoder::isOpen()       const            { return d_->mf.isOpen(); }
AudioFormat  M4aDecoder::format()       const            { return d_->mf.format(); }
std::int64_t M4aDecoder::totalFrames()  const            { return d_->mf.totalFrames(); }
std::int64_t M4aDecoder::currentFrame() const            { return d_->mf.currentFrame(); }
bool         M4aDecoder::seek(std::int64_t frame)        { return d_->mf.seek(frame); }
std::size_t  M4aDecoder::read(std::uint8_t* dst, std::size_t bytes) { return d_->mf.read(dst, bytes); }
std::wstring M4aDecoder::lastError()    const            { return d_->mf.lastError(); }

} // namespace apx
