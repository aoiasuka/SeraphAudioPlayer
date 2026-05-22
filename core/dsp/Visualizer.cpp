// =============================================================================
//  core/dsp/Visualizer.cpp
// =============================================================================
#include "Visualizer.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace apx {

namespace {

// 16 段中心频率 (Hz),60 ~ 16000 对数分布
constexpr float kCenters[Visualizer::kNumBands] = {
    60.f,    100.f,   165.f,   270.f,   440.f,
    700.f,   1150.f,  1900.f,  3100.f,  5000.f,
    8000.f,  12000.f, 14000.f, 15000.f, 15500.f, 16000.f
};
constexpr float kQ = 1.4f;

void designBandpass(Visualizer::Biquad& bq, float f0, float sr, float Q)
{
    if (f0 >= sr * 0.45f) f0 = sr * 0.45f;
    float w0 = 2.f * 3.14159265f * f0 / sr;
    float alpha = std::sin(w0) / (2.f * Q);
    float cos_w0 = std::cos(w0);

    // bandpass (constant 0 dB peak gain), RBJ cookbook
    float b0 = alpha;
    float b1 = 0.f;
    float b2 = -alpha;
    float a0 = 1.f + alpha;
    float a1 = -2.f * cos_w0;
    float a2 = 1.f - alpha;

    bq.b0 = b0 / a0;
    bq.b1 = b1 / a0;
    bq.b2 = b2 / a0;
    bq.a1 = a1 / a0;
    bq.a2 = a2 / a0;
    bq.reset();
}

// 把不同格式 sample 转 float [-1, 1]
inline float toFloat_S16(int16_t v) {
    return static_cast<float>(v) * (1.0f / 32768.f);
}
inline float toFloat_S32(int32_t v) {
    return static_cast<float>(v) * (1.0f / 2147483648.f);
}
// 24-bit packed LE → s32
inline int32_t s24To32(const uint8_t* p) {
    uint32_t u = uint32_t(p[0]) | (uint32_t(p[1]) << 8) | (uint32_t(p[2]) << 16);
    // 符号扩展
    if (u & 0x800000u) u |= 0xFF000000u;
    return static_cast<int32_t>(u);
}
inline float toFloat_S24(const uint8_t* p) {
    return static_cast<float>(s24To32(p)) * (1.0f / 8388608.f);
}
inline float toFloat_F32(float v) { return v; }

} // namespace

Visualizer::Visualizer() = default;

void Visualizer::reset()
{
    std::lock_guard<std::mutex> lk(mtx_);
    rms_l_acc_ = rms_r_acc_ = 0.0;
    rms_n_ = 0;
    vu_l_ = vu_r_ = peak_l_ = peak_r_ = 0;
    band_acc_.fill(0.0);
    band_disp_.fill(0.0f);
    for (auto& b : band_filters_) b.reset();
}

void Visualizer::rebuildFiltersIfNeeded(int sample_rate)
{
    if (sample_rate == sr_) return;
    sr_ = sample_rate;
    if (sample_rate <= 0) return;
    for (int i = 0; i < kNumBands; ++i) {
        designBandpass(band_filters_[i], kCenters[i],
                       static_cast<float>(sample_rate), kQ);
    }
}

void Visualizer::push(const std::uint8_t* data, std::size_t bytes, const AudioFormat& fmt)
{
    if (!data || bytes == 0) return;
    std::lock_guard<std::mutex> lk(mtx_);
    rebuildFiltersIfNeeded(static_cast<int>(fmt.sample_rate));
    if (sr_ <= 0 || fmt.channels == 0) return;

    const std::size_t fb = fmt.frame_bytes();
    if (fb == 0) return;
    const std::size_t frames = bytes / fb;

    // DSD 走 DoP 通路时 sample_type == Int24Packed, marker byte 在最高 byte,
    // 真实采样信息几乎不可视化 (是 1-bit 流);为了避免乱七八糟噪声,跳过。
    if (fmt.sample_rate >= 176400 && fmt.sample_type == SampleType::Int24Packed) {
        // 仍然推个零静默 VU,避免上一次的值卡死
        rms_l_acc_ += 0.0; rms_r_acc_ += 0.0; rms_n_ += static_cast<int>(frames);
        return;
    }

    const int chs = fmt.channels;
    auto getSample = [&](const std::uint8_t* p, int ch) -> float {
        switch (fmt.sample_type) {
        case SampleType::Int16:
            return toFloat_S16(reinterpret_cast<const int16_t*>(p)[ch]);
        case SampleType::Int24Packed:
            return toFloat_S24(p + ch * 3);
        case SampleType::Int32:
            return toFloat_S32(reinterpret_cast<const int32_t*>(p)[ch]);
        case SampleType::Float32:
            return toFloat_F32(reinterpret_cast<const float*>(p)[ch]);
        default:
            return 0.0f;
        }
    };

    // 为节省 CPU,只对每帧的左/右通道(0/1) 做累计;频谱用 (L+R)/2 单通道。
    const int rch = (chs >= 2) ? 1 : 0;
    const std::uint8_t* p = data;
    for (std::size_t i = 0; i < frames; ++i, p += fb) {
        float l = getSample(p, 0);
        float r = getSample(p, rch);
        if (!std::isfinite(l)) l = 0; if (!std::isfinite(r)) r = 0;
        rms_l_acc_ += l * l;
        rms_r_acc_ += r * r;
        ++rms_n_;
        if (std::abs(l) > peak_l_) peak_l_ = std::abs(l);
        if (std::abs(r) > peak_r_) peak_r_ = std::abs(r);

        float mono = 0.5f * (l + r);
        for (int b = 0; b < kNumBands; ++b) {
            float y = band_filters_[b].process(mono);
            band_acc_[b] += y * y;
        }
    }
}

Visualizer::Snapshot Visualizer::snapshot()
{
    std::lock_guard<std::mutex> lk(mtx_);
    Snapshot s;
    if (rms_n_ > 0) {
        float new_l = static_cast<float>(std::sqrt(rms_l_acc_ / rms_n_));
        float new_r = static_cast<float>(std::sqrt(rms_r_acc_ / rms_n_));
        // 慢衰减
        vu_l_ = std::max(new_l, vu_l_ * 0.85f);
        vu_r_ = std::max(new_r, vu_r_ * 0.85f);
        peak_l_ *= 0.92f;
        peak_r_ *= 0.92f;

        for (int b = 0; b < kNumBands; ++b) {
            float v = static_cast<float>(std::sqrt(band_acc_[b] / rms_n_));
            // 略微增益、对数压缩
            float norm = std::clamp(v * 4.0f, 0.0f, 1.0f);
            band_disp_[b] = std::max(norm, band_disp_[b] * 0.80f);
        }

        rms_l_acc_ = rms_r_acc_ = 0.0;
        band_acc_.fill(0.0);
        rms_n_ = 0;
    } else {
        vu_l_   *= 0.85f;
        vu_r_   *= 0.85f;
        peak_l_ *= 0.92f;
        peak_r_ *= 0.92f;
        for (auto& b : band_disp_) b *= 0.80f;
    }

    s.vu_left    = std::clamp(vu_l_,   0.0f, 1.0f);
    s.vu_right   = std::clamp(vu_r_,   0.0f, 1.0f);
    s.peak_left  = std::clamp(peak_l_, 0.0f, 1.0f);
    s.peak_right = std::clamp(peak_r_, 0.0f, 1.0f);
    s.bands = band_disp_;
    return s;
}

} // namespace apx
