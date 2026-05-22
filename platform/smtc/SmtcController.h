// =============================================================================
//  platform/smtc/SmtcController.h
//
//  封装 Windows.Media.SystemMediaTransportControls,把媒体键、蓝牙耳机控制、
//  系统媒体浮层(Win+G、锁屏)等系统级输入接到本播放器。
//
//  必须在主线程(创建了消息循环并持有窗口的线程)上 initialize();
//  按钮事件回调发生在 WinRT 派发线程,调用方需自行 marshal 到 UI 线程。
// =============================================================================
#pragma once

#include <cstdint>
#include <functional>
#include <memory>
#include <string>

namespace apx {

enum class SmtcButton {
    Play,
    Pause,
    Stop,
    Next,
    Previous
};

enum class SmtcStatus {
    Closed,
    Stopped,
    Playing,
    Paused
};

class SmtcController {
public:
    SmtcController();
    ~SmtcController();

    SmtcController(const SmtcController&)            = delete;
    SmtcController& operator=(const SmtcController&) = delete;

    // 绑定到主窗口的 HWND。SystemMediaTransportControls::GetForWindow
    // 在 Win10 1607+ 始终可用。失败 (旧系统/headless) 返回 false。
    bool initialize(void* hwnd);
    void shutdown();

    // 更新当前曲目元数据。空字符串清空。
    void setMetadata(const std::wstring& title,
                     const std::wstring& artist,
                     const std::wstring& album);

    // 更新播放状态
    void setStatus(SmtcStatus s);

    // 更新封面 (JPEG/PNG 原始二进制)。空 vector 清空封面。
    void setThumbnail(const std::uint8_t* data, std::size_t size);

    // 时间线 (秒)
    void setTimeline(double position_sec, double duration_sec);

    // 按钮按下回调
    using ButtonHandler = std::function<void(SmtcButton)>;
    void setOnButton(ButtonHandler cb);

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
