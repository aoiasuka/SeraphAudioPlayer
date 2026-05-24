// =============================================================================
//  core/dsp/PolyphaseResampler_avx2.cpp
//
//  AVX2 内核 (独立编译,带 /arch:AVX2)。运行时由 PolyphaseResampler 根据
//  CPUID 决定是否调用本文件中的函数。kTaps=32 → AVX2 走 4 个 __m256,
//  相比 SSE2 的 8 个 __m128 大致再快 1.5–2 倍。
//
//  注意:本文件只能在 AVX2 可用的 CPU 上被实际调用,但符号本身可在任何 x86
//  二进制里链接。函数被调用前主文件必须做 CPUID 探测。
// =============================================================================
#if defined(_M_X64) || defined(_M_AMD64) || defined(__x86_64__) || defined(__AVX2__)

#include <immintrin.h>

namespace apx {

// 32-tap dot product (newest first 顺序的 hist[] · coef[])
float dot32_avx2(const float* a, const float* b) noexcept
{
    __m256 acc = _mm256_setzero_ps();
    for (int i = 0; i < 32; i += 8) {
        const __m256 va = _mm256_loadu_ps(a + i);
        const __m256 vb = _mm256_loadu_ps(b + i);
        acc = _mm256_add_ps(acc, _mm256_mul_ps(va, vb));
    }
    // horizontal reduce __m256 → float
    __m128 hi = _mm256_extractf128_ps(acc, 1);
    __m128 lo = _mm256_castps256_ps128(acc);
    __m128 s128 = _mm_add_ps(hi, lo);
    __m128 shuf = _mm_shuffle_ps(s128, s128, _MM_SHUFFLE(2, 3, 0, 1));
    s128        = _mm_add_ps(s128, shuf);
    shuf        = _mm_movehl_ps(shuf, s128);
    s128        = _mm_add_ss(s128, shuf);
    return _mm_cvtss_f32(s128);
}

// coef[t] = w_lo * f_lo[t] + w_hi * f_hi[t]  for t in [0, 32)
void combine32_avx2(const float* f_lo, const float* f_hi,
                    float w_lo, float w_hi, float* coef) noexcept
{
    const __m256 vlo = _mm256_set1_ps(w_lo);
    const __m256 vhi = _mm256_set1_ps(w_hi);
    for (int t = 0; t < 32; t += 8) {
        const __m256 a = _mm256_loadu_ps(f_lo + t);
        const __m256 b = _mm256_loadu_ps(f_hi + t);
        _mm256_store_ps(coef + t, _mm256_add_ps(_mm256_mul_ps(a, vlo),
                                                _mm256_mul_ps(b, vhi)));
    }
}

} // namespace apx

#endif  // x86_64
