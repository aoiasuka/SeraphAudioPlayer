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
inline std::uint64_t rd_u64le(const std::uint8_t* p) {
    std::uint64_t v = 0;
    for (int i = 0; i < 8; ++i) v |= (static_cast<std::uint64_t>(p[i]) << (i * 8));
    return v;
}

constexpr std::uint16_t WF_PCM    = 0x0001;
constexpr std::uint16_t WF_FLOAT  = 0x0003;
constexpr std::uint16_t WF_EXTBL  = 0xFFFE;

// Wave64: chunk ID 是 16-byte GUID,GUID 前 4 字节通常是 ASCII "riff"/"wave"/
// "fmt "/"data" 等(全小写,与 RIFF 的大小写不同);后 12 字节是 spec 规定的固定 tail。
// 我们只校验前 4 字节,在已经走 Wave64 路径(顶层 GUID 命中)的前提下足够。
inline bool guid_id_is(const std::uint8_t* g, const char* fourcc) {
    return std::memcmp(g, fourcc, 4) == 0;
}

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

    // 取真实文件大小,用于截断 data chunk size(防止部分工具写错 size)
    APX_FSEEK64(d_->fp, 0, SEEK_END);
    const std::int64_t file_size = APX_FTELL64(d_->fp);
    APX_FSEEK64(d_->fp, 0, SEEK_SET);

    // 读首部 16 字节用于分发 RIFF / RF64 / Wave64
    std::uint8_t hdr[16];
    if (std::fread(hdr, 1, 16, d_->fp) != 16) {
        d_->set_error(L"short file (no container header)"); close(); return false;
    }

    bool fmt_done = false;
    bool data_done = false;

    // -------- 分支 1: 标准 RIFF/WAVE --------
    // -------- 分支 2: RF64/WAVE (BWF >4GB) --------
    const bool is_riff = (std::memcmp(hdr, "RIFF", 4) == 0
                          && std::memcmp(hdr + 8, "WAVE", 4) == 0);
    const bool is_rf64 = (std::memcmp(hdr, "RF64", 4) == 0
                          && std::memcmp(hdr + 8, "WAVE", 4) == 0);

    if (is_riff || is_rf64) {
        // RF64 时,下一个 chunk 必须是 "ds64",含真实 64-bit data_size 等
        std::int64_t rf64_data_size_override = -1;
        if (is_rf64) {
            APX_FSEEK64(d_->fp, 12, SEEK_SET);
            std::uint8_t ds64_hdr[8];
            if (std::fread(ds64_hdr, 1, 8, d_->fp) != 8
                || std::memcmp(ds64_hdr, "ds64", 4) != 0) {
                d_->set_error(L"RF64: missing ds64 chunk"); close(); return false;
            }
            const std::uint32_t ds64_size = rd_u32le(ds64_hdr + 4);
            if (ds64_size < 28) {
                d_->set_error(L"RF64: ds64 chunk too small"); close(); return false;
            }
            std::vector<std::uint8_t> ds64(ds64_size);
            if (std::fread(ds64.data(), 1, ds64_size, d_->fp) != ds64_size) {
                d_->set_error(L"RF64: ds64 truncated"); close(); return false;
            }
            // u64 riffSize, u64 dataSize, u64 sampleCount, u32 tableLen, ...
            rf64_data_size_override = static_cast<std::int64_t>(rd_u64le(ds64.data() + 8));
            // 跳过 padding 到偶字节
            if (ds64_size & 1u) APX_FSEEK64(d_->fp, 1, SEEK_CUR);
        }

        while (!fmt_done || !data_done) {
            std::uint8_t ch_hdr[8];
            const std::size_t r = std::fread(ch_hdr, 1, 8, d_->fp);
            if (r != 8) break;
            const std::uint32_t ch_size = rd_u32le(ch_hdr + 4);
            const std::int64_t  payload_start = APX_FTELL64(d_->fp);
            // RF64: data chunk 的 32-bit size 字段 = 0xFFFFFFFF 时,真实大小来自 ds64;
            // 跳过此 chunk 时也应用真实大小,否则 fmt 在 data 之后这种罕见排列会跑偏
            std::int64_t real_size = static_cast<std::int64_t>(ch_size);
            if (is_rf64 && ch_size == 0xFFFFFFFFu
                && std::memcmp(ch_hdr, "data", 4) == 0
                && rf64_data_size_override >= 0) {
                real_size = rf64_data_size_override;
            }
            const std::int64_t after = payload_start + real_size + (real_size & 1);

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
                d_->data_offset = payload_start;
                const std::int64_t actual = file_size - d_->data_offset;
                d_->data_size = (real_size > actual && actual >= 0) ? actual : real_size;
                data_done = true;
                // 不再用 "if (fmt_done) break;" 显式退出 —— 循环条件本身已包含
                // "两件都齐了就停",这种写法也允许 fmt 在 data 之后 (spec 允许但罕见)
                APX_FSEEK64(d_->fp, after, SEEK_SET);
            } else {
                // 跳过未知 chunk
                APX_FSEEK64(d_->fp, after, SEEK_SET);
            }
        }
    } else if (std::memcmp(hdr, "riff", 4) == 0) {
        // -------- 分支 3: Sony Wave64 --------
        // 文件首部:16-byte GUID("riff" + tail) + 8-byte u64 totalSize +
        //          16-byte GUID("wave" + tail);共 40 字节
        // 我们已经读了 16 字节(到 hdr),需要再读 24 字节补齐
        std::uint8_t hdr2[24];
        if (std::fread(hdr2, 1, 24, d_->fp) != 24) {
            d_->set_error(L"Wave64: short header"); close(); return false;
        }
        // hdr2[0..8) = totalSize (含本头),hdr2[8..24) = "wave" GUID
        if (!guid_id_is(hdr2 + 8, "wave")) {
            d_->set_error(L"Wave64: missing wave form GUID"); close(); return false;
        }
        // 此后 chunk 格式: 16-byte GUID + 8-byte u64 chunkSize(含头) + payload,8-byte 对齐 pad
        while (!fmt_done || !data_done) {
            std::uint8_t ck[24];
            const std::size_t r = std::fread(ck, 1, 24, d_->fp);
            if (r != 24) break;
            const std::uint64_t ck_size = rd_u64le(ck + 16);  // 含本头 24 字节
            if (ck_size < 24) { d_->set_error(L"Wave64: bad chunk size"); close(); return false; }
            const std::uint64_t payload_size = ck_size - 24;
            const std::int64_t  payload_start = APX_FTELL64(d_->fp);
            const std::uint64_t pad = (8 - (ck_size & 7)) & 7;   // 8-byte 对齐
            const std::int64_t  after = payload_start
                                      + static_cast<std::int64_t>(payload_size + pad);

            if (guid_id_is(ck, "fmt ")) {
                if (payload_size > 4096) {
                    d_->set_error(L"Wave64: fmt chunk implausibly large"); close(); return false;
                }
                std::vector<std::uint8_t> buf(static_cast<std::size_t>(payload_size));
                if (payload_size
                    && std::fread(buf.data(), 1, static_cast<std::size_t>(payload_size), d_->fp)
                       != payload_size) {
                    d_->set_error(L"Wave64: fmt chunk truncated"); close(); return false;
                }
                std::wstring err;
                if (!parse_fmt_chunk(buf, d_->fmt, err)) {
                    d_->set_error(err); close(); return false;
                }
                fmt_done = true;
                APX_FSEEK64(d_->fp, after, SEEK_SET);
            } else if (guid_id_is(ck, "data")) {
                d_->data_offset = payload_start;
                const std::int64_t reported = static_cast<std::int64_t>(payload_size);
                const std::int64_t actual   = file_size - d_->data_offset;
                d_->data_size = (reported > actual && actual >= 0) ? actual : reported;
                data_done = true;
                APX_FSEEK64(d_->fp, after, SEEK_SET);
            } else {
                APX_FSEEK64(d_->fp, after, SEEK_SET);
            }
        }
    } else {
        d_->set_error(L"not RIFF/RF64/Wave64"); close(); return false;
    }

    if (!fmt_done)  { d_->set_error(L"fmt chunk not found");  close(); return false; }
    if (!data_done) { d_->set_error(L"data chunk not found"); close(); return false; }
    if (d_->data_size <= 0) {
        d_->set_error(L"data chunk has zero/negative size"); close(); return false;
    }

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
