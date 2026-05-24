// =============================================================================
//  core/decoder/DsdDecoder.cpp
//
//  DSF -> DoP 转换。
//
//  DSF 文件结构 (Sony):
//    DSD chunk (28 字节):
//      4   "DSD "
//      8   chunk size (28)
//      8   total file size
//      8   metadata pointer (ID3v2 偏移;0=无)
//
//    fmt chunk (52 字节):
//      4   "fmt "
//      8   chunk size (52)
//      4   format version (1)
//      4   format ID (0 = DSD raw)
//      4   channel type
//      4   channel num
//      4   sample frequency (Hz, DSD 速率)
//      4   bits per sample (1 = LSB-first / 8 = MSB-first;DSF 规范多为 1)
//      8   sample count per channel (DSD 1-bit 样本数)
//      4   block size per channel (固定 4096)
//      4   reserved (0)
//
//    data chunk:
//      4   "data"
//      8   chunk size (= total - DSDhdr - fmt - 12)
//      N   交错 block:每 block 4096 byte/channel;先 ch0 整块,再 ch1 整块,...
//
//  DoP 打包:
//    每帧 = N_channels * 24-bit;每帧的 24-bit 由
//      [byte0=marker(0xFA/0x05 交替)] [byte1=DSD bits 15..8] [byte2=DSD bits 7..0]
//    构成。WASAPI 的 PCM-24 packed 是 LE,所以字节顺序在内存中:
//      offset 0 = byte2 (LSB), 1 = byte1, 2 = byte0 (MSB)
//    每帧消耗每通道 16 个 DSD bits = 2 bytes。
// =============================================================================
#include "DsdDecoder.h"

#include <atomic>
#include <cstdio>
#include <cstring>
#include <sstream>
#include <vector>

namespace apx {

namespace {

uint64_t readU64LE(const uint8_t* p) {
    uint64_t v = 0;
    for (int i = 0; i < 8; ++i) v |= (uint64_t(p[i]) << (i * 8));
    return v;
}
uint32_t readU32LE(const uint8_t* p) {
    return  uint32_t(p[0]) | (uint32_t(p[1]) << 8) |
           (uint32_t(p[2]) << 16) | (uint32_t(p[3]) << 24);
}

// 翻转 byte 中的 8 位顺序
inline uint8_t reverseBits(uint8_t b) {
    b = static_cast<uint8_t>(((b >> 1) & 0x55) | ((b & 0x55) << 1));
    b = static_cast<uint8_t>(((b >> 2) & 0x33) | ((b & 0x33) << 2));
    b = static_cast<uint8_t>(((b >> 4) & 0x0F) | ((b & 0x0F) << 4));
    return b;
}

} // namespace

struct DsdDecoder::Impl {
    FILE*        fp = nullptr;
    AudioFormat  fmt{};

    // DSF 信息
    uint32_t     channels        = 0;
    uint32_t     dsd_rate        = 0;   // DSD 采样率 (e.g. 2822400)
    uint8_t      bits_per_sample = 1;   // 1 = LSB-first, 8 = MSB-first
    uint64_t     dsd_samples     = 0;   // 每 channel 的 DSD 1-bit 样本数
    uint32_t     block_size      = 4096;
    uint64_t     data_offset     = 0;   // data chunk 中 raw 数据起点 (绝对文件偏移)
    uint64_t     data_size       = 0;

    // 流位置
    uint64_t     cur_block_index = 0;       // 已读 / 当前 block 编号
    uint32_t     cur_byte_in_block = 0;     // block 内已消耗字节 (每通道维度)
    std::vector<uint8_t> block_buf;         // block_size * channels
    bool         block_loaded = false;
    bool         eof = false;

    // DoP marker 在每 PCM 帧交替;按帧累计计数,取 [0xFA, 0x05] 的索引
    uint64_t     pcm_frame_count = 0;

    // marker 策略(可被 UI/控制线程动态切换)
    std::atomic<DopMarkerMode> marker_mode{DopMarkerMode::PerFrame};

