// =============================================================================
//  core/dsp/Equalizer.h
//
//  10 段 RBJ peaking EQ。处理 in-place,支持 Int16 / Int24Packed / Int32 /
//  Float32 输入。每通道独立 biquad 状态。
//
//  使用模式:
//      Equalizer eq;
//      eq.setEnabled(true);
//      eq.setGain(0, +6.0);     // 31 Hz +6 dB
//      ...
//      eq.process(pcm_data, bytes, fmt);   // 在 producer 线程或 callback 中
// =============================================================================
#pragma once

#include "core/format/AudioFormat.h"

#include <array>
#include <atomic>
#include <cstdint>
#include <mutex>
#include <vector>

namespace apx {

class Equalizer {
public:
    static constexpr int kNumBands = 10;
    // 中心频率 (Hz) — ISO 八度风格(31..16k)
    static constexpr float kCenters[kNumBands] = {
        31.f, 62.f, 125.f, 250.f, 500.f,
        1000.f, 2000.f, 4000.f, 8000.f, 16000.f
    };

    Equalizer();
    ~Equalizer() = default;

    Equalizer(const Equalizer&)            = delete;
    Equalizer& operator=(const Equalizer&) = delete;

    // 设置启用 / dB 增益
    void setEnabled(bool on)             { enabled_.store(on); }
    bool enabled() const                 { return enabled_.load(); }

    void setGain(int band, double db);
    double gain(int band) const;

    // 重置(切换设备/格式/seek 时调用,避免状态残留)
    void reset();

    // 处理 PCM(in-place)。在 producer 线程或 output callback 中调用均可。
    void process(std::uint8_t* data, std::size_t bytes, const AudioFormat& fmt);

private:
    struct Biquad {
        float b0=1, b1=0, b2=0, a1=0, a2=0;
        // 每通道独立状态(最多 8 ch),vector 在 prepare 时分配
        std::vector<float> x1, x2, y1, y2;
        void resize(int channels) {
            x1.assign(channels, 0); x2.assign(channels, 0);
            y1.assign(channels, 0); y2.assign(channels, 0);
        }
        void reset() { for (auto& v : x1) v = 0; for (auto& v : x2) v = 0;
                       for (auto& v : y1) v = 0; for (auto& v : y2) v = 0; }
        float process(int ch, float x) {
            float y = b0*x + b1*x1[ch] + b2*x2[ch] - a1*y1[ch] - a2*y2[ch];
            x2[ch] = x1[ch]; x1[ch] = x;
            y2[ch] = y1[ch]; y1[ch] = y;
            return y;
        }
    };

    void prepare(int sample_rate, int channels);
    void recomputeAll();
    void recomputeBand(int b);

    std::atomic<bool>            enabled_{false};

    std::mutex                   mtx_;
    int                          sr_       = 0;
    int                          ch_       = 0;
    std::array<double, kNumBands> gains_db_{}; // 默认 0
    std::array<Biquad, kNumBands> bands_{};
};

} // namespace apx
