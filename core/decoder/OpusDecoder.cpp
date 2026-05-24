// =============================================================================
//  core/decoder/OpusDecoder.cpp
//
//  通过 opusfile (xiph) 解码 .opus 文件 (Ogg-Opus 容器)。
//    - opusfile 自动处理 OggS 包拆分 + 多比特率序列
//    - 内部固定 48000 Hz 输出 (Opus 规范),解码出 Int16 或 Float32
//
//  启用方式:
//    vcpkg install opusfile
//    cmake .. -DCMAKE_TOOLCHAIN_FILE=<vcpkg>/scripts/buildsystems/vcpkg.cmake
//  根 CMakeLists 检测到 opusfile 即自动定义 APX_HAVE_OPUS=1。
//
//  未启用时,本文件退化为桩,链接器仍可解析。
// =============================================================================
#include "OpusDecoder.h"

#if APX_HAVE_OPUS

#include <opusfile.h>

#include <atomic>
#include <cstdio>
#include <sstream>

namespace apx {

namespace {

constexpr int kOpusSampleRate = 48000;     // Opus 解码恒定输出 48 kHz

// 把 opusfile 错误码映射为可读字符串
const wchar_t* op_err_str(int e)
{
    switch (e) {
    case OP_FALSE:         return L"OP_FALSE";
    case OP_EOF:           return L"OP_EOF";
    case OP_HOLE:          return L"OP_HOLE (lost packet)";
    case OP_EREAD:         return L"OP_EREAD";
    case OP_EFAULT:        return L"OP_EFAULT";
    case OP_EIMPL:         return L"OP_EIMPL";
    case OP_EINVAL:        return L"OP_EINVAL";
    case OP_ENOTFORMAT:    return L"OP_ENOTFORMAT";
    case OP_EBADHEADER:    return L"OP_EBADHEADER";
    case OP_EVERSION:      return L"OP_EVERSION";
    case OP_ENOTAUDIO:     return L"OP_ENOTAUDIO";
    case OP_EBADPACKET:    return L"OP_EBADPACKET";
    case OP_EBADLINK:      return L"OP_EBADLINK";
    case OP_ENOSEEK:       return L"OP_ENOSEEK";
    case OP_EBADTIMESTAMP: return L"OP_EBADTIMESTAMP";
    default:               return L"?";
    }
}

} // namespace

struct OpusDecoder::Impl {
    OggOpusFile*         of = nullptr;
    AudioFormat          fmt{};
    std::atomic<std::int64_t> total_frames{0};
    std::int64_t         cur_frame    = 0;
    std::uint32_t        frame_bytes  = 0;
    int                  channels     = 0;
    std::wstring         last_error;
};

OpusDecoder::OpusDecoder()  : d_(std::make_unique<Impl>()) {}
OpusDecoder::~OpusDecoder() { close(); }

bool         OpusDecoder::isOpen()       const { return d_->of != nullptr; }
AudioFormat  OpusDecoder::format()       const { return d_->fmt; }
std::int64_t OpusDecoder::totalFrames()  const { return d_->total_frames.load(std::memory_order_acquire); }
std::int64_t OpusDecoder::currentFrame() const { return d_->cur_frame; }
std::wstring OpusDecoder::lastError()    const { return d_->last_error; }

void OpusDecoder::close()
{
    if (d_->of) { op_free(d_->of); d_->of = nullptr; }
    d_->fmt = {};
    d_->total_frames.store(0, std::memory_order_release);
    d_->cur_frame   = 0;
    d_->frame_bytes = 0;
    d_->channels    = 0;
}

bool OpusDecoder::open(const std::wstring& path)
{
    if (d_->of) close();

    // opusfile 在 Windows 下需要先 fopen 拿 FILE*,然后用 op_open_callbacks
    // 与默认 callbacks。但 op_open_file 是 char* 路径,wchar 路径要用 _wfopen + op_open_callbacks
    FILE* fp = nullptr;
    if (_wfopen_s(&fp, path.c_str(), L"rb") != 0 || !fp) {
        d_->last_error = L"open file failed: " + path;
        return false;
    }
    int err = 0;
    OggOpusFile* of = op_open_callbacks(fp, &OP_FILE_CALLBACKS, nullptr, 0, &err);
    if (!of) {
        std::wostringstream ss;
        ss << L"op_open_callbacks failed: " << op_err_str(err) << L" (" << err << L")";
        d_->last_error = ss.str();
        std::fclose(fp);
        return false;
    }
    // op_open_callbacks 成功后,文件句柄归 opusfile 管(op_free 内部 fclose)

    const int channels = op_channel_count(of, -1);
    if (channels < 1 || channels > 8) {
        std::wostringstream ss; ss << L"unsupported channels=" << channels;
        d_->last_error = ss.str();
        op_free(of);
        return false;
    }

    AudioFormat fmt;
    fmt.sample_rate     = static_cast<std::uint32_t>(kOpusSampleRate);
    fmt.channels        = static_cast<std::uint16_t>(channels);
    fmt.bits_per_sample = 16;
    fmt.valid_bits      = 16;
    fmt.sample_type     = SampleType::Int16;
    fmt.channel_mask    = default_channel_mask(fmt.channels);
    if (!fmt.valid()) {
        d_->last_error = L"Opus produced invalid AudioFormat";
        op_free(of);
        return false;
    }

    const ogg_int64_t total = op_pcm_total(of, -1);   // 48 kHz 单声道 sample 计
    d_->of          = of;
    d_->fmt         = fmt;
    d_->channels    = channels;
    d_->frame_bytes = fmt.frame_bytes();
    d_->cur_frame   = 0;
    d_->total_frames.store(total > 0 ? static_cast<std::int64_t>(total) : 0,
                           std::memory_order_release);
    return true;
}

bool OpusDecoder::seek(std::int64_t frame)
{
    if (!d_->of) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    const std::int64_t total = d_->total_frames.load();
    if (total > 0 && frame > total) frame = total;
    const int r = op_pcm_seek(d_->of, static_cast<ogg_int64_t>(frame));
    if (r != 0) {
        std::wostringstream ss; ss << L"op_pcm_seek failed: " << op_err_str(r);
        d_->last_error = ss.str();
        return false;
    }
    d_->cur_frame = frame;
    return true;
}

std::size_t OpusDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->of || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;
    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;

