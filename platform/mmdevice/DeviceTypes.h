// =============================================================================
//  platform/mmdevice/DeviceTypes.h
//
//  与 Windows 头文件解耦的设备类型 / 监听接口。
// =============================================================================
#pragma once

#include <cstdint>
#include <string>

namespace apx {

// 与 Windows DEVICE_STATE_* 一一对应
enum class DeviceState : std::uint8_t {
    Active     = 0,    // 设备可用
    Disabled   = 1,    // 用户在控制面板禁用
    NotPresent = 2,    // 设备不存在(罕见,旧驱动残留)
    Unplugged  = 3,    // 设备被拔出(物理离线)
};

// 默认设备角色
enum class DefaultRole : std::uint8_t {
    None           = 0,
    Console        = 1 << 0,
    Multimedia     = 1 << 1,
    Communications = 1 << 2,
};

inline DefaultRole operator|(DefaultRole a, DefaultRole b) {
    return static_cast<DefaultRole>(static_cast<std::uint8_t>(a) | static_cast<std::uint8_t>(b));
}
inline bool has_role(DefaultRole r, DefaultRole bit) {
    return (static_cast<std::uint8_t>(r) & static_cast<std::uint8_t>(bit)) != 0;
}

struct DeviceInfo {
    std::wstring id;            // IMMDevice::GetId 返回的字符串
    std::wstring friendly_name; // PKEY_Device_FriendlyName
    std::wstring desc;          // PKEY_DeviceInterface_FriendlyName(适配器/接口名)
    DeviceState  state = DeviceState::Active;
    DefaultRole  default_for = DefaultRole::None;

    bool is_default_console() const noexcept { return has_role(default_for, DefaultRole::Console); }
};

// 热插拔监听器。
// 注意:回调由 WASAPI 内部线程调入,实现方需自己处理跨线程同步
//      (例如通过 std::mutex / 投递到主线程消息队列)。
class IDeviceChangeListener {
public:
    virtual ~IDeviceChangeListener() = default;
    virtual void onDeviceAdded   (const std::wstring& /*id*/)                                {}
    virtual void onDeviceRemoved (const std::wstring& /*id*/)                                {}
    virtual void onDeviceStateChanged(const std::wstring& /*id*/, DeviceState /*new_state*/) {}
    virtual void onDefaultDeviceChanged(const std::wstring& /*id*/, DefaultRole /*role*/)    {}
};

} // namespace apx
