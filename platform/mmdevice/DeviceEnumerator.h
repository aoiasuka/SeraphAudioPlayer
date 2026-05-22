// =============================================================================
//  platform/mmdevice/DeviceEnumerator.h
//
//  渲染端点枚举与热插拔通知封装。
//  .h 不暴露 windows.h / mmdeviceapi.h。
// =============================================================================
#pragma once

#include "platform/mmdevice/DeviceTypes.h"

#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace apx {

class DeviceEnumerator final {
public:
    // Impl 在此处前向声明仅为允许 .cpp 内的辅助类(NotifClient)访问其类型;
    // 实际定义在 .cpp 中,外部代码无法触达 Impl 的字段
    struct Impl;

    DeviceEnumerator();
    ~DeviceEnumerator();

    DeviceEnumerator(const DeviceEnumerator&)            = delete;
    DeviceEnumerator& operator=(const DeviceEnumerator&) = delete;

    // 列出所有 eRender 端点。
    //   include_inactive=false: 仅 Active
    //   include_inactive=true : Active + Disabled + Unplugged + NotPresent
    std::vector<DeviceInfo> listRenderEndpoints(bool include_inactive = false);

    // eConsole 默认渲染设备 id;失败返回空串。
    std::wstring defaultRenderId();

    // 精确按 id 查询(无论 state)。
    std::optional<DeviceInfo> findById(const std::wstring& id);

    // 在 friendly_name 中做大小写不敏感的子串匹配。多于一个时返回第一个 Active 设备。
    std::optional<DeviceInfo> findByNameSubstring(const std::wstring& sub);

    // 注册监听器。只允许一个监听器(注册第二次会覆盖前者)。
    // 返回 false 表示底层 RegisterEndpointNotificationCallback 失败,lastError() 含原因。
    bool registerListener(IDeviceChangeListener* listener);
    void unregisterListener();

    std::wstring lastError() const;

    // 显式初始化与释放(否则构造/析构会做)。允许重复调用。
    bool init();
    void shutdown();

private:
    std::unique_ptr<Impl> d_;
};

} // namespace apx
