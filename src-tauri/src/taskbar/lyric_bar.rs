//! 任务栏歌词条窗口:动态创建/销毁 + 贴任务栏定位与跟随。
//!
//! 技术路线为「置顶浮层窗口」(DeskBand 在 Win11 已废弃、SetParent 嵌入
//! XAML 任务栏不稳定):无边框透明窗口,经 ABM_GETTASKBARPOS 取任务栏
//! rect 与停靠边,贴靠其内侧;WS_EX_NOACTIVATE 保证点击播控按钮不抢走
//! 前台焦点。窗口内容是独立前端入口 taskbar.html(不 import 任何主窗口
//! store,见项目 persist 门闩约束)。
//!
//! - 用户拖拽后经 `position` 命令记忆横向偏移(持久化在歌词条窗口自己的
//!   localStorage key,由前端负责),纵向始终吸附回任务栏;
//! - 跟随线程低频轮询任务栏 rect(分辨率/任务栏位置变化时重新吸附),
//!   窗口不存在时空转,开销可忽略。

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tracing::{debug, warn};
use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE,
};

/// 歌词条窗口 label(capabilities/taskbar-lyrics.json 按此授权)。
const WINDOW_LABEL: &str = "taskbar-lyrics";

/// 拖拽记忆的横向位置(物理像素;None = 默认位置)。前端持久化,启动时回传。
static STORED_X: Mutex<Option<i32>> = Mutex::new(None);
static FOLLOW_THREAD_STARTED: AtomicBool = AtomicBool::new(false);
/// 开关串行锁:async 命令并发执行,快速 关→开 时旧窗口的异步销毁与新窗口
/// 创建交错会"丢窗口"(开关显示开、窗口已被旧销毁带走)。
static TOGGLE_LOCK: Mutex<()> = Mutex::new(());
/// 歌词条 HWND(创建时记录,供跟随线程免派发直接重申 Z 序)。
static BAR_HWND: Mutex<Option<isize>> = Mutex::new(None);
/// 仅歌词模式(鼠标穿透)。标志常驻:开关销毁重建窗口时沿用。
static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);
/// 避让状态:任务栏收起 / 全屏应用占据显示器时临时隐藏(非用户关闭)。
static AVOID_HIDDEN: AtomicBool = AtomicBool::new(false);

/// 任务栏停靠边(APPBARDATA::uEdge)。
const ABE_LEFT: u32 = 0;
const ABE_RIGHT: u32 = 2;

/// 任务栏几何信息(纯数据,便于单测)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskbarInfo {
    /// (left, top, right, bottom),物理像素
    pub rect: (i32, i32, i32, i32),
    pub edge: u32,
}

/// (任务栏几何, 记忆横向位置) → 歌词条窗口 (x, y, w, h),物理像素。
///
/// - 水平任务栏(常态):条高 = clamp(任务栏高 - 8, 28..=40) 垂直居中,
///   宽 = 9×高(随 DPI 缩放的任务栏高度等比放大);默认横向位置靠右
///   (右缘留 6×高 ≈ 托盘区),拖拽记忆值钳制在任务栏范围内。
/// - 垂直任务栏(Win10 左/右停靠,罕见):条贴任务栏底部、横向出挑到
///   任务栏内侧,尺寸取上限档。
pub(crate) fn compute_bar_rect(
    taskbar: &TaskbarInfo,
    stored_x: Option<i32>,
) -> (i32, i32, i32, i32) {
    let (left, top, right, bottom) = taskbar.rect;

    if taskbar.edge == ABE_LEFT || taskbar.edge == ABE_RIGHT {
        let height = 40;
        let width = 9 * height;
        let y = bottom - height - 8;
        let x = if taskbar.edge == ABE_LEFT {
            right + 8
        } else {
            left - width - 8
        };
        return (x, y, width, height);
    }

    let taskbar_height = bottom - top;
    let height = (taskbar_height - 8).clamp(28, 40);
    let width = 9 * height;
    let y = top + (taskbar_height - height) / 2;
    let default_x = right - width - 6 * height;
    let x = stored_x
        .unwrap_or(default_x)
        .clamp(left + 4, (right - width - 4).max(left + 4));
    (x, y, width, height)
}

