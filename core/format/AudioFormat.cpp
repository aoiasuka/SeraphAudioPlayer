// =============================================================================
//  core/format/AudioFormat.cpp
// =============================================================================
#include "AudioFormat.h"

#include <sstream>

// 仅这里使用,避免在 header 中引入 windows.h
// 这些常量与 ksmedia.h 中 SPEAKER_* 完全一致(KSAUDIO_SPEAKER_*)
namespace {
constexpr std::uint32_t SPK_FRONT_LEFT   = 0x1;
constexpr std::uint32_t SPK_FRONT_RIGHT  = 0x2;
constexpr std::uint32_t SPK_FRONT_CENTER = 0x4;
} // namespace

namespace apx {

bool AudioFormat::valid() const noexcept
{
    if (sample_rate == 0 || channels == 0 || bits_per_sample == 0 || valid_bits == 0)
        return false;
    if (bits_per_sample % 8 != 0)        return false;
    if (valid_bits > bits_per_sample)    return false;
    switch (sample_type) {
    case SampleType::Int16:       return bits_per_sample == 16 && valid_bits == 16;
    case SampleType::Int24Packed: return bits_per_sample == 24 && valid_bits == 24;
    case SampleType::Int32:       return bits_per_sample == 32;       // valid_bits 可以是 24 或 32
    case SampleType::Float32:     return bits_per_sample == 32 && valid_bits == 32;
    }
    return false;
}

bool AudioFormat::operator==(const AudioFormat& o) const noexcept
{
    return sample_rate     == o.sample_rate
        && channels        == o.channels
        && bits_per_sample == o.bits_per_sample
        && valid_bits      == o.valid_bits
        && sample_type     == o.sample_type
        && channel_mask    == o.channel_mask;
}

std::wstring AudioFormat::to_wstring() const
{
    const wchar_t* tname = L"?";
    switch (sample_type) {
    case SampleType::Int16:       tname = L"int16";       break;
    case SampleType::Int24Packed: tname = L"int24packed"; break;
    case SampleType::Int32:       tname = L"int32";       break;
    case SampleType::Float32:     tname = L"float32";     break;
    }
    std::wostringstream ss;
    ss << sample_rate << L" Hz, "
       << channels    << L"ch, "
       << valid_bits  << L"/" << bits_per_sample << L"-bit "
       << tname;
    return ss.str();
}

std::uint32_t default_channel_mask(std::uint16_t channels) noexcept
{
    switch (channels) {
    case 1:  return SPK_FRONT_CENTER;
    case 2:  return SPK_FRONT_LEFT | SPK_FRONT_RIGHT;
    default: return 0;
    }
}

AudioFormat AudioFormat::pcm16(std::uint32_t sr, std::uint16_t ch)
{
    AudioFormat f;
    f.sample_rate = sr; f.channels = ch;
    f.bits_per_sample = 16; f.valid_bits = 16;
    f.sample_type = SampleType::Int16;
    f.channel_mask = default_channel_mask(ch);
    return f;
}

AudioFormat AudioFormat::pcm24in32(std::uint32_t sr, std::uint16_t ch)
{
    AudioFormat f;
    f.sample_rate = sr; f.channels = ch;
    f.bits_per_sample = 32; f.valid_bits = 24;
    f.sample_type = SampleType::Int32;
    f.channel_mask = default_channel_mask(ch);
    return f;
}

AudioFormat AudioFormat::pcm24(std::uint32_t sr, std::uint16_t ch)
{
    AudioFormat f;
    f.sample_rate = sr; f.channels = ch;
    f.bits_per_sample = 24; f.valid_bits = 24;
    f.sample_type = SampleType::Int24Packed;
    f.channel_mask = default_channel_mask(ch);
    return f;
}

AudioFormat AudioFormat::int32(std::uint32_t sr, std::uint16_t ch)
{
    AudioFormat f;
    f.sample_rate = sr; f.channels = ch;
    f.bits_per_sample = 32; f.valid_bits = 32;
    f.sample_type = SampleType::Int32;
    f.channel_mask = default_channel_mask(ch);
    return f;
}

AudioFormat AudioFormat::float32(std::uint32_t sr, std::uint16_t ch)
{
    AudioFormat f;
    f.sample_rate = sr; f.channels = ch;
    f.bits_per_sample = 32; f.valid_bits = 32;
    f.sample_type = SampleType::Float32;
    f.channel_mask = default_channel_mask(ch);
    return f;
}

} // namespace apx
