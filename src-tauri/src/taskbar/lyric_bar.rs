//! 任务栏歌词条窗口:动态创建/销毁 + 贴任务栏定位与跟随。
//!
//! 技术路线为「置顶浮层窗口」(DeskBand 在 Win11 已废弃、SetParent 嵌入
//! XAML 任务栏不稳定):无边框透明窗口,经 ABM_GETTASKBARPOS 取任务栏
//! rect 与停靠边,贴靠其内侧;WS_EX_NOACTIVATE 保证点击播控按钮不抢走
//! 前台焦点。窗口内容是独立前端入口 taskbar.html(不 import 任何主窗口
//! store,见项目 persist 门闩约束)。
//!
//! - 位置以「沿任务栏长边的比例」(0..=1)表达:设置页滑块直接给比例,
//!   用户拖拽则由 `position` 反解成比例;单一事实来源是主窗口 store,
//!   水合与拖拽后都同步到这里,纵向(水平任务栏时)始终吸附回任务栏;
//! - 跟随线程 100ms 心跳 + `EVENT_SYSTEM_FOREGROUND` 钩子(切换应用时
//!   Shell 会重排置顶带把任务栏抬到条之上,须即时重申 TOPMOST),
//!   窗口不存在时空转,开销可忽略。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, GetClassNameW, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, KillTimer, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, HWND_TOPMOST, MSG,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT,
    WM_TIMER, WS_EX_NOACTIVATE,
};

/// 歌词条窗口 label(capabilities/taskbar-lyrics.json 按此授权)。
const WINDOW_LABEL: &str = "taskbar-lyrics";

/// 歌词条沿任务栏长边的位置比例(0.0 = 最靠起点,1.0 = 最靠终点;
/// None = 默认位置)。设置页滑块与用户拖拽都收敛到这一个量。
static STORED_RATIO: Mutex<Option<f64>> = Mutex::new(None);
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
/// 前台切换后待补的 TOPMOST 重申拍数(WinEvent 钩子置位,心跳递减)。
static RESETTLE_TICKS: AtomicU32 = AtomicU32::new(0);

/// 跟随线程心跳周期(毫秒)。取 100ms 是为了「切换应用后条被任务栏盖住」
/// 的可见时长压到一帧级别;每拍只做原子读与幂等 SetWindowPos,重活按
/// 下面两个倍数分摊。
const HEARTBEAT_MS: u32 = 100;
/// 避让检查(全屏/任务栏收起)节拍:每 5 拍 = 500ms。
const AVOID_EVERY: u32 = 5;
/// 任务栏 rect 复查节拍:每 20 拍 = 2s。
const REPOSITION_EVERY: u32 = 20;

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

/// 歌词条尺寸与它沿任务栏长边的可移动区间(纯几何,便于单测)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BarMetrics {
    pub width: i32,
    pub height: i32,
    /// 可移动区间起点(水平任务栏是 x,垂直任务栏是 y)
    pub span_start: i32,
    /// 可移动区间终点(含);span_end < span_start 不会出现,已 max 兜底
    pub span_end: i32,
    /// 未指定比例时的落点
    pub default_pos: i32,
}

/// 条尺寸与可移动区间:
/// - 水平任务栏(常态):条高 = clamp(任务栏高 - 8, 28..=40),宽 = 9×高
///   (随 DPI 缩放的任务栏高度等比放大);默认落点靠右(右缘留 6×高 ≈ 托盘区);
/// - 垂直任务栏(Win10 左/右停靠,罕见):尺寸取上限档,沿纵向移动,默认贴底。
pub(crate) fn bar_metrics(taskbar: &TaskbarInfo) -> BarMetrics {
    let (left, top, right, bottom) = taskbar.rect;

    if taskbar.edge == ABE_LEFT || taskbar.edge == ABE_RIGHT {
        let height = 40;
        let width = 9 * height;
        let span_start = top + 8;
        let span_end = (bottom - height - 8).max(span_start);
        return BarMetrics {
            width,
            height,
            span_start,
            span_end,
            default_pos: span_end,
        };
    }

    let taskbar_height = bottom - top;
    let height = (taskbar_height - 8).clamp(28, 40);
    let width = 9 * height;
    let span_start = left + 4;
    let span_end = (right - width - 4).max(span_start);
    BarMetrics {
        width,
        height,
        span_start,
        span_end,
        default_pos: (right - width - 6 * height).clamp(span_start, span_end),
    }
}

