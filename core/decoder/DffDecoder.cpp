// =============================================================================
//  core/decoder/DffDecoder.cpp
//
//  DSDIFF (Philips DFF) 解析。
//
//  容器结构 (IFF 风格,大端 32-bit chunk size,FRM8 用 64-bit):
//
//  "FRM8" <u64 totalSize> "DSD " <子 chunks>
//    "FVER" <u64 size=4> <u32 version=0x01040000>
//    "PROP" <u64 size>   "SND "
//      "FS  " <u64 size=4> <u32 sampleFrequency>
//      "CHNL" <u64 size>   <u16 numChannels> <id*4 numChannels>
//      "CMPR" <u64 size>   <id*4 type> <u8 nameLen> <name...>
//      "ABSS" / "LSCO" / "ID3 " 等,可跳过
//    "DSD " <u64 size>   <data>     // 实际 1-bit 数据
//
//  data 布局:
//    若 CMPR == "DSD " (raw),则 frame-interleaved:
//      每 frame N_ch 字节,每字节 8 个 DSD bits (MSB-first 时间方向)
//    若 CMPR == "DST " (压缩):本实现不支持
// =============================================================================
#include "DffDecoder.h"

#include <atomic>
#include <cstdio>
#include <cstring>
#include <sstream>
#include <vector>

namespace apx {

namespace {

uint16_t readU16BE(const uint8_t* p) {
    return static_cast<uint16_t>((uint16_t(p[0]) << 8) | uint16_t(p[1]));
}
uint32_t readU32BE(const uint8_t* p) {
    return  (uint32_t(p[0]) << 24) | (uint32_t(p[1]) << 16) |
            (uint32_t(p[2]) << 8)  |  uint32_t(p[3]);
}
uint64_t readU64BE(const uint8_t* p) {
    return  (uint64_t(p[0]) << 56) | (uint64_t(p[1]) << 48) |
            (uint64_t(p[2]) << 40) | (uint64_t(p[3]) << 32) |
            (uint64_t(p[4]) << 24) | (uint64_t(p[5]) << 16) |
            (uint64_t(p[6]) << 8)  |  uint64_t(p[7]);
}

inline uint8_t reverseBits(uint8_t b) {
    b = static_cast<uint8_t>(((b >> 1) & 0x55) | ((b & 0x55) << 1));
    b = static_cast<uint8_t>(((b >> 2) & 0x33) | ((b & 0x33) << 2));
    b = static_cast<uint8_t>(((b >> 4) & 0x0F) | ((b & 0x0F) << 4));
    return b;
}

} // namespace

struct DffDecoder::Impl {
    FILE*        fp = nullptr;
    AudioFormat  fmt{};

    uint32_t     channels    = 0;
    uint32_t     dsd_rate    = 0;
    uint64_t     data_offset = 0;    // raw DSD data 起点
    uint64_t     data_size   = 0;    // 字节数
    uint64_t     dsd_samples = 0;    // 每通道 DSD 1-bit 样本数 = data_size * 8 / channels

    uint64_t     cur_byte_offset = 0;  // 自 data_offset 起的相对偏移
    uint64_t     pcm_frame_count = 0;
    bool         eof = false;

    // 复用 read() 内的 DSD 字节缓冲;按需 resize,不缩,避免实时路径分配
    std::vector<uint8_t> read_buf;

    // marker 策略
    std::atomic<DopMarkerMode> marker_mode{DopMarkerMode::PerFrame};
    // 输出模式
    std::atomic<bool> native_dsd{false};

