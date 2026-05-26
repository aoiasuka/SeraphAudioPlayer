// =============================================================================
//  core/dsp/PolyphaseResampler.cpp
//
//  原理 (短版):
//    输出帧坐标 n_out 对应源坐标 n_src = n_out * (src_rate/dst_rate);
//    src_phase = n_src 的小数部分(取负作为对当前 history 最新样本的偏移)。
//    把 [0, 1) 的 src_phase 量化到 kPhases 个 sub-phase,各 sub-phase 是预先生成
//    的一组 kTaps 抽头 FIR;phase 内部小数部分 frac 用线性插值在相邻两组系数间。
//    history 保持 kTaps 个最新源样本 (环形),每步生成一个目的帧:
//        y[ch] = Σ_k history[ch][k] * filter[ph][k]  (粗略)
//
//  滤波器:windowed-sinc with Blackman window, cutoff = 0.5/L (Nyquist of upsampled grid)
//  其中 L = max(1, ceil(src_rate/dst_rate)) 给降采样时收紧 cutoff,防混叠。
//  这里简化为 cutoff = 0.5 * min(1.0, dst_rate/src_rate),与 L 取代;够用。
// =============================================================================
#include "PolyphaseResampler.h"

#include <algorithm>
#include <cmath>
#include <cstring>

#if defined(_M_X64) || defined(_M_AMD64) || defined(__x86_64__) || (defined(_M_IX86_FP) && _M_IX86_FP >= 2) || defined(__SSE2__)
#  include <emmintrin.h>   // SSE2 intrinsics
#  define APX_HAVE_SSE2 1
#else
#  define APX_HAVE_SSE2 0
#endif

#if defined(_MSC_VER) && (defined(_M_X64) || defined(_M_IX86))
#  include <intrin.h>      // __cpuid / __cpuidex / _xgetbv
#  define APX_X86 1
#else
#  define APX_X86 0
#endif

