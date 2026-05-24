// =============================================================================
//  core/decoder/DecoderFactory.cpp
//
//  分发策略:
//    1) 先按文件扩展名命中(便宜、覆盖 95%)
//    2) 扩展名无匹配 / 扩展名缺失时,读取前 16 字节做魔数嗅探;
//       这样误命名 (.dat、.bin、.mp3 实为 flac …) 也能识别
// =============================================================================
#include "DecoderFactory.h"
#include "WavDecoder.h"
#include "FlacDecoder.h"
#include "Mp3Decoder.h"
#include "DsdDecoder.h"
#include "DffDecoder.h"
#include "VorbisDecoder.h"
#include "AacDecoder.h"
#include "M4aDecoder.h"
#include "OpusDecoder.h"

#include <cstdio>
#include <cstring>
#include <cwctype>

namespace apx {

namespace {

// 读文件前 N 字节(不影响后续 IDecoder::open,各 decoder 自己 fopen)
std::size_t read_head(const std::wstring& path, std::uint8_t* buf, std::size_t n)
{
    FILE* fp = nullptr;
    if (_wfopen_s(&fp, path.c_str(), L"rb") != 0 || !fp) return 0;
    std::size_t got = std::fread(buf, 1, n, fp);
    std::fclose(fp);
    return got;
}

enum class Kind { Unknown, Wav, Flac, Mp3, Dsf, Dff, Ogg, Aac, M4a, Opus };

// 通过魔数判定容器
Kind sniff_kind(const std::uint8_t* h, std::size_t n)
{
    if (n >= 12 && std::memcmp(h, "RIFF", 4) == 0 && std::memcmp(h + 8, "WAVE", 4) == 0)
        return Kind::Wav;
    if (n >= 12 && std::memcmp(h, "RF64", 4) == 0 && std::memcmp(h + 8, "WAVE", 4) == 0)
        return Kind::Wav;
    if (n >= 16 && std::memcmp(h, "riff", 4) == 0)  // Wave64 (GUID 前 4 字节为小写 riff)
        return Kind::Wav;
    if (n >= 4 && std::memcmp(h, "fLaC", 4) == 0) return Kind::Flac;
    if (n >= 16 && std::memcmp(h,      "FRM8", 4) == 0
                && std::memcmp(h + 12, "DSD ", 4) == 0) return Kind::Dff;
    if (n >= 4 && std::memcmp(h, "DSD ", 4) == 0) return Kind::Dsf;
    // OggS: 还需要在容器内区分 Vorbis 与 Opus;Opus 第一包是 "OpusHead"(标识在 8..16 字节)
    // 这里只能在拿到容器内更多字节后判;先做粗判,精细分发交给扩展名兜底
    if (n >= 4 && std::memcmp(h, "OggS", 4) == 0) return Kind::Ogg;
    // ISO Base Media (MP4/M4A): "ftyp" 在 offset 4
    if (n >= 12 && std::memcmp(h + 4, "ftyp", 4) == 0) return Kind::M4a;
    // ADTS AAC sync word: 0xFFF0 ~ 0xFFFF, 高 12 位全 1, 第 13 位为 layer (0 for AAC)
    if (n >= 2 && h[0] == 0xFF && (h[1] & 0xF6) == 0xF0) return Kind::Aac;
    // MP3: ID3v2 头 / MPEG sync(0xFFE0+)
    if (n >= 3 && std::memcmp(h, "ID3", 3) == 0) return Kind::Mp3;
    if (n >= 2 && h[0] == 0xFF && (h[1] & 0xE0) == 0xE0) return Kind::Mp3;
    return Kind::Unknown;
}

std::unique_ptr<IDecoder> make(Kind k)
{
    switch (k) {
    case Kind::Wav:  return std::make_unique<WavDecoder>();
    case Kind::Flac: return std::make_unique<FlacDecoder>();
    case Kind::Mp3:  return std::make_unique<Mp3Decoder>();
    case Kind::Dsf:  return std::make_unique<DsdDecoder>();
    case Kind::Dff:  return std::make_unique<DffDecoder>();
    case Kind::Ogg:  return std::make_unique<VorbisDecoder>();
    case Kind::Aac:  return std::make_unique<AacDecoder>();
    case Kind::M4a:  return std::make_unique<M4aDecoder>();
    case Kind::Opus: return std::make_unique<OpusDecoder>();
    default:         return nullptr;
    }
}

} // namespace

std::wstring DecoderFactory::extensionLower(const std::wstring& path)
{
    const auto pos_dot = path.find_last_of(L'.');
    if (pos_dot == std::wstring::npos) return L"";
    const auto pos_sep = path.find_last_of(L"\\/");
    if (pos_sep != std::wstring::npos && pos_dot < pos_sep) return L"";

    std::wstring ext = path.substr(pos_dot);
    for (auto& c : ext) c = static_cast<wchar_t>(std::towlower(c));
    return ext;
}

std::unique_ptr<IDecoder> DecoderFactory::createForFile(const std::wstring& path)
{
    Kind by_ext = Kind::Unknown;
    const auto ext = extensionLower(path);
    if      (ext == L".wav" || ext == L".wave" || ext == L".w64") by_ext = Kind::Wav;
    else if (ext == L".flac")                                      by_ext = Kind::Flac;
    else if (ext == L".mp3")                                       by_ext = Kind::Mp3;
    else if (ext == L".dsf")                                       by_ext = Kind::Dsf;
    else if (ext == L".dff")                                       by_ext = Kind::Dff;
    else if (ext == L".ogg" || ext == L".oga")                     by_ext = Kind::Ogg;
    else if (ext == L".opus")                                      by_ext = Kind::Opus;
    else if (ext == L".aac")                                       by_ext = Kind::Aac;
    else if (ext == L".m4a" || ext == L".mp4")                     by_ext = Kind::M4a;

    // 读 16 字节做魔数检查
    std::uint8_t head[16] = {};
    const std::size_t got = read_head(path, head, sizeof(head));
    Kind by_magic = sniff_kind(head, got);

    // OggS:扩展名告知是 Opus 就走 Opus,否则 Vorbis
    if (by_magic == Kind::Ogg && by_ext == Kind::Opus) by_magic = Kind::Opus;

    // 1) 嗅探命中优先采信
    if (by_magic != Kind::Unknown) return make(by_magic);
    // 2) 扩展名兜底
    if (by_ext   != Kind::Unknown) return make(by_ext);
    return nullptr;
}

} // namespace apx