    std::wstring last_error;
};

DffDecoder::DffDecoder()  : d_(std::make_unique<Impl>()) {}
DffDecoder::~DffDecoder() { close(); }

bool         DffDecoder::isOpen()       const { return d_->fp != nullptr; }
AudioFormat  DffDecoder::format()       const { return d_->fmt; }
std::wstring DffDecoder::lastError()    const { return d_->last_error; }

void          DffDecoder::setMarkerMode(DopMarkerMode m) { d_->marker_mode.store(m, std::memory_order_release); }
DopMarkerMode DffDecoder::markerMode() const             { return d_->marker_mode.load(std::memory_order_acquire); }

bool DffDecoder::setNativeDsd(bool native)
{
    if (!d_->fp) { d_->last_error = L"setNativeDsd: not open"; return false; }
    if (d_->native_dsd.load() == native) return true;
    d_->native_dsd.store(native, std::memory_order_release);
    AudioFormat& afmt = d_->fmt;
    if (native) {
        afmt.sample_rate     = d_->dsd_rate;
        afmt.bits_per_sample = 8;
        afmt.valid_bits      = 1;
        afmt.sample_type     = SampleType::DsdLsb8;
    } else {
        afmt.sample_rate     = d_->dsd_rate / 16;
        afmt.bits_per_sample = 24;
        afmt.valid_bits      = 24;
        afmt.sample_type     = SampleType::Int24Packed;
    }
    return true;
}

std::int64_t DffDecoder::totalFrames()  const {
    return static_cast<std::int64_t>(d_->dsd_samples / 16);
}
std::int64_t DffDecoder::currentFrame() const {
    return static_cast<std::int64_t>(d_->pcm_frame_count);
}

void DffDecoder::close()
{
    if (d_->fp) { std::fclose(d_->fp); d_->fp = nullptr; }
    d_->fmt = {};
    d_->channels    = 0;
    d_->dsd_rate    = 0;
    d_->data_offset = 0;
    d_->data_size   = 0;
    d_->dsd_samples = 0;
    d_->cur_byte_offset = 0;
    d_->pcm_frame_count = 0;
    d_->eof = false;
    d_->read_buf.clear();
    d_->read_buf.shrink_to_fit();
}

bool DffDecoder::open(const std::wstring& path)
{
    if (d_->fp) close();

    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) {
        d_->last_error = L"open file failed: " + path;
        return false;
    }
    auto fail = [&](const std::wstring& msg) {
        d_->last_error = msg;
        std::fclose(f);
        return false;
    };

    // 读 FRM8 容器头 (4 + 8 + 4 = 16 bytes)
    uint8_t hdr[16];
    if (std::fread(hdr, 1, 16, f) != 16) return fail(L"DFF: short header");
    if (std::memcmp(hdr, "FRM8", 4) != 0) return fail(L"DFF: not a DSDIFF file (FRM8 missing)");
    if (std::memcmp(hdr + 12, "DSD ", 4) != 0) return fail(L"DFF: form type != DSD");

    uint64_t frmSize = readU64BE(hdr + 4);
    (void)frmSize;

    uint32_t channels = 0;
    uint32_t dsdRate  = 0;
    bool isRawDsd     = false;
    uint64_t dsdDataOff = 0;
    uint64_t dsdDataSize = 0;

    // 扫描内部 chunks
    auto seekPad = [&](uint64_t sz) {
        // DSDIFF chunk 数据后补齐到偶数
        if (sz & 1ULL) _fseeki64(f, 1, SEEK_CUR);
    };

