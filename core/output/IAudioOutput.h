// =============================================================================
//  core/output/IAudioOutput.h
//
//  音频输出后端抽象。
//
//  生命周期:
//        ┌───────────────────────────────────────────────┐
//        ▼                                               │
//    Closed ──open()──▶ Stopped ──start()──▶ Running ────┘ stop()
//                          │                     │
//                          └────── close() ──────┘
//
//  线程模型:
//    - open / close / start / stop / setDataCallback 由"控制线程"调用,串行
//    - DataCallback 由实现内部的渲染线程调用(WASAPI 端是 AVRT Pro Audio 线程)
//    - 回调内严禁阻塞、加锁、IO;只做 memcpy / 简单生成
//
//  数据回调约定:
//    返回值 < bytes 时,实现会用 0 静音填充剩余部分,避免噪音 glitch。
// =============================================================================
#pragma once

#include "core/format/AudioFormat.h"

#include <cstddef>
#include <cstdint>
#include <functional>
#include <string>

namespace apx {

enum class OutputState : std::uint8_t {
    Closed,
    Stopped,
    Running,
    Error,
};

// 打开设备时的可选参数。空 device_id 表示使用系统默认渲染设备(eConsole)。
// 未来可扩展:event_driven、buffer_ms、exclusive 等。
struct OpenOptions {
    std::wstring device_id;     // 来自 DeviceEnumerator::findById/...
};

// 设备打开成功后,实现填充本结构告知调用方实际协商参数。
struct OpenResult {
    AudioFormat    actual_format{};
    std::uint32_t  buffer_frames = 0;       // 设备缓冲区帧数(独占模式)
    double         buffer_ms     = 0.0;     // 等价毫秒
    double         period_ms     = 0.0;     // 设备默认周期
    std::wstring   device_name;
    std::wstring   device_id;
};

class IAudioOutput {
public:
    // 渲染线程会反复调用此回调,要求最多写 dst[0..bytes) 字节。
    // 返回实际写入的字节数;< bytes 时由输出端静音补齐。
    using DataCallback = std::function<std::size_t(std::uint8_t* dst, std::size_t bytes)>;

    virtual ~IAudioOutput() = default;

    // 按 format 协商独占模式格式。
    //   opts.device_id 为空 → 默认设备
    //   opts.device_id 非空 → 严格命中,失败返回 false(不悄悄换设备)
    virtual bool open(const AudioFormat& format,
                      const OpenOptions& opts,
                      OpenResult*        result) = 0;

    // 释放所有资源,回到 Closed。
    virtual void close() = 0;

    // 启动渲染线程,进入 Running。setDataCallback 必须先于 start 调用。
    virtual bool start() = 0;

    // 停止渲染线程并等待退出,回到 Stopped。
    virtual void stop() = 0;

    virtual OutputState  state()     const = 0;
    virtual std::wstring lastError() const = 0;

    // 必须在 start() 之前设置。设备运行中替换回调的行为未定义。
    virtual void setDataCallback(DataCallback cb) = 0;
};

} // namespace apx
