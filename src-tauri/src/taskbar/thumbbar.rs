//! Win32/COM 接线层:主窗口子类化 + ITaskbarList3 调用。
//!
//! 线程模型(与 smtc.rs 同一原则,COM 对象绝不跨线程):
//! - `taskbar` 线程订阅 EventBus,把事件折叠进共享 [`PlanState`],渲染计划
//!   变化时 PostMessage 唤醒主线程刷新——消息只是"醒来重算"信号,刷新时
//!   总是读取共享状态的最新值,丢消息不丢状态;
//! - ITaskbarList3 与 HICON 全部活在主窗口线程的子类化 wndproc 里
//!   (`UiSide`,经 dwRefData 裸指针持有),COM 不出线程;
//! - 按钮点击(WM_COMMAND/THBN_CLICKED)经 spawn_blocking 调 AppState 播放
//!   控制——与 IPC 命令/SMTC 同一路径,且不在 wndproc 里阻塞 UI 线程。
//!
//! Explorer 重启会重发 TaskbarButtonCreated 注册消息,此时整套 COM 对象、
//! 图标与按钮全量重建,天然自愈。

use core::ffi::c_void;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tracing::{debug, warn};
use windows::core::w;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, RemoveWindowSubclass, SetWindowSubclass, TaskbarList,
    TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, THBF_ENABLED, THBF_HIDDEN,
    THBN_CLICKED, THB_FLAGS, THB_ICON, THB_TOOLTIP, THUMBBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetSystemMetrics, PostMessageW, RegisterWindowMessageW, HICON, SM_CXSMICON,
    WM_APP, WM_COMMAND, WM_NCDESTROY, WM_SETTINGCHANGE,
};

use super::icons::{self, IconGlyph};
use super::plan::{
    render_plan, Features, PlanState, ProgressPlan, RenderPlan, ToggleGlyph, BTN_NEXT, BTN_PREV,
    BTN_TOGGLE,
};
use crate::state::{AppState, TrackAdvance};
use seraph_core::PlayerEvent;

/// 子类化标识与自定义刷新消息(WM_APP 偏移取 0x5EA ≈ "SEA"raph)。
const SUBCLASS_ID: usize = 0x5EA0;
const WM_TASKBAR_REFRESH: u32 = WM_APP + 0x5EA;

/// 跨线程共享:事件线程写、主线程刷新时读。
struct Shared {
    app: AppHandle,
    hwnd: isize,
    state: Mutex<PlanState>,
    features: Mutex<Features>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

/// 主线程私有(经 dwRefData 传入 wndproc,COM/GDI 句柄不跨线程)。
struct UiSide {
    taskbar: Option<ITaskbarList3>,
    icons: Option<IconSet>,
    light_theme: bool,
    buttons_added: bool,
    last: Option<RenderPlan>,
    created_msg: u32,
}

struct IconSet {
    prev: HICON,
    play: HICON,
    pause: HICON,
    next: HICON,
}

impl Drop for IconSet {
    fn drop(&mut self) {
        for icon in [self.prev, self.play, self.pause, self.next] {
            if !icon.is_invalid() {
                unsafe {
                    let _ = DestroyIcon(icon);
                }
            }
        }
    }
}

/// 设置页开关(set_taskbar_features 命令调用)。未初始化时静默忽略。
pub fn set_features(buttons: bool, progress: bool) {
    if let Some(shared) = SHARED.get() {
        *shared.features.lock() = Features { buttons, progress };
        post_refresh(shared.hwnd);
    }
}

/// 在 Tauri setup 阶段调用。初始化失败只记日志,绝不阻断应用启动。
pub fn init(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        warn!("taskbar init skipped: main window not found");
        return;
    };
    let hwnd_addr = match window.hwnd() {
        Ok(hwnd) => hwnd.0 as isize,
        Err(err) => {
            warn!("taskbar init skipped: failed to get hwnd: {err}");
            return;
        }
    };

    let shared = Arc::new(Shared {
        app: app.clone(),
        hwnd: hwnd_addr,
        state: Mutex::new(PlanState::default()),
        features: Mutex::new(Features::default()),
    });
    if SHARED.set(shared.clone()).is_err() {
        warn!("taskbar init skipped: already initialized");
        return;
    }

    let event_rx = app.state::<AppState>().event_bus.subscribe();
    std::thread::Builder::new()
        .name("taskbar".into())
        .spawn(move || run_event_pump(shared, event_rx))
        .map(|_| ())
        .unwrap_or_else(|err| warn!("taskbar thread spawn failed: {err}"));

