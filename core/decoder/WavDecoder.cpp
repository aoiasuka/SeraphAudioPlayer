// =============================================================================
//  core/decoder/WavDecoder.cpp
// =============================================================================
#include "WavDecoder.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <sstream>
#include <vector>

// MSVC 的 64-bit 文件位移
#if defined(_MSC_VER)
  #define APX_FSEEK64  _fseeki64
  #define APX_FTELL64  _ftelli64
#else
  #define APX_FSEEK64  fseeko64
  #define APX_FTELL64  ftello64
#endif

namespace {

inline std::uint16_t rd_u16le(const std::uint8_t* p) {
    return static_cast<std::uint16_t>(p[0] | (p[1] << 8));
}
inline std::uint32_t rd_u32le(const std::uint8_t* p) {
    return static_cast<std::uint32_t>(p[0])
         | (static_cast<std::uint32_t>(p[1]) << 8)
         | (static_cast<std::uint32_t>(p[2]) << 16)
         | (static_cast<std::uint32_t>(p[3]) << 24);
}

constexpr std::uint16_t WF_PCM    = 0x0001;
constexpr std::uint16_t WF_FLOAT  = 0x0003;
constexpr std::uint16_t WF_EXTBL  = 0xFFFE;

} // namespace

namespace apx {

struct WavDecoder::Impl {
    FILE*         fp           = nullptr;
    AudioFormat   fmt{};
    std::int64_t  data_offset  = 0;     // 字节
    std::int64_t  data_size    = 0;     // 字节(可能被 file size 截断)
    std::int64_t  total_frames = 0;
    std::int64_t  cur_frame    = 0;
    std::uint32_t frame_bytes  = 0;
    std::wstring  last_error;

