// =============================================================================
//  platform/taskbar/JumpList.h
//
//  Windows 7+ 任务栏跳转列表 (ICustomDestinationList)。
//
//  当前实现:
//    - install(): 在跳转列表"任务"区放固定项 (打开/退出)
//    - addRecent(path): 把音频文件加入系统"最近"列表
//                        (后续 Windows 自动在跳转列表显示)
// =============================================================================
#pragma once

#include <string>

namespace apx {

class JumpList {
public:
    // 一次性写入"任务"区
    static bool install();

    // 通知 Windows 该文件被打开,加入"最近"
    static void addRecent(const std::wstring& path);
};

} // namespace apx