namespace apx {

// 来自 PolyphaseResampler_avx2.cpp (独立 /arch:AVX2 编译);未编译时这些符号不
// 存在 — 我们通过运行时探测保证只在确实有 AVX2 的 CPU 上调用它们。
#if defined(_M_X64) || defined(_M_AMD64) || defined(__x86_64__)
extern float dot32_avx2(const float* a, const float* b) noexcept;
extern void  combine32_avx2(const float* f_lo, const float* f_hi,
                            float w_lo, float w_hi, float* coef) noexcept;
#  define APX_HAVE_AVX2_BUILD 1
#else
#  define APX_HAVE_AVX2_BUILD 0
#endif

namespace {

// ---- CPU 特性运行时探测 ----
struct CpuCaps {
    bool has_avx2 = false;
};
CpuCaps detect_cpu() noexcept
{
    CpuCaps c;
#if APX_X86
    int info[4] = {0,0,0,0};
    __cpuid(info, 0);
    const int max_leaf = info[0];
    if (max_leaf >= 1) {
        __cpuid(info, 1);
        const bool osxsave = (info[2] & (1 << 27)) != 0;
        const bool avx     = (info[2] & (1 << 28)) != 0;
        bool ymm_enabled = false;
        if (osxsave && avx) {
            const unsigned long long xcr0 = _xgetbv(0);
            ymm_enabled = (xcr0 & 0x6) == 0x6;
        }
        if (max_leaf >= 7 && ymm_enabled) {
            __cpuidex(info, 7, 0);
            c.has_avx2 = (info[1] & (1 << 5)) != 0;
        }
    }
#endif
    return c;
}

const CpuCaps& cpu_caps() {
    static const CpuCaps c = detect_cpu();
    return c;
}

const char* simd_path_str() {
#if APX_HAVE_AVX2_BUILD
    if (cpu_caps().has_avx2) return "avx2";
#endif
#if APX_HAVE_SSE2
    return "sse2";
#else
    return "scalar";
#endif
}

constexpr double kPi = 3.14159265358979323846;

inline double sinc(double x)
{
    if (x == 0.0) return 1.0;
    const double px = kPi * x;
    return std::sin(px) / px;
}

inline double blackman(int n, int N)
{
    const double a = 2.0 * kPi * n / (N - 1);
    return 0.42 - 0.5 * std::cos(a) + 0.08 * std::cos(2.0 * a);
}

// SSE2 / scalar dot product
inline float dot32_sse2(const float* a, const float* b)
{
#if APX_HAVE_SSE2
    __m128 acc = _mm_setzero_ps();
    for (int i = 0; i < 32; i += 4) {
        const __m128 va = _mm_loadu_ps(a + i);
        const __m128 vb = _mm_loadu_ps(b + i);
        acc = _mm_add_ps(acc, _mm_mul_ps(va, vb));
    }
    __m128 shuf = _mm_shuffle_ps(acc, acc, _MM_SHUFFLE(2, 3, 0, 1));
    __m128 sums = _mm_add_ps(acc, shuf);
    shuf        = _mm_movehl_ps(shuf, sums);
    sums        = _mm_add_ss(sums, shuf);
    return _mm_cvtss_f32(sums);
#else
    double acc = 0.0;
    for (int i = 0; i < 32; ++i) acc += static_cast<double>(a[i]) * b[i];
    return static_cast<float>(acc);
#endif
}

inline void combine32_sse2(const float* f_lo, const float* f_hi,
                           float w_lo, float w_hi, float* coef)
{
#if APX_HAVE_SSE2
    const __m128 vlo = _mm_set1_ps(w_lo);
    const __m128 vhi = _mm_set1_ps(w_hi);
    for (int t = 0; t < 32; t += 4) {
        const __m128 a = _mm_loadu_ps(f_lo + t);
        const __m128 b = _mm_loadu_ps(f_hi + t);
        _mm_store_ps(coef + t, _mm_add_ps(_mm_mul_ps(a, vlo),
                                          _mm_mul_ps(b, vhi)));
    }
#else
    for (int t = 0; t < 32; ++t) coef[t] = w_lo * f_lo[t] + w_hi * f_hi[t];
#endif
}

inline float dot32(const float* a, const float* b)
{
#if APX_HAVE_AVX2_BUILD
    if (cpu_caps().has_avx2) return dot32_avx2(a, b);
#endif
    return dot32_sse2(a, b);
}

inline void combine32(const float* f_lo, const float* f_hi,
                      float w_lo, float w_hi, float* coef)
{
#if APX_HAVE_AVX2_BUILD
    if (cpu_caps().has_avx2) { combine32_avx2(f_lo, f_hi, w_lo, w_hi, coef); return; }
#endif
    combine32_sse2(f_lo, f_hi, w_lo, w_hi, coef);
}

} // namespace

const char* PolyphaseResampler::simdPath() noexcept { return simd_path_str(); }

PolyphaseResampler::PolyphaseResampler()  = default;
PolyphaseResampler::~PolyphaseResampler() = default;

bool PolyphaseResampler::configure(int channels, double src_rate, double dst_rate)
{
    if (channels <= 0 || src_rate <= 0 || dst_rate <= 0) return false;
    channels_  = channels;
    src_rate_  = src_rate;
    dst_rate_  = dst_rate;
    step_      = src_rate / dst_rate;

    history_.assign(static_cast<std::size_t>(channels_) * kTaps, 0.0f);
    src_phase_ = 0.0;

    buildFilters();
    return true;
}

void PolyphaseResampler::reset()
{
    std::fill(history_.begin(), history_.end(), 0.0f);
    src_phase_ = 0.0;
}

void PolyphaseResampler::buildFilters()
{
    filters_.assign(static_cast<std::size_t>(kPhases) * kTaps, 0.0f);
    const double cutoff =
        (dst_rate_ < src_rate_) ? (dst_rate_ / src_rate_) : 1.0;
    const int N = kTaps * kPhases;
    const double center = (N - 1) / 2.0;
    std::vector<double> raw(static_cast<std::size_t>(N));
    for (int n = 0; n < N; ++n) {
        const double t = (n - center) / static_cast<double>(kPhases);
        raw[n] = cutoff * sinc(cutoff * t) * blackman(n, N);
    }
    // 拆分到 [phase][tap]:phase = n % kPhases,tap = n / kPhases
    // 与 history "newest first" 对应:tap 0 = 最旧, tap kTaps-1 = 最新
    // 不,等等 — 让 tap 0 = 最新更直观,我们把 tap 反一下:
    for (int n = 0; n < N; ++n) {
        const int phase = n % kPhases;
        const int tap   = (kTaps - 1) - (n / kPhases);    // 翻转,使 tap 0 对应最新样本
        filters_[static_cast<std::size_t>(phase) * kTaps + tap] =
            static_cast<float>(raw[n]);
    }
    // 各 phase 归一化
    for (int ph = 0; ph < kPhases; ++ph) {
        double s = 0.0;
        for (int t = 0; t < kTaps; ++t)
            s += filters_[static_cast<std::size_t>(ph) * kTaps + t];
        if (s > 1e-12) {
            const float inv = static_cast<float>(1.0 / s);
            for (int t = 0; t < kTaps; ++t)
                filters_[static_cast<std::size_t>(ph) * kTaps + t] *= inv;
        }
    }
}

std::size_t PolyphaseResampler::process(const float* src, std::size_t src_frames,
                                        float*       dst, std::size_t dst_capacity_frames)
{
    if (channels_ <= 0 || src_frames == 0 || dst_capacity_frames == 0) return 0;

    std::size_t produced = 0;
    std::size_t consumed = 0;
    // 每输出 sample 都要 combined coef = w_lo*f_lo + w_hi*f_hi;用 stack buffer 复用
    // alignas(32): AVX2 内核用 _mm256_store_ps 要求 32 字节对齐,
    // SSE2 内核 _mm_store_ps 要求 16 字节也顺带满足;
    // 此前 alignas(16) 在 AVX2 路径上是真 bug(GP fault)。
    alignas(32) float coef[kTaps];

    while (produced < dst_capacity_frames) {
        if (src_phase_ > 0.0) {
            // 需要更多 src 帧才能生成下一目的样本
            if (consumed >= src_frames) break;
            // 推入一个新 src 帧:每通道 history[1..]<-history[0..],history[0]=new
            const float* s = src + consumed * channels_;
            for (int c = 0; c < channels_; ++c) {
                float* h = &history_[static_cast<std::size_t>(c) * kTaps];
                std::memmove(h + 1, h, sizeof(float) * (kTaps - 1));
                h[0] = s[c];
            }
            consumed   += 1;
            src_phase_ -= 1.0;
            continue;
        }

        // src_phase_ ∈ (-1, 0];映射到 phase index
        const double frac = -src_phase_;       // ∈ [0, 1)
        const double phase_f = frac * kPhases;
        int    ph_lo  = static_cast<int>(std::floor(phase_f));
        if (ph_lo >= kPhases) ph_lo = kPhases - 1;
        if (ph_lo < 0)        ph_lo = 0;
        int    ph_hi  = ph_lo + 1;
        float  w_hi   = static_cast<float>(phase_f - ph_lo);
        float  w_lo   = 1.0f - w_hi;
        if (ph_hi >= kPhases) { ph_hi = ph_lo; w_lo = 1.0f; w_hi = 0.0f; }

        const float* f_lo = &filters_[static_cast<std::size_t>(ph_lo) * kTaps];
        const float* f_hi = &filters_[static_cast<std::size_t>(ph_hi) * kTaps];

        combine32(f_lo, f_hi, w_lo, w_hi, coef);

        for (int c = 0; c < channels_; ++c) {
            const float* hist = &history_[static_cast<std::size_t>(c) * kTaps];
            dst[produced * channels_ + c] = dot32(hist, coef);
        }

        produced   += 1;
        src_phase_ += step_;
    }

    return produced;
}

} // namespace apx