    void set_error(const std::wstring& msg) { last_error = msg; }
    void set_error(const wchar_t* msg)      { last_error = msg; }
};

WavDecoder::WavDecoder()  : d_(std::make_unique<Impl>()) {}
WavDecoder::~WavDecoder() { close(); }

bool WavDecoder::isOpen()           const { return d_->fp != nullptr; }
AudioFormat WavDecoder::format()    const { return d_->fmt; }
std::int64_t WavDecoder::totalFrames()   const { return d_->total_frames; }
std::int64_t WavDecoder::currentFrame()  const { return d_->cur_frame; }
std::wstring WavDecoder::lastError() const { return d_->last_error; }

void WavDecoder::close()
{
    if (d_->fp) { std::fclose(d_->fp); d_->fp = nullptr; }
    d_->fmt = {};
    d_->data_offset = 0;
    d_->data_size   = 0;
    d_->total_frames = 0;
    d_->cur_frame   = 0;
    d_->frame_bytes = 0;
}

// -----------------------------------------------------------------------------

namespace {

bool parse_fmt_chunk(const std::vector<std::uint8_t>& buf,
                     AudioFormat& out,
                     std::wstring& err)
{
    if (buf.size() < 16) { err = L"fmt chunk too small"; return false; }
    std::uint16_t format_tag     = rd_u16le(&buf[0]);
    const std::uint16_t channels = rd_u16le(&buf[2]);
    const std::uint32_t rate     = rd_u32le(&buf[4]);
    const std::uint16_t bps      = rd_u16le(&buf[14]);
    std::uint16_t valid_bits     = bps;
    std::uint32_t channel_mask   = 0;

    if (format_tag == WF_EXTBL) {
        if (buf.size() < 40) { err = L"WAVE_FORMAT_EXTENSIBLE but fmt chunk < 40 bytes"; return false; }
        valid_bits   = rd_u16le(&buf[18]);
        channel_mask = rd_u32le(&buf[20]);
        // SubFormat GUID 前 2 字节即真实 format tag
        format_tag   = rd_u16le(&buf[24]);
    }

    if (channels == 0 || rate == 0 || bps == 0 || (bps % 8) != 0) {
        err = L"invalid fmt fields"; return false;
    }
    if (valid_bits == 0 || valid_bits > bps) {
        err = L"invalid wValidBitsPerSample"; return false;
    }

    AudioFormat f;
    f.sample_rate     = rate;
    f.channels        = channels;
    f.bits_per_sample = bps;
    f.valid_bits      = valid_bits;
    f.channel_mask    = channel_mask;

    if (format_tag == WF_PCM) {
        switch (bps) {
        case 16: f.sample_type = SampleType::Int16;       f.valid_bits = 16; break;
        case 24: f.sample_type = SampleType::Int24Packed; f.valid_bits = 24; break;
        case 32: f.sample_type = SampleType::Int32;       break;
        default: err = L"unsupported PCM bit depth"; return false;
        }
    } else if (format_tag == WF_FLOAT) {
        if (bps != 32) { err = L"only 32-bit IEEE_FLOAT is supported"; return false; }
        f.sample_type = SampleType::Float32;
        f.valid_bits  = 32;
    } else {
        std::wostringstream ss;
        ss << L"unsupported wFormatTag=0x" << std::hex << format_tag;
        err = ss.str();
        return false;
    }

    if (!f.valid()) { err = L"AudioFormat::valid() failed after parse"; return false; }
    out = f;
    return true;
}

} // namespace

bool WavDecoder::open(const std::wstring& path)
{
    if (d_->fp) close();

    FILE* fp = nullptr;
    if (_wfopen_s(&fp, path.c_str(), L"rb") != 0 || !fp) {
        d_->set_error(L"_wfopen_s failed");
        return false;
    }
    d_->fp = fp;

    // 取真实文件大小,用于截断 data chunk size(防止部分 WAV 工具写错 size)
    APX_FSEEK64(d_->fp, 0, SEEK_END);
    const std::int64_t file_size = APX_FTELL64(d_->fp);
    APX_FSEEK64(d_->fp, 0, SEEK_SET);

    // RIFF header (12 bytes)
    std::uint8_t hdr[12];
    if (std::fread(hdr, 1, 12, d_->fp) != 12) {
        d_->set_error(L"short file (no RIFF header)"); close(); return false;
    }
    if (std::memcmp(hdr, "RIFF", 4) != 0 || std::memcmp(hdr + 8, "WAVE", 4) != 0) {
        d_->set_error(L"not RIFF/WAVE"); close(); return false;
    }

    bool fmt_done = false;
    bool data_done = false;

    while (!fmt_done || !data_done) {
        std::uint8_t ch_hdr[8];
        const std::size_t r = std::fread(ch_hdr, 1, 8, d_->fp);
        if (r != 8) break;
        const std::uint32_t ch_size = rd_u32le(ch_hdr + 4);
        const std::int64_t  after   = APX_FTELL64(d_->fp) + ch_size + (ch_size & 1);

        if (std::memcmp(ch_hdr, "fmt ", 4) == 0) {
            std::vector<std::uint8_t> buf(ch_size);
            if (ch_size && std::fread(buf.data(), 1, ch_size, d_->fp) != ch_size) {
                d_->set_error(L"fmt chunk truncated"); close(); return false;
            }
            std::wstring err;
            if (!parse_fmt_chunk(buf, d_->fmt, err)) {
                d_->set_error(err); close(); return false;
            }
            fmt_done = true;
            APX_FSEEK64(d_->fp, after, SEEK_SET);
        } else if (std::memcmp(ch_hdr, "data", 4) == 0) {
            d_->data_offset = APX_FTELL64(d_->fp);
            std::int64_t reported = static_cast<std::int64_t>(ch_size);
            const std::int64_t actual = file_size - d_->data_offset;
            d_->data_size = (reported > actual && actual >= 0) ? actual : reported;
            data_done = true;
            // 不读 data,跳过(下次 read 时再回到 data_offset)
            APX_FSEEK64(d_->fp, after, SEEK_SET);
            // 一旦同时拿到 fmt + data 即可退出
            if (fmt_done) break;
        } else {
            // 跳过未知 chunk(含 LIST/bext/cue 等)
            APX_FSEEK64(d_->fp, after, SEEK_SET);
        }
    }

    if (!fmt_done)  { d_->set_error(L"fmt chunk not found");  close(); return false; }
    if (!data_done) { d_->set_error(L"data chunk not found"); close(); return false; }

    d_->frame_bytes  = d_->fmt.frame_bytes();
    if (d_->frame_bytes == 0) {
        d_->set_error(L"frame_bytes == 0"); close(); return false;
    }
    d_->total_frames = d_->data_size / d_->frame_bytes;
    d_->cur_frame    = 0;

    // 把读指针定位到 data 起点
    if (APX_FSEEK64(d_->fp, d_->data_offset, SEEK_SET) != 0) {
        d_->set_error(L"seek to data failed"); close(); return false;
    }
    return true;
}

bool WavDecoder::seek(std::int64_t frame)
{
    if (!d_->fp) { d_->set_error(L"not open"); return false; }
    if (frame < 0) frame = 0;
    if (frame > d_->total_frames) frame = d_->total_frames;

    const std::int64_t off = d_->data_offset + frame * d_->frame_bytes;
    if (APX_FSEEK64(d_->fp, off, SEEK_SET) != 0) {
        d_->set_error(L"_fseeki64 failed"); return false;
    }
    d_->cur_frame = frame;
    return true;
}

std::size_t WavDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->fp || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;

    // 对齐到 frame 边界
    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;

    const std::int64_t remaining_frames = d_->total_frames - d_->cur_frame;
    if (remaining_frames <= 0) return 0;

    const std::size_t max_bytes =
        static_cast<std::size_t>(remaining_frames) * d_->frame_bytes;
    if (bytes > max_bytes) bytes = max_bytes;

    const std::size_t got = std::fread(dst, 1, bytes, d_->fp);
    const std::size_t got_frames = got / d_->frame_bytes;
    d_->cur_frame += static_cast<std::int64_t>(got_frames);

    // 若读出的字节不在 frame 边界(罕见,通常是文件被截断),抹掉尾巴
    const std::size_t aligned = got_frames * d_->frame_bytes;
    return aligned;
}

} // namespace apx
