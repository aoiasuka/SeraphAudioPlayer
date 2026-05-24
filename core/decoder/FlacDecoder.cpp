// =============================================================================
//  core/decoder/FlacDecoder.cpp
//
//  这是 dr_flac.h 的 "implementation" 翻译单元 —— 全项目只此一处
//  定义 DR_FLAC_IMPLEMENTATION,其它 cpp 即使 include 也不会重复实例化。
// =============================================================================

// 第三方代码会触发的一批 MSVC 警告,统一隔离
#if defined(_MSC_VER)
#  pragma warning(push)
#  pragma warning(disable: 4127) // conditional expression is constant
#  pragma warning(disable: 4244) // conversion, possible loss of data
#  pragma warning(disable: 4245) // signed/unsigned conversion
#  pragma warning(disable: 4267) // size_t conversion
#  pragma warning(disable: 4310) // cast truncates constant value
#  pragma warning(disable: 4456) // declaration hides previous local
#  pragma warning(disable: 4457) // declaration hides function parameter
#  pragma warning(disable: 4701) // potentially uninitialized local
#  pragma warning(disable: 4703) // potentially uninitialized pointer
#  pragma warning(disable: 5045) // spectre mitigation
#endif

#define DR_FLAC_IMPLEMENTATION
#include "dr_libs/dr_flac.h"

#if defined(_MSC_VER)
#  pragma warning(pop)
#endif

#include "FlacDecoder.h"

#include <sstream>
#include <vector>

namespace apx {

struct FlacDecoder::Impl {
    drflac*       flac         = nullptr;
    AudioFormat   fmt{};
    std::int64_t  total_frames = 0;
    std::int64_t  cur_frame    = 0;
    std::uint32_t frame_bytes  = 0;

    // 24-bit FLAC 输出路径:
    //   dr_flac 没有原生 24-bit packed API,只能先 read_pcm_frames_s32(数据落在高 24 位),
    //   然后右移 8 + 写成 3 字节 LE。pack_24 == true 时启用此路径,tmp_s32 复用避免分配。
    bool                     pack_24 = false;
    std::vector<drflac_int32> tmp_s32;