    // 输出模式:false = DoP 24-bit, true = native LSB8 packed
    std::atomic<bool> native_dsd{false};

    std::wstring last_error;
};

DsdDecoder::DsdDecoder()  : d_(std::make_unique<Impl>()) {}
DsdDecoder::~DsdDecoder() { close(); }

bool         DsdDecoder::isOpen()       const { return d_->fp != nullptr; }
AudioFormat  DsdDecoder::format()       const { return d_->fmt; }
std::wstring DsdDecoder::lastError()    const { return d_->last_error; }

void          DsdDecoder::setMarkerMode(DopMarkerMode m) { d_->marker_mode.store(m, std::memory_order_release); }
DopMarkerMode DsdDecoder::markerMode() const             { return d_->marker_mode.load(std::memory_order_acquire); }

bool DsdDecoder::setNativeDsd(bool native)
{
    if (!d_->fp) { d_->last_error = L"setNativeDsd: not open"; return false; }
    if (d_->native_dsd.load() == native) return true;
    d_->native_dsd.store(native, std::memory_order_release);
    // 重写 fmt
    AudioFormat& afmt = d_->fmt;
    if (native) {
        afmt.sample_rate     = d_->dsd_rate;     // DSD rate, 不除 16
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

// 总帧数 = 每通道 DSD 样本数 / 16
std::int64_t DsdDecoder::totalFrames()  const {
    return static_cast<std::int64_t>(d_->dsd_samples / 16);
}
std::int64_t DsdDecoder::currentFrame() const {
    return static_cast<std::int64_t>(d_->pcm_frame_count);
}

void DsdDecoder::close()
{
    if (d_->fp) { std::fclose(d_->fp); d_->fp = nullptr; }
    d_->fmt = {};
    d_->channels = 0;
    d_->dsd_rate = 0;
    d_->bits_per_sample = 1;
    d_->dsd_samples = 0;
    d_->block_size = 4096;
    d_->data_offset = 0;
    d_->data_size = 0;
    d_->cur_block_index = 0;
    d_->cur_byte_in_block = 0;
    d_->block_loaded = false;
    d_->eof = false;
    d_->block_buf.clear();
    d_->pcm_frame_count = 0;
}

bool DsdDecoder::open(const std::wstring& path)
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

    // DSD header (28 bytes)
    uint8_t hdr[28];
    if (std::fread(hdr, 1, 28, f) != 28) return fail(L"DSF: short header");
    if (std::memcmp(hdr, "DSD ", 4) != 0) return fail(L"DSF: not a DSF file (missing DSD chunk)");

    // fmt chunk (52 bytes)
    uint8_t fmt[52];
    if (std::fread(fmt, 1, 52, f) != 52) return fail(L"DSF: short fmt chunk");
    if (std::memcmp(fmt, "fmt ", 4) != 0) return fail(L"DSF: missing fmt chunk");

    uint32_t formatId = readU32LE(fmt + 16);
    if (formatId != 0) return fail(L"DSF: unsupported format ID (only DSD raw supported)");

    uint32_t channels    = readU32LE(fmt + 24);
    uint32_t sampleFreq  = readU32LE(fmt + 28);
    uint32_t bitsPerSamp = readU32LE(fmt + 32);
    uint64_t sampleCount = readU64LE(fmt + 36);
    uint32_t blockSize   = readU32LE(fmt + 44);

    if (channels < 1 || channels > 8) {
        std::wostringstream ss; ss << L"DSF: invalid channels=" << channels;
        return fail(ss.str());
    }
    if (sampleFreq == 0 || (sampleFreq % 64) != 0) {
        std::wostringstream ss; ss << L"DSF: invalid sample frequency " << sampleFreq;
        return fail(ss.str());
    }
    if (bitsPerSamp != 1 && bitsPerSamp != 8) {
        std::wostringstream ss; ss << L"DSF: unsupported bits per sample " << bitsPerSamp;
        return fail(ss.str());
    }
    if (blockSize == 0 || blockSize > 65536) {
        std::wostringstream ss; ss << L"DSF: invalid block size " << blockSize;
        return fail(ss.str());
    }

    // data chunk
    uint8_t dataHdr[12];
    if (std::fread(dataHdr, 1, 12, f) != 12) return fail(L"DSF: short data chunk header");
    if (std::memcmp(dataHdr, "data", 4) != 0) return fail(L"DSF: missing data chunk");
    uint64_t dataSize = readU64LE(dataHdr + 4) - 12; // chunk size 含头本身

    long long dataOffset = _ftelli64(f);
    if (dataOffset < 0) return fail(L"DSF: ftelli64 failed");

    d_->fp              = f;
    d_->channels        = channels;
    d_->dsd_rate        = sampleFreq;
    d_->bits_per_sample = static_cast<uint8_t>(bitsPerSamp);
    d_->dsd_samples     = sampleCount;
    d_->block_size      = blockSize;
    d_->data_offset     = static_cast<uint64_t>(dataOffset);
    d_->data_size       = dataSize;
    d_->cur_block_index = 0;
    d_->cur_byte_in_block = 0;
    d_->block_loaded    = false;
    d_->eof             = false;
    d_->pcm_frame_count = 0;
    d_->block_buf.assign(static_cast<size_t>(blockSize) * channels, 0);

    AudioFormat afmt;
    afmt.sample_rate     = sampleFreq / 16;    // DoP PCM 速率
    afmt.channels        = static_cast<std::uint16_t>(channels);
    afmt.bits_per_sample = 24;
    afmt.valid_bits      = 24;
    afmt.sample_type     = SampleType::Int24Packed;
    afmt.channel_mask    = default_channel_mask(static_cast<std::uint16_t>(channels));
    if (!afmt.valid()) {
        close();
        return fail(L"DSF produced invalid AudioFormat");
    }
    d_->fmt = afmt;
    return true;
}

bool DsdDecoder::seek(std::int64_t frame)
{
    if (!d_->fp) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    auto total = totalFrames();
    if (total > 0 && frame > total) frame = total;

    // 1 PCM 帧 = 每通道 16 DSD bits = 2 byte/channel
    // block 包含 block_size 字节/channel = block_size * 8 DSD bits = block_size / 2 PCM 帧
    const uint64_t framesPerBlock = static_cast<uint64_t>(d_->block_size) / 2;
    if (framesPerBlock == 0) return false;

    uint64_t blockIdx = static_cast<uint64_t>(frame) / framesPerBlock;
    uint64_t frameInBlock = static_cast<uint64_t>(frame) % framesPerBlock;
    uint64_t byteInBlock  = frameInBlock * 2;  // 每帧消耗 2 byte/channel

    uint64_t fileOff = d_->data_offset
                     + blockIdx * (static_cast<uint64_t>(d_->block_size) * d_->channels);
    if (_fseeki64(d_->fp, static_cast<__int64>(fileOff), SEEK_SET) != 0) {
        d_->last_error = L"DSF seek failed";
        return false;
    }
    d_->cur_block_index   = blockIdx;
    d_->cur_byte_in_block = static_cast<uint32_t>(byteInBlock);
    d_->block_loaded      = false;
    d_->eof               = false;
    d_->pcm_frame_count   = static_cast<uint64_t>(frame);
    return true;
}

std::size_t DsdDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->fp || !dst || bytes == 0) return 0;
    const uint32_t channels  = d_->channels;
    const bool native = d_->native_dsd.load(std::memory_order_acquire);
    const uint32_t frameBytes = native ? channels : (channels * 3);

