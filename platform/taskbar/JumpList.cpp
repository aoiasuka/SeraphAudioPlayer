// =============================================================================
//  platform/taskbar/JumpList.cpp
// =============================================================================
#include "JumpList.h"

#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <shobjidl.h>
#include <shlobj.h>
#include <propkey.h>
#include <propvarutil.h>

#pragma comment(lib, "shell32")
#pragma comment(lib, "propsys")

namespace apx {

namespace {

bool getExePath(std::wstring& out)
{
    wchar_t buf[MAX_PATH] = {};
    DWORD n = GetModuleFileNameW(nullptr, buf, MAX_PATH);
    if (n == 0 || n >= MAX_PATH) return false;
    out.assign(buf, n);
    return true;
}

// 创建一个 IShellLink 任务,启动当前 exe 并传指定参数
HRESULT makeTask(const std::wstring& exePath,
                 const std::wstring& args,
                 const std::wstring& title,
                 IShellLinkW** outLink)
{
    *outLink = nullptr;
    IShellLinkW* link = nullptr;
    HRESULT hr = CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&link));
    if (FAILED(hr)) return hr;

    link->SetPath(exePath.c_str());
    if (!args.empty()) link->SetArguments(args.c_str());
    link->SetIconLocation(exePath.c_str(), 0);

    IPropertyStore* propStore = nullptr;
    hr = link->QueryInterface(IID_PPV_ARGS(&propStore));
    if (SUCCEEDED(hr) && propStore) {
        PROPVARIANT pv;
        if (SUCCEEDED(InitPropVariantFromString(title.c_str(), &pv))) {
            propStore->SetValue(PKEY_Title, pv);
            propStore->Commit();
            PropVariantClear(&pv);
        }
        propStore->Release();
    }
    *outLink = link;
    return S_OK;
}

} // namespace

bool JumpList::install()
{
    std::wstring exePath;
    if (!getExePath(exePath)) return false;

    // 保护性 CoInitializeEx：调用方未必已经初始化 COM。
    // 若线程已 STA（如 Qt main 线程通常如此），返回 S_FALSE / RPC_E_CHANGED_MODE。
    // 不持有 com 句柄就调用 CoCreateInstance 直接 0x800401F0 失败。
    HRESULT comHr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    const bool need_uninit = (comHr == S_OK || comHr == S_FALSE);

    ICustomDestinationList* dlist = nullptr;
    if (FAILED(CoCreateInstance(CLSID_DestinationList, nullptr, CLSCTX_INPROC_SERVER,
                                IID_PPV_ARGS(&dlist)))) {
        if (need_uninit) CoUninitialize();
        return false;
    }

    UINT maxSlots = 0;
    IObjectArray* removed = nullptr;
    if (FAILED(dlist->BeginList(&maxSlots, IID_PPV_ARGS(&removed)))) {
        dlist->Release();
        if (need_uninit) CoUninitialize();
        return false;
    }

    IObjectCollection* tasks = nullptr;
    if (FAILED(CoCreateInstance(CLSID_EnumerableObjectCollection, nullptr,
                                CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&tasks)))) {
        if (removed) removed->Release();
        dlist->AbortList();
        dlist->Release();
        if (need_uninit) CoUninitialize();
        return false;
    }

    IShellLinkW* link = nullptr;
    if (SUCCEEDED(makeTask(exePath, L"--open", L"打开音频文件...", &link)) && link) {
        tasks->AddObject(link);
        link->Release();
        link = nullptr;
    }
    if (SUCCEEDED(makeTask(exePath, L"--play", L"继续播放", &link)) && link) {
        tasks->AddObject(link);
        link->Release();
        link = nullptr;
    }
    if (SUCCEEDED(makeTask(exePath, L"--pause", L"暂停", &link)) && link) {
        tasks->AddObject(link);
        link->Release();
        link = nullptr;
    }
    if (SUCCEEDED(makeTask(exePath, L"--next", L"下一首", &link)) && link) {
        tasks->AddObject(link);
        link->Release();
        link = nullptr;
    }
    if (SUCCEEDED(makeTask(exePath, L"--prev", L"上一首", &link)) && link) {
        tasks->AddObject(link);
        link->Release();
        link = nullptr;
    }

    IObjectArray* taskArray = nullptr;
    HRESULT hr = tasks->QueryInterface(IID_PPV_ARGS(&taskArray));
    if (SUCCEEDED(hr) && taskArray) {
        dlist->AddUserTasks(taskArray);
        taskArray->Release();
    }
    tasks->Release();
    if (removed) removed->Release();

    dlist->CommitList();
    dlist->Release();
    if (need_uninit) CoUninitialize();
    return true;
}

void JumpList::addRecent(const std::wstring& path)
{
    if (path.empty()) return;
    SHARDAPPIDINFO info{};
    // 直接用 SHARD_PATHW 简化(不指定 AppID 也能用)
    SHAddToRecentDocs(SHARD_PATHW, path.c_str());
    (void)info;
}

} // namespace apx