/// 两矩形交集在较薄一维上的厚度(不相交为 0)。用于判断自动隐藏任务栏是否
/// 已收起:收起时任务栏窗口几乎整体滑出屏幕,与显示器交集只剩约 2px。
pub(crate) fn intersection_thickness(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> i32 {
    let width = a.2.min(b.2) - a.0.max(b.0);
    let height = a.3.min(b.3) - a.1.max(b.1);
    if width <= 0 || height <= 0 {
        0
    } else {
        width.min(height)
    }
}

/// window 矩形是否完全覆盖 monitor 矩形(全屏应用判定;无边框全屏窗口
/// 可能比显示器略大,故用「覆盖」而非「相等」)。
pub(crate) fn rect_covers(window: (i32, i32, i32, i32), monitor: (i32, i32, i32, i32)) -> bool {
    window.0 <= monitor.0 && window.1 <= monitor.1 && window.2 >= monitor.2 && window.3 >= monitor.3
}

fn rect_tuple(rect: RECT) -> (i32, i32, i32, i32) {
    (rect.left, rect.top, rect.right, rect.bottom)
}

/// 是否应临时避让(隐藏)歌词条:
/// - 自动隐藏任务栏已收起——条孤零零悬在屏幕边缘反而遮内容;
/// - 前台窗口全屏铺满任务栏所在显示器(视频/游戏)——置顶条会浮在其上。
///
/// 排除桌面/Shell 自身(Progman、WorkerW 常年铺满整屏)与歌词条自己。
fn should_avoid(bar: HWND) -> bool {
    unsafe {
        let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), None) else {
            return false;
        };
        let mut tray_rect = RECT::default();
        if GetWindowRect(tray, &mut tray_rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(tray, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let monitor_rect = rect_tuple(info.rcMonitor);

        if intersection_thickness(rect_tuple(tray_rect), monitor_rect) < 8 {
            return true;
        }

        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == bar || fg == tray {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(fg, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..len.min(class.len())]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
        if MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST) != monitor {
            return false;
        }
        let mut fg_rect = RECT::default();
        GetWindowRect(fg, &mut fg_rect).is_ok() && rect_covers(rect_tuple(fg_rect), monitor_rect)
    }
}

fn query_taskbar() -> Option<TaskbarInfo> {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    let found = unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) };
    if found == 0 {
        return None;
    }
    Some(TaskbarInfo {
        rect: (data.rc.left, data.rc.top, data.rc.right, data.rc.bottom),
        edge: data.uEdge,
    })
}

/// 启用/停用歌词条(set_taskbar_lyrics_enabled 命令调用,阻塞线程池执行)。
/// 停用即销毁窗口(而非隐藏),不保留 WebView2 实例的内存开销。
pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let _guard = TOGGLE_LOCK.lock();

    if !enabled {
        *BAR_HWND.lock() = None;
        AVOID_HIDDEN.store(false, Ordering::SeqCst);
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            window.destroy().map_err(|err| err.to_string())?;
            debug!("taskbar lyrics window closed");
        }
        return Ok(());
    }

    // 开启一律走"销毁重建":现存窗口可能是上一次关闭尚未完成销毁的僵尸,
    // 直接复用会在其随后销毁时一起消失(实测"开关偶尔不生效"的根因);
    // 前端 setter 已按状态去重,不会重复触发重建。
    *BAR_HWND.lock() = None;
    if let Some(stale) = app.get_webview_window(WINDOW_LABEL) {
        let _ = stale.destroy();
    }
    // destroy 经主线程异步执行,等待其从窗口管理器移除后再重建
    for _ in 0..40 {
        if app.get_webview_window(WINDOW_LABEL).is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if app.get_webview_window(WINDOW_LABEL).is_some() {
        return Err("旧歌词条窗口销毁超时,请再试一次".into());
    }

    let window =
        WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("taskbar.html".into()))
            .title("Seraph 任务栏歌词")
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .visible(false)
            .build()
            .map_err(|err| format!("创建歌词条窗口失败: {err}"))?;

    apply_noactivate(&window);
    AVOID_HIDDEN.store(false, Ordering::SeqCst);
    if CLICK_THROUGH.load(Ordering::SeqCst) {
        if let Err(err) = window.set_ignore_cursor_events(true) {
            warn!("taskbar lyrics click-through apply failed: {err}");
        }
    }
    if let Err(err) = reposition_window(&window, *STORED_X.lock()) {
        warn!("taskbar lyrics initial position failed: {err}");
    }
    window.show().map_err(|err| err.to_string())?;
    ensure_follow_thread(app.clone());
    debug!("taskbar lyrics window created");
    Ok(())
}

