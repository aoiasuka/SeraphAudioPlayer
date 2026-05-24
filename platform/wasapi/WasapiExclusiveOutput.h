// =============================================================================
//  platform/wasapi/WasapiExclusiveOutput.h
//
//  IAudioOutput 的 WASAPI 独占模式实现。
//
//  .h 中刻意不暴露任何 windows.h / mmdeviceapi.h / audioclient.h 等头文件,
//  COM 接口由 .cpp 内的 pImpl 持有,降低传染性和编译耦合。
// =============================================================================
#pragma once

#include "core/output/IAudioOutput.h"

#include <memory>

namespace apx::wasapi {

class WasapiExclusiveOutput final : public IAudioOutput {
public:
    WasapiExclusiveOutput();
    ~WasapiExclusiveOutput() override;

    WasapiExclusiveOutput(const WasapiExclusiveOutput&)            = delete;
    WasapiExclusiveOutput& operator=(const WasapiExclusiveOutput&) = delete;

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
