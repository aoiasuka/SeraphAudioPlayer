// =============================================================================
//  platform/asio/AsioOutput.h
//
//  ASIO 输出后端,实现 IAudioOutput 接口。
//
//  ⚠ 依赖 Steinberg ASIO SDK (asio.h / asio.cpp / asiodrivers.cpp 等)。
//    由于 SDK 许可不允许重新分发,源码不入库。请按下述方式手动启用:
//
//    1. 从 https://www.steinberg.net/asiosdk 下载 ASIO SDK 解压
//    2. 把 SDK 内容复制到:
//          third_party/asiosdk/common
//          third_party/asiosdk/host
//          third_party/asiosdk/host/pc
//    3. CMake 自动检测 third_party/asiosdk/common/asio.h 即启用 ASIO 编译
//
//  无 SDK 时编译为桩,所有方法均返回失败/空,不影响 WASAPI 工作。
// =============================================================================
#pragma once

#include "core/output/IAudioOutput.h"

#include <memory>
#include <string>
#include <vector>

namespace apx {

struct AsioDeviceInfo {
    int          index = -1;
    std::wstring name;
};

class AsioOutput final : public IAudioOutput {
public:
    AsioOutput();
    ~AsioOutput() override;

    // 枚举系统已安装的 ASIO 驱动
    static std::vector<AsioDeviceInfo> enumerate();
    // SDK 是否在编译时可用
    static bool sdkAvailable();

    // 在 open() 之前选择驱动索引;未调则用 0
    void setDeviceIndex(int idx);

    // IAudioOutput
    bool         open(const AudioFormat& format,
                      const OpenOptions& opts,
                      OpenResult*        result) override;
    void         close() override;
    bool         start() override;
    void         stop() override;
    OutputState  state() const override;
    std::wstring lastError() const override;
    void         setDataCallback(DataCallback cb) override;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
