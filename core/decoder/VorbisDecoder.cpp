// =============================================================================
//  core/decoder/VorbisDecoder.cpp
//
//  stb_vorbis 的 implementation 单元(STB_VORBIS_IMPLEMENTATION 不存在,
//  stb_vorbis.c 本身就是实现;此处直接 include 整个 .c)
// =============================================================================
#include "VorbisDecoder.h"

#if APX_HAVE_STB_VORBIS

#if defined(_MSC_VER)
#  pragma warning(push)
#  pragma warning(disable: 4100)  // unreferenced formal parameter
#  pragma warning(disable: 4127)
#  pragma warning(disable: 4244)
#  pragma warning(disable: 4245)
#  pragma warning(disable: 4267)
#  pragma warning(disable: 4456)
#  pragma warning(disable: 4457)
#  pragma warning(disable: 4459)  // declaration hides global
#  pragma warning(disable: 4701)
#  pragma warning(disable: 4703)
#  pragma warning(disable: 5045)
#  pragma warning(disable: 4505)  // unreferenced local function
#endif

// 禁用 PUSHDATA API,只用 PULLDATA(我们走文件路径)
#define STB_VORBIS_NO_PUSHDATA_API
// 我们不需要 stdio 默认 helpers 之外的额外 wrapper
#include "stb/stb_vorbis.c"

#if defined(_MSC_VER)
#  pragma warning(pop)
#endif

#include <cstdio>
#include <sstream>

namespace apx {

struct VorbisDecoder::Impl {
    stb_vorbis*  vorbis = nullptr;
    AudioFormat  fmt{};
    int64_t      total_frames = 0;
    int64_t      cur_frame    = 0;
    uint32_t     frame_bytes  = 0;
    std::wstring last_error;
};

VorbisDecoder::VorbisDecoder()  : d_(std::make_unique<Impl>()) {}
VorbisDecoder::~VorbisDecoder() { close(); }

bool         VorbisDecoder::isOpen()       const { return d_->vorbis != nullptr; }
AudioFormat  VorbisDecoder::format()       const { return d_->fmt; }
std::int64_t VorbisDecoder::totalFrames()  const { return d_->total_frames; }
std::int64_t VorbisDecoder::currentFrame() const { return d_->cur_frame; }
std::wstring VorbisDecoder::lastError()    const { return d_->last_error; }

void VorbisDecoder::close()
{
    if (d_->vorbis) { stb_vorbis_close(d_->vorbis); d_->vorbis = nullptr; }
    d_->fmt = {};
    d_->total_frames = 0;
    d_->cur_frame    = 0;
    d_->frame_bytes  = 0;
}

bool VorbisDecoder::open(const std::wstring& path)
{
    if (d_->vorbis) close();

    // stb_vorbis 没有 _w fopen,我们用 _wfopen_s 拿到 FILE*
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) {
        d_->last_error = L"open file failed: " + path;
        return false;
    }
    int err = 0;
    stb_vorbis* v = stb_vorbis_open_file(f, /*close_on_free=*/1, &err, nullptr);
    if (!v) {
        std::wostringstream ss;
        ss << L"stb_vorbis_open_file failed: err=" << err;
        d_->last_error = ss.str();
        std::fclose(f);
        return false;
    }
    stb_vorbis_info info = stb_vorbis_get_info(v);

    AudioFormat fmt;
    fmt.sample_rate     = info.sample_rate;
    fmt.channels        = static_cast<std::uint16_t>(info.channels);
    fmt.bits_per_sample = 16;
    fmt.valid_bits      = 16;
    fmt.sample_type     = SampleType::Int16;
    fmt.channel_mask    = default_channel_mask(fmt.channels);
    if (!fmt.valid()) {
        d_->last_error = L"Vorbis produced invalid AudioFormat";
        stb_vorbis_close(v);
        return false;
    }

    d_->vorbis       = v;
    d_->fmt          = fmt;
    d_->frame_bytes  = fmt.frame_bytes();
    d_->total_frames = static_cast<int64_t>(stb_vorbis_stream_length_in_samples(v));
    d_->cur_frame    = 0;
    return true;
}

bool VorbisDecoder::seek(std::int64_t frame)
{
    if (!d_->vorbis) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    if (d_->total_frames > 0 && frame > d_->total_frames) frame = d_->total_frames;
    if (!stb_vorbis_seek(d_->vorbis, static_cast<unsigned int>(frame))) {
        d_->last_error = L"stb_vorbis_seek failed";
        return false;
    }
    d_->cur_frame = frame;
    return true;
}

std::size_t VorbisDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->vorbis || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;
    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;
    int frames_wanted = static_cast<int>(bytes / d_->frame_bytes);

    int frames_read = stb_vorbis_get_samples_short_interleaved(
        d_->vorbis,
        d_->fmt.channels,
        reinterpret_cast<short*>(dst),
        frames_wanted * d_->fmt.channels);

    if (frames_read < 0) frames_read = 0;
    d_->cur_frame += frames_read;
    return static_cast<std::size_t>(frames_read) * d_->frame_bytes;
}

} // namespace apx

#else  // !APX_HAVE_STB_VORBIS

namespace apx {

struct VorbisDecoder::Impl {};

VorbisDecoder::VorbisDecoder()  : d_(std::make_unique<Impl>()) {}
VorbisDecoder::~VorbisDecoder() = default;

bool         VorbisDecoder::open(const std::wstring&)    { return false; }
void         VorbisDecoder::close()                    {}
bool         VorbisDecoder::isOpen()       const         { return false; }
AudioFormat  VorbisDecoder::format()       const         { return {}; }
std::int64_t VorbisDecoder::totalFrames()  const         { return 0; }
std::int64_t VorbisDecoder::currentFrame() const         { return 0; }
bool         VorbisDecoder::seek(std::int64_t)           { return false; }
std::size_t  VorbisDecoder::read(std::uint8_t*, std::size_t) { return 0; }
std::wstring VorbisDecoder::lastError()    const         { return L"OGG Vorbis support not compiled in"; }

} // namespace apx

#endif // APX_HAVE_STB_VORBIS
