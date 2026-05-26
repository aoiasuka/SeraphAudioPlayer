// =============================================================================
//  core/dsp/FormatConverter.cpp
// =============================================================================
#include "FormatConverter.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace apx {

namespace {

inline std::int32_t read_int24_packed(const std::uint8_t* p)
{
    std::uint32_t u = static_cast<std::uint32_t>(p[0])
                   | (static_cast<std::uint32_t>(p[1]) << 8)
                   | (static_cast<std::uint32_t>(p[2]) << 16);
    if (u & 0x00800000u) u |= 0xFF000000u;
    return static_cast<std::int32_t>(u);
}
inline void write_int24_packed(std::uint8_t* p, std::int32_t v)
{
    if (v >  8388607)  v =  8388607;
    if (v < -8388608)  v = -8388608;
    p[0] = static_cast<std::uint8_t>( v        & 0xFF);
    p[1] = static_cast<std::uint8_t>((v >>  8) & 0xFF);
    p[2] = static_cast<std::uint8_t>((v >> 16) & 0xFF);
}

// 将 src 单帧的 channels 个样本读成 float [-1, 1]
void load_frame(const std::uint8_t* src, const AudioFormat& fmt, float* out)
{
    const int ch = fmt.channels;
    switch (fmt.sample_type) {
    case SampleType::Int16: {
        auto* p = reinterpret_cast<const std::int16_t*>(src);
        for (int i = 0; i < ch; ++i) out[i] = static_cast<float>(p[i]) / 32768.0f;
        break;
    }
    case SampleType::Int24Packed: {
        for (int i = 0; i < ch; ++i) {
            const std::int32_t v = read_int24_packed(src + i * 3);
            out[i] = static_cast<float>(v) / 8388608.0f;
        }
        break;
    }
    case SampleType::Int32: {
        auto* p = reinterpret_cast<const std::int32_t*>(src);
        for (int i = 0; i < ch; ++i)
            out[i] = static_cast<float>(p[i]) / 2147483648.0f;
        break;
    }
    case SampleType::Float32: {
        auto* p = reinterpret_cast<const float*>(src);
        for (int i = 0; i < ch; ++i) out[i] = p[i];
        break;
    }
    case SampleType::DsdLsb8:
        // DSD 不参与 PCM 转换路径;调用方应避免触发这里
        for (int i = 0; i < ch; ++i) out[i] = 0.0f;
        break;
    }
}

// 将 float 帧写到 dst (按 fmt.sample_type)
void store_frame(std::uint8_t* dst, const AudioFormat& fmt, const float* in)
{
    const int ch = fmt.channels;
    switch (fmt.sample_type) {
    case SampleType::Int16: {
        auto* p = reinterpret_cast<std::int16_t*>(dst);
        for (int i = 0; i < ch; ++i) {
            float v = std::clamp(in[i], -1.0f, 1.0f) * 32767.0f;
            p[i] = static_cast<std::int16_t>(std::lrintf(v));
        }
        break;
    }
    case SampleType::Int24Packed: {
        for (int i = 0; i < ch; ++i) {
            float v = std::clamp(in[i], -1.0f, 1.0f) * 8388607.0f;
            write_int24_packed(dst + i * 3, static_cast<std::int32_t>(std::lrintf(v)));
        }
        break;
    }
    case SampleType::Int32: {
        auto* p = reinterpret_cast<std::int32_t*>(dst);
        for (int i = 0; i < ch; ++i) {
            // lrint 在 float 转 int32 时容易越界,用 double 中转
            double v = std::clamp(static_cast<double>(in[i]), -1.0, 1.0) * 2147483647.0;
            p[i] = static_cast<std::int32_t>(std::llround(v));
        }
        break;
    }
    case SampleType::Float32: {
        auto* p = reinterpret_cast<float*>(dst);
        for (int i = 0; i < ch; ++i) p[i] = in[i];
        break;
    }
    case SampleType::DsdLsb8:
        // 同 load_frame:不参与 PCM 路径,清零
        std::memset(dst, 0, ch);
        break;
    }
}

} // namespace

FormatConverter::FormatConverter()  = default;
FormatConverter::~FormatConverter() = default;

bool FormatConverter::configure(const AudioFormat& src, const AudioFormat& dst)
{
    if (!src.valid() || !dst.valid()) return false;
    if (src.channels != dst.channels) return false;

    src_ = src;
    dst_ = dst;
    ratio_ = static_cast<double>(dst.sample_rate) / static_cast<double>(src.sample_rate);
    reset();
    active_ = (src != dst);     // 完全一致就走 no-op fast path

    // 高质量路径需要预配置 resampler;只在采样率不同时启用
    if (active_ && high_quality_ && src.sample_rate != dst.sample_rate) {
        resampler_.configure(src.channels,
                             static_cast<double>(src.sample_rate),
                             static_cast<double>(dst.sample_rate));
    }
    dither_err_.assign(src.channels, 0.0f);
    // 预分配逐帧 buffer，避免实时路径上 std::vector 临时分配。
    frame_buf_.assign(src.channels, 0.0f);
    linear_a_.assign(src.channels, 0.0f);
    linear_b_.assign(src.channels, 0.0f);
    linear_out_.assign(src.channels, 0.0f);
    return true;
}

void FormatConverter::reset()
{
    phase_ = 0.0;
    resampler_.reset();
    std::fill(dither_err_.begin(), dither_err_.end(), 0.0f);
}

