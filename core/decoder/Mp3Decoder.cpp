// =============================================================================
//  core/decoder/Mp3Decoder.cpp
//
//  dr_mp3.h 的 implementation 翻译单元。
// =============================================================================

#include "Mp3Decoder.h"

#if APX_HAVE_DR_MP3

#if defined(_MSC_VER)
#  pragma warning(push)
#  pragma warning(disable: 4127)
#  pragma warning(disable: 4244)
#  pragma warning(disable: 4245)
#  pragma warning(disable: 4267)
#  pragma warning(disable: 4310)
#  pragma warning(disable: 4456)
#  pragma warning(disable: 4457)
#  pragma warning(disable: 4701)
#  pragma warning(disable: 4703)
#  pragma warning(disable: 5045)
#endif

#define DR_MP3_IMPLEMENTATION
#include "dr_libs/dr_mp3.h"

#if defined(_MSC_VER)
#  pragma warning(pop)
#endif

#include <sstream>

namespace apx {

struct Mp3Decoder::Impl {
    drmp3         mp3{};
    bool          opened = false;
    AudioFormat   fmt{};
    std::int64_t  total_frames = 0;
    std::int64_t  cur_frame    = 0;
    std::uint32_t frame_bytes  = 0;
    std::wstring  last_error;
};

Mp3Decoder::Mp3Decoder()  : d_(std::make_unique<Impl>()) {}
Mp3Decoder::~Mp3Decoder() { close(); }

bool         Mp3Decoder::isOpen()       const { return d_->opened; }
AudioFormat  Mp3Decoder::format()       const { return d_->fmt; }
std::int64_t Mp3Decoder::totalFrames()  const { return d_->total_frames; }
std::int64_t Mp3Decoder::currentFrame() const { return d_->cur_frame; }
std::wstring Mp3Decoder::lastError()    const { return d_->last_error; }

void Mp3Decoder::close()
{
    if (d_->opened) { drmp3_uninit(&d_->mp3); d_->opened = false; }
    d_->fmt = {};
    d_->total_frames = 0;
    d_->cur_frame    = 0;
    d_->frame_bytes  = 0;
}

bool Mp3Decoder::open(const std::wstring& path)
{
    if (d_->opened) close();

    if (!drmp3_init_file_w(&d_->mp3, path.c_str(), nullptr)) {
        d_->last_error = L"drmp3_init_file_w failed: " + path;
        return false;
    }
    d_->opened = true;

    AudioFormat fmt;
    fmt.sample_rate     = d_->mp3.sampleRate;
    fmt.channels        = static_cast<std::uint16_t>(d_->mp3.channels);
    fmt.bits_per_sample = 16;
    fmt.valid_bits      = 16;
    fmt.sample_type     = SampleType::Int16;
    fmt.channel_mask    = default_channel_mask(fmt.channels);

    if (!fmt.valid()) {
        d_->last_error = L"MP3 produced invalid AudioFormat";
        drmp3_uninit(&d_->mp3);
        d_->opened = false;
        return false;
    }

    d_->fmt          = fmt;
    d_->frame_bytes  = fmt.frame_bytes();
    d_->total_frames = static_cast<std::int64_t>(drmp3_get_pcm_frame_count(&d_->mp3));
    d_->cur_frame    = 0;
    return true;
}

bool Mp3Decoder::seek(std::int64_t frame)
{
    if (!d_->opened) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    if (d_->total_frames > 0 && frame > d_->total_frames) frame = d_->total_frames;

    if (!drmp3_seek_to_pcm_frame(&d_->mp3, static_cast<drmp3_uint64>(frame))) {
        d_->last_error = L"drmp3_seek_to_pcm_frame failed";
        return false;
    }
    d_->cur_frame = frame;
    return true;
}

std::size_t Mp3Decoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->opened || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;

    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;

    const std::size_t frames_wanted =
        static_cast<std::size_t>(bytes / d_->frame_bytes);

    drmp3_uint64 frames_read = drmp3_read_pcm_frames_s16(
        &d_->mp3,
        static_cast<drmp3_uint64>(frames_wanted),
        reinterpret_cast<drmp3_int16*>(dst));

    d_->cur_frame += static_cast<std::int64_t>(frames_read);
    return static_cast<std::size_t>(frames_read) * d_->frame_bytes;
}

} // namespace apx

#else  // !APX_HAVE_DR_MP3 — dr_mp3.h 不存在时,提供桩实现保证链接

namespace apx {

struct Mp3Decoder::Impl {};

Mp3Decoder::Mp3Decoder()  : d_(std::make_unique<Impl>()) {}
Mp3Decoder::~Mp3Decoder() = default;

bool         Mp3Decoder::open(const std::wstring&) { return false; }
void         Mp3Decoder::close()                 {}
bool         Mp3Decoder::isOpen()       const     { return false; }
AudioFormat  Mp3Decoder::format()       const     { return {}; }
std::int64_t Mp3Decoder::totalFrames()  const     { return 0; }
std::int64_t Mp3Decoder::currentFrame() const     { return 0; }
bool         Mp3Decoder::seek(std::int64_t)       { return false; }
std::size_t  Mp3Decoder::read(std::uint8_t*, std::size_t) { return 0; }
std::wstring Mp3Decoder::lastError()    const     { return L"MP3 support not compiled in"; }

} // namespace apx

#endif // APX_HAVE_DR_MP3