/// 仅歌词模式(鼠标穿透):窗口不再响应任何鼠标事件,点击落到任务栏。
/// 恢复交互只能走设置页开关(条本身收不到鼠标)。标志常驻,开关销毁重建
/// 窗口时沿用;窗口已存在时立即生效。
pub fn set_click_through(app: &AppHandle, enabled: bool) -> Result<(), String> {
    CLICK_THROUGH.store(enabled, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window
            .set_ignore_cursor_events(enabled)
            .map_err(|err| err.to_string())?;
        debug!("taskbar lyrics click-through -> {enabled}");
    }
    Ok(())
}

/// 定位命令:前端拖拽结束(或启动时回传持久化位置)调用。
/// 返回钳制后的实际横向位置,供前端持久化。
pub fn position(app: &AppHandle, x: Option<f64>) -> Result<f64, String> {
    let stored = x.map(|value| value.round() as i32);
    *STORED_X.lock() = stored;
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Err("歌词条窗口不存在".into());
    };
    reposition_window(&window, stored).map(|applied| applied as f64)
}

fn reposition_window(window: &WebviewWindow, stored_x: Option<i32>) -> Result<i32, String> {
    let info = query_taskbar().ok_or_else(|| "未找到任务栏".to_string())?;
    let (x, y, width, height) = compute_bar_rect(&info, stored_x);
    window
        .set_size(PhysicalSize::new(width.max(1) as u32, height.max(1) as u32))
        .map_err(|err| err.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|err| err.to_string())?;
    Ok(x)
}

/// 点击播控按钮不激活窗口(不从游戏/全屏应用抢焦点);拖拽与点击仍正常。
/// 同时把 HWND 记入 [`BAR_HWND`],供跟随线程免派发重申 Z 序。
fn apply_noactivate(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        warn!("taskbar lyrics: hwnd unavailable, NOACTIVATE skipped");
        return;
    };
    let hwnd = HWND(hwnd.0);
    *BAR_HWND.lock() = Some(hwnd.0 as isize);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
    }
}