/// 比例(0..=1)→ 沿任务栏长边的物理坐标;None 用默认落点。
fn position_from_ratio(metrics: &BarMetrics, ratio: Option<f64>) -> i32 {
    let pos = match ratio {
        Some(ratio) => {
            let span = (metrics.span_end - metrics.span_start) as f64;
            metrics.span_start + (span * ratio.clamp(0.0, 1.0)).round() as i32
        }
        None => metrics.default_pos,
    };
    pos.clamp(metrics.span_start, metrics.span_end)
}

/// 沿任务栏长边的物理坐标 → 比例(拖拽后反解,越界自动钳回 0..=1)。
pub(crate) fn ratio_from_position(taskbar: &TaskbarInfo, pos: i32) -> f64 {
    let metrics = bar_metrics(taskbar);
    let span = metrics.span_end - metrics.span_start;
    if span <= 0 {
        return 0.0;
    }
    (((pos - metrics.span_start) as f64) / span as f64).clamp(0.0, 1.0)
}

/// (任务栏几何, 位置比例) → 歌词条窗口 (x, y, w, h),物理像素。
///
/// 水平任务栏时条垂直居中于任务栏、沿横向按比例落位;垂直任务栏时条横向
/// 出挑到任务栏内侧、沿纵向按比例落位。
pub(crate) fn compute_bar_rect(taskbar: &TaskbarInfo, ratio: Option<f64>) -> (i32, i32, i32, i32) {
    let (left, top, right, bottom) = taskbar.rect;
    let metrics = bar_metrics(taskbar);
    let pos = position_from_ratio(&metrics, ratio);

    if taskbar.edge == ABE_LEFT || taskbar.edge == ABE_RIGHT {
        let x = if taskbar.edge == ABE_LEFT {
            right + 8
        } else {
            left - metrics.width - 8
        };
        return (x, pos, metrics.width, metrics.height);
    }

    let y = top + (bottom - top - metrics.height) / 2;
    (pos, y, metrics.width, metrics.height)
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
    if let Err(err) = reposition_window(&window, *STORED_RATIO.lock()) {
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

/// 设置页滑块:直接指定条沿任务栏长边的位置比例(0..=1)。
/// 设置的单一事实来源是主窗口 store,水合后与滑块调整时同步到这里;
/// 歌词条关着也接受(记住比例,下次开启即用),故窗口不存在不算失败。
pub fn set_position(app: &AppHandle, ratio: f64) -> Result<(), String> {
    let ratio = ratio.clamp(0.0, 1.0);
    *STORED_RATIO.lock() = Some(ratio);
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
    reposition_window(&window, Some(ratio)).map(|_| ())
}

/// 定位命令:前端拖拽结束后调用,(x, y) 是窗口左上角的物理坐标——沿任务栏
/// 长边的那一维才有意义(水平任务栏取 x,垂直任务栏取 y),另一维由吸附决定。
/// 返回反解并钳制后的位置比例,由歌词条回传主窗口落进设置(滑块随之同步)。
pub fn position(app: &AppHandle, x: Option<f64>, y: Option<f64>) -> Result<f64, String> {
    let info = query_taskbar().ok_or_else(|| "未找到任务栏".to_string())?;
    let along = if info.edge == ABE_LEFT || info.edge == ABE_RIGHT {
        y
    } else {
        x
    };
    let ratio = along.map(|value| ratio_from_position(&info, value.round() as i32));
    *STORED_RATIO.lock() = ratio;
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Err("歌词条窗口不存在".into());
    };
    reposition_window(&window, ratio)?;
    Ok(ratio.unwrap_or_else(|| ratio_from_position(&info, bar_metrics(&info).default_pos)))
}

fn reposition_window(window: &WebviewWindow, ratio: Option<f64>) -> Result<i32, String> {
    let info = query_taskbar().ok_or_else(|| "未找到任务栏".to_string())?;
    let (x, y, width, height) = compute_bar_rect(&info, ratio);
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

/// 把条重新抬到置顶带顶端。任务栏自身也是置顶窗口,应用切换时 Shell 会重排
/// 置顶带 Z 序把任务栏抬到歌词条之上,条在任务栏矩形内即被盖住"消失"。
/// SWP_NOACTIVATE 不抢焦点,已在顶端时是无副作用的幂等调用。
fn reassert_topmost() {
    // 同 follow_tick:先释放锁再动窗口,避免与后续路径上的取锁纠缠
    let bar_hwnd = *BAR_HWND.lock();
    let Some(hwnd_addr) = bar_hwnd else {
        return;
    };
    if AVOID_HIDDEN.load(Ordering::SeqCst) {
        return;
    }
    unsafe {
        let _ = SetWindowPos(
            HWND(hwnd_addr as *mut core::ffi::c_void),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// `EVENT_SYSTEM_FOREGROUND` 钩子回调(在跟随线程的消息泵中派发)。
///
/// 只等 500ms 心跳纠正 Z 序时,用户切换应用会明显看到条"闪一下、消失又
/// 出现"。这里在前台切换的同一刻先重申一次,再让随后几拍各补一次——
/// Shell 的置顶带重排未必与前台事件同拍到达。
unsafe extern "system" fn on_foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    reassert_topmost();
    RESETTLE_TICKS.store(3, Ordering::SeqCst);
}

#[derive(Default)]
struct FollowState {
    last: Option<TaskbarInfo>,
    tick: u32,
}

/// 跟随线程,[`HEARTBEAT_MS`] 一拍:
/// - 前台刚切换过的几拍补重申 TOPMOST(钩子已经即时重申过一次);
/// - 每 [`AVOID_EVERY`] 拍做避让检查(任务栏收起 / 全屏应用),状态翻转时
///   隐藏或恢复条,未避让时重申 TOPMOST;
/// - 每 [`REPOSITION_EVERY`] 拍查一次任务栏 rect,分辨率/DPI/任务栏位置
///   变化时重新吸附。
fn follow_tick(app: &AppHandle, state: &mut FollowState) {
    state.tick = state.tick.wrapping_add(1);
    if app.get_webview_window(WINDOW_LABEL).is_none() {
        state.last = None;
        return;
    }

    if RESETTLE_TICKS.load(Ordering::SeqCst) > 0 {
        RESETTLE_TICKS.fetch_sub(1, Ordering::SeqCst);
        reassert_topmost();
    }

    if state.tick.is_multiple_of(AVOID_EVERY) {
        // 先把 HWND 取出再进分支:Rust 2021 下 `if let ... = *M.lock()` 的守卫
        // 活到整个 if let 体结束,而体内 reassert_topmost 会再锁同一个
        // 非重入 Mutex——写成 `if let` 会当场自锁。
        let bar_hwnd = *BAR_HWND.lock();
        if let Some(hwnd_addr) = bar_hwnd {
            let bar = HWND(hwnd_addr as *mut core::ffi::c_void);
            let avoid = should_avoid(bar);
            if AVOID_HIDDEN.swap(avoid, Ordering::SeqCst) != avoid {
                unsafe {
                    let _ = ShowWindow(bar, if avoid { SW_HIDE } else { SW_SHOWNOACTIVATE });
                }
                debug!(
                    "taskbar lyrics {} (fullscreen / taskbar-hidden avoidance)",
                    if avoid { "avoiding" } else { "restored" }
                );
            }
            reassert_topmost();
        }
    }

    if !state.tick.is_multiple_of(REPOSITION_EVERY) {
        return;
    }
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let Some(info) = query_taskbar() else {
        return;
    };
    if state.last == Some(info) {
        return;
    }
    state.last = Some(info);
    if let Err(err) = reposition_window(&window, *STORED_RATIO.lock()) {
        debug!("taskbar lyrics follow reposition failed: {err}");
    }
}

/// 起跟随线程。线程跑 Win32 消息泵:WINEVENT_OUTOFCONTEXT 钩子回调只在安装
/// 线程的消息泵中派发,心跳也就顺势改用线程计时器(SetTimer hwnd=None)。
/// 全程只起一次,永不退出;窗口不存在时空转。
fn ensure_follow_thread(app: AppHandle) {
    if FOLLOW_THREAD_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("taskbar-lyrics-follow".into())
        .spawn(move || unsafe {
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if hook.is_invalid() {
                // 降级:仍有心跳兜底,只是切换应用时可能看到条闪一下
                warn!("taskbar lyrics: foreground hook unavailable, heartbeat only");
            }
            let timer = SetTimer(None, 0, HEARTBEAT_MS, None);
            if timer == 0 {
                warn!("taskbar lyrics: heartbeat timer creation failed");
                return;
            }

            let mut state = FollowState::default();
            let mut msg = MSG::default();
            // GetMessageW 出错返回 -1,`> 0` 同时挡掉出错与 WM_QUIT
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if msg.message == WM_TIMER && msg.wParam.0 == timer {
                    follow_tick(&app, &mut state);
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = KillTimer(None, timer);
            if !hook.is_invalid() {
                let _ = UnhookWinEvent(hook);
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
    fn ratio_maps_across_the_full_travel_range() {
        let taskbar = bottom_taskbar();
        let metrics = bar_metrics(&taskbar);
        assert_eq!(metrics.span_start, 4);
        assert_eq!(metrics.span_end, 1920 - 9 * 32 - 4);

        // 0 / 1 打到区间两端，0.5 在正中
        assert_eq!(compute_bar_rect(&taskbar, Some(0.0)).0, metrics.span_start);
        assert_eq!(compute_bar_rect(&taskbar, Some(1.0)).0, metrics.span_end);
        assert_eq!(
            compute_bar_rect(&taskbar, Some(0.5)).0,
            metrics.span_start + (metrics.span_end - metrics.span_start) / 2
        );
        // 越界比例钳回端点
        assert_eq!(compute_bar_rect(&taskbar, Some(-3.0)).0, metrics.span_start);
        assert_eq!(compute_bar_rect(&taskbar, Some(9.0)).0, metrics.span_end);
    }

    #[test]
    fn ratio_from_position_inverts_compute_bar_rect() {
        let taskbar = bottom_taskbar();
        for ratio in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let (x, _, _, _) = compute_bar_rect(&taskbar, Some(ratio));
            let recovered = ratio_from_position(&taskbar, x);
            assert!(
                (recovered - ratio).abs() < 0.001,
                "比例 {ratio} 反解得到 {recovered}"
            );
        }
        // 拖出任务栏范围时反解钳回 0..=1
        assert_eq!(ratio_from_position(&taskbar, -9999), 0.0);
        assert_eq!(ratio_from_position(&taskbar, 99999), 1.0);
    }

    #[test]
    fn vertical_taskbar_ratio_travels_along_the_long_edge() {
        let left = TaskbarInfo {
            rect: (0, 0, 62, 1080),
            edge: 0,
        };
        // 垂直任务栏时比例控制纵向:0 贴顶、1 贴底
        assert_eq!(compute_bar_rect(&left, Some(0.0)).1, 8);
        assert_eq!(compute_bar_rect(&left, Some(1.0)).1, 1080 - 40 - 8);
        // 横向出挑到任务栏内侧,与比例无关
        assert_eq!(compute_bar_rect(&left, Some(0.3)).0, 62 + 8);
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
