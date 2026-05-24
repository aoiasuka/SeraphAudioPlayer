// =============================================================================
//  core/dsp/PolyphaseResampler.h
//
//  Windowed-sinc 多相 FIR 重采样器。质量介于"线性插值"与 SoXR/Speex 之间:
//    - 阻带 ~ -60 dB 以下 (Blackman 窗 + N=32 taps, L=64 phases)
//    - 通带平坦度足够 hi-res 听感
//    - 复杂度 O(N) per output sample per channel (CPU 友好)
//
//  非"通用"重采样器:
//    - 只做 float32 in → float32 out
//    - channels 必须一致 (无 up/down-mix)
//    - 任意 rate (in_rate, out_rate),内部用浮点 step
//
//  推荐当下作为 WasapiSharedOutput 的 fallback 重采样器使用,替换
//  FormatConverter 的原生线性插值。FormatConverter 仍保留以便快速 mock。
//
//  线程模型:单线程持有 (resampler 内部有状态)。
// =============================================================================
#pragma once

#include <cstddef>
#include <cstdint>
#include <vector>

namespace apx {

class PolyphaseResampler {
public:
    static constexpr int kTaps   = 32;   // 每个 phase 的 FIR 长度
    static constexpr int kPhases = 64;   // polyphase 分段数 (越大越细,内存 = kTaps*kPhases*4 bytes)

    PolyphaseResampler();
    ~PolyphaseResampler();

    // 重新配置 src/dst rate 与通道数。可重复调用,每次清状态。
    bool configure(int channels, double src_rate, double dst_rate);

    // 清状态(seek / 切歌时调)
    void reset();

    // 处理一批 src float 帧,写到 dst,返回写入的"目的帧数"。
    // src_frames 不一定全用完(尾巴若不够 kTaps 个,会暂存到下次)。
    std::size_t process(const float* src, std::size_t src_frames,
                        float*       dst, std::size_t dst_capacity_frames);

    int    channels() const { return channels_; }
    double srcRate()  const { return src_rate_; }
    double dstRate()  const { return dst_rate_; }

    // 真实运行时挑中的 SIMD 路径名(诊断用):"avx2" / "sse2" / "scalar"
    static const char* simdPath() noexcept;

private:
    void buildFilters();    // 在 configure() 内调用,生成 kPhases * kTaps 系数

    int    channels_   = 0;
    double src_rate_   = 0.0;
    double dst_rate_   = 0.0;
    double step_       = 1.0;    // src 帧步进 = src_rate / dst_rate

    // 历史缓冲布局:每通道 kTaps 个浮点,以"最新在 index 0"的线性序列存放,
    // 每次推入新样本用 memmove(history[1..]<-history[0..]) + history[0]=new。
    // 形状: [channels_][kTaps]。memmove 一次 128 字节(kTaps=32),性能可忽略,
    // 换来内层 dot product 可直接 SIMD,不需任何环形回绕。
    std::vector<float> history_;
    double    src_phase_  = 0.0;

    // 多相系数: [kPhases][kTaps],SSE2 友好布局(16-byte 对齐由 vector 自然提供)
    std::vector<float> filters_;
};

} // namespace apx
