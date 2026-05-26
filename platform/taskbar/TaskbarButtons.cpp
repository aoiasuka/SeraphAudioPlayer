// =============================================================================
//  platform/taskbar/TaskbarButtons.cpp
// =============================================================================
#include "TaskbarButtons.h"

#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <shobjidl.h>
#include <commctrl.h>
#include <cstring>
#include <cwchar>

#pragma comment(lib, "comctl32")

namespace apx {

namespace {

constexpr UINT kCmdPrev      = 0xAB10;
constexpr UINT kCmdPlayPause = 0xAB11;
constexpr UINT kCmdNext      = 0xAB12;

// 用 GDI 画一个 16x16 黑底白色形状的 HICON,kind:
//   0 = prev (两条竖线 + 反三角)
//   1 = play (右指三角)
//   2 = pause (双竖线)
//   3 = next (正三角 + 两条竖线)
HICON makeIcon(int kind)
{
    constexpr int sz = 16;

    BITMAPV5HEADER bi{};
    bi.bV5Size        = sizeof(bi);
    bi.bV5Width       = sz;
    bi.bV5Height      = sz;
    bi.bV5Planes      = 1;
    bi.bV5BitCount    = 32;
    bi.bV5Compression = BI_BITFIELDS;
    bi.bV5RedMask     = 0x00FF0000;
    bi.bV5GreenMask   = 0x0000FF00;
    bi.bV5BlueMask    = 0x000000FF;
    bi.bV5AlphaMask   = 0xFF000000;

    HDC hdc = GetDC(nullptr);
    void* bits = nullptr;
    HBITMAP hColor = CreateDIBSection(hdc, reinterpret_cast<BITMAPINFO*>(&bi),
                                      DIB_RGB_COLORS, &bits, nullptr, 0);
    ReleaseDC(nullptr, hdc);
    if (!hColor || !bits) return nullptr;

    // 全透明
    std::memset(bits, 0, sz * sz * 4);

    auto putPixel = [&](int x, int y, uint32_t argb) {
        if (x < 0 || x >= sz || y < 0 || y >= sz) return;
        auto* p = static_cast<uint32_t*>(bits);
        // DIB is bottom-up;翻转 y
        p[(sz - 1 - y) * sz + x] = argb;
    };
    auto fillRect = [&](int x, int y, int w, int h, uint32_t argb) {
        for (int dy = 0; dy < h; ++dy)
            for (int dx = 0; dx < w; ++dx) putPixel(x + dx, y + dy, argb);
    };
    auto fillTri = [&](bool right, int x0, int y0, int sz2) {
        // 实心三角:right=true 指右,false 指左
        for (int dy = 0; dy < sz2; ++dy) {
            int w = (dy <= sz2 / 2) ? (dy * 2 + 1)
                                    : ((sz2 - dy) * 2 - 1);
            if (w < 1) w = 1;
            int cx = x0 + (right ? 0 : sz2 - 1);
            for (int dx = 0; dx < w; ++dx) {
                int x = right ? cx + dx : cx - dx;
                putPixel(x, y0 + dy, 0xFFFFFFFF);
            }
        }
    };

    switch (kind) {
    case 0: { // prev
        fillRect(2, 3, 2, 10, 0xFFFFFFFF);          // 左侧竖线
        fillTri(false, 13, 3, 10);                   // 指向左的三角
        break;
    }
    case 1: { // play
        fillTri(true, 4, 3, 10);
        break;
    }
    case 2: { // pause
        fillRect(4, 3, 3, 10, 0xFFFFFFFF);
        fillRect(9, 3, 3, 10, 0xFFFFFFFF);
        break;
    }
    case 3: { // next
        fillTri(true, 3, 3, 10);
        fillRect(12, 3, 2, 10, 0xFFFFFFFF);
        break;
    }
    }

    HBITMAP hMask = CreateBitmap(sz, sz, 1, 1, nullptr);

    ICONINFO ii{};
    ii.fIcon    = TRUE;
    ii.hbmMask  = hMask;
    ii.hbmColor = hColor;
    HICON icon = CreateIconIndirect(&ii);
    DeleteObject(hColor);
    DeleteObject(hMask);
    return icon;
}

} // namespace

struct TaskbarButtons::Impl {
    HWND hwnd = nullptr;
    ITaskbarList3* taskbar = nullptr;
    HIMAGELIST imageList = nullptr;
    HICON iconPrev = nullptr, iconPlay = nullptr, iconPause = nullptr, iconNext = nullptr;
    bool added = false;
    bool playing = false;
    bool canPrev = true, canNext = true;
    Handler cb;
    UINT taskbarCreatedMsg = 0;     // 任务栏重启后需要重新初始化

    void buildImageList()
    {
        if (imageList) ImageList_Destroy(imageList);
        imageList = ImageList_Create(16, 16, ILC_COLOR32, 4, 0);

        if (!iconPrev)  iconPrev  = makeIcon(0);
        if (!iconPlay)  iconPlay  = makeIcon(1);
        if (!iconPause) iconPause = makeIcon(2);
        if (!iconNext)  iconNext  = makeIcon(3);

        if (iconPrev)  ImageList_AddIcon(imageList, iconPrev);
        if (iconPlay)  ImageList_AddIcon(imageList, iconPlay);
        if (iconPause) ImageList_AddIcon(imageList, iconPause);
        if (iconNext)  ImageList_AddIcon(imageList, iconNext);

        if (taskbar) {
            taskbar->ThumbBarSetImageList(hwnd, imageList);
        }
    }

