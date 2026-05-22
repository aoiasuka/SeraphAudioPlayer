// =============================================================================
//  app/controller/PlayerState.h
//
//  播放器整体状态。UI 层、控制器、监听器都用此枚举。
// =============================================================================
#pragma once

#include <cstdint>

namespace apx {

enum class PlayerState : std::uint8_t {
    Idle    = 0,    // 无文件
    Stopped = 1,    // 文件已加载,未播放(初次加载 / 用户 stop / 从 Ended 复位后)
    Playing = 2,    // 正在播放
    Paused  = 3,    // 用户暂停(保留位置)
    Ended   = 4,    // 自然播放到 EOF(文件仍打开)
    Error   = 5,    // 出错(需重新 loadFile)
};

inline const wchar_t* to_wstring(PlayerState s)
{
    switch (s) {
    case PlayerState::Idle:    return L"Idle";
    case PlayerState::Stopped: return L"Stopped";
    case PlayerState::Playing: return L"Playing";
    case PlayerState::Paused:  return L"Paused";
    case PlayerState::Ended:   return L"Ended";
    case PlayerState::Error:   return L"Error";
    }
    return L"?";
}

} // namespace apx