/// 跟随线程,500ms 心跳:
/// - 每拍先做避让检查(任务栏收起 / 全屏应用),状态翻转时隐藏或恢复条;
/// - 未避让时重申 TOPMOST——任务栏自身也是置顶窗口,应用切换时 Shell 会
///   重排置顶带 Z 序把任务栏抬到歌词条之上,条在任务栏矩形内即被盖住
///   "消失"(实测症状:切走后条不见,重开开关才恢复)。SWP_NOACTIVATE
///   不抢焦点,已在顶端时是无副作用的幂等调用;
/// - 每 4 拍(2s)查一次任务栏 rect,分辨率/DPI/任务栏位置变化时重新吸附。
///
/// 全程只起一次;窗口不存在时空转。
fn ensure_follow_thread(app: AppHandle) {
    if FOLLOW_THREAD_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("taskbar-lyrics-follow".into())
        .spawn(move || {
            let mut last: Option<TaskbarInfo> = None;
            let mut tick: u32 = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                tick = tick.wrapping_add(1);
                if app.get_webview_window(WINDOW_LABEL).is_none() {
                    last = None;
                    continue;
                }

                if let Some(hwnd_addr) = *BAR_HWND.lock() {
                    let bar = HWND(hwnd_addr as *mut core::ffi::c_void);
                    let avoid = should_avoid(bar);
                    if AVOID_HIDDEN.swap(avoid, Ordering::SeqCst) != avoid {
                        unsafe {
                            let _ =
                                ShowWindow(bar, if avoid { SW_HIDE } else { SW_SHOWNOACTIVATE });
                        }
                        debug!(
                            "taskbar lyrics {} (fullscreen / taskbar-hidden avoidance)",
                            if avoid { "avoiding" } else { "restored" }
                        );
                    }
                    if !avoid {
                        unsafe {
                            let _ = SetWindowPos(
                                bar,
                                Some(HWND_TOPMOST),
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                        }
                    }
                }

                if !tick.is_multiple_of(4) {
                    continue;
                }
                let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
                    continue;
                };
                let Some(info) = query_taskbar() else {
                    continue;
                };
                if last == Some(info) {
                    continue;
                }
                last = Some(info);
                if let Err(err) = reposition_window(&window, *STORED_X.lock()) {
                    debug!("taskbar lyrics follow reposition failed: {err}");
                }
            }
        })
        .map(|_| ())
        .unwrap_or_else(|err| warn!("taskbar lyrics follow thread spawn failed: {err}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1080p 100% 缩放的标准底部任务栏(高 40)。
    fn bottom_taskbar() -> TaskbarInfo {
        TaskbarInfo {
            rect: (0, 1040, 1920, 1080),
            edge: 3,
        }
    }

    #[test]
    fn bottom_taskbar_bar_is_vertically_centered_and_right_aligned() {
        let (x, y, w, h) = compute_bar_rect(&bottom_taskbar(), None);
        assert_eq!(h, 32, "40px 任务栏 → 条高 40-8=32");
        assert_eq!(w, 9 * 32);
        assert_eq!(y, 1040 + (40 - 32) / 2);
        // 默认位置:右缘留 6×高 的托盘区
        assert_eq!(x, 1920 - w - 6 * 32);
    }

    #[test]
    fn win11_48px_taskbar_caps_height_at_40() {
        let taskbar = TaskbarInfo {
            rect: (0, 2112, 3840, 2160),
            edge: 3,
        };
        let (_, y, _, h) = compute_bar_rect(&taskbar, None);
        assert_eq!(h, 40);
        assert_eq!(y, 2112 + (48 - 40) / 2);
    }

    #[test]
    fn tiny_taskbar_keeps_minimum_height() {
        let taskbar = TaskbarInfo {
            rect: (0, 1050, 1920, 1080),
            edge: 3,
        };
        let (_, _, _, h) = compute_bar_rect(&taskbar, None);
        assert_eq!(h, 28);
    }

    #[test]
    fn stored_x_is_honored_and_clamped() {
        let taskbar = bottom_taskbar();
        let (x, _, w, _) = compute_bar_rect(&taskbar, Some(100));
        assert_eq!(x, 100);
        // 超出右界 → 钳回
        let (x, _, _, _) = compute_bar_rect(&taskbar, Some(99999));
        assert_eq!(x, 1920 - w - 4);
        // 超出左界 → 钳回
        let (x, _, _, _) = compute_bar_rect(&taskbar, Some(-500));
        assert_eq!(x, 4);
    }

    #[test]
    fn top_taskbar_centers_within_top_strip() {
        let taskbar = TaskbarInfo {
            rect: (0, 0, 1920, 40),
            edge: 1,
        };
        let (_, y, _, h) = compute_bar_rect(&taskbar, None);
        assert_eq!(h, 32);
        assert_eq!(y, (40 - 32) / 2);
    }

    #[test]
    fn vertical_taskbar_places_bar_at_bottom_inner_side() {
        let left = TaskbarInfo {
            rect: (0, 0, 62, 1080),
            edge: 0,
        };
        let (x, y, w, h) = compute_bar_rect(&left, None);
        assert_eq!((w, h), (360, 40));
        assert_eq!(x, 62 + 8);
        assert_eq!(y, 1080 - 40 - 8);

        let right = TaskbarInfo {
            rect: (1858, 0, 1920, 1080),
            edge: 2,
        };
        let (x, _, w, _) = compute_bar_rect(&right, None);
        assert_eq!(x, 1858 - w - 8);
    }

    const MONITOR: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

    #[test]
    fn expanded_taskbar_has_full_thickness() {
        // 正常展开的底部任务栏:交集厚度 = 任务栏高 40
        assert_eq!(intersection_thickness((0, 1040, 1920, 1080), MONITOR), 40);
    }

    #[test]
    fn collapsed_autohide_taskbar_thickness_below_threshold() {
        // 自动隐藏收起:窗口滑出屏幕,只露 2px 边缘
        assert_eq!(intersection_thickness((0, 1078, 1920, 1120), MONITOR), 2);
        // 完全滑出(副屏方向)→ 0
        assert_eq!(intersection_thickness((0, 1080, 1920, 1120), MONITOR), 0);
    }

    #[test]
    fn fullscreen_window_covers_monitor_but_maximized_does_not() {
        // 无边框全屏(可能比显示器略大)→ 覆盖
        assert!(rect_covers(MONITOR, MONITOR));
        assert!(rect_covers((-8, -8, 1928, 1088), MONITOR));
        // 最大化窗口止步于工作区(任务栏之上)→ 不算全屏
        assert!(!rect_covers((0, 0, 1920, 1040), MONITOR));
        // 副屏上的全屏窗口不覆盖本显示器
        assert!(!rect_covers((1920, 0, 3840, 1080), MONITOR));
    }
}