    while (!std::feof(f)) {
        uint8_t ck[12];
        size_t got = std::fread(ck, 1, 12, f);
        if (got < 12) break;
        char id[5] = {0};
        std::memcpy(id, ck, 4);
        uint64_t sz = readU64BE(ck + 4);

        if (std::memcmp(id, "PROP", 4) == 0) {
            // PROP <u64 size> "SND " <sub chunks>
            // 损坏/恶意 DFF 可能给出极大 sz，限制 16 MiB 防 OOM。
            constexpr uint64_t kMaxPropSize = 16ull * 1024 * 1024;
            if (sz == 0 || sz > kMaxPropSize) {
                return fail(L"DFF: PROP size out of range");
            }
            std::vector<uint8_t> buf(static_cast<std::size_t>(sz));
            if (std::fread(buf.data(), 1, sz, f) != sz) {
                return fail(L"DFF: PROP read failed");
            }
            seekPad(sz);

            if (sz < 4 || std::memcmp(buf.data(), "SND ", 4) != 0) {
                return fail(L"DFF: PROP not SND");
            }
            size_t pos = 4;
            while (pos + 12 <= buf.size()) {
                char sid[5] = {0};
                std::memcpy(sid, buf.data() + pos, 4);
                uint64_t ssz = readU64BE(buf.data() + pos + 4);
                pos += 12;
                if (pos + ssz > buf.size()) break;

                if (std::memcmp(sid, "FS  ", 4) == 0 && ssz >= 4) {
                    dsdRate = readU32BE(buf.data() + pos);
                } else if (std::memcmp(sid, "CHNL", 4) == 0 && ssz >= 2) {
                    channels = readU16BE(buf.data() + pos);
                } else if (std::memcmp(sid, "CMPR", 4) == 0 && ssz >= 4) {
                    if (std::memcmp(buf.data() + pos, "DSD ", 4) == 0) {
                        isRawDsd = true;
                    }
                }
                pos += static_cast<size_t>(ssz);
                if (ssz & 1ULL) pos += 1;   // pad
            }
        } else if (std::memcmp(id, "DSD ", 4) == 0) {
            // 真正的 DSD raw 数据
            const __int64 here = _ftelli64(f);
            if (here < 0) return fail(L"DFF: ftelli64 failed");
            dsdDataOff  = static_cast<uint64_t>(here);
            dsdDataSize = sz;
            if (_fseeki64(f, static_cast<__int64>(sz), SEEK_CUR) != 0) {
                return fail(L"DFF: fseek over DSD chunk failed");
            }
            seekPad(sz);
        } else {
            // 跳过未识别 chunk (FVER / ID3 / DIIN / ...)
            if (_fseeki64(f, static_cast<__int64>(sz), SEEK_CUR) != 0) {
                return fail(L"DFF: fseek over unknown chunk failed");
            }
            seekPad(sz);
        }
    }

    if (channels == 0 || dsdRate == 0) {
        return fail(L"DFF: missing channels/sampleRate");
    }
    if (!isRawDsd) {
        return fail(L"DFF: only raw DSD (uncompressed) supported, not DST");
    }
    if (dsdDataSize == 0) {
        return fail(L"DFF: no DSD data chunk");
    }
    if ((dsdRate % 64) != 0) {
        return fail(L"DFF: DSD rate not multiple of 64");
    }

    d_->fp              = f;
    d_->channels        = channels;
    d_->dsd_rate        = dsdRate;
    d_->data_offset     = dsdDataOff;
    d_->data_size       = dsdDataSize;
    d_->dsd_samples     = (dsdDataSize / channels) * 8ULL;
    d_->cur_byte_offset = 0;
    d_->pcm_frame_count = 0;
    d_->eof             = false;

    // 定位到 data 开头
    _fseeki64(f, static_cast<__int64>(dsdDataOff), SEEK_SET);

    AudioFormat afmt;
    afmt.sample_rate     = dsdRate / 16;
    afmt.channels        = static_cast<std::uint16_t>(channels);
    afmt.bits_per_sample = 24;
    afmt.valid_bits      = 24;
    afmt.sample_type     = SampleType::Int24Packed;
    afmt.channel_mask    = default_channel_mask(static_cast<std::uint16_t>(channels));
    if (!afmt.valid()) {
        close();
        return fail(L"DFF produced invalid AudioFormat");
    }
    d_->fmt = afmt;
    return true;
}

bool DffDecoder::seek(std::int64_t frame)
{
    if (!d_->fp) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    auto total = totalFrames();
    if (total > 0 && frame > total) frame = total;

    // 1 PCM 帧 = 2 DSD bytes/channel = 2 * channels bytes in DFF interleave
    uint64_t byteOff = static_cast<uint64_t>(frame) * 2ULL * d_->channels;
    if (_fseeki64(d_->fp,
                  static_cast<__int64>(d_->data_offset + byteOff),
                  SEEK_SET) != 0) {
        d_->last_error = L"DFF seek failed";
        return false;
    }
    d_->cur_byte_offset = byteOff;
    d_->pcm_frame_count = static_cast<uint64_t>(frame);
    d_->eof = false;
    return true;
}

