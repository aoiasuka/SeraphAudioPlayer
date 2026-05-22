// =============================================================================
//  core/dsp/Visualizer.h
//
//  音频可视化数据源:
//    - 由 producer 线程通过 push() 写入最近的 PCM 样本
//    - UI 线程通过 snapshot() 读取最近一帧的 VU 与 16 段频谱
//
//  实现策略:
//    - VU: 滑动 RMS,衰减系数 0.85,峰值衰减 0.92
//    - 频谱: 16 段并行 biquad 带通滤波,中心频率从 60 Hz 到 16 kHz 对数分布
//      每段输出本段 RMS,UI 取归一化后 0..1 值
//
//  线程:push() 与 snapshot() 之间靠 mutex 同步,频率约 30 Hz,无性能压力。
// =============================================================================
#pragma once

#include "core/format/AudioFormat.h"

#include <array>
#include <cstdint>
#include <mutex>

namespace apx {

class Visualizer {
public:
    static constexpr int kNumBands = 16;

    // biquad 带通滤波器系数 + 状态(public,供 .cpp 内自由函数访问)
    struct Biquad {
        float b0=0, b1=0, b2=0, a1=0, a2=0;
        float x1=0, x2=0, y1=0, y2=0;
        float process(float x) {
            float y = b0*x + b1*x1 + b2*x2 - a1*y1 - a2*y2;
            x2 = x1; x1 = x;
            y2 = y1; y1 = y;
            return y;
        }
        void reset() { x1=x2=y1=y2=0; }
    };

    Visualizer();
    ~Visualizer() = default;

    Visualizer(const Visualizer&)            = delete;
    Visualizer& operator=(const Visualizer&) = delete;

    // 在 producer 线程调用:把 PCM 块按 fmt 转 float 后喂给可视化
    void push(const std::uint8_t* data, std::size_t bytes, const AudioFormat& fmt);

    // 在 UI 线程调用:取当前 VU(左右)与频谱
    struct Snapshot {
        float vu_left  = 0.0f;
        float vu_right = 0.0f;
        float peak_left  = 0.0f;
        float peak_right = 0.0f;
        std::array<float, kNumBands> bands{};
    };
    Snapshot snapshot();

    // 清零状态(切歌/停止)
    void reset();

private:
    void rebuildFiltersIfNeeded(int sample_rate);

    std::mutex                            mtx_;
    int                                   sr_ = 0;
    // VU 累积(从上次 snapshot 起的)
    double                                rms_l_acc_ = 0.0;
    double                                rms_r_acc_ = 0.0;
    int                                   rms_n_     = 0;
    // 慢衰减保留显示值
    float                                 vu_l_ = 0, vu_r_ = 0;
    float                                 peak_l_ = 0, peak_r_ = 0;
    // 16 段带通能量累积
    std::array<double, kNumBands>         band_acc_{};
    std::array<float,  kNumBands>         band_disp_{};
    // 单通道 biquad (在 mono-sum 上跑)
    std::array<Biquad, kNumBands>         band_filters_{};
};

} // namespace apx
