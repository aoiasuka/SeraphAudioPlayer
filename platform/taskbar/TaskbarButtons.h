// =============================================================================
//  platform/taskbar/TaskbarButtons.h
//
//  Windows 7+ 任务栏缩略图按钮 (ITaskbarList3::ThumbBarAddButtons)。
//  在任务栏预览图上添加 上一首 / 播放暂停 / 下一首 三个按钮。
//
//  调用方需要把窗口的 WM_COMMAND 转给 handleCommand;Qt 端用
//  QAbstractNativeEventFilter 完成。
// =============================================================================
#pragma once

#include <cstdint>
#include <functional>
#include <memory>

namespace apx {

enum class TaskbarButton {
    Previous,
    PlayPause,
    Next
};

class TaskbarButtons {
public:
    TaskbarButtons();
    ~TaskbarButtons();

    TaskbarButtons(const TaskbarButtons&)            = delete;
    TaskbarButtons& operator=(const TaskbarButtons&) = delete;

    bool initialize(void* hwnd);
    void shutdown();

    // 主图标显示为 "播放" 或 "暂停"
    void setPlaying(bool playing);
    // 禁用/启用 prev/next
    void setNavEnabled(bool can_prev, bool can_next);

    using Handler = std::function<void(TaskbarButton)>;
    void setOnButton(Handler cb);

    // 由 native event filter 调用,wParam = 命令 ID;返回 true 表示已处理
    bool handleCommand(uint32_t cmd);

    // 返回 RegisterWindowMessageW(L"TaskbarButtonCreated") 的消息号；
    // native event filter 拦到该消息时应调用 onTaskbarRestart() 重建按钮。
    uint32_t taskbarCreatedMessageId() const;
    // explorer 重启 / WM_TaskbarButtonCreated 触发后调用：丢弃旧 ITaskbarList3 句柄并重建。
    void onTaskbarRestart();

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