    // SetWindowSubclass 必须在窗口所属线程(主线程)调用
    let installed = app.run_on_main_thread(move || unsafe { install_subclass(hwnd_addr) });
    if let Err(err) = installed {
        warn!("taskbar subclass dispatch failed: {err}");
    }
}

fn run_event_pump(shared: Arc<Shared>, event_rx: crossbeam_channel::Receiver<PlayerEvent>) {
    // 只在计划变化时唤醒主线程:Progress 高于 1Hz,整秒不变则计划相等
    let mut last: Option<RenderPlan> = None;
    while let Ok(event) = event_rx.recv() {
        let plan = {
            let mut state = shared.state.lock();
            state.apply_event(&event);
            render_plan(&state, &shared.features.lock())
        };
        if last != Some(plan) {
            last = Some(plan);
            post_refresh(shared.hwnd);
        }
    }
}

fn post_refresh(hwnd_addr: isize) {
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd_addr as *mut c_void)),
            WM_TASKBAR_REFRESH,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

fn current_plan() -> Option<RenderPlan> {
    let shared = SHARED.get()?;
    let state = shared.state.lock();
    Some(render_plan(&state, &shared.features.lock()))
}

unsafe fn install_subclass(hwnd_addr: isize) {
    let hwnd = HWND(hwnd_addr as *mut c_void);
    let ui = Box::into_raw(Box::new(UiSide {
        taskbar: None,
        icons: None,
        light_theme: false,
        buttons_added: false,
        last: None,
        created_msg: RegisterWindowMessageW(w!("TaskbarButtonCreated")),
    }));
    if !SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, ui as usize).as_bool() {
        warn!("taskbar subclass install failed");
        drop(Box::from_raw(ui));
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    ref_data: usize,
) -> LRESULT {
    let ui = &mut *(ref_data as *mut UiSide);

    if msg == ui.created_msg && ui.created_msg != 0 {
        on_taskbar_created(hwnd, ui);
    } else {
        match msg {
            WM_TASKBAR_REFRESH => refresh(hwnd, ui),
            WM_COMMAND => {
                let notification = ((wparam.0 >> 16) & 0xffff) as u32;
                if notification == THBN_CLICKED {
                    dispatch_button((wparam.0 & 0xffff) as u16);
                    return LRESULT(0);
                }
            }
            // 任务栏深浅主题切换:重建图标并重发按钮
            WM_SETTINGCHANGE => retheme_if_needed(hwnd, ui),
            WM_NCDESTROY => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
                drop(Box::from_raw(ref_data as *mut UiSide));
                return result;
            }
            _ => {}
        }
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// TaskbarButtonCreated:应用启动时任务栏按钮就绪或 Explorer 重启后重建。
/// 每次都全量重建 COM 对象、图标与按钮(旧代理在 Explorer 重启后已失效)。
unsafe fn on_taskbar_created(hwnd: HWND, ui: &mut UiSide) {
    ui.taskbar = None;
    ui.buttons_added = false;
    ui.last = None;

    let taskbar: ITaskbarList3 = match CoCreateInstance(&TaskbarList, None, CLSCTX_ALL) {
        Ok(taskbar) => taskbar,
        Err(err) => {
            warn!("taskbar list unavailable: {err:?}");
            return;
        }
    };
    if let Err(err) = taskbar.HrInit() {
        warn!("taskbar HrInit failed: {err:?}");
        return;
    }

    ui.light_theme = taskbar_uses_light_theme();
    ui.icons = build_icons(ui.light_theme);

    let Some(plan) = current_plan() else { return };
    if let Some(buttons) = build_buttons(ui, &plan) {
        match taskbar.ThumbBarAddButtons(hwnd, &buttons) {
            Ok(()) => ui.buttons_added = true,
            Err(err) => warn!("taskbar ThumbBarAddButtons failed: {err:?}"),
        }
    }
    apply_progress(&taskbar, hwnd, plan.progress);
    ui.last = Some(plan);
    ui.taskbar = Some(taskbar);
    debug!("taskbar thumbbar attached");
}

unsafe fn refresh(hwnd: HWND, ui: &mut UiSide) {
    let Some(taskbar) = ui.taskbar.as_ref() else {
        return;
    };
    let Some(plan) = current_plan() else { return };
    if ui.last == Some(plan) {
        return;
    }

    if ui.buttons_added {
        if let Some(buttons) = build_buttons(ui, &plan) {
            if let Err(err) = taskbar.ThumbBarUpdateButtons(hwnd, &buttons) {
                debug!("taskbar ThumbBarUpdateButtons failed: {err:?}");
            }
        }
    }
    apply_progress(taskbar, hwnd, plan.progress);
    ui.last = Some(plan);
}

unsafe fn retheme_if_needed(hwnd: HWND, ui: &mut UiSide) {
    if ui.taskbar.is_none() {
        return;
    }
    let light_theme = taskbar_uses_light_theme();
    if light_theme == ui.light_theme {
        return;
    }
    ui.light_theme = light_theme;
    ui.icons = build_icons(light_theme);
    // 置空缓存强制 refresh 用新图标重发按钮
    ui.last = None;
    refresh(hwnd, ui);
}

fn build_icons(light_theme: bool) -> Option<IconSet> {
    let color = if light_theme {
        icons::INK
    } else {
        icons::PAPER
    };
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as usize;
    let make = |glyph| unsafe { icons::create_hicon(glyph, size, color) };
    Some(IconSet {
        prev: make(IconGlyph::Prev)?,
        play: make(IconGlyph::Play)?,
        pause: make(IconGlyph::Pause)?,
        next: make(IconGlyph::Next)?,
    })
}

fn build_buttons(ui: &UiSide, plan: &RenderPlan) -> Option<[THUMBBUTTON; 3]> {
    let icons = ui.icons.as_ref()?;
    let toggle_icon = match plan.toggle {
        ToggleGlyph::Play => icons.play,
        ToggleGlyph::Pause => icons.pause,
    };
    Some([
        thumb_button(BTN_PREV, icons.prev, "上一首", plan.buttons_visible),
        thumb_button(BTN_TOGGLE, toggle_icon, "播放 / 暂停", plan.buttons_visible),
        thumb_button(BTN_NEXT, icons.next, "下一首", plan.buttons_visible),
    ])
}

fn thumb_button(id: u16, icon: HICON, tip_text: &str, visible: bool) -> THUMBBUTTON {
    let mut tip = [0u16; 260];
    for (slot, unit) in tip.iter_mut().zip(tip_text.encode_utf16().take(259)) {
        *slot = unit;
    }
    THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id as u32,
        hIcon: icon,
        szTip: tip,
        dwFlags: if visible { THBF_ENABLED } else { THBF_HIDDEN },
        ..Default::default()
    }
}

