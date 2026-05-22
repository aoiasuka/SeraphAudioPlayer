// =============================================================================
//  core/decoder/DecoderFactory.cpp
// =============================================================================
#include "DecoderFactory.h"
#include "WavDecoder.h"
#include "FlacDecoder.h"
#include "Mp3Decoder.h"
#include "DsdDecoder.h"
#include "DffDecoder.h"
#include "VorbisDecoder.h"

#include <cwctype>

namespace apx {

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
    const auto ext = extensionLower(path);
    if (ext == L".wav" || ext == L".wave") {
        return std::make_unique<WavDecoder>();
    }
    if (ext == L".flac") {
        return std::make_unique<FlacDecoder>();
    }
    if (ext == L".mp3") {
        return std::make_unique<Mp3Decoder>();
    }
    if (ext == L".dsf") {
        return std::make_unique<DsdDecoder>();
    }
    if (ext == L".dff") {
        return std::make_unique<DffDecoder>();
    }
    if (ext == L".ogg" || ext == L".oga") {
        return std::make_unique<VorbisDecoder>();
    }
    // 后续:.aac/.m4a → FFmpegDecoder, .ape → ApeDecoder, .opus → 待加 (opusfile)
    return nullptr;
}

} // namespace apx