    void addButtons()
    {
        if (!taskbar || !hwnd) return;
        THUMBBUTTON btns[3] = {};
        btns[0].dwMask  = THB_BITMAP | THB_TOOLTIP | THB_FLAGS;
        btns[0].iId     = kCmdPrev;
        btns[0].iBitmap = 0;
        wcscpy_s(btns[0].szTip, L"上一首");
        btns[0].dwFlags = canPrev ? THBF_ENABLED : THBF_DISABLED;

        btns[1].dwMask  = THB_BITMAP | THB_TOOLTIP | THB_FLAGS;
        btns[1].iId     = kCmdPlayPause;
        btns[1].iBitmap = playing ? 2 : 1;       // 播放中显示暂停图标
        wcscpy_s(btns[1].szTip, playing ? L"暂停" : L"播放");
        btns[1].dwFlags = THBF_ENABLED;

        btns[2].dwMask  = THB_BITMAP | THB_TOOLTIP | THB_FLAGS;
        btns[2].iId     = kCmdNext;
        btns[2].iBitmap = 3;
        wcscpy_s(btns[2].szTip, L"下一首");
        btns[2].dwFlags = canNext ? THBF_ENABLED : THBF_DISABLED;

        if (!added) {
            taskbar->ThumbBarAddButtons(hwnd, 3, btns);
            added = true;
        } else {
            taskbar->ThumbBarUpdateButtons(hwnd, 3, btns);
        }
    }
};

TaskbarButtons::TaskbarButtons()
    : d_(std::make_unique<Impl>())
{
}

TaskbarButtons::~TaskbarButtons()
{
    shutdown();
}

bool TaskbarButtons::initialize(void* hwndPtr)
{
    if (!hwndPtr) return false;
    d_->hwnd = reinterpret_cast<HWND>(hwndPtr);
    d_->taskbarCreatedMsg = RegisterWindowMessageW(L"TaskbarButtonCreated");

    // 通知 shell 我们要 ThumbBar
    HRESULT hr = CoCreateInstance(CLSID_TaskbarList, nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&d_->taskbar));
    if (FAILED(hr) || !d_->taskbar) return false;
    if (FAILED(d_->taskbar->HrInit())) {
        d_->taskbar->Release();
        d_->taskbar = nullptr;
        return false;
    }
    d_->buildImageList();
    d_->addButtons();
    return true;
}

void TaskbarButtons::shutdown()
{
    if (!d_) return;
    if (d_->imageList) { ImageList_Destroy(d_->imageList); d_->imageList = nullptr; }
    if (d_->iconPrev)  { DestroyIcon(d_->iconPrev);  d_->iconPrev = nullptr; }
    if (d_->iconPlay)  { DestroyIcon(d_->iconPlay);  d_->iconPlay = nullptr; }
    if (d_->iconPause) { DestroyIcon(d_->iconPause); d_->iconPause = nullptr; }
    if (d_->iconNext)  { DestroyIcon(d_->iconNext);  d_->iconNext = nullptr; }
    if (d_->taskbar)   { d_->taskbar->Release();    d_->taskbar = nullptr; }
    d_->added = false;
}

void TaskbarButtons::setPlaying(bool playing)
{
    if (!d_->taskbar) return;
    if (d_->playing == playing) return;
    d_->playing = playing;
    d_->addButtons();
}

void TaskbarButtons::setNavEnabled(bool can_prev, bool can_next)
{
    if (!d_->taskbar) return;
    if (d_->canPrev == can_prev && d_->canNext == can_next) return;
    d_->canPrev = can_prev;
    d_->canNext = can_next;
    d_->addButtons();
}

void TaskbarButtons::setOnButton(Handler cb)
{
    d_->cb = std::move(cb);
}

bool TaskbarButtons::handleCommand(uint32_t cmd)
{
    if (!d_->cb) return false;
    switch (cmd) {
    case kCmdPrev:      d_->cb(TaskbarButton::Previous);  return true;
    case kCmdPlayPause: d_->cb(TaskbarButton::PlayPause); return true;
    case kCmdNext:      d_->cb(TaskbarButton::Next);      return true;
    default:            return false;
    }
}

uint32_t TaskbarButtons::taskbarCreatedMessageId() const
{
    return d_ ? d_->taskbarCreatedMsg : 0;
}

void TaskbarButtons::onTaskbarRestart()
{
    // explorer.exe 重启或 DPI 变更后任务栏会重发 WM_TaskbarButtonCreated。
    // 旧的 ITaskbarList3 对象已失效，必须 Release 后重新 CoCreate + HrInit + addButtons。
    if (!d_) return;
    HWND hwnd = d_->hwnd;
    if (!hwnd) return;
    if (d_->taskbar) { d_->taskbar->Release(); d_->taskbar = nullptr; }
    d_->added = false;
    HRESULT hr = CoCreateInstance(CLSID_TaskbarList, nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&d_->taskbar));
    if (FAILED(hr) || !d_->taskbar) return;
    if (FAILED(d_->taskbar->HrInit())) {
        d_->taskbar->Release();
        d_->taskbar = nullptr;
        return;
    }
    d_->addButtons();
}

} // namespace apx
