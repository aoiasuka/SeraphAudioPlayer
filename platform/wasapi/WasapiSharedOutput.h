// =============================================================================
//  platform/wasapi/WasapiSharedOutput.h
//
//  IAudioOutput 的 WASAPI 共享模式实现 (回退路径)。
//
//  与独占模式相比:
//    - 设备的 mix format 是固定的(由 Windows 取系统设定),不可协商。
//    - 我们的 AudioFormat 若与 mix format 不一致,内部用 FormatConverter
//      把每个 callback buffer 转一次(位深 + 线性插值重采样)。
//    - 延迟显著高于独占模式;音质受 Windows mixer 影响,不"bit-perfect"。
//
//  使用场景:独占模式 IsFormatSupported 失败 / 设备被其它进程占用时的兜底。
// =============================================================================
#pragma once

#include "core/output/IAudioOutput.h"

#include <memory>

namespace apx::wasapi {

class WasapiSharedOutput final : public IAudioOutput {
public:
    WasapiSharedOutput();
    ~WasapiSharedOutput() override;

    WasapiSharedOutput(const WasapiSharedOutput&)            = delete;
    WasapiSharedOutput& operator=(const WasapiSharedOutput&) = delete;

    bool         open(const AudioFormat& format,
                      const OpenOptions& opts,
                      OpenResult*        result) override;
    void         close() override;
    bool         start() override;
    void         stop() override;
    OutputState  state()     const override;
    std::wstring lastError() const override;
    void         setDataCallback(DataCallback cb) override;
    void         setErrorCallback(ErrorCallback cb) override;
    RenderStats  renderStats() const override;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx::wasapi