namespace {
// xorshift32 — 快、足以做 dither 噪声;非加密用途
inline std::uint32_t xs32(std::uint32_t& s) {
    s ^= s << 13; s ^= s >> 17; s ^= s << 5;
    return s;
}
// 返回 [-1, 1) 的均匀随机浮点
inline float urand_pm1(std::uint32_t& s) {
    // 上 24 位 → [0, 1),再映射到 [-1, 1)
    const std::uint32_t u = xs32(s);
    return (static_cast<float>(u >> 8) * (1.0f / 8388608.0f)) - 1.0f;
}
} // namespace

// 仅 Int16 dst 走这里:对 in_f[ch] (-1, 1] 应用 TPDF + noise shaping,
// 再 round 到 16-bit。其它 dst 走原 store_frame (调用方判定)
void FormatConverter::storeFrameWithDither(std::uint8_t* dst, const float* in_f)
{
    const int ch = dst_.channels;
    auto* p = reinterpret_cast<std::int16_t*>(dst);
    constexpr float kLSB = 1.0f / 32767.0f;   // 16-bit LSB in float [-1, 1) scale
    for (int c = 0; c < ch; ++c) {
        // 一阶 noise shaping:本帧输入 - 上一帧量化误差
        float x = in_f[c] - dither_err_[c];
        // TPDF 噪声:两路均匀 [-LSB, +LSB),相加得三角分布,峰值 ±2*LSB
        const float n1 = urand_pm1(rng_state_) * kLSB;
        const float n2 = urand_pm1(rng_state_) * kLSB;
        const float x_d = x + 0.5f * (n1 + n2);
        // 量化
        float v = std::clamp(x_d, -1.0f, 1.0f) * 32767.0f;
        const std::int16_t q = static_cast<std::int16_t>(std::lrintf(v));
        p[c] = q;
        // 记录量化误差(在 dither 之前的 x 与量化值之差),供下一帧 shaping 用
        dither_err_[c] = static_cast<float>(q) * kLSB - x;
    }
}

std::size_t FormatConverter::process(const std::uint8_t* src, std::size_t src_frames,
                                     std::uint8_t* dst, std::size_t dst_capacity_frames)
{
    if (!active_) {
        // 同格式 -> 直接 memcpy
        const std::size_t copy_frames = std::min(src_frames, dst_capacity_frames);
        std::memcpy(dst, src, copy_frames * src_.frame_bytes());
        return copy_frames;
    }
    if (src_frames == 0 || dst_capacity_frames == 0) return 0;

    const int ch = src_.channels;
    const std::uint32_t src_fb = src_.frame_bytes();
    const std::uint32_t dst_fb = dst_.frame_bytes();

    // 同采样率 → 只做位深/类型转换,逐帧
    if (src_.sample_rate == dst_.sample_rate) {
        const std::size_t n = std::min(src_frames, dst_capacity_frames);
        float* frame = frame_buf_.data();
        const bool use_dither = dither_
                              && dst_.sample_type == SampleType::Int16
                              && src_.sample_type != SampleType::Int16;
        for (std::size_t i = 0; i < n; ++i) {
            load_frame(src + i * src_fb, src_, frame);
            if (use_dither) storeFrameWithDither(dst + i * dst_fb, frame);
            else            store_frame(dst + i * dst_fb, dst_, frame);
        }
        return n;
    }

    // 不同采样率
    if (high_quality_) {
        // 高质量路径:src→float 平铺,PolyphaseResampler 做 SRC,float→dst
        const std::size_t need_src_f = src_frames * ch;
        const std::size_t need_dst_f = dst_capacity_frames * ch;
        if (src_f_.size() < need_src_f) src_f_.resize(need_src_f);
        if (dst_f_.size() < need_dst_f) dst_f_.resize(need_dst_f);
        for (std::size_t i = 0; i < src_frames; ++i)
            load_frame(src + i * src_fb, src_, src_f_.data() + i * ch);
        const std::size_t produced = resampler_.process(
            src_f_.data(), src_frames,
            dst_f_.data(), dst_capacity_frames);
        const bool use_dither = dither_
                              && dst_.sample_type == SampleType::Int16
                              && src_.sample_type != SampleType::Int16;
        for (std::size_t i = 0; i < produced; ++i) {
            if (use_dither) storeFrameWithDither(dst + i * dst_fb, dst_f_.data() + i * ch);
            else            store_frame(dst + i * dst_fb, dst_, dst_f_.data() + i * ch);
        }
        return produced;
    }

    // 低开销 fallback:线性插值
    const double step = 1.0 / ratio_;
    float* a   = linear_a_.data();
    float* b   = linear_b_.data();
    float* out = linear_out_.data();
    std::size_t dst_written = 0;
    const bool use_dither = dither_
                          && dst_.sample_type == SampleType::Int16
                          && src_.sample_type != SampleType::Int16;

    while (dst_written < dst_capacity_frames) {
        const double pos = phase_;
        const std::size_t i0 = static_cast<std::size_t>(pos);
        const std::size_t i1 = i0 + 1;
        if (i1 >= src_frames) break;        // 源数据不够再插一帧,留给下次

        const double frac = pos - static_cast<double>(i0);
        load_frame(src + i0 * src_fb, src_, a);
        load_frame(src + i1 * src_fb, src_, b);
        for (int c = 0; c < ch; ++c)
            out[c] = static_cast<float>(a[c] + (b[c] - a[c]) * frac);
        if (use_dither) storeFrameWithDither(dst + dst_written * dst_fb, out);
        else            store_frame(dst + dst_written * dst_fb, dst_, out);
        dst_written += 1;
        phase_ += step;
    }

    // 把 phase_ 减去本次已消耗的源帧数
    const double consumed = std::floor(phase_);
    phase_ -= consumed;
    (void)consumed;
    return dst_written;
}

} // namespace apx