    bytes -= (bytes % frameBytes);
    if (bytes == 0 || d_->eof) return 0;

    std::size_t framesWanted = bytes / frameBytes;
    std::size_t framesProduced = 0;
    std::uint8_t* out = dst;

    const DopMarkerMode mmode = d_->marker_mode.load(std::memory_order_acquire);

    while (framesProduced < framesWanted && !d_->eof) {
        if (!d_->block_loaded) {
            // 读一个完整 block (block_size * channels 字节)
            size_t needed = static_cast<size_t>(d_->block_size) * channels;
            size_t got = std::fread(d_->block_buf.data(), 1, needed, d_->fp);
            if (got == 0) { d_->eof = true; break; }
            if (got < needed) {
                // 末尾不完整块,清零剩余
                std::memset(d_->block_buf.data() + got, 0, needed - got);
            }
            d_->block_loaded = true;
            d_->cur_block_index += 1;
        }

        if (native) {
            // Native LSB8 packed:每帧 channels 字节,从 block 当前 byte 取每通道一个 byte。
            // DSF bits_per_sample==1 已是 LSB-first,直接拷贝;==8 是 MSB-first,需 reverseBits.
            while (d_->cur_byte_in_block + 1 <= d_->block_size
                   && framesProduced < framesWanted) {
                for (uint32_t ch = 0; ch < channels; ++ch) {
                    const uint8_t* chData =
                        d_->block_buf.data() + static_cast<size_t>(ch) * d_->block_size;
                    uint8_t b = chData[d_->cur_byte_in_block];
                    if (d_->bits_per_sample == 8) b = reverseBits(b);
                    out[ch] = b;
                }
                out += channels;
                d_->cur_byte_in_block += 1;
                d_->pcm_frame_count   += 1;
                framesProduced        += 1;
                // EOF: native 每帧 = 8 DSD samples
                if (d_->pcm_frame_count * 8 >= d_->dsd_samples) {
                    d_->eof = true;
                    break;
                }
            }
            if (d_->cur_byte_in_block >= d_->block_size) {
                d_->block_loaded      = false;
                d_->cur_byte_in_block = 0;
            }
            continue;
        }

        // 每帧消耗每通道 2 byte。从 cur_byte_in_block 开始
        while (d_->cur_byte_in_block + 2 <= d_->block_size
               && framesProduced < framesWanted) {
            // marker 字节策略:
            //   PerFrame  → 整帧用一个 marker,帧间 0xFA<->0x05 交替
            //   PerSample → 每个 (frame, ch) 各自交替,粒度更细
            const uint64_t base_sample =
                (mmode == DopMarkerMode::PerSample)
                    ? d_->pcm_frame_count * channels
                    : 0;
            const uint8_t frame_marker = (d_->pcm_frame_count & 1ULL) ? 0x05 : 0xFA;
            for (uint32_t ch = 0; ch < channels; ++ch) {
                const uint8_t* chData =
                    d_->block_buf.data() + static_cast<size_t>(ch) * d_->block_size;
                uint8_t b0 = chData[d_->cur_byte_in_block + 0];
                uint8_t b1 = chData[d_->cur_byte_in_block + 1];
                // DSF 1-bit (bits_per_sample==1) 是 LSB-first;DoP 期望
                // MSB-first 时间顺序,需要翻转每个 byte 的位
                if (d_->bits_per_sample == 1) {
                    b0 = reverseBits(b0);
                    b1 = reverseBits(b1);
                }
                const uint8_t marker = (mmode == DopMarkerMode::PerSample)
                    ? (((base_sample + ch) & 1ULL) ? 0x05 : 0xFA)
                    : frame_marker;
                // PCM-24 packed LE:offset 0 = LSB ... offset 2 = MSB
                out[0] = b1;
                out[1] = b0;
                out[2] = marker;
                out += 3;
            }
            d_->cur_byte_in_block += 2;
            d_->pcm_frame_count   += 1;
            framesProduced        += 1;

            // 全局 DSD 样本结束?
            if (d_->pcm_frame_count * 16 >= d_->dsd_samples) {
                d_->eof = true;
                break;
            }
        }

        if (d_->cur_byte_in_block + 2 > d_->block_size) {
            d_->block_loaded     = false;
            d_->cur_byte_in_block = 0;
        }
    }
    return framesProduced * frameBytes;
}

} // namespace apx