    std::wstring last_error;
};

FlacDecoder::FlacDecoder()  : d_(std::make_unique<Impl>()) {}
FlacDecoder::~FlacDecoder() { close(); }

bool         FlacDecoder::isOpen()       const { return d_->flac != nullptr; }
AudioFormat  FlacDecoder::format()       const { return d_->fmt; }
std::int64_t FlacDecoder::totalFrames()  const { return d_->total_frames; }
std::int64_t FlacDecoder::currentFrame() const { return d_->cur_frame; }
std::wstring FlacDecoder::lastError()    const { return d_->last_error; }

void FlacDecoder::close()
{
    if (d_->flac) { drflac_close(d_->flac); d_->flac = nullptr; }
    d_->fmt = {};
    d_->total_frames = 0;
    d_->cur_frame    = 0;
    d_->frame_bytes  = 0;
    d_->pack_24      = false;
    d_->tmp_s32.clear();
    d_->tmp_s32.shrink_to_fit();
}

bool FlacDecoder::open(const std::wstring& path)
{
    if (d_->flac) close();

    drflac* f = drflac_open_file_w(path.c_str(), nullptr);
    if (!f) {
        d_->last_error = L"drflac_open_file_w failed: " + path;
        return false;
    }

    AudioFormat fmt;
    fmt.sample_rate = f->sampleRate;
    fmt.channels    = static_cast<std::uint16_t>(f->channels);

    switch (f->bitsPerSample) {
    case 16:
        fmt.bits_per_sample = 16;
        fmt.valid_bits      = 16;
        fmt.sample_type     = SampleType::Int16;
        d_->pack_24         = false;
        break;
    case 24:
        // 与 WavDecoder 24-bit 行为一致:输出 packed(3 字节 / sample),
        // 这是绝大多数 Windows 设备"24 位"独占格式所期待的 layout
        fmt.bits_per_sample = 24;
        fmt.valid_bits      = 24;
        fmt.sample_type     = SampleType::Int24Packed;
        d_->pack_24         = true;
        break;
    case 20:
        // 罕见:20-bit FLAC 用 32-bit 容器承载,但保留 valid_bits=20 给下游
        // (DSP / WASAPI WAVEFORMATEXTENSIBLE.Samples.wValidBitsPerSample)
        fmt.bits_per_sample = 32;
        fmt.valid_bits      = 20;
        fmt.sample_type     = SampleType::Int32;
        d_->pack_24         = false;
        break;
    case 32:
        fmt.bits_per_sample = 32;
        fmt.valid_bits      = 32;
        fmt.sample_type     = SampleType::Int32;
        d_->pack_24         = false;
        break;
    default: {
        std::wostringstream ss;
        ss << L"unsupported FLAC bitsPerSample=" << f->bitsPerSample;
        d_->last_error = ss.str();
        drflac_close(f);
        return false;
    }}

    fmt.channel_mask = default_channel_mask(fmt.channels);
    if (!fmt.valid()) {
        d_->last_error = L"FLAC produced invalid AudioFormat";
        drflac_close(f);
        return false;
    }

    d_->flac         = f;
    d_->fmt          = fmt;
    d_->total_frames = static_cast<std::int64_t>(f->totalPCMFrameCount);
    d_->cur_frame    = 0;
    d_->frame_bytes  = fmt.frame_bytes();
    return true;
}

bool FlacDecoder::seek(std::int64_t frame)
{
    if (!d_->flac) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    if (frame > d_->total_frames) frame = d_->total_frames;

    if (!drflac_seek_to_pcm_frame(d_->flac, static_cast<drflac_uint64>(frame))) {
        d_->last_error = L"drflac_seek_to_pcm_frame failed";
        return false;
    }
    d_->cur_frame = frame;
    return true;
}

std::size_t FlacDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->flac || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;

    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;

    const std::size_t frames_wanted =
        static_cast<std::size_t>(bytes / d_->frame_bytes);

    drflac_uint64 frames_read = 0;

    if (d_->fmt.sample_type == SampleType::Int16) {
        // 直接读 16-bit
        frames_read = drflac_read_pcm_frames_s16(
            d_->flac,
            static_cast<drflac_uint64>(frames_wanted),
            reinterpret_cast<drflac_int16*>(dst));
    } else if (d_->pack_24) {
        // 24-bit packed: 临时读 s32,然后右移 8 + 写 3 字节 LE
        const std::size_t samples_wanted = frames_wanted * d_->fmt.channels;
        if (d_->tmp_s32.size() < samples_wanted) d_->tmp_s32.resize(samples_wanted);

        frames_read = drflac_read_pcm_frames_s32(
            d_->flac,
            static_cast<drflac_uint64>(frames_wanted),
            d_->tmp_s32.data());

        const std::size_t samples_read =
            static_cast<std::size_t>(frames_read) * d_->fmt.channels;
        std::uint8_t* p = dst;
        for (std::size_t i = 0; i < samples_read; ++i) {
            const std::int32_t v = d_->tmp_s32[i] >> 8;     // 算术右移,保留符号
            p[0] = static_cast<std::uint8_t>( v        & 0xFF);
            p[1] = static_cast<std::uint8_t>((v >>  8) & 0xFF);
            p[2] = static_cast<std::uint8_t>((v >> 16) & 0xFF);
            p += 3;
        }
    } else {
        // Int32 (32-bit / 20-bit): dr_flac 已 normalize 到 full 32-bit range
        frames_read = drflac_read_pcm_frames_s32(
            d_->flac,
            static_cast<drflac_uint64>(frames_wanted),
            reinterpret_cast<drflac_int32*>(dst));
    }

    d_->cur_frame += static_cast<std::int64_t>(frames_read);
    return static_cast<std::size_t>(frames_read) * d_->frame_bytes;
}

} // namespace apx