std::size_t DffDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->fp || !dst || bytes == 0 || d_->eof) return 0;
    const uint32_t channels   = d_->channels;
    const bool native = d_->native_dsd.load(std::memory_order_acquire);

    if (native) {
        // Native LSB8: 每 PCM 帧 = channels 字节, 来源是 1 个 DFF frame (channels bytes)
        // DFF 是 MSB-first 时间方向, 需 reverseBits 转 LSB-first
        const uint32_t frameBytes = channels;
        bytes -= (bytes % frameBytes);
        if (bytes == 0) return 0;

        std::size_t framesWanted = bytes / frameBytes;
        const std::size_t needed = framesWanted * channels;
        if (d_->read_buf.size() < needed) d_->read_buf.resize(needed);

        std::size_t got = std::fread(d_->read_buf.data(), 1, needed, d_->fp);
        if (got == 0) { d_->eof = true; return 0; }
        if (got < needed) std::memset(d_->read_buf.data() + got, 0, needed - got);
        const std::size_t framesAvailable = got / channels;
        if (framesAvailable < framesWanted) framesWanted = framesAvailable;

        std::uint8_t* out = dst;
        const uint8_t* tmp = d_->read_buf.data();
        for (std::size_t f = 0; f < framesWanted; ++f) {
            for (uint32_t ch = 0; ch < channels; ++ch) {
                out[ch] = reverseBits(tmp[f * channels + ch]);
            }
            out += channels;
            d_->pcm_frame_count += 1;
            d_->cur_byte_offset += channels;
            if (d_->pcm_frame_count * 8 >= d_->dsd_samples) {
                d_->eof = true;
                return (f + 1) * frameBytes;
            }
        }
        return framesWanted * frameBytes;
    }

    const uint32_t frameBytes = channels * 3;
    bytes -= (bytes % frameBytes);
    if (bytes == 0) return 0;

    std::size_t framesWanted   = bytes / frameBytes;
    std::size_t framesProduced = 0;
    std::uint8_t* out = dst;

    // 一次读 framesWanted 个 PCM 帧需要的 DSD 字节(每帧 2 byte/channel)。
    // read_buf 是 Impl 成员,resize 不缩,避免在实时路径反复分配
    const std::size_t needed = framesWanted * 2 * channels;
    if (d_->read_buf.size() < needed) d_->read_buf.resize(needed);

    std::size_t got = std::fread(d_->read_buf.data(), 1, needed, d_->fp);
    if (got == 0) { d_->eof = true; return 0; }
    if (got < needed) {
        std::memset(d_->read_buf.data() + got, 0, needed - got);
    }

    std::size_t framesAvailable = got / (2 * channels);
    if (framesAvailable < framesWanted) framesWanted = framesAvailable;

    const uint8_t* tmp = d_->read_buf.data();
    const DopMarkerMode mmode = d_->marker_mode.load(std::memory_order_acquire);
    for (std::size_t f = 0; f < framesWanted; ++f) {
        const uint64_t base_sample =
            (mmode == DopMarkerMode::PerSample)
                ? d_->pcm_frame_count * channels
                : 0;
        const uint8_t frame_marker = (d_->pcm_frame_count & 1ULL) ? 0x05 : 0xFA;
        for (uint32_t ch = 0; ch < channels; ++ch) {
            // DFF frame-interleave:第 i 个 channel 在每 frame 的 byte i
            // PCM 帧需要 2 个 DFF frame 的同一 channel 字节
            uint8_t b0 = tmp[(f * 2 + 0) * channels + ch];
            uint8_t b1 = tmp[(f * 2 + 1) * channels + ch];
            const uint8_t marker = (mmode == DopMarkerMode::PerSample)
                ? (((base_sample + ch) & 1ULL) ? 0x05 : 0xFA)
                : frame_marker;
            // DFF 已是 MSB-first 时间方向,不翻转
            out[0] = b1;
            out[1] = b0;
            out[2] = marker;
            out += 3;
        }
        d_->pcm_frame_count += 1;
        framesProduced      += 1;
        d_->cur_byte_offset += 2ULL * channels;

        if (d_->pcm_frame_count * 16 >= d_->dsd_samples) {
            d_->eof = true;
            break;
        }
    }
    return framesProduced * frameBytes;
}

} // namespace apx