    const int frames_wanted = static_cast<int>(bytes / d_->frame_bytes);
    // opusfile 的 op_read 一次最多返回当前 link 的剩余;循环直到填满或 EOF
    int filled_frames = 0;
    while (filled_frames < frames_wanted) {
        opus_int16* out = reinterpret_cast<opus_int16*>(dst)
                        + filled_frames * d_->channels;
        const int max = (frames_wanted - filled_frames) * d_->channels;
        // op_read 返回"per channel sample count"(等同于 PCM 帧数);< 0 错误,0 EOF
        const int got = op_read(d_->of, out, max, nullptr);
        if (got < 0) {
            // 错误中只有 OP_HOLE 算 "可恢复",其它致命
            if (got == OP_HOLE) continue;
            std::wostringstream ss; ss << L"op_read failed: " << op_err_str(got);
            d_->last_error = ss.str();
            break;
        }
        if (got == 0) break;     // EOF
        filled_frames += got;
    }
    d_->cur_frame += filled_frames;
    return static_cast<std::size_t>(filled_frames) * d_->frame_bytes;
}

} // namespace apx

#else  // !APX_HAVE_OPUS

namespace apx {

struct OpusDecoder::Impl {};

OpusDecoder::OpusDecoder()  : d_(std::make_unique<Impl>()) {}
OpusDecoder::~OpusDecoder() = default;

bool         OpusDecoder::open(const std::wstring&)       { return false; }
void         OpusDecoder::close()                       {}
bool         OpusDecoder::isOpen()       const            { return false; }
AudioFormat  OpusDecoder::format()       const            { return {}; }
std::int64_t OpusDecoder::totalFrames()  const            { return 0; }
std::int64_t OpusDecoder::currentFrame() const            { return 0; }
bool         OpusDecoder::seek(std::int64_t)              { return false; }
std::size_t  OpusDecoder::read(std::uint8_t*, std::size_t){ return 0; }
std::wstring OpusDecoder::lastError()    const            { return L"Opus support not compiled in (vcpkg install opusfile)"; }

} // namespace apx

#endif