unsafe fn apply_progress(taskbar: &ITaskbarList3, hwnd: HWND, progress: ProgressPlan) {
    // SetProgressValue 会把 NOPROGRESS/INDETERMINATE 隐式切回 NORMAL,
    // 因此先设值再显式设状态(Paused 需要覆盖隐式 NORMAL)
    let result = match progress {
        ProgressPlan::None => taskbar.SetProgressState(hwnd, TBPF_NOPROGRESS),
        ProgressPlan::Indeterminate => taskbar.SetProgressState(hwnd, TBPF_INDETERMINATE),
        ProgressPlan::Normal { done, total } => taskbar
            .SetProgressValue(hwnd, done, total)
            .and_then(|()| taskbar.SetProgressState(hwnd, TBPF_NORMAL)),
        ProgressPlan::Paused { done, total } => taskbar
            .SetProgressValue(hwnd, done, total)
            .and_then(|()| taskbar.SetProgressState(hwnd, TBPF_PAUSED)),
    };
    if let Err(err) = result {
        debug!("taskbar progress update failed: {err:?}");
    }
}

/// 任务栏是否为浅色主题(SystemUsesLightTheme;缺省视为深色,与系统默认一致)。
fn taskbar_uses_light_theme() -> bool {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && value == 1
}

/// 按钮点击 → 播放控制。spawn_blocking 移出 UI 线程(播放命令可阻塞数百毫秒),
/// 与 IPC/SMTC 同一 AppState 路径,事件流自然回推前端与本模块。
fn dispatch_button(id: u16) {
    let Some(shared) = SHARED.get() else { return };
    let app = shared.app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let result = match id {
            BTN_PREV => state.advance_track(TrackAdvance::Previous),
            BTN_NEXT => state.advance_track(TrackAdvance::Next),
            BTN_TOGGLE => crate::smtc::toggle_playback(&state),
            _ => Ok(()),
        };
        if let Err(err) = result {
            warn!("taskbar button action failed: {err}");
        }
    });
}
