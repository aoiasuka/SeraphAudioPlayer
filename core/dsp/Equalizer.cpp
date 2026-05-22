// =============================================================================
//  core/dsp/Equalizer.cpp
// =============================================================================
#include "Equalizer.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace apx {

constexpr float Equalizer::kCenters[Equalizer::kNumBands];

namespace {
constexpr double kQ = 1.0;

inline int32_t s24To32(const uint8_t* p) {
    uint32_t u = uint32_t(p[0]) | (uint32_t(p[1]) << 8) | (uint32_t(p[2]) << 16);
    if (u & 0x800000u) u |= 0xFF000000u;
    return static_cast<int32_t>(u);
}
inline void s32To24(int32_t v, uint8_t* p) {
    p[0] = static_cast<uint8_t>( v        & 0xFF);
    p[1] = static_cast<uint8_t>((v >>  8) & 0xFF);
    p[2] = static_cast<uint8_t>((v >> 16) & 0xFF);
}
template <typename T>
T clampSat(double v) {
    constexpr double lo = static_cast<double>(std::numeric_limits<T>::min());
    constexpr double hi = static_cast<double>(std::numeric_limits<T>::max());
    if (v < lo) v = lo;
    if (v > hi) v = hi;
    return static_cast<T>(v);
}
} // namespace

Equalizer::Equalizer() = default;

void Equalizer::setGain(int band, double db)
{
    if (band < 0 || band >= kNumBands) return;
    std::lock_guard<std::mutex> lk(mtx_);
    db = std::clamp(db, -12.0, 12.0);
    if (gains_db_[band] == db) return;
    gains_db_[band] = db;
    recomputeBand(band);
}

double Equalizer::gain(int band) const
{
    if (band < 0 || band >= kNumBands) return 0.0;
    return gains_db_[band];
}

void Equalizer::reset()
{
    std::lock_guard<std::mutex> lk(mtx_);
    for (auto& b : bands_) b.reset();
}

void Equalizer::prepare(int sample_rate, int channels)
{
    if (sample_rate == sr_ && channels == ch_) return;
    sr_ = sample_rate;
    ch_ = channels;
    for (auto& b : bands_) b.resize(channels);
    recomputeAll();
}

void Equalizer::recomputeAll()
{
    for (int i = 0; i < kNumBands; ++i) recomputeBand(i);
}

void Equalizer::recomputeBand(int b)
{
    if (sr_ <= 0) return;
    double f0 = kCenters[b];
    if (f0 >= sr_ * 0.45) f0 = sr_ * 0.45;
    double A = std::pow(10.0, gains_db_[b] / 40.0);
    double w0 = 2.0 * 3.14159265358979323846 * f0 / sr_;
    double alpha = std::sin(w0) / (2.0 * kQ);
    double cosw0 = std::cos(w0);

    double b0 = 1 + alpha * A;
    double b1 = -2 * cosw0;
    double b2 = 1 - alpha * A;
    double a0 = 1 + alpha / A;
    double a1n = -2 * cosw0;
    double a2 = 1 - alpha / A;

    auto& bq = bands_[b];
    bq.b0 = static_cast<float>(b0 / a0);
    bq.b1 = static_cast<float>(b1 / a0);
    bq.b2 = static_cast<float>(b2 / a0);
    bq.a1 = static_cast<float>(a1n / a0);
    bq.a2 = static_cast<float>(a2 / a0);
}

void Equalizer::process(std::uint8_t* data, std::size_t bytes, const AudioFormat& fmt)
{
    if (!enabled_.load() || !data || bytes == 0) return;

    // 跳过 DSD/DoP (识别条件:24-bit 且采样率 >= 176.4k 是常见 DoP)
    if (fmt.sample_type == SampleType::Int24Packed && fmt.sample_rate >= 176400) return;

    const std::size_t fb = fmt.frame_bytes();
    if (fb == 0) return;
    const std::size_t frames = bytes / fb;
    const int channels = fmt.channels;
    if (channels == 0) return;

    std::lock_guard<std::mutex> lk(mtx_);
    prepare(static_cast<int>(fmt.sample_rate), channels);

    switch (fmt.sample_type) {
    case SampleType::Int16: {
        auto* p = reinterpret_cast<int16_t*>(data);
        for (std::size_t i = 0; i < frames; ++i) {
            for (int ch = 0; ch < channels; ++ch) {
                float x = static_cast<float>(p[ch]) * (1.0f / 32768.f);
                for (int b = 0; b < kNumBands; ++b) x = bands_[b].process(ch, x);
                p[ch] = clampSat<int16_t>(static_cast<double>(x) * 32768.0);
            }
            p += channels;
        }
        break;
    }
    case SampleType::Int24Packed: {
        std::uint8_t* p = data;
        for (std::size_t i = 0; i < frames; ++i) {
            for (int ch = 0; ch < channels; ++ch) {
                int32_t s = s24To32(p + ch * 3);
                float x = static_cast<float>(s) * (1.0f / 8388608.f);
                for (int b = 0; b < kNumBands; ++b) x = bands_[b].process(ch, x);
                int32_t out = clampSat<int32_t>(static_cast<double>(x) * 8388608.0);
                if (out > 8388607)  out = 8388607;
                if (out < -8388608) out = -8388608;
                s32To24(out, p + ch * 3);
            }
            p += channels * 3;
        }
        break;
    }
    case SampleType::Int32: {
        auto* p = reinterpret_cast<int32_t*>(data);
        for (std::size_t i = 0; i < frames; ++i) {
            for (int ch = 0; ch < channels; ++ch) {
                float x = static_cast<float>(p[ch]) * (1.0f / 2147483648.f);
                for (int b = 0; b < kNumBands; ++b) x = bands_[b].process(ch, x);
                p[ch] = clampSat<int32_t>(static_cast<double>(x) * 2147483648.0);
            }
            p += channels;
        }
        break;
    }
    case SampleType::Float32: {
        auto* p = reinterpret_cast<float*>(data);
        for (std::size_t i = 0; i < frames; ++i) {
            for (int ch = 0; ch < channels; ++ch) {
                float x = p[ch];
                for (int b = 0; b < kNumBands; ++b) x = bands_[b].process(ch, x);
                if (x > 1.0f)  x = 1.0f;
                if (x < -1.0f) x = -1.0f;
                p[ch] = x;
            }
            p += channels;
        }
        break;
    }
    default:
        break;
    }
}

} // namespace apx
