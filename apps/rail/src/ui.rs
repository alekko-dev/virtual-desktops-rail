pub(crate) const ACTION_TIMEOUT_NOTICE: &str = "A window action could not be verified in time. Its button remains disabled while that action is outstanding.";

use crate::{
    EVENTS, Event, MAIN_WINDOW, OVERRUN, WAKE, actions, bridge, icons, native::*, notify, platform,
    rename, store, theme, theme::dark,
};
use crossbeam_channel::{Receiver, Sender};
use rail_core::{Model, Saved};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const CLASS: &str = "VirtualDesktopRail";
// LR_SHARED keeps resource icons cached by Windows; callers must not destroy them.
fn app_icon(size: i32) -> Handle {
    unsafe {
        LoadImageW(
            GetModuleHandleW(null()),
            101usize as *const u16,
            1,
            size,
            size,
            0x8000,
        )
    }
}
const PAUSE: usize = 5000;
const SETTINGS: usize = 5001;
const LIGHT_SWITCH: usize = 5002;
const SET_PAUSE: usize = 6000;
const STARTUP: usize = 6001;
const RETRY: usize = 6002;
const EXIT: usize = 6003;
const DONE: usize = 6004;
const AUTO_RESTORE: usize = 6005;
const MINIMIZE_SNAPPED_ELSEWHERE: usize = 6006;
const DESKTOP_TINT: usize = 6007;
const SHOW_DESKTOP_NAME: usize = 6008;
const SETTINGS_FOOTER_Y: i32 = 420;
const SAVE_REPORT: usize = 6105;
const REPORT_PICKER_NOTICE: &str = "Choose a location to save the diagnostic report…";
const REPORT_OUTPUT_PATH_CAPACITY: usize = 32768;
const REVEAL_TIMER: usize = 0x524c;
const REVEAL_DURATION_MS: u64 = 200;

#[derive(Default)]
struct SlideIn {
    started: Option<u64>,
}

impl SlideIn {
    fn offset(&mut self, now: u64, width: i32) -> i32 {
        let Some(started) = self.started else {
            return 0;
        };
        let elapsed = now.saturating_sub(started);
        if elapsed >= REVEAL_DURATION_MS {
            self.started = None;
            return 0;
        }
        let remaining = 1.0 - elapsed as f64 / REVEAL_DURATION_MS as f64;
        -(f64::from(width.max(0)) * remaining.powi(3)).round() as i32
    }
}

fn animations_enabled() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let mut enabled = windows::Win32::Foundation::BOOL(0);
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && enabled.as_bool()
    }
}

#[derive(Clone, PartialEq, Eq)]
enum ButtonKind {
    Desktop(String),
    Window(String),
    Pause,
    LightSwitch,
    Settings,
    Exit,
}
struct Button {
    hwnd: Handle,
    id: usize,
    kind: ButtonKind,
    caption: String,
    enabled: bool,
    y: i32,
    cell: i32,
    width: i32,
}

#[repr(C)]
struct TrackMouseEventData {
    size: u32,
    flags: u32,
    track: Handle,
    hover_time: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn TrackMouseEvent(event: *mut TrackMouseEventData) -> i32;
}

static HOVERED_BUTTON: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static KEYBOARD_FOCUS: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn button_procedure(
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == 0x007b || (message == 0x0100 && wparam == 0x71) {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{GetParent, PostMessageW};
        if let Ok(parent) = unsafe { GetParent(HWND(window)) } {
            let _ =
                unsafe { PostMessageW(parent, rename::OPEN, WPARAM(window as usize), LPARAM(0)) };
        }
        return 0;
    }
    match message {
        0x0014 => return 1,
        0x0201 | 0x0203 if KEYBOARD_FOCUS.swap(false, Ordering::AcqRel) => unsafe {
            InvalidateRect(window, null(), 0);
        },
        0x0100
            if matches!(wparam, 0x09 | 0x20 | 0x0d | 0x25..=0x28)
                && !KEYBOARD_FOCUS.swap(true, Ordering::AcqRel) =>
        unsafe {
            InvalidateRect(window, null(), 0);
        },
        0x0200 => {
            let previous = HOVERED_BUTTON.swap(window as usize, Ordering::AcqRel);
            if previous != window as usize {
                if previous != 0 {
                    unsafe {
                        InvalidateRect(previous as Handle, null(), 0);
                    }
                }
                let mut track = TrackMouseEventData {
                    size: size_of::<TrackMouseEventData>() as u32,
                    flags: 0x0000_0002, // TME_LEAVE
                    track: window,
                    hover_time: 0,
                };
                unsafe {
                    TrackMouseEvent(&mut track);
                    InvalidateRect(window, null(), 0);
                }
            }
        }
        0x02a3
            if HOVERED_BUTTON
                .compare_exchange(window as usize, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok() =>
        unsafe {
            InvalidateRect(window, null(), 0);
        },
        _ => {}
    }
    let orig = unsafe { GetWindowLongPtrW(window, -21) };
    if orig != 0 {
        let proc: unsafe extern "system" fn(
            windows::Win32::Foundation::HWND,
            u32,
            windows::Win32::Foundation::WPARAM,
            windows::Win32::Foundation::LPARAM,
        ) -> windows::Win32::Foundation::LRESULT = unsafe { std::mem::transmute(orig) };
        return unsafe {
            windows::Win32::UI::WindowsAndMessaging::CallWindowProcW(
                Some(proc),
                windows::Win32::Foundation::HWND(window),
                message,
                windows::Win32::Foundation::WPARAM(wparam),
                windows::Win32::Foundation::LPARAM(lparam),
            )
            .0
        };
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Settings,
    Pause,
    Resume,
    Retry,
    Exit,
    Status { nonce: String },
    Click { key: String },
    Switch { guid: String },
    AutomationUi { enabled: bool },
    ExperimentalIntegration { enabled: bool },
    AutoRestore { enabled: bool },
    DesktopTint { enabled: bool },
    ShowDesktopName { enabled: bool },
    MinimizeSnappedElsewhere { enabled: bool },
}

#[derive(Default)]
pub(crate) struct DesktopNameRaster {
    name: String,
    dpi: u32,
    rail_width: i32,
    height: i32,
    mask: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DesktopNameLayout {
    pub rect: Rect,
    pub text_top: i32,
    pub desktop_id: String,
    pub name: String,
}

pub(crate) fn compute_desktop_name_layout(
    show_desktop_name: bool,
    current_desktop: Option<(&str, &str, usize)>,
    font_dpi: u32,
    rail_width: i32,
    content_bottom: i32,
    footer_top: i32,
    raster: &mut DesktopNameRaster,
) -> Option<DesktopNameLayout> {
    if !show_desktop_name {
        return None;
    }
    let (id, raw_name, index) = current_desktop?;
    let dpi = font_dpi.max(96);
    let scale = |dip: i32| (dip * dpi as i32 + 48) / 96;
    let pad = scale(8);
    let bottom = footer_top - scale(12);
    let name = if raw_name.is_empty() {
        format!("Desktop {}", index + 1)
    } else {
        raw_name.to_string()
    };

    if raster.name != name || raster.dpi != dpi || raster.rail_width != rail_width {
        let (height, mask) = rasterize_desktop_name(&name, dpi, rail_width);
        *raster = DesktopNameRaster {
            name: name.clone(),
            dpi,
            rail_width,
            height,
            mask,
        };
    }

    let total_height = raster.height + pad * 2;
    if raster.height <= 0
        || total_height > bottom - content_bottom - scale(12)
        || raster.rail_width > rail_width
    {
        return None;
    }

    let top = bottom - total_height;
    Some(DesktopNameLayout {
        rect: Rect {
            left: 0,
            top,
            right: rail_width,
            bottom,
        },
        text_top: top + pad,
        desktop_id: id.to_string(),
        name,
    })
}

struct App {
    window: Handle,
    settings: Handle,
    rename: Option<rename::Popover>,
    tooltip: Handle,
    font: Handle,
    desktop_name_raster: DesktopNameRaster,
    desktop_name_hovered: bool,
    desktop_name_pressed: bool,
    font_dpi: u32,
    settings_fonts: Vec<usize>,
    settings_signature: String,
    model: Model,
    events: Receiver<Event>,
    actions: Sender<actions::Work>,
    integration: Option<Sender<bridge::CommandMessage>>,
    storage: store::Storage,
    startup: Sender<bool>,
    icon_jobs: Sender<(rail_core::Window, u32)>,
    icon_cache: BTreeMap<(String, u32), usize>,
    icon_pending: BTreeSet<(String, u32)>,
    generation: Arc<AtomicU64>,
    paused: Arc<AtomicBool>,
    tickets: BTreeMap<String, Arc<AtomicU64>>,
    focus_from: BTreeMap<String, usize>,
    buttons: Vec<Button>,
    tips: Vec<Vec<u16>>,
    signature: String,
    status: String,
    notice: String,
    snap_notice: crate::snap_notice::Lifecycle,
    loaded: bool,
    ready: bool,
    transition_received_us: Option<u64>,
    dispatch_latency: crate::metrics::DispatchLatency,
    startup_busy: bool,
    theme_busy: bool,
    report_busy: bool,
    diagnostic_window_labels: BTreeMap<String, String>,
    diagnostic_desktop_labels: BTreeMap<String, String>,
    next_diagnostic_window: u32,
    next_diagnostic_desktop: u32,
    pre_foreground: Option<usize>,
    hooks: Vec<usize>,
    taskbar_message: u32,
    shutdown_ack: Option<Receiver<Result<(), String>>>,
    stopped: bool,
    reveal: SlideIn,
    reveal_offset: i32,
    shell_switcher: Option<usize>,
}

struct ReportDraft {
    context: serde_json::Value,
    windows: Vec<store::ReportWindow>,
    previous_notice: String,
}

impl App {
    fn open_rename(&mut self, anchor: Handle) {
        if !self.ready {
            return;
        }
        if anchor == self.window {
            self.open_rename_desktop_name();
            return;
        }
        let Some(button) = self.buttons.iter().find(|button| button.hwnd == anchor) else {
            return;
        };
        let ButtonKind::Desktop(guid) = &button.kind else {
            return;
        };
        let Some((index, desktop)) = self
            .model
            .desktops()
            .iter()
            .enumerate()
            .find(|(_, desktop)| desktop.id == *guid)
        else {
            return;
        };
        let name = if desktop.name.is_empty() {
            format!("Desktop {}", index + 1)
        } else {
            desktop.name.clone()
        };
        let guid = guid.clone();
        let mut anchor_rect = windows::Win32::Foundation::RECT::default();
        if unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                windows::Win32::Foundation::HWND(anchor),
                &mut anchor_rect,
            )
        }
        .is_err()
        {
            return;
        }
        self.rename.take();
        self.rename = rename::Popover::show(self.window, anchor_rect, guid, &name);
    }

    fn desktop_name_layout(&mut self, width: i32, footer_top: i32) -> Option<DesktopNameLayout> {
        let current_desktop = self.model.current().and_then(|current_id| {
            self.model
                .desktops()
                .iter()
                .enumerate()
                .find(|(_, d)| d.id == current_id)
                .map(|(index, d)| (d.id.as_str(), d.name.as_str(), index))
        });
        let dpi = self.font_dpi.max(96);
        let scale = |dip: i32| (dip * dpi as i32 + 48) / 96;
        let content_bottom = self
            .buttons
            .iter()
            .filter(|button| matches!(button.kind, ButtonKind::Desktop(_) | ButtonKind::Window(_)))
            .map(|button| button.y + button.cell)
            .max()
            .unwrap_or(scale(42));
        compute_desktop_name_layout(
            self.model.saved().show_desktop_name,
            current_desktop,
            dpi,
            width,
            content_bottom,
            footer_top,
            &mut self.desktop_name_raster,
        )
    }

    fn current_desktop_name_layout(&mut self) -> Option<DesktopNameLayout> {
        let mut client = Rect::default();
        unsafe {
            GetClientRect(self.window, &mut client);
        }
        let dpi = unsafe { GetDpiForWindow(self.window).max(96) as i32 };
        let scale = |dip: i32| (dip * dpi + 48) / 96;
        let footer_top = client.bottom - scale(172);
        let width = client.right - 1;
        self.desktop_name_layout(width, footer_top)
    }

    fn open_rename_desktop_name(&mut self) {
        if !self.ready {
            return;
        }
        self.desktop_name_hovered = false;
        self.desktop_name_pressed = false;
        let Some(layout) = self.current_desktop_name_layout() else {
            return;
        };
        let mut top_left = windows::Win32::Foundation::POINT {
            x: layout.rect.left,
            y: layout.rect.top,
        };
        let mut bottom_right = windows::Win32::Foundation::POINT {
            x: layout.rect.right,
            y: layout.rect.bottom,
        };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(
                windows::Win32::Foundation::HWND(self.window),
                &mut top_left,
            );
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(
                windows::Win32::Foundation::HWND(self.window),
                &mut bottom_right,
            );
        }
        let anchor_rect = windows::Win32::Foundation::RECT {
            left: top_left.x,
            top: top_left.y,
            right: bottom_right.x,
            bottom: bottom_right.y,
        };
        self.rename.take();
        self.rename =
            rename::Popover::show(self.window, anchor_rect, layout.desktop_id, &layout.name);
    }

    fn save_rename(&mut self, source: Handle) {
        let desktops = self.model.desktops();
        let Some(popover) = self
            .rename
            .as_mut()
            .filter(|popover| popover.window == source)
        else {
            return;
        };
        let Some((guid, name, token)) = popover.request(desktops) else {
            return;
        };
        if !self.ready
            || !self.integration.as_ref().is_some_and(|sender| {
                sender
                    .try_send(bridge::CommandMessage::Rename { guid, name, token })
                    .is_ok()
            })
        {
            popover.fail("Desktop integration is unavailable. Try again.");
        }
    }

    fn reconcile_shell_switcher(&mut self) {
        let switcher = platform::shell_switcher_visible(self.shell_switcher);
        if switcher == self.shell_switcher {
            return;
        }
        let was_open = self.shell_switcher.is_some();
        self.shell_switcher = switcher;
        let _ = self.storage.sender.try_send(store::Work::Trace(
            serde_json::json!({"event":"shell_switcher_visibility","visible":switcher.is_some()}),
        ));
        if switcher.is_some() {
            // Exclude from Task View previews and hold an empty region without a
            // frame timer while Task View is open.
            self.finish_reveal();
        } else if was_open {
            self.begin_reveal(0);
        }
        self.update_window_frame();
    }

    fn begin_reveal(&mut self, delay_ms: u64) {
        use windows::Win32::UI::WindowsAndMessaging::SetTimer;
        if !animations_enabled() || self.shell_switcher.is_some() {
            self.finish_reveal();
            return;
        }
        self.reveal.started = Some(platform::tick().saturating_add(delay_ms));
        // Only this short-lived presentation timer runs at frame cadence.
        if unsafe { SetTimer(platform::hwnd(self.window as usize), REVEAL_TIMER, 16, None) } == 0 {
            self.reveal.started = None;
        }
        self.step_reveal();
    }

    fn finish_reveal(&mut self) {
        self.reveal.started = None;
        self.step_reveal();
    }

    fn step_reveal(&mut self) {
        use windows::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*};
        let window = platform::hwnd(self.window as usize);
        let mut client = Rect::default();
        unsafe {
            crate::native::GetClientRect(self.window, &mut client);
        }
        let mut offset = if self.shell_switcher.is_some() {
            -client.right
        } else {
            self.reveal.offset(platform::tick(), client.right)
        };
        if offset != self.reveal_offset {
            offset = self.apply_window_region(offset, client.right, client.bottom);
            self.reveal_offset = offset;
            self.update_window_frame();
            for button in &self.buttons {
                unsafe {
                    let _ = SetWindowPos(
                        platform::hwnd(button.hwnd as usize),
                        None,
                        offset,
                        button.y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                    );
                }
            }
            unsafe {
                let _ = RedrawWindow(window, None, None, RDW_INVALIDATE | RDW_ALLCHILDREN);
            }
        }
        if self.reveal.started.is_none() {
            unsafe {
                let _ = KillTimer(window, REVEAL_TIMER);
            }
        }
    }

    fn update_window_frame(&self) {
        use windows::Win32::Foundation::{BOOL, HWND};
        use windows::Win32::Graphics::Dwm::{
            DWMWA_EXCLUDED_FROM_PEEK, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        };
        let hwnd = HWND(self.window);
        let dark_mode = BOOL(i32::from(dark()));
        let excluded = BOOL(i32::from(self.shell_switcher.is_some()));
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as *const std::ffi::c_void,
                size_of::<BOOL>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_EXCLUDED_FROM_PEEK,
                &excluded as *const _ as *const std::ffi::c_void,
                size_of::<BOOL>() as u32,
            );
        }
    }

    fn apply_window_region(&mut self, mut offset: i32, width: i32, height: i32) -> i32 {
        use windows::Win32::Graphics::Gdi::*;
        let window = platform::hwnd(self.window as usize);
        let region = if offset == 0 {
            HRGN::default()
        } else {
            unsafe { CreateRectRgn(0, 0, (width + offset).max(0), height) }
        };
        if (offset != 0 && region.is_invalid())
            || unsafe { SetWindowRgn(window, region, false) } == 0
        {
            if !region.is_invalid() {
                unsafe {
                    let _ = DeleteObject(region);
                }
            }
            self.reveal.started = None;
            offset = 0;
            unsafe {
                SetWindowRgn(window, HRGN::default(), true);
            }
        }
        offset
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::AutomationUi { enabled } => unsafe {
                // Temporary test visibility. Keep WS_EX_NOACTIVATE and the same
                // button procedure; normal launches retain the validated tool style.
                let style = GetWindowLongPtrW(self.window, -20);
                SetWindowLongPtrW(
                    self.window,
                    -20,
                    if enabled {
                        (style | 0x40000) & !0x80
                    } else {
                        (style | 0x80) & !0x40000
                    },
                );
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                    platform::hwnd(self.window as usize),
                    None,
                    0,
                    0,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED,
                );
            },
            Command::Settings => self.show_settings(),
            Command::DesktopTint { enabled } => {
                self.model.set_desktop_tint(enabled);
            }
            Command::ShowDesktopName { enabled } => {
                self.model.set_show_desktop_name(enabled);
            }
            Command::AutoRestore { enabled } => {
                self.model.set_auto_restore(enabled);
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"auto_restore_changed","enabled":enabled}),
                ));
            }
            Command::MinimizeSnappedElsewhere { enabled } => {
                self.model.set_minimize_snapped_elsewhere(enabled);
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"minimize_snapped_elsewhere_changed","enabled":enabled}),
                ));
            }
            Command::ExperimentalIntegration { enabled } => {
                if self.model.set_experimental_integration(enabled) {
                    self.command(Command::Retry);
                }
            }
            Command::Pause => {
                self.model.pause(true);
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"paused","value":true}),
                ));
            }
            Command::Resume => {
                self.model.pause(false);
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"paused","value":false}),
                ));
            }
            Command::Retry => {
                self.shell_switcher = None;
                self.finish_reveal();
                self.update_window_frame();
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"integration_retry_requested"}),
                ));
                self.model.unavailable();
                self.ready = false;
                self.status = "Reconnecting to Windows desktops…".into();
                let experimental = self.model.saved().experimental_integration;
                if let Some(integration) = &self.integration {
                    let _ = integration.try_send(bridge::CommandMessage::Retry { experimental });
                } else {
                    match platform::supported(experimental) {
                        Ok(()) => self.integration = Some(bridge::start(experimental)),
                        Err(error) => self.status = error,
                    }
                }
            }
            Command::Exit => {
                self.stop();
                unsafe {
                    DestroyWindow(self.window);
                }
                return;
            }
            Command::Status { nonce } => {
                let saved = self.model.saved();
                let value = serde_json::json!({"schema_version":1,"build":crate::version::info(),"pid":std::process::id(),"status":self.status,"ready":self.ready,"notice":self.notice,
                    "paused":saved.paused,"startup":saved.startup,"generation":self.model.generation(),"current":self.model.current(),
                    "appbar":crate::appbar::diagnostics(self.window),
                    "reveal":{"active":self.reveal.started.is_some(),"offset_px":self.reveal_offset,"held_for_switcher":self.shell_switcher.is_some()},
                    "dispatch_latency":self.dispatch_latency,
                    "experimental_integration":saved.experimental_integration,
                    "auto_restore":saved.auto_restore,
                    "minimize_snapped_elsewhere":saved.minimize_snapped_elsewhere,
                    "desktop_tint":saved.desktop_tint,
                    "show_desktop_name":saved.show_desktop_name,
                    "desktops":self.model.desktops(),"windows":self.model.present().map(|(key,r)|serde_json::json!({
                        "key":key,"hwnd":r.window().hwnd,"pid":r.window().pid,"minimized":r.window().minimized,"blocked":r.blocked(),
                        "responsive":r.window().responsive,"pending":r.pending().is_some(),"layout":r.window().layout,
                        "saved_layouts":saved.windows.get(key).map(|r|&r.layouts),
                        "minimized_desktops":saved.windows.get(key).map(|r|&r.minimized_desktops),
                        "layout_count":saved.windows.get(key).map(|r|r.layouts.len()).unwrap_or(0)})).collect::<Vec<_>>()});
                if self
                    .storage
                    .sender
                    .try_send(store::Work::Export(nonce, value))
                    .is_err()
                {
                    self.notice = "Diagnostic output queue is busy.".into();
                }
            }
            Command::Switch { guid } => {
                if let (true, Some(integration), Some(current)) =
                    (self.ready, &self.integration, self.model.current())
                    && self.model.desktops().iter().any(|d| d.id == guid)
                    && current != guid
                {
                    let _ = integration.try_send(bridge::CommandMessage::Switch {
                        guid,
                        expected_current: current.into(),
                        expected_serial: self.model.transition_serial(),
                    });
                }
            }
            Command::Click { key } => {
                if !self.ready {
                    return;
                }
                let foreground = self
                    .pre_foreground
                    .take()
                    .unwrap_or_else(|| unsafe { GetForegroundWindow() } as usize);
                if let Some((hwnd, popup)) = self
                    .model
                    .runtime(&key)
                    .map(|runtime| (runtime.window().hwnd, runtime.window().popup))
                {
                    self.focus_from.insert(key.clone(), foreground);
                    // This is a nonblocking USER state query, not an application message.
                    let minimized = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::IsIconic(platform::hwnd(hwnd))
                    }
                    .as_bool();
                    let root = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::GetAncestor(
                            platform::hwnd(foreground),
                            windows::Win32::UI::WindowsAndMessaging::GA_ROOTOWNER,
                        )
                    }
                    .0 as usize;
                    let was_foreground =
                        foreground == hwnd || root == hwnd || Some(foreground) == popup;
                    self.model.click(
                        &key,
                        rail_core::Click {
                            minimized,
                            was_foreground,
                        },
                    );
                }
            }
        }
        self.dispatch();
        self.render(false);
    }

    fn dispatch(&mut self) {
        self.focus_from
            .retain(|key, _| self.model.runtime(key).is_some());
        self.generation
            .store(self.model.generation(), Ordering::Release);
        self.paused
            .store(self.model.saved().paused, Ordering::Release);
        for (key, sequence) in self.model.tickets() {
            self.tickets
                .entry(key.to_owned())
                .or_insert_with(|| Arc::new(AtomicU64::new(sequence)))
                .store(sequence, Ordering::Release);
        }
        for request in self.model.take_requests(platform::tick()) {
            let key = request.key.clone();
            let sequence = request.sequence;
            let job = actions::Job {
                sequence: self.tickets[&key].clone(),
                generation: self.generation.clone(),
                paused: self.paused.clone(),
                rail: self.window as usize,
                focus_from: self.focus_from.get(&key).copied().unwrap_or(0),
                transition_received_us: self.transition_received_us,
                request,
            };
            if self.actions.try_send(actions::Work::Action(job)).is_err() {
                self.model.settle(
                    &key,
                    sequence,
                    platform::tick(),
                    rail_core::Outcome::Rejected,
                );
                self.notice = "Window action queue is busy; try again.".into();
            }
        }
        if self.loaded
            && self.model.is_dirty()
            && self
                .storage
                .sender
                .try_send(store::Work::Save(self.model.saved().clone()))
                .is_ok()
        {
            self.model.mark_clean();
        }
    }

    fn drain(&mut self) {
        if OVERRUN.swap(false, Ordering::AcqRel) {
            self.icon_pending.clear();
            self.command(Command::Retry);
            self.notice = "An event queue overflowed; establishing a fresh baseline.".into();
        }
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Command(command) => self.command(command),
                Event::Renamed { token, result } => {
                    if let Some(popover) = self
                        .rename
                        .as_mut()
                        .filter(|popover| popover.token == token)
                    {
                        match result {
                            Ok(()) => {
                                self.rename.take();
                            }
                            Err(error) => popover.fail(&error),
                        }
                    }
                }
                Event::Loaded {
                    saved,
                    path,
                    warning,
                } => {
                    self.loaded = true;
                    self.model = Model::new(saved);
                    self.notice = warning.unwrap_or_default();
                    if path.as_os_str().is_empty() && self.notice.is_empty() {
                        self.notice =
                            "Saved layouts are unavailable; this run uses memory only.".into();
                    }
                    if self.model.saved().startup {
                        self.startup_busy = true;
                        let _ = self.startup.try_send(true);
                    }
                    let experimental = self.model.saved().experimental_integration;
                    match platform::supported(experimental) {
                        Ok(()) => self.integration = Some(bridge::start(experimental)),
                        Err(error) => self.status = error,
                    }
                }
                Event::Frame(frame, received, experimental) => {
                    // Ignore queued frames from a worker superseded by a legacy
                    // preference change while its replacement is reconnecting.
                    if experimental != self.model.saved().experimental_integration {
                        continue;
                    }
                    // The existing reconciliation is also a fallback if a
                    // foreground event was dropped; it adds no polling timer.
                    self.reconcile_shell_switcher();
                    crate::appbar::foreground_changed(self.window);
                    for desktop in &frame.desktops {
                        if !self.diagnostic_desktop_labels.contains_key(&desktop.id)
                            && self.diagnostic_desktop_labels.len() < 128
                        {
                            self.next_diagnostic_desktop += 1;
                            self.diagnostic_desktop_labels.insert(
                                desktop.id.clone(),
                                format!("desktop-{:03}", self.next_diagnostic_desktop),
                            );
                        }
                    }
                    if !self.ready {
                        let _ = self
                            .storage
                            .sender
                            .try_send(store::Work::Log("Desktop integration is ready.".into()));
                    }
                    self.ready = true;
                    self.status = "Desktop integration is ready.".into();
                    if self.model.current().is_some_and(|current| {
                        current != frame.current
                            || self.model.transition_serial() != frame.transition_serial
                    }) {
                        let from = self
                            .model
                            .current()
                            .and_then(|id| self.diagnostic_desktop_labels.get(id).cloned())
                            .unwrap_or_else(|| "unknown".into());
                        let to = self
                            .diagnostic_desktop_labels
                            .get(&frame.current)
                            .cloned()
                            .unwrap_or_else(|| "unknown".into());
                        let _ = self.storage.sender.try_send(store::Work::Trace(
                            serde_json::json!({"event":"desktop_transition_observed","from":from,"to":to,"serial":frame.transition_serial}),
                        ));
                        self.transition_received_us = received;
                    } else if self.model.current().is_none() {
                        self.transition_received_us = None;
                    }
                    self.snap_notice
                        .observe(&self.model, &frame, &mut self.notice);
                    self.model.snapshot(frame);
                    self.snap_notice.reconcile(&self.model, &mut self.notice);
                    for (key, _) in self.model.present() {
                        if !self.diagnostic_window_labels.contains_key(key)
                            && self.diagnostic_window_labels.len() < 512
                        {
                            self.next_diagnostic_window += 1;
                            self.diagnostic_window_labels.insert(
                                key.clone(),
                                format!("window-{:03}", self.next_diagnostic_window),
                            );
                        }
                    }
                }
                Event::Minimized(observation) => {
                    let key = observation.window.identity.key();
                    if self.model.observe_minimized(observation).is_ok() {
                        let label = self.diagnostic_window_label(&key);
                        let desktop = self
                            .model
                            .current()
                            .and_then(|id| self.diagnostic_desktop_labels.get(id));
                        let _ = self.storage.sender.try_send(store::Work::Trace(
                            serde_json::json!({"event":"window_minimized_intent","window":label,"desktop":desktop,"source":"external"}),
                        ));
                    }
                }
                Event::DispatchLatency(us) => self.dispatch_latency.record(us),
                Event::Status(status, ready) => {
                    if self.status != status {
                        let _ = self
                            .storage
                            .sender
                            .try_send(store::Work::Log(status.clone()));
                    }
                    self.status = status;
                    self.ready = ready;
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"integration_status","ready":ready}),
                    ));
                    if !ready {
                        if let Some(popover) =
                            self.rename.as_mut().filter(|popover| popover.pending())
                        {
                            popover.fail(
                                "Could not confirm the rename. Check the name and try again.",
                            );
                        }
                        self.shell_switcher = None;
                        self.finish_reveal();
                        self.update_window_frame();
                        self.transition_received_us = None;
                        self.model.unavailable();
                    }
                }
                Event::SnapPreservation(notice) => {
                    if self
                        .snap_notice
                        .rejected(*notice, &self.model, &mut self.notice)
                    {
                        let _ = self
                            .storage
                            .sender
                            .try_send(store::Work::Log(self.notice.clone()));
                        let _ = self.storage.sender.try_send(store::Work::Trace(
                            serde_json::json!({"event":"user_notice","kind":"other"}),
                        ));
                    }
                }
                Event::Notice(notice) => {
                    let _ = self
                        .storage
                        .sender
                        .try_send(store::Work::Log(notice.clone()));
                    self.notice = notice;
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"user_notice","kind":if self.notice==ACTION_TIMEOUT_NOTICE{"action_timeout"}else{"other"}}),
                    ));
                }
                Event::ObservedArrangement {
                    z_above,
                    request,
                    layout,
                    tick,
                } => {
                    if layout.visible.is_some() {
                        self.model.observed_z_above(&request, z_above, tick);
                    }
                    self.model.observed_arrangement(&request, &layout, tick);
                }
                Event::Accepted {
                    key,
                    sequence,
                    acceptance,
                } => {
                    let label = self.diagnostic_window_label(&key);
                    let source = self
                        .model
                        .runtime(&key)
                        .and_then(|runtime| runtime.pending())
                        .filter(|pending| pending.request().sequence == sequence)
                        .map(|pending| pending.request().action.source())
                        .unwrap_or("explicit");
                    let target_summary = acceptance.target.as_ref().map(|layout| {
                        serde_json::json!({
                            "normal":layout.normal,"maximized":layout.maximized,
                            "visible":layout.visible,"arranged":layout.arranged
                        })
                    });
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"window_action_accepted","window":label,"sequence":sequence,"target":target_summary,"action":acceptance.effect.kind(),"source":source}),
                    ));
                    self.model.accepted(&key, sequence, acceptance);
                }
                Event::ActionResult {
                    key,
                    sequence,
                    tick,
                    outcome,
                } => {
                    let settled = self.model.settle(&key, sequence, tick, outcome);
                    self.snap_notice.settled(
                        &self.model,
                        (&key, sequence, tick, outcome),
                        settled.as_ref(),
                        &mut self.notice,
                    );
                    self.snap_notice.reconcile(&self.model, &mut self.notice);
                    let observed = self.model.runtime(&key).map(|runtime| {
                        let window = runtime.window();
                        serde_json::json!({
                            "normal":window.layout.normal,
                            "visible":window.layout.visible,
                            "maximized":window.layout.maximized,
                            "arranged":window.layout.arranged,
                            "minimized":window.minimized,
                            "pending":runtime.pending().is_some(),
                            "blocked":runtime.blocked()
                        })
                    });
                    let verified = outcome == rail_core::Outcome::Verified;
                    let cancelled = matches!(
                        outcome,
                        rail_core::Outcome::Rejected | rail_core::Outcome::IdentityLost
                    );
                    let action = settled.as_ref().map(|s| {
                        s.effect
                            .map(rail_core::Effect::kind)
                            .unwrap_or_else(|| s.intent.kind())
                    });
                    let source = settled.as_ref().map(|s| s.intent.source());
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"window_action_completed","window":self.diagnostic_window_label(&key),"sequence":sequence,"outcome":outcome.name(),"verified":verified,"cancelled":cancelled,"tick_ms":tick,"observed":observed,"action":action,"source":source}),
                    ));
                    if !outcome.releases() {
                        self.notice = ACTION_TIMEOUT_NOTICE.into();
                    } else if self.notice == ACTION_TIMEOUT_NOTICE && !self.model.has_quarantined()
                    {
                        self.notice.clear();
                    }
                    if let Some(integration) = &self.integration {
                        let _ = integration.try_send(bridge::CommandMessage::Refresh);
                    }
                }
                Event::Captured {
                    key,
                    generation,
                    tick,
                    layout: Ok(layout),
                } => {
                    self.model.captured(&key, generation, tick, layout);
                }
                Event::Captured { layout: Err(_), .. } => {}
                Event::Stored(result) => {
                    if let Err(error) = result {
                        self.notice = format!("Saved layouts could not be written: {error}");
                        self.model.mark_dirty();
                    }
                }
                Event::Report(result) => {
                    self.report_busy = false;
                    self.notice = match result {
                        Ok(path) => format!("Diagnostic report saved: {}", path.display()),
                        Err(error) => format!("Diagnostic report could not be saved: {error}"),
                    };
                    self.update_settings();
                }
                Event::Startup(value, result) => {
                    self.startup_busy = false;
                    match result {
                        Ok(()) => self.model.set_startup(value),
                        Err(error) => {
                            self.notice =
                                format!("Windows startup preference could not be changed: {error}")
                        }
                    }
                }
                Event::Theme(result) => {
                    self.theme_busy = false;
                    if let Err(error) = result {
                        self.notice = format!("Windows theme could not be changed: {error}");
                        self.show_settings();
                    }
                    self.signature.clear();
                }
                Event::Icon(key, dpi, icon) => {
                    self.icon_pending.remove(&(key.clone(), dpi));
                    if self.model.runtime(&key).is_some_and(|r| r.present())
                        && dpi == unsafe { GetDpiForWindow(self.window) }.max(96)
                    {
                        if let Some(old) = self.icon_cache.insert((key, dpi), icon)
                            && old != 0
                        {
                            unsafe {
                                DestroyIcon(old as Handle);
                            }
                        }
                        self.signature.clear();
                    } else if icon != 0 {
                        unsafe {
                            DestroyIcon(icon as Handle);
                        }
                    }
                }
                Event::NativeWindow(event, hwnd, tick) => match event {
                    0x16 => {
                        if let (Some(integration), Some(probe)) =
                            (&self.integration, self.model.minimize_probe(hwnd, tick))
                        {
                            let _ = integration
                                .try_send(bridge::CommandMessage::ObserveMinimize(Box::new(probe)));
                        }
                    }
                    3 => {
                        self.reconcile_shell_switcher();
                        crate::appbar::foreground_changed(self.window);
                        if let Some(integration) = &self.integration {
                            let _ = integration.try_send(bridge::CommandMessage::Refresh);
                        }
                    }
                    10 => self.model.moving(hwnd, true, tick, None),
                    11 => {
                        self.model.moving(hwnd, false, tick, None);
                        if let Some(capture) = self.model.capture_request(hwnd) {
                            let _ = self.actions.try_send(actions::Work::Capture {
                                key: capture.key,
                                window: capture.window,
                                generation: capture.generation,
                                tick,
                            });
                        }
                    }
                    0x7e | 0x2e0 => {
                        self.signature.clear();
                    }
                    _ => {
                        if matches!(event, 0x8001..=0x8003 | 0x8017 | 0x8018)
                            && self.shell_switcher.is_some()
                        {
                            self.reconcile_shell_switcher();
                        }
                        if !matches!(event, 0x8003 | 0x8017 | 0x8018)
                            && let Some(integration) = &self.integration
                        {
                            let _ = integration.try_send(bridge::CommandMessage::Refresh);
                        }
                    }
                },
            }
            if self.stopped {
                return;
            }
        }
        self.dispatch();
        self.render(false);
    }

    fn render(&mut self, force: bool) {
        let dpi = unsafe { GetDpiForWindow(self.window) }.max(96);
        let scale = |dip: i32| (dip * dpi as i32 + 48) / 96;
        let signature = format!(
            "{:?}|{:?}|{}|{}|{:?}|{}|{}|{}|{}|{}|{}",
            self.model.desktops(),
            self.model.current(),
            self.ready,
            self.model.saved().paused,
            self.model
                .present()
                .map(|(key, r)| (
                    key,
                    r.window().minimized,
                    r.blocked(),
                    r.window().responsive
                ))
                .collect::<Vec<_>>(),
            dpi,
            self.status,
            self.model.saved().desktop_tint,
            self.model.saved().show_desktop_name,
            self.theme_busy,
            dark()
        );
        if force || signature != self.signature {
            self.signature = signature;
            if self.font.is_null() || self.font_dpi != dpi {
                let old_font = self.font;
                self.font = unsafe {
                    CreateFontW(
                        -(((12 * dpi + 48) / 96) as i32),
                        0,
                        0,
                        0,
                        400,
                        0,
                        0,
                        0,
                        1,
                        0,
                        0,
                        5,
                        0,
                        wide("Segoe UI").as_ptr(),
                    )
                };
                self.font_dpi = dpi;
                self.desktop_name_raster = DesktopNameRaster::default();
                if !old_font.is_null() {
                    unsafe {
                        DeleteObject(old_font);
                    }
                }
            }
            let windows = self
                .model
                .present()
                .map(|(k, r)| (k.clone(), r.window().clone(), r.blocked()))
                .collect::<Vec<_>>();
            self.update_window_frame();
            let mut bounds = Rect::default();
            unsafe {
                GetClientRect(self.window, &mut bounds);
            }
            self.reveal_offset =
                self.apply_window_region(self.reveal_offset, bounds.right, bounds.bottom);
            let button_width = scale(25) - 1;
            let mut specs = Vec::new();
            let mut y = scale(42);
            for (index, desktop) in self.model.desktops().iter().enumerate() {
                let name = if desktop.name.is_empty() {
                    format!("Desktop {}", index + 1)
                } else {
                    desktop.name.clone()
                };
                let caption = if self.model.current() == Some(desktop.id.as_str()) {
                    format!("{name} (current)")
                } else {
                    name
                };
                specs.push((
                    100 + index,
                    caption,
                    y,
                    scale(30),
                    ButtonKind::Desktop(desktop.id.clone()),
                    self.ready,
                ));
                y += scale(33);
            }
            y += scale(15);
            for (index, (key, window, blocked)) in windows.iter().enumerate() {
                let name = std::path::Path::new(&window.identity.executable)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                specs.push((
                    1000 + index,
                    format!("Shared {name} window {}", index + 1),
                    y,
                    scale(39),
                    ButtonKind::Window(key.clone()),
                    self.ready && !blocked && window.responsive,
                ));
                let icon_key = (key.clone(), dpi);
                if !self.icon_cache.contains_key(&icon_key)
                    && !self.icon_pending.contains(&icon_key)
                    && self.icon_jobs.try_send((window.clone(), dpi)).is_ok()
                {
                    self.icon_pending.insert(icon_key);
                }
                y += scale(45);
            }
            specs.push((
                LIGHT_SWITCH,
                if self.theme_busy {
                    "Changing Windows theme…"
                } else if dark() {
                    "Switch Windows to light theme"
                } else {
                    "Switch Windows to dark theme"
                }
                .into(),
                bounds.bottom - scale(162),
                scale(34),
                ButtonKind::LightSwitch,
                !self.theme_busy,
            ));
            specs.push((
                PAUSE,
                format!(
                    "{}. {}",
                    if self.model.saved().paused {
                        "Resume automatic window actions"
                    } else {
                        "Pause automatic window actions"
                    },
                    self.status
                ),
                bounds.bottom - scale(122),
                scale(34),
                ButtonKind::Pause,
                true,
            ));
            specs.push((
                SETTINGS,
                format!("Settings. {}", self.status),
                bounds.bottom - scale(82),
                scale(34),
                ButtonKind::Settings,
                true,
            ));
            specs.push((
                EXIT,
                "Exit application".into(),
                bounds.bottom - scale(42),
                scale(34),
                ButtonKind::Exit,
                true,
            ));
            let can_reuse = !force
                && !self.tooltip.is_null()
                && self.buttons.len() == specs.len()
                && self
                    .buttons
                    .iter()
                    .zip(&specs)
                    .all(|(b, (id, _, y, cell, kind, _))| {
                        b.id == *id
                            && b.kind == *kind
                            && b.y == *y
                            && b.cell == *cell
                            && b.width == button_width
                    });
            if can_reuse {
                self.tips.clear();
                for (button, (_, caption, _, _, _, enabled)) in self.buttons.iter_mut().zip(&specs)
                {
                    self.tips.push(wide(caption));
                    let mut info = ToolInfo {
                        size: size_of::<ToolInfo>() as u32,
                        flags: 0x01,
                        window: self.window,
                        id: button.hwnd as usize,
                        rect: Rect::default(),
                        instance: null_mut(),
                        text: self.tips.last_mut().unwrap().as_mut_ptr(),
                        parameter: 0,
                        reserved: null_mut(),
                    };
                    unsafe {
                        if button.caption != *caption {
                            SetWindowTextW(button.hwnd, info.text);
                            button.caption = caption.clone();
                            SendMessageW(self.tooltip, 0x0439, 0, &mut info as *mut _ as isize);
                        }
                        if button.enabled != *enabled {
                            button.enabled = *enabled;
                            EnableWindow(button.hwnd, *enabled as i32);
                        }
                        InvalidateRect(button.hwnd, null(), 0);
                    }
                }
            } else {
                let focused = if unsafe { GetForegroundWindow() } == self.window {
                    self.buttons
                        .iter()
                        .find(|button| button.hwnd == unsafe { GetFocus() })
                        .map(|button| button.kind.clone())
                } else {
                    None
                };
                unsafe {
                    if !self.tooltip.is_null() {
                        DestroyWindow(self.tooltip);
                    }
                    HOVERED_BUTTON.store(0, Ordering::Release);
                    for button in self.buttons.drain(..) {
                        DestroyWindow(button.hwnd);
                    }
                }
                self.tooltip = unsafe {
                    CreateWindowExW(
                        8,
                        wide("tooltips_class32").as_ptr(),
                        wide("").as_ptr(),
                        0x80000003,
                        0,
                        0,
                        0,
                        0,
                        self.window,
                        null_mut(),
                        GetModuleHandleW(null()),
                        null_mut(),
                    )
                };
                self.tips.clear();
                for (id, caption, y, cell, kind, enabled) in specs {
                    let button = self.button(id, &caption, y, cell, kind, enabled);
                    unsafe {
                        EnableWindow(button, enabled as i32);
                    }
                }
                if unsafe { GetForegroundWindow() } == self.window
                    && let Some(focused) = focused
                    && let Some(button) = self.buttons.iter().find(|button| button.kind == focused)
                {
                    unsafe {
                        SetFocus(button.hwnd);
                    }
                }
            }
            let live = self
                .model
                .present()
                .map(|(k, _)| k)
                .collect::<BTreeSet<_>>();
            self.icon_cache.retain(|(key, icon_dpi), icon| {
                if live.contains(key) && *icon_dpi == dpi {
                    true
                } else {
                    if *icon != 0 {
                        unsafe {
                            DestroyIcon(*icon as Handle);
                        }
                    }
                    false
                }
            });
            unsafe {
                InvalidateRect(self.window, null(), 0);
            }
        }
        self.update_settings();
    }

    fn button(
        &mut self,
        id: usize,
        name: &str,
        y: i32,
        cell: i32,
        kind: ButtonKind,
        enabled: bool,
    ) -> Handle {
        let button_width =
            unsafe { ((25 * GetDpiForWindow(self.window).max(96) + 48) / 96) as i32 - 1 };
        let button = unsafe {
            CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide(name).as_ptr(),
                0x5001000b,
                self.reveal_offset,
                y,
                button_width,
                cell,
                self.window,
                id as Handle,
                GetModuleHandleW(null()),
                null_mut(),
            )
        };
        unsafe {
            let orig = GetWindowLongPtrW(button, -4);
            SetWindowLongPtrW(button, -21, orig);
            SetWindowLongPtrW(button, -4, button_procedure as *const () as isize);
        }
        self.tips.push(wide(name));
        let mut info = ToolInfo {
            size: size_of::<ToolInfo>() as u32,
            flags: 0x01, // TTF_IDISHWND; relay mouse events explicitly for the passive rail.
            window: self.window,
            id: button as usize,
            rect: Rect::default(),
            instance: null_mut(),
            text: self.tips.last_mut().unwrap().as_mut_ptr(),
            parameter: 0,
            reserved: null_mut(),
        };
        unsafe {
            SendMessageW(self.tooltip, 0x432, 0, &mut info as *mut _ as isize);
        }
        self.buttons.push(Button {
            hwnd: button,
            id,
            kind,
            caption: name.into(),
            enabled,
            y,
            cell,
            width: button_width,
        });
        button
    }

    fn show_settings(&mut self) {
        if !self.settings.is_null() {
            unsafe {
                ShowWindow(self.settings, 5);
            }
            return;
        }
        let dpi = unsafe { GetDpiForWindow(self.window).max(96) } as i32;
        let scale = |dip: i32| (dip * dpi + 48) / 96;
        let pointer = unsafe { GetWindowLongPtrW(self.window, -21) };
        self.settings = unsafe {
            CreateWindowExW(
                0x40000,
                wide(CLASS).as_ptr(),
                wide("Virtual Desktop Rail — Settings").as_ptr(),
                0x12c80000,
                200,
                160,
                scale(560),
                scale(526),
                self.window,
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            )
        };
        unsafe {
            SetWindowLongPtrW(self.settings, -21, pointer);
        }
        // Settings use their own typographic hierarchy, independent of rail icon labels.
        for font in self.settings_fonts.drain(..) {
            unsafe {
                DeleteObject(font as Handle);
            }
        }
        for (size, weight) in [(14, 400), (14, 600), (12, 400), (10, 400)] {
            let font = unsafe {
                CreateFontW(
                    -scale(size),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    0,
                    wide("Segoe UI").as_ptr(),
                )
            };
            self.settings_fonts.push(font as usize);
        }
        for (id, class, label, style, x, y, width, height) in [
            (
                6102,
                "STATIC",
                "Window behavior",
                0x50000000,
                24,
                20,
                496,
                22,
            ),
            (
                SET_PAUSE,
                "BUTTON",
                "Pause automatic window actions",
                0x50010003,
                24,
                48,
                496,
                28,
            ),
            (
                AUTO_RESTORE,
                "BUTTON",
                "Restore saved layouts on desktop switch",
                0x50010003,
                24,
                82,
                496,
                28,
            ),
            (
                MINIMIZE_SNAPPED_ELSEWHERE,
                "BUTTON",
                "Minimize snapped windows on other desktops",
                0x50010003,
                24,
                116,
                496,
                28,
            ),
            (6103, "STATIC", "Startup", 0x50000000, 24, 160, 496, 22),
            (
                STARTUP,
                "BUTTON",
                "Start with Windows",
                0x50010003,
                24,
                188,
                496,
                28,
            ),
            (6110, "STATIC", "Appearance", 0x50000000, 24, 232, 496, 22),
            (
                DESKTOP_TINT,
                "BUTTON",
                "Tint rail with active desktop color",
                0x50010003,
                24,
                260,
                496,
                28,
            ),
            (
                SHOW_DESKTOP_NAME,
                "BUTTON",
                "Show desktop name",
                0x50010003,
                24,
                294,
                496,
                28,
            ),
            (
                6104,
                "STATIC",
                "Desktop integration",
                0x50000000,
                24,
                344,
                496,
                22,
            ),
            (
                RETRY,
                "BUTTON",
                "Retry desktop integration",
                0x50010000,
                24,
                372,
                212,
                32,
            ),
            (
                SAVE_REPORT,
                "BUTTON",
                "Save diagnostic report…",
                0x50010000,
                248,
                372,
                272,
                32,
            ),
            (6106, "STATIC", "", 0x50000010, 0, SETTINGS_FOOTER_Y, 560, 1),
            (DONE, "BUTTON", "Done", 0x50010001, 432, 436, 88, 34),
            (6107, "STATIC", "●", 0x50000000, 24, 431, 10, 18),
            (6100, "STATIC", "", 0x50000000, 36, 432, 372, 28),
            // Selectable, scrollable details; it occupies no space while hidden.
            (6109, "EDIT", "", 0x40a10844, 24, 456, 496, 0),
        ] {
            let child = unsafe {
                CreateWindowExW(
                    0,
                    wide(class).as_ptr(),
                    wide(label).as_ptr(),
                    style,
                    scale(x),
                    scale(y),
                    scale(width),
                    scale(height),
                    self.settings,
                    id as Handle,
                    GetModuleHandleW(null()),
                    null_mut(),
                )
            };
            unsafe {
                let font = self.settings_fonts[match id {
                    6102..=6104 | 6110 => 1,
                    6100 | 6107 | 6109 => 2,
                    _ => 0,
                }];
                SendMessageW(child, 0x30, font, 1);
            }
        }
        let label = format!("Version {}", crate::version::label());
        unsafe {
            let child = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide(&label).as_ptr(),
                0x50000000,
                scale(36),
                scale(460),
                scale(372),
                scale(14),
                self.settings,
                6108 as Handle,
                GetModuleHandleW(null()),
                null_mut(),
            );
            SendMessageW(child, 0x30, self.settings_fonts[3], 1);
        }
        self.update_settings();
    }

    fn layout_settings_notice(&self) {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::Graphics::Gdi::{
            DT_CALCRECT, DT_NOPREFIX, DT_WORDBREAK, DrawTextW, GetDC, HGDIOBJ, ReleaseDC,
            SelectObject,
        };
        use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, WINDOW_EX_STYLE,
            WINDOW_STYLE,
        };
        let window = HWND(self.settings);
        let dpi = unsafe { GetDpiForWindow(self.settings) }.max(96);
        let px = |value: i32| (i64::from(value) * i64::from(dpi) / 96) as i32;
        unsafe {
            let notice = GetDlgItem(self.settings, 6109);
            SetWindowTextW(notice, wide(&self.notice).as_ptr());
            let mut outer = RECT::default();
            let _ = GetWindowRect(window, &mut outer);
            let mut frame = RECT::default();
            let _ = AdjustWindowRectExForDpi(
                &mut frame,
                WINDOW_STYLE(0x12c80000),
                false,
                WINDOW_EX_STYLE(0x40000),
                dpi,
            );
            let chrome = frame.bottom - frame.top;
            let monitor = windows::Win32::Graphics::Gdi::MonitorFromWindow(
                window,
                windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
            );
            let work = platform::monitor(monitor)
                .ok()
                .map(|context| context.2)
                .unwrap_or([outer.left, outer.top, outer.right, outer.top + px(760)]);
            let mut notice_height = 0;
            if !self.notice.is_empty() {
                let dc = GetDC(window);
                let previous = SelectObject(dc, HGDIOBJ(self.settings_fonts[2] as *mut _));
                let mut bounds = RECT {
                    right: px(480),
                    ..Default::default()
                };
                let mut text = wide(&self.notice);
                text.pop();
                DrawTextW(
                    dc,
                    &mut text,
                    &mut bounds,
                    DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
                );
                SelectObject(dc, previous);
                ReleaseDC(window, dc);
                // The read-only edit keeps oversized details selectable and scrollable.
                let available = (work[3] - work[1] - chrome - px(526)).max(px(24));
                notice_height =
                    (bounds.bottom - bounds.top + px(12)).clamp(px(28), available.max(px(28)));
            }
            ShowWindow(notice, if self.notice.is_empty() { 0 } else { 5 });
            let extra = if self.notice.is_empty() {
                0
            } else {
                notice_height + px(12)
            };
            for (id, x, y, width, height) in [
                (6109, px(24), px(456), px(496), notice_height),
                (6108, px(36), px(460) + extra, px(372), px(14)),
                (
                    DONE as i32,
                    px(432),
                    if extra == 0 { px(436) } else { px(460) + extra },
                    px(88),
                    px(34),
                ),
            ] {
                let _ = SetWindowPos(
                    HWND(GetDlgItem(self.settings, id)),
                    None,
                    x,
                    y,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            let height = if extra == 0 {
                px(526)
            } else {
                px(526) + notice_height + chrome
            };
            let top = outer.top.clamp(work[1], (work[3] - height).max(work[1]));
            let left = outer
                .left
                .clamp(work[0], (work[2] - (outer.right - outer.left)).max(work[0]));
            let _ = SetWindowPos(
                window,
                None,
                left,
                top,
                outer.right - outer.left,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            let _ = windows::Win32::Graphics::Gdi::RedrawWindow(
                window,
                None,
                None,
                windows::Win32::Graphics::Gdi::RDW_INVALIDATE
                    | windows::Win32::Graphics::Gdi::RDW_ERASE
                    | windows::Win32::Graphics::Gdi::RDW_ALLCHILDREN,
            );
        }
    }

    fn update_settings(&mut self) {
        if self.settings.is_null() {
            return;
        }
        let saved = self.model.saved();
        let signature = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.status,
            self.notice,
            saved.paused,
            saved.startup,
            self.startup_busy,
            self.loaded,
            saved.experimental_integration,
            saved.auto_restore,
            saved.minimize_snapped_elsewhere,
            saved.desktop_tint,
            saved.show_desktop_name
        );
        if signature == self.settings_signature {
            return;
        }
        let paused = saved.paused;
        let startup = saved.startup;
        let auto_restore = saved.auto_restore;
        let desktop_tint = saved.desktop_tint;
        let show_desktop_name = saved.show_desktop_name;
        let minimize_snapped_elsewhere = saved.minimize_snapped_elsewhere;
        self.settings_signature = signature;
        self.layout_settings_notice();
        unsafe {
            SendMessageW(
                GetDlgItem(self.settings, SHOW_DESKTOP_NAME as i32),
                0xf1,
                show_desktop_name as usize,
                0,
            );
            EnableWindow(
                GetDlgItem(self.settings, SHOW_DESKTOP_NAME as i32),
                self.loaded as i32,
            );
            SendMessageW(
                GetDlgItem(self.settings, DESKTOP_TINT as i32),
                0xf1,
                desktop_tint as usize,
                0,
            );
            EnableWindow(
                GetDlgItem(self.settings, DESKTOP_TINT as i32),
                self.loaded as i32,
            );
            let light = GetDlgItem(self.settings, 6107);
            SetWindowLongPtrW(light, -21, self.ready as isize);
            InvalidateRect(light, null(), 1);
            let text = GetDlgItem(self.settings, 6100);
            SetWindowTextW(text, wide(&self.status).as_ptr());
            SendMessageW(
                GetDlgItem(self.settings, SET_PAUSE as i32),
                0xf1,
                paused as usize,
                0,
            );
            SendMessageW(
                GetDlgItem(self.settings, STARTUP as i32),
                0xf1,
                startup as usize,
                0,
            );
            SendMessageW(
                GetDlgItem(self.settings, AUTO_RESTORE as i32),
                0xf1,
                auto_restore as usize,
                0,
            );
            EnableWindow(
                GetDlgItem(self.settings, AUTO_RESTORE as i32),
                self.loaded as i32,
            );
            SendMessageW(
                GetDlgItem(self.settings, MINIMIZE_SNAPPED_ELSEWHERE as i32),
                0xf1,
                minimize_snapped_elsewhere as usize,
                0,
            );
            EnableWindow(
                GetDlgItem(self.settings, MINIMIZE_SNAPPED_ELSEWHERE as i32),
                self.loaded as i32,
            );
            EnableWindow(
                GetDlgItem(self.settings, STARTUP as i32),
                (!self.startup_busy && self.loaded) as i32,
            );
            EnableWindow(
                GetDlgItem(self.settings, SET_PAUSE as i32),
                self.loaded as i32,
            );
            EnableWindow(
                GetDlgItem(self.settings, SAVE_REPORT as i32),
                (!self.report_busy) as i32,
            );
        }
    }

    fn begin_report(&mut self) -> Option<ReportDraft> {
        if self.report_busy {
            return None;
        }
        let previous_notice = self.notice.clone();
        self.report_busy = true;
        self.notice = REPORT_PICKER_NOTICE.into();
        self.update_settings();
        let aliases: BTreeMap<_, _> = self
            .model
            .desktops()
            .iter()
            .filter_map(|desktop| {
                self.diagnostic_desktop_labels
                    .get(&desktop.id)
                    .map(|label| (desktop.id.clone(), label.clone()))
            })
            .collect();
        let desktops = self
            .model
            .desktops()
            .iter()
            .filter_map(|desktop| {
                let label = self.diagnostic_desktop_labels.get(&desktop.id)?;
                Some(serde_json::json!({
                    "label":label,
                    "current":self.model.current()==Some(desktop.id.as_str())
                }))
            })
            .collect::<Vec<_>>();
        let saved = self.model.saved();
        let windows = self
            .model
            .present()
            .map(|(key, runtime)| {
                let record = saved.windows.get(key);
                let layouts = record
                    .map(|record| {
                        record
                            .layouts
                            .iter()
                            .enumerate()
                            .map(|(index, (id, layout))| {
                                (
                                    aliases.get(id).cloned().unwrap_or_else(|| {
                                        format!("unavailable-desktop-{:03}", index + 1)
                                    }),
                                    layout.clone(),
                                )
                            })
                            .collect::<BTreeMap<_, _>>()
                    })
                    .unwrap_or_default();
                store::ReportWindow {
                    label: self.diagnostic_window_label(key),
                    window: runtime.window().clone(),
                    cached_layout: runtime.window().layout.clone(),
                    last_usable_layout: record.map(|record| record.last_usable.clone()),
                    saved_layouts: serde_json::json!(layouts),
                    minimized_desktops: record
                        .map(|record| {
                            record
                                .minimized_desktops
                                .iter()
                                .enumerate()
                                .map(|(index, id)| {
                                    aliases.get(id).cloned().unwrap_or_else(|| {
                                        format!("unavailable-minimized-desktop-{:03}", index + 1)
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    pending: runtime.pending().is_some(),
                    blocked: runtime.blocked(),
                }
            })
            .collect();
        let current_desktop = self
            .model
            .current()
            .and_then(|id| self.diagnostic_desktop_labels.get(id));
        let context = serde_json::json!({
            "schema_version":1,
            "diagnostic_run":self.storage.diagnostic_session,
            "generated_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
            "application":crate::version::info(),
            "integration":{"status":self.status,"ready":self.ready},
            "settings":{"paused":saved.paused,"auto_restore":saved.auto_restore,"minimize_snapped_elsewhere":saved.minimize_snapped_elsewhere,
                    "desktop_tint":saved.desktop_tint,"show_desktop_name":saved.show_desktop_name,"startup":saved.startup},
            "desktops":desktops,
            "model":{"generation":self.model.generation(),"transition_serial":self.model.transition_serial(),"current_desktop":current_desktop},
            "dispatch_latency":self.dispatch_latency,
        });
        Some(ReportDraft {
            context,
            windows,
            previous_notice,
        })
    }

    fn save_report(&mut self, destination: std::path::PathBuf, draft: ReportDraft) {
        self.notice = "Collecting a diagnostic report…".into();
        self.update_settings();
        if self
            .storage
            .sender
            .try_send(store::Work::Report(store::ReportRequest {
                destination,
                context: draft.context,
                windows: draft.windows,
            }))
            .is_err()
        {
            self.report_busy = false;
            self.notice = "Diagnostic report queue is busy. Try again shortly.".into();
            self.update_settings();
        }
    }

    fn cancel_report(&mut self, draft: ReportDraft, error: Option<String>) {
        self.report_busy = false;
        if let Some(error) = error {
            self.notice = format!("The save dialog could not be opened: {error}");
        } else if self.notice == REPORT_PICKER_NOTICE {
            self.notice = self.snap_notice.previous(draft.previous_notice);
        }
        self.update_settings();
    }

    fn diagnostic_window_label(&self, key: &str) -> String {
        self.diagnostic_window_labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| "window-untracked".into())
    }

    fn stop(&mut self) {
        self.rename.take();
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.shell_switcher = None;
        self.finish_reveal();
        self.update_window_frame();
        if let Some(integration) = &self.integration {
            let _ = integration.try_send(bridge::CommandMessage::Stop);
        }
        let _ = self.actions.try_send(actions::Work::Stop);
        let (ack, result) = crossbeam_channel::bounded(1);
        let _ = self
            .storage
            .stop
            .try_send((self.loaded.then(|| self.model.saved().clone()), ack));
        self.shutdown_ack = Some(result);
        for hook in self.hooks.drain(..) {
            unsafe {
                let _ = windows::Win32::UI::Accessibility::UnhookWinEvent(
                    windows::Win32::UI::Accessibility::HWINEVENTHOOK(hook as *mut _),
                );
            }
        }
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetDlgItem(window: Handle, id: i32) -> Handle;
}

fn report_output_path_buffer(suggested_name: &str) -> Vec<u16> {
    let suggested_name = wide(suggested_name);
    let mut buffer = vec![0; REPORT_OUTPUT_PATH_CAPACITY];
    let count = suggested_name.len().min(buffer.len());
    buffer[..count].copy_from_slice(&suggested_name[..count]);
    buffer
}

fn save_report_dialog(owner: Handle) -> Result<Option<std::path::PathBuf>, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPEN_FILENAME_FLAGS, OPENFILENAMEW,
    };
    use windows::core::{PCWSTR, PWSTR};

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut filename =
        report_output_path_buffer(&format!("VirtualDesktopRail-diagnostic-{timestamp}.zip"));
    let filter: Vec<u16> = "ZIP archive (*.zip)\0*.zip\0All files (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title = wide("Save diagnostic report");
    let extension = wide("zip");
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(owner),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(filename.as_mut_ptr()),
        nMaxFile: filename.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        Flags: OPEN_FILENAME_FLAGS(
            OFN_EXPLORER.0 | OFN_NOCHANGEDIR.0 | OFN_OVERWRITEPROMPT.0 | OFN_PATHMUSTEXIST.0,
        ),
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
        let error = unsafe { CommDlgExtendedError() };
        return if error.0 == 0 {
            Ok(None)
        } else {
            Err(format!("Windows common-dialog error 0x{:04x}", error.0))
        };
    }
    let Some(end) = filename.iter().position(|value| *value == 0) else {
        return Err("The selected path was not terminated.".into());
    };
    Ok(Some(std::path::PathBuf::from(String::from_utf16_lossy(
        &filename[..end],
    ))))
}

unsafe fn fill(dc: Handle, rect: &Rect, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, rect, brush);
        DeleteObject(brush);
    }
}

pub(crate) fn high_contrast() -> bool {
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some(&mut contrast as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
        && contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
}

fn palette() -> (u32, u32, u32, u32) {
    use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor};
    if high_contrast() {
        // A shape also distinguishes the active desktop.
        // Use the user's foreground/background pair for guaranteed contrast.
        let background = unsafe { GetSysColor(COLOR_WINDOW) };
        let text = unsafe { GetSysColor(COLOR_WINDOWTEXT) };
        return (background, text, text, text);
    }
    if dark() {
        (0x00252120, 0x00efecec, 0x00f6b564, 0x00006add)
    } else {
        (0x00f9f7f7, 0x00252120, 0x00b06800, 0x00006add)
    }
}

// Blend the same COLORREF used by the active dot into every rail surface.
// Keep system high-contrast colors and the neutral fallback when no desktop is known.
fn rail_background(app: &App, base: u32, high_contrast: bool, dark: bool) -> u32 {
    if !app.model.saved().desktop_tint || high_contrast {
        return base;
    }
    let Some(index) = app
        .model
        .desktops()
        .iter()
        .position(|desktop| app.model.current() == Some(desktop.id.as_str()))
    else {
        return base;
    };
    let tint = desktop_color(index, dark);
    let weight = if dark { 0.18 } else { 0.14 };
    let mut blended = 0;
    for shift in [0, 8, 16] {
        let a = ((base >> shift) & 255) as f64;
        let b = ((tint >> shift) & 255) as f64;
        blended |= ((a + (b - a) * weight).round() as u32) << shift;
    }
    blended
}

// Alternate hue families so adjacent desktops are easy to distinguish.
fn desktop_color(index: usize, dark: bool) -> u32 {
    const LIGHT: [u32; 8] = [
        0xC56B25, 0x087ED0, 0xBD2D94, 0x738500, 0x6250CD, 0x00949E, 0x896746, 0xA957BC,
    ];
    const DARK: [u32; 8] = [
        0xF5B366, 0x56ADF5, 0xF08CDC, 0xB4CE50, 0xA09AFF, 0x63DCE5, 0xCEB190, 0xDB94ED,
    ];
    if index < LIGHT.len() {
        return if dark { DARK[index] } else { LIGHT[index] };
    }
    let hue = ((index - LIGHT.len()) as f64 * 137.508 + 17.0) % 360.0;
    let chroma = if dark { 0.50 } else { 0.65 };
    let base = if dark { 0.40 } else { 0.12 };
    let x = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as usize {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let channel = |v: f64| ((v + base) * 255.0).round() as u32;
    channel(r) | channel(g) << 8 | channel(b) << 16
}

// Analytic pixel coverage avoids jagged circles and scales with the rail DPI.
fn smooth_shape(
    dc: Handle,
    width: i32,
    height: i32,
    dpi: u32,
    color: u32,
    distance: impl Fn(f64, f64) -> f64,
) {
    use windows::Win32::{
        Foundation::COLORREF,
        Graphics::Gdi::{GetPixel, HDC, SetPixelV},
    };
    let scale = dpi.max(96) as f64 / 96.0;
    for y in 0..height {
        for x in 0..width {
            let d = distance(
                (x as f64 + 0.5 - width as f64 / 2.0) / scale,
                (y as f64 + 0.5 - height as f64 / 2.0) / scale,
            );
            let coverage = (0.5 - d * scale).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }
            let base = unsafe { GetPixel(HDC(dc), x, y).0 };
            let mut blended = 0;
            for shift in [0, 8, 16] {
                let a = ((base >> shift) & 255) as f64;
                let b = ((color >> shift) & 255) as f64;
                blended |= ((a + (b - a) * coverage).round() as u32) << shift;
            }
            unsafe {
                let _ = SetPixelV(HDC(dc), x, y, COLORREF(blended));
            }
        }
    }
}

fn rasterize_desktop_name(name: &str, dpi: u32, rail_width: i32) -> (i32, Vec<u8>) {
    use windows::Win32::{
        Foundation::{COLORREF, HANDLE, SIZE},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetTextExtentPoint32W,
            GetTextMetricsW, HDC, HGDIOBJ, SelectObject, SetBkMode, SetTextCharacterExtra,
            SetTextColor, TEXTMETRICW, TRANSPARENT, TextOutW,
        },
    };

    if name.is_empty() || rail_width <= 0 {
        return (0, Vec::new());
    }

    const SS: i32 = 4;
    let font_height = ((11 * dpi as i32 * SS) + 48) / 96;
    let tracking = ((dpi as i32 * SS) + 144) / 288;
    let text_utf16 = wide(name);
    let len = text_utf16.len().saturating_sub(1);
    if len == 0 {
        return (0, Vec::new());
    }

    unsafe {
        let mem_dc = CreateCompatibleDC(HDC::default());
        if mem_dc.is_invalid() {
            return (0, Vec::new());
        }
        let font = CreateFontW(
            -font_height,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            4,
            0,
            wide("Segoe UI").as_ptr(),
        );
        if font.is_null() {
            let _ = DeleteDC(mem_dc);
            return (0, Vec::new());
        }
        let font_obj = HGDIOBJ(font as *mut _);
        let old_font = SelectObject(mem_dc, font_obj);
        let _ = SetTextCharacterExtra(mem_dc, tracking);

        let mut size = SIZE::default();
        if !GetTextExtentPoint32W(mem_dc, &text_utf16[..len], &mut size).as_bool()
            || size.cx <= 0
            || size.cy <= 0
        {
            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font_obj);
            let _ = DeleteDC(mem_dc);
            return (0, Vec::new());
        }

        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(mem_dc, &mut tm);

        let pad = SS * 2;
        let w4 = size.cx + pad * 2;
        let h4 = size.cy + pad * 2;

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w4,
                biHeight: -h4,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(
            mem_dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        );
        let Ok(dib) = dib else {
            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font_obj);
            let _ = DeleteDC(mem_dc);
            return (0, Vec::new());
        };
        if bits.is_null() {
            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(dib);
            let _ = DeleteObject(font_obj);
            let _ = DeleteDC(mem_dc);
            return (0, Vec::new());
        }

        std::ptr::write_bytes(bits as *mut u8, 0, (w4 * h4 * 4) as usize);
        let old_dib = SelectObject(mem_dc, dib);
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(mem_dc, COLORREF(0x00FFFFFF));
        let _ = TextOutW(mem_dc, pad, pad, &text_utf16[..len]);
        // Synchronize batched drawing before accessing the CreateDIBSection bits.
        let _ = GdiFlush();

        let raw = std::slice::from_raw_parts(bits as *const u32, (w4 * h4) as usize);
        let baseline_y4 = pad + tm.tmAscent;
        let mut min_x = w4;
        let mut max_x = -1;
        let mut min_y = h4;
        let mut max_y = -1;
        let mut sum_y = 0.0;
        let mut total_w = 0.0;

        for y in 0..h4 {
            let desc_weight = if y > baseline_y4 { 0.25 } else { 1.0 };
            for x in 0..w4 {
                let v = (raw[(y * w4 + x) as usize] & 0xFF) as f64;
                if v > 0.0 {
                    if x < min_x {
                        min_x = x;
                    }
                    if x > max_x {
                        max_x = x;
                    }
                    if y < min_y {
                        min_y = y;
                    }
                    if y > max_y {
                        max_y = y;
                    }
                    let w = v * desc_weight;
                    sum_y += (y as f64 + 0.5) * w;
                    total_w += w;
                }
            }
        }

        if max_x < min_x || max_y < min_y {
            SelectObject(mem_dc, old_dib);
            let _ = DeleteObject(dib);
            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font_obj);
            let _ = DeleteDC(mem_dc);
            return (0, Vec::new());
        }

        let optical_y = if total_w > 0.0 {
            sum_y / total_w
        } else {
            0.5 * (min_y + max_y + 1) as f64
        };
        let shift_y4 = (rail_width as f64 * SS as f64 * 0.5 - optical_y).round() as i32;

        let span_x4 = max_x - min_x + 1;
        let out_h = (span_x4 + SS - 1) / SS;
        let mut mask = vec![0u8; (rail_width * out_h) as usize];

        for vy in 0..out_h {
            for vx in 0..rail_width {
                let mut acc = 0u32;
                for dy in 0..SS {
                    let ux = max_x - (vy * SS + dy);
                    if ux < min_x || ux > max_x {
                        continue;
                    }
                    for dx in 0..SS {
                        let uy = vx * SS + dx - shift_y4;
                        if uy >= 0 && uy < h4 {
                            acc += (raw[(uy * w4 + ux) as usize] & 0xFF) as u32;
                        }
                    }
                }
                mask[(vy * rail_width + vx) as usize] = ((acc + 8) / 16) as u8;
            }
        }

        SelectObject(mem_dc, old_dib);
        let _ = DeleteObject(dib);
        SelectObject(mem_dc, old_font);
        let _ = DeleteObject(font_obj);
        let _ = DeleteDC(mem_dc);

        (out_h, mask)
    }
}

fn paint_desktop_name(
    app: &mut App,
    dc: Handle,
    width: i32,
    footer_top: i32,
    colors: (u32, u32),
    high_contrast: bool,
) {
    let Some(layout) = app.desktop_name_layout(width, footer_top) else {
        return;
    };
    let (background, foreground) = colors;
    let raster = &app.desktop_name_raster;
    let hovered = app.desktop_name_hovered;
    let pressed = app.desktop_name_pressed;
    let is_dark = crate::theme::dark();

    let dpi = app.font_dpi.max(96);
    let scale = dpi as f64 / 96.0;

    use windows::Win32::{
        Foundation::COLORREF,
        Graphics::Gdi::{GetPixel, HDC, SetPixelV},
    };

    if (hovered || pressed) && !high_contrast {
        let surface = if is_dark {
            if pressed { 0x00484342 } else { 0x003a3534 }
        } else if pressed {
            0x00e0dcdc
        } else {
            0x00ece8e7
        };
        let surface = rail_background(app, surface, high_contrast, is_dark);
        let pill_h = layout.rect.bottom - layout.rect.top;
        let half_w = (raster.rail_width as f64 / scale) * 0.5 - 2.0;
        let half_h = (pill_h as f64 / scale) * 0.5 - 2.0;
        let center_x = raster.rail_width as f64 / 2.0;
        let center_y = (layout.rect.top + layout.rect.bottom) as f64 / 2.0;

        for vy in 0..pill_h {
            let py = layout.rect.top + vy;
            let dy = (py as f64 + 0.5 - center_y) / scale;
            for vx in 0..raster.rail_width {
                let dx = (vx as f64 + 0.5 - center_x) / scale;
                let d = rounded_rect_sdf(dx, dy, half_w.max(2.0), half_h.max(2.0), 4.0);
                let coverage = (0.5 - d * scale).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let base = unsafe { GetPixel(HDC(dc), vx, py).0 };
                let mut blended = 0u32;
                for shift in [0, 8, 16] {
                    let a = ((base >> shift) & 255) as f64;
                    let b = ((surface >> shift) & 255) as f64;
                    blended |= ((a + (b - a) * coverage).round() as u32) << shift;
                }
                unsafe {
                    let _ = SetPixelV(HDC(dc), vx, py, COLORREF(blended));
                }
            }
        }
    }

    let color = if high_contrast || hovered || pressed {
        foreground
    } else {
        let mut muted = 0;
        for shift in [0, 8, 16] {
            let base = (background >> shift) & 255;
            let text = (foreground >> shift) & 255;
            muted |= ((base * 40 + text * 60) / 100) << shift;
        }
        muted
    };

    for vy in 0..raster.height {
        let py = layout.text_top + vy;
        for vx in 0..raster.rail_width {
            let alpha = raster.mask[(vy * raster.rail_width + vx) as usize];
            if alpha == 0 {
                continue;
            }
            let cov = (alpha as f64 / 255.0).powf(0.80);
            let base = unsafe { GetPixel(HDC(dc), vx, py).0 };
            let mut blended = 0u32;
            for shift in [0, 8, 16] {
                let a = ((base >> shift) & 255) as f64;
                let b = ((color >> shift) & 255) as f64;
                blended |= ((a + (b - a) * cov).round() as u32) << shift;
            }
            unsafe {
                let _ = SetPixelV(HDC(dc), vx, py, COLORREF(blended));
            }
        }
    }
}

fn rounded_rect_sdf(x: f64, y: f64, half_w: f64, half_h: f64, radius: f64) -> f64 {
    let qx = x.abs() - (half_w - radius);
    let qy = y.abs() - (half_h - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn segment_sdf(x: f64, y: f64, ax: f64, ay: f64, bx: f64, by: f64, radius: f64) -> f64 {
    let pax = x - ax;
    let pay = y - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    (pax - bax * h).hypot(pay - bay * h) - radius
}

fn draw(app: &App, item: &DrawItem) {
    let width = item.rect.right - item.rect.left;
    let height = item.rect.bottom - item.rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(app.window) }.max(96);
    let scale = dpi as f64 / 96.0;
    let is_dark = dark();
    let (background, text, accent, unavailable) = palette();
    let high_contrast = accent == text;
    let background = rail_background(app, background, high_contrast, is_dark);
    let muted = if high_contrast {
        text
    } else if is_dark {
        0x00bab1b0
    } else {
        0x006e6462
    };
    let hovered = HOVERED_BUTTON.load(Ordering::Acquire) == item.window as usize;
    let pressed = item.state & 0x01 != 0;
    let disabled = item.state & 0x04 != 0;
    unsafe {
        let dc = CreateCompatibleDC(item.dc);
        let bitmap = CreateCompatibleBitmap(item.dc, width, height);
        let previous = SelectObject(dc, bitmap);
        let bounds = Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        fill(dc, &bounds, background);
        if !disabled && (hovered || pressed) && !high_contrast {
            let surface = if is_dark {
                if pressed { 0x00484342 } else { 0x003a3534 }
            } else if pressed {
                0x00e0dcdc
            } else {
                0x00ece8e7
            };
            let surface = rail_background(app, surface, high_contrast, is_dark);
            let half_w = (width as f64 / scale) * 0.5 - 2.0;
            let half_h = (height as f64 / scale) * 0.5 - 2.5;
            smooth_shape(dc, width, height, dpi, surface, |x, y| {
                rounded_rect_sdf(x, y, half_w.max(2.0), half_h.max(2.0), 4.0)
            });
        }
        if let Some(button) = app.buttons.iter().find(|b| b.hwnd == item.window) {
            match &button.kind {
                ButtonKind::Desktop(id) => {
                    let active = app.model.current() == Some(id.as_str());
                    let index = app
                        .model
                        .desktops()
                        .iter()
                        .position(|d| &d.id == id)
                        .unwrap_or(0);
                    let color = if high_contrast {
                        text
                    } else {
                        desktop_color(index, is_dark)
                    };
                    smooth_shape(dc, width, height, dpi, color, |x, y| {
                        let distance = (x * x + y * y).sqrt();
                        if active {
                            distance - 4.75
                        } else {
                            (distance - 2.75).abs() - 0.75
                        }
                    });
                }
                ButtonKind::Window(key) => {
                    let icon = app
                        .icon_cache
                        .get(&(key.clone(), dpi))
                        .copied()
                        .filter(|icon| *icon != 0)
                        .map(|icon| icon as Handle)
                        .unwrap_or_else(|| LoadIconW(null_mut(), 32512usize as *const u16));
                    let size = icons::pixel_size(dpi);
                    let minimized = app.model.runtime(key).is_some_and(|r| r.window().minimized);
                    // Preserve the application's normal icon in every state.
                    // Only the underline changes: outlined minimized, solid visible.
                    DrawIconEx(
                        dc,
                        ((width - size) as f64 / 2.0).round() as i32,
                        ((height - size) as f64 / 2.0 - 2.5 * scale).round() as i32,
                        icon,
                        size,
                        size,
                        0,
                        null_mut(),
                        3,
                    );
                    let indicator_y = height as f64 / (2.0 * scale) - 5.5;
                    let indicator_color = if minimized { muted } else { accent };
                    smooth_shape(dc, width, height, dpi, indicator_color, |x, y| {
                        let dy = y - indicator_y;
                        if minimized {
                            rounded_rect_sdf(x, dy, 4.0, 1.25, 1.25).abs() - 0.55
                        } else {
                            rounded_rect_sdf(x, dy, 4.75, 1.4, 1.4)
                        }
                    });
                }
                ButtonKind::Pause
                | ButtonKind::LightSwitch
                | ButtonKind::Settings
                | ButtonKind::Exit => {
                    let color = if matches!(button.kind, ButtonKind::Pause) && !app.ready {
                        unavailable
                    } else if hovered || pressed {
                        text
                    } else {
                        muted
                    };
                    smooth_shape(dc, width, height, dpi, color, |x, y| {
                        if matches!(button.kind, ButtonKind::Pause) && !app.ready {
                            // A full warning symbol stays legible in the narrow rail and
                            // distinguishes unavailable integration without relying on color.
                            let triangle = (x.abs() * 0.866 - y * 0.5 - 3.1).max(y - 5.2);
                            let outline = triangle.abs() - 0.65;
                            let stem = segment_sdf(x, y, 0.0, -2.2, 0.0, 1.2, 0.65);
                            let dot = (x * x + (y - 3.4).powi(2)).sqrt() - 0.75;
                            outline.min(stem).min(dot)
                        } else if matches!(button.kind, ButtonKind::LightSwitch) {
                            // Show the destination mode: sun for light, crescent for dark.
                            if is_dark {
                                let disk = x.hypot(y) - 2.6;
                                let mut rays = f64::INFINITY;
                                for ray in 0..8 {
                                    let angle = f64::from(ray) * std::f64::consts::FRAC_PI_4;
                                    let (sin, cos) = angle.sin_cos();
                                    rays = rays.min(segment_sdf(
                                        x,
                                        y,
                                        cos * 4.2,
                                        sin * 4.2,
                                        cos * 5.8,
                                        sin * 5.8,
                                        0.6,
                                    ));
                                }
                                disk.min(rays)
                            } else {
                                let disk = x.hypot(y) - 5.1;
                                let cutout = (x - 2.6).hypot(y + 2.1) - 4.8;
                                disk.max(-cutout)
                            }
                        } else if matches!(button.kind, ButtonKind::Settings) {
                            let upper_left = segment_sdf(x, y, -5.2, -3.0, -4.4, -3.0, 0.65);
                            let upper_right = segment_sdf(x, y, -0.2, -3.0, 5.2, -3.0, 0.65);
                            let lower_left = segment_sdf(x, y, -5.2, 3.0, 0.2, 3.0, 0.65);
                            let lower_right = segment_sdf(x, y, 4.4, 3.0, 5.2, 3.0, 0.65);
                            let ring = |xx: f64, yy: f64| {
                                (((x - xx).powi(2) + (y - yy).powi(2)).sqrt() - 1.9).abs() - 0.65
                            };
                            upper_left
                                .min(upper_right)
                                .min(lower_left)
                                .min(lower_right)
                                .min(ring(-2.3, -3.0))
                                .min(ring(2.3, 3.0))
                        } else if matches!(button.kind, ButtonKind::Exit) {
                            let angle = x.atan2(-y).abs();
                            let radius = 4.6;
                            let gap = 0.62;
                            let arc = if angle < gap {
                                let gx = radius * gap.sin();
                                let gy = -radius * gap.cos();
                                ((x.abs() - gx).hypot(y - gy)) - 0.68
                            } else {
                                ((x * x + y * y).sqrt() - radius).abs() - 0.68
                            };
                            let stem = segment_sdf(x, y, 0.0, -5.4, 0.0, -0.6, 0.68);
                            arc.min(stem)
                        } else if app.model.saved().paused {
                            let s1 = segment_sdf(x, y, -2.5, -4.2, -2.5, 4.2, 0.6);
                            let s2 = segment_sdf(x, y, -2.5, -4.2, 3.6, 0.0, 0.6);
                            let s3 = segment_sdf(x, y, 3.6, 0.0, -2.5, 4.2, 0.6);
                            let inside = (-x - 2.5).max(y.abs() * 1.45 + x - 3.6);
                            s1.min(s2).min(s3).min(inside)
                        } else {
                            let left = segment_sdf(x, y, -2.3, -4.2, -2.3, 4.2, 0.72);
                            let right = segment_sdf(x, y, 2.3, -4.2, 2.3, 4.2, 0.72);
                            left.min(right)
                        }
                    });
                }
            }
        }
        if item.state & 0x10 != 0 && KEYBOARD_FOCUS.load(Ordering::Acquire) {
            DrawFocusRect(
                dc,
                &Rect {
                    left: 2,
                    top: 2,
                    right: width - 2,
                    bottom: height - 2,
                },
            );
        }
        BitBlt(
            item.dc,
            item.rect.left,
            item.rect.top,
            width,
            height,
            dc,
            0,
            0,
            0x00cc0020,
        );
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
    }
}

unsafe extern "system" fn procedure(
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        handle_message(window, message, wparam, lparam)
    }))
    .unwrap_or_else(|_| {
        unsafe {
            PostQuitMessage(1);
        }
        0
    })
}

unsafe fn handle_message(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize {
    let main = MAIN_WINDOW.load(Ordering::Acquire) == window as usize;
    let pointer = unsafe { GetWindowLongPtrW(window, -21) };
    if main {
        if pointer != 0
            && let Ok(app) = unsafe { &*(pointer as *const RefCell<App>) }.try_borrow()
            && message == app.taskbar_message
        {
            drop(app);
            notify(Event::Status(
                "Windows Explorer restarted; establishing a fresh baseline.".into(),
                false,
            ));
            if let Ok(app) = unsafe { &*(pointer as *const RefCell<App>) }.try_borrow()
                && let Some(integration) = &app.integration
            {
                let _ = integration.try_send(bridge::CommandMessage::Retry {
                    experimental: app.model.saved().experimental_integration,
                });
            }
        }
        if let Some(result) = crate::appbar::message(window, message, wparam, lparam) {
            if message == 0x7e || message == 0x2e0 {
                notify(Event::NativeWindow(
                    message,
                    window as usize,
                    platform::tick(),
                ));
            }
            return result;
        }
    }
    if pointer == 0 {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let cell = unsafe { &*(pointer as *const RefCell<App>) };
    if !main && message == 0x0111 && (wparam & 0xffff) == SAVE_REPORT && lparam != 0 {
        let control = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(windows::Win32::Foundation::HWND(
                lparam as *mut _,
            ))
        };
        if control == SAVE_REPORT as i32 {
            // Snapshot under the borrow, then drop it before the common dialog's
            // nested message loop so WAKE and native events continue to drain.
            let draft = {
                let Ok(mut app) = cell.try_borrow_mut() else {
                    return 0;
                };
                app.begin_report()
            };
            if let Some(draft) = draft {
                match save_report_dialog(window) {
                    Ok(Some(destination)) => {
                        if let Ok(mut app) = cell.try_borrow_mut() {
                            app.save_report(destination, draft);
                        }
                    }
                    Ok(None) => {
                        if let Ok(mut app) = cell.try_borrow_mut() {
                            app.cancel_report(draft, None);
                        }
                    }
                    Err(error) => {
                        if let Ok(mut app) = cell.try_borrow_mut() {
                            app.cancel_report(draft, Some(error));
                        }
                    }
                }
            }
            return 0;
        }
    }
    if main && message == 0x4a && lparam != 0 {
        let data = unsafe { &*(lparam as *const CopyData) };
        if data.tag == 0x5241494c
            && data.size > 0
            && data.size <= 65536
            && !data.data.is_null()
            && let Ok(command) = serde_json::from_slice::<Command>(unsafe {
                std::slice::from_raw_parts(data.data, data.size as usize)
            })
        {
            notify(Event::Command(command));
            return 1;
        }
        return 0;
    }
    if main && message == 2 {
        if let Ok(mut app) = cell.try_borrow_mut() {
            app.stop();
        }
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    // Color/erase messages can reenter while ShowWindow/SetWindowPos/EnableWindow hold the model borrow.
    if main {
        match message {
            0x0014 => return 1,
            0x0135 => unsafe {
                return windows::Win32::Graphics::Gdi::GetStockObject(
                    windows::Win32::Graphics::Gdi::NULL_BRUSH,
                )
                .0 as isize;
            },
            _ => {}
        }
    } else {
        match message {
            0x0135 | 0x0138 => unsafe {
                let control = windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(
                    windows::Win32::Foundation::HWND(lparam as *mut _),
                );
                let color = if control == 6107 {
                    windows::Win32::Foundation::COLORREF(
                        if GetWindowLongPtrW(lparam as Handle, -21) != 0 {
                            0x00438320
                        } else {
                            0x000078ad
                        },
                    )
                } else {
                    windows::Win32::Foundation::COLORREF(
                        windows::Win32::Graphics::Gdi::GetSysColor(if control == 6108 {
                            windows::Win32::Graphics::Gdi::COLOR_GRAYTEXT
                        } else {
                            windows::Win32::Graphics::Gdi::COLOR_WINDOWTEXT
                        }),
                    )
                };
                windows::Win32::Graphics::Gdi::SetTextColor(
                    windows::Win32::Graphics::Gdi::HDC(wparam as *mut _),
                    color,
                );
                windows::Win32::Graphics::Gdi::SetBkMode(
                    windows::Win32::Graphics::Gdi::HDC(wparam as *mut _),
                    windows::Win32::Graphics::Gdi::TRANSPARENT,
                );
                return windows::Win32::Graphics::Gdi::GetSysColorBrush(
                    if windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(
                        windows::Win32::Foundation::HWND(lparam as *mut _),
                    ) == 6100
                        || windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(
                            windows::Win32::Foundation::HWND(lparam as *mut _),
                        ) == 6107
                        || windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(
                            windows::Win32::Foundation::HWND(lparam as *mut _),
                        ) == 6108
                    {
                        windows::Win32::Graphics::Gdi::COLOR_BTNFACE
                    } else {
                        windows::Win32::Graphics::Gdi::COLOR_WINDOW
                    },
                )
                .0 as isize;
            },
            0x0014 => unsafe {
                let mut rect = Rect::default();
                GetClientRect(window, &mut rect);
                fill(
                    wparam as Handle,
                    &rect,
                    windows::Win32::Graphics::Gdi::GetSysColor(
                        windows::Win32::Graphics::Gdi::COLOR_WINDOW,
                    ),
                );
                let dpi = GetDpiForWindow(window).max(96) as i32;
                rect.top = (SETTINGS_FOOTER_Y * dpi + 48) / 96;
                fill(
                    wparam as Handle,
                    &rect,
                    windows::Win32::Graphics::Gdi::GetSysColor(
                        windows::Win32::Graphics::Gdi::COLOR_BTNFACE,
                    ),
                );
                return 1;
            },
            _ => {}
        }
    }
    let Ok(mut app) = cell.try_borrow_mut() else {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    match message {
        rename::OPEN if main => {
            app.open_rename(wparam as Handle);
            0
        }
        rename::SAVE if main => {
            app.save_rename(wparam as Handle);
            0
        }
        0x0020 if main => {
            if (lparam & 0xffff) == 1 {
                let mut pt = windows::Win32::Foundation::POINT::default();
                if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt) }.is_ok()
                    && unsafe {
                        windows::Win32::Graphics::Gdi::ScreenToClient(
                            windows::Win32::Foundation::HWND(window),
                            &mut pt,
                        )
                    }
                    .as_bool()
                    && app
                        .current_desktop_name_layout()
                        .is_some_and(|layout| layout.rect.contains(pt.x, pt.y))
                {
                    unsafe {
                        windows::Win32::UI::WindowsAndMessaging::SetCursor(
                            windows::Win32::UI::WindowsAndMessaging::HCURSOR(LoadCursorW(
                                null_mut(),
                                32649usize as *const u16,
                            )
                                as *mut _),
                        );
                    }
                    return 1;
                }
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        0x0200 if main => {
            let x = (lparam & 0xffff) as i16 as i32;
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let is_over = app
                .current_desktop_name_layout()
                .is_some_and(|layout| layout.rect.contains(x, y));
            let mut invalidate = false;
            if is_over != app.desktop_name_hovered {
                app.desktop_name_hovered = is_over;
                if is_over {
                    let mut track = TrackMouseEventData {
                        size: size_of::<TrackMouseEventData>() as u32,
                        flags: 0x0000_0002, // TME_LEAVE
                        track: window,
                        hover_time: 0,
                    };
                    unsafe {
                        TrackMouseEvent(&mut track);
                    }
                }
                invalidate = true;
            }
            let has_capture = unsafe { GetCapture() } == window;
            if has_capture {
                if app.desktop_name_pressed != is_over {
                    app.desktop_name_pressed = is_over;
                    invalidate = true;
                }
            } else if app.desktop_name_pressed {
                app.desktop_name_pressed = false;
                invalidate = true;
            }
            if invalidate {
                unsafe {
                    InvalidateRect(window, null(), 0);
                }
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        0x02a3 if main => {
            let changed = app.desktop_name_hovered || app.desktop_name_pressed;
            if changed {
                app.desktop_name_hovered = false;
                app.desktop_name_pressed = false;
                unsafe {
                    InvalidateRect(window, null(), 0);
                }
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        0x0201 if main => {
            let x = (lparam & 0xffff) as i16 as i32;
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let is_over = app
                .current_desktop_name_layout()
                .is_some_and(|layout| layout.rect.contains(x, y));
            if is_over {
                app.desktop_name_pressed = true;
                unsafe {
                    SetCapture(window);
                    InvalidateRect(window, null(), 0);
                }
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        0x0202 if main => {
            let x = (lparam & 0xffff) as i16 as i32;
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let had_capture = unsafe { GetCapture() } == window;
            if had_capture {
                unsafe {
                    ReleaseCapture();
                }
            }
            let was_pressed = std::mem::replace(&mut app.desktop_name_pressed, false);
            let is_over = app
                .current_desktop_name_layout()
                .is_some_and(|layout| layout.rect.contains(x, y));
            if was_pressed {
                unsafe {
                    InvalidateRect(window, null(), 0);
                }
            }
            if (had_capture || was_pressed) && is_over {
                app.open_rename_desktop_name();
                return 0;
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        0x001f | 0x0215 if main => {
            if unsafe { GetCapture() } == window {
                unsafe {
                    ReleaseCapture();
                }
            }
            let changed = app.desktop_name_hovered || app.desktop_name_pressed;
            if changed {
                app.desktop_name_hovered = false;
                app.desktop_name_pressed = false;
                unsafe {
                    InvalidateRect(window, null(), 0);
                }
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        0x007b if main => {
            let pt_x = (lparam & 0xffff) as i16 as i32;
            let pt_y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let hit = if lparam == -1 {
                true
            } else {
                let mut pt = windows::Win32::Foundation::POINT { x: pt_x, y: pt_y };
                if unsafe {
                    windows::Win32::Graphics::Gdi::ScreenToClient(
                        windows::Win32::Foundation::HWND(window),
                        &mut pt,
                    )
                }
                .as_bool()
                {
                    app.current_desktop_name_layout()
                        .is_some_and(|layout| layout.rect.contains(pt.x, pt.y))
                } else {
                    false
                }
            };
            if hit {
                app.open_rename_desktop_name();
                return 0;
            }
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WAKE if main => {
            app.drain();
            0
        }
        0x0021 if main => {
            let foreground = unsafe { GetForegroundWindow() } as usize;
            if foreground != app.window as usize {
                app.pre_foreground = Some(foreground);
            }
            3 // MA_NOACTIVATE: the passive rail does not consume application focus.
        }
        0x0006 if main => {
            // Keyboard/assistive activation can precede mouse delivery. Preserve
            // the application active before the rail, rather than toggling based
            // on the rail becoming foreground during input routing.
            if unsafe { GetCapture() } == window {
                unsafe {
                    ReleaseCapture();
                }
            }
            if app.desktop_name_hovered || app.desktop_name_pressed {
                app.desktop_name_hovered = false;
                app.desktop_name_pressed = false;
                unsafe {
                    InvalidateRect(window, null(), 0);
                }
            }
            if wparam & 0xffff != 0 && lparam != 0 {
                app.pre_foreground = Some(lparam as usize);
            } else if wparam & 0xffff == 0 && KEYBOARD_FOCUS.swap(false, Ordering::AcqRel) {
                for button in &app.buttons {
                    unsafe {
                        InvalidateRect(button.hwnd, null(), 0);
                    }
                }
            }
            0
        }
        0x0005 if main => {
            app.finish_reveal();
            app.render(true);
            0
        }
        0x0015 | 0x001a | 0x031a => {
            app.finish_reveal();
            app.signature.clear();
            app.render(true);
            0
        }
        0x0113 if main && wparam == REVEAL_TIMER => {
            app.step_reveal();
            0
        }
        0x000f if main => {
            let mut paint: Paint = unsafe { zeroed() };
            unsafe {
                let screen_dc = BeginPaint(window, &mut paint);
                let mut client = Rect::default();
                GetClientRect(window, &mut client);
                let width = client.right.max(1);
                let height = client.bottom.max(1);
                let dc = CreateCompatibleDC(screen_dc);
                let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
                let previous = SelectObject(dc, bitmap);
                let (base, text, accent, _) = palette();
                let bg = rail_background(&app, base, text == accent, dark());
                fill(dc, &client, bg);
                let _ = windows::Win32::Graphics::Gdi::SetViewportOrgEx(
                    windows::Win32::Graphics::Gdi::HDC(dc),
                    app.reveal_offset,
                    0,
                    None,
                );
                let dpi = GetDpiForWindow(window).max(96) as i32;
                let scale = |dip: i32| (dip * dpi + 48) / 96;

                let size = scale(14);
                let icon_x = ((client.right - 1 - size) as f64 / 2.0).round() as i32;
                let icon_y = scale(10);
                DrawIconEx(
                    dc,
                    icon_x,
                    icon_y,
                    app_icon(size),
                    size,
                    size,
                    0,
                    null_mut(),
                    3,
                );
                for y in icon_y..icon_y + size {
                    for x in icon_x..icon_x + size {
                        let px = windows::Win32::Graphics::Gdi::GetPixel(
                            windows::Win32::Graphics::Gdi::HDC(dc),
                            x,
                            y,
                        )
                        .0;
                        if px != bg {
                            let mut blended = 0;
                            for shift in [0, 8, 16] {
                                let a = ((bg >> shift) & 255) as f64;
                                let b = ((px >> shift) & 255) as f64;
                                blended |= ((a + (b - a) * 0.85).round() as u32) << shift;
                            }
                            let _ = windows::Win32::Graphics::Gdi::SetPixelV(
                                windows::Win32::Graphics::Gdi::HDC(dc),
                                x,
                                y,
                                windows::Win32::Foundation::COLORREF(blended),
                            );
                        }
                    }
                }
                let border = if dark() { 0x00484140 } else { 0x00e0dad8 };
                fill(
                    dc,
                    &Rect {
                        left: client.right - 1,
                        top: 0,
                        right: client.right,
                        bottom: client.bottom,
                    },
                    border,
                );
                let header_sep_y = scale(33);
                fill(
                    dc,
                    &Rect {
                        left: scale(5),
                        top: header_sep_y,
                        right: client.right - scale(5),
                        bottom: header_sep_y + scale(1).max(1),
                    },
                    border,
                );
                let y = scale(42 + 33 * app.model.desktops().len() as i32 + 3);
                fill(
                    dc,
                    &Rect {
                        left: scale(5),
                        top: y,
                        right: client.right - scale(5),
                        bottom: y + scale(1).max(1),
                    },
                    border,
                );
                let footer_sep_y = client.bottom - scale(172);
                paint_desktop_name(
                    &mut app,
                    dc,
                    client.right - 1,
                    footer_sep_y,
                    (bg, text),
                    text == accent,
                );
                if footer_sep_y > y + scale(16) {
                    fill(
                        dc,
                        &Rect {
                            left: scale(5),
                            top: footer_sep_y,
                            right: client.right - scale(5),
                            bottom: footer_sep_y + scale(1).max(1),
                        },
                        border,
                    );
                }
                let _ = windows::Win32::Graphics::Gdi::SetViewportOrgEx(
                    windows::Win32::Graphics::Gdi::HDC(dc),
                    0,
                    0,
                    None,
                );
                BitBlt(screen_dc, 0, 0, width, height, dc, 0, 0, 0x00cc0020);
                SelectObject(dc, previous);
                DeleteObject(bitmap);
                DeleteDC(dc);
                EndPaint(window, &paint);
            }
            0
        }
        0x002b if main && lparam != 0 => {
            draw(&app, unsafe { &*(lparam as *const DrawItem) });
            1
        }
        0x004a if main && lparam != 0 => {
            let data = unsafe { &*(lparam as *const CopyData) };
            if data.tag == 0x5241494c && data.size > 0 && data.size <= 65536 && !data.data.is_null()
            {
                let bytes = unsafe { std::slice::from_raw_parts(data.data, data.size as usize) };
                if let Ok(command) = serde_json::from_slice::<Command>(bytes) {
                    app.command(command);
                    return 1;
                }
            }
            0
        }
        0x0111 => {
            let id = wparam & 0xffff;
            if main {
                if let Some(button) = app
                    .buttons
                    .iter()
                    .find(|b| b.id == id && b.hwnd as isize == lparam)
                {
                    let kind = button.kind.clone();
                    if !matches!(kind, ButtonKind::Window(_)) {
                        app.pre_foreground = None;
                    }
                    match kind {
                        ButtonKind::Desktop(guid) => app.command(Command::Switch { guid }),
                        ButtonKind::Window(key) => app.command(Command::Click { key }),
                        ButtonKind::Pause => {
                            let command = if app.model.saved().paused {
                                Command::Resume
                            } else {
                                Command::Pause
                            };
                            app.command(command);
                        }
                        ButtonKind::Settings => app.command(Command::Settings),
                        ButtonKind::LightSwitch => {
                            if !app.theme_busy {
                                match theme::start_toggle() {
                                    Ok(()) => app.theme_busy = true,
                                    Err(error) => {
                                        app.notice =
                                            format!("Windows theme could not be changed: {error}");
                                        app.show_settings();
                                    }
                                }
                                app.render(false);
                            }
                        }
                        ButtonKind::Exit => app.command(Command::Exit),
                    }
                }
            } else {
                match id {
                    SET_PAUSE => {
                        let paused = unsafe {
                            SendMessageW(GetDlgItem(window, SET_PAUSE as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(if paused {
                            Command::Pause
                        } else {
                            Command::Resume
                        });
                    }
                    STARTUP => {
                        if !app.startup_busy {
                            let enabled = unsafe {
                                SendMessageW(GetDlgItem(window, STARTUP as i32), 0xf0, 0, 0)
                            } != 0;
                            if app.startup.try_send(enabled).is_ok() {
                                app.startup_busy = true;
                            }
                            app.update_settings();
                        }
                    }
                    RETRY => app.command(Command::Retry),
                    DESKTOP_TINT => {
                        let enabled = unsafe {
                            SendMessageW(GetDlgItem(window, DESKTOP_TINT as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(Command::DesktopTint { enabled });
                    }
                    SHOW_DESKTOP_NAME => {
                        let enabled = unsafe {
                            SendMessageW(GetDlgItem(window, SHOW_DESKTOP_NAME as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(Command::ShowDesktopName { enabled });
                    }
                    AUTO_RESTORE => {
                        let enabled = unsafe {
                            SendMessageW(GetDlgItem(window, AUTO_RESTORE as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(Command::AutoRestore { enabled });
                    }
                    MINIMIZE_SNAPPED_ELSEWHERE => {
                        let enabled = unsafe {
                            SendMessageW(
                                GetDlgItem(window, MINIMIZE_SNAPPED_ELSEWHERE as i32),
                                0xf0,
                                0,
                                0,
                            )
                        } != 0;
                        app.command(Command::MinimizeSnappedElsewhere { enabled });
                    }
                    DONE => unsafe {
                        app.settings = null_mut();
                        app.settings_signature.clear();
                        DestroyWindow(window);
                    },
                    _ => {}
                }
            }
            0
        }
        0x0002 => {
            if main {
                app.stop();
                unsafe {
                    PostQuitMessage(0);
                }
            } else if app.settings == window {
                app.settings = null_mut();
                app.settings_signature.clear();
            }
            0
        }
        _ => {
            drop(app);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }
}

unsafe extern "system" fn window_event(
    _: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    window: windows::Win32::Foundation::HWND,
    object: i32,
    child: i32,
    _: u32,
    time: u32,
) {
    if object != 0 || child != 0 || window.is_invalid() {
        return;
    }
    let now = platform::tick();
    let mut tick = (now & !0xffff_ffff) | u64::from(time);
    if tick > now {
        tick = tick.wrapping_sub(1u64 << 32);
    }
    notify(Event::NativeWindow(event, window.0 as usize, tick));
}

pub fn run(settings: bool) -> Result<(), String> {
    #[repr(C)]
    struct Controls {
        size: u32,
        classes: u32,
    }
    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn InitCommonControlsEx(controls: *const Controls) -> i32;
    }
    unsafe {
        InitCommonControlsEx(&Controls {
            size: size_of::<Controls>() as u32,
            classes: 0xff,
        });
    }
    let (sender, events) = crossbeam_channel::bounded(64);
    let _ = EVENTS.set(sender);
    let app = Box::new(RefCell::new(App {
        window: null_mut(),
        settings: null_mut(),
        rename: None,
        tooltip: null_mut(),
        font: null_mut(),
        desktop_name_raster: DesktopNameRaster::default(),
        desktop_name_hovered: false,
        desktop_name_pressed: false,
        font_dpi: 0,
        settings_fonts: Vec::new(),
        settings_signature: String::new(),
        model: Model::new(Saved::default()),
        events,
        actions: actions::start(),
        integration: None,
        storage: store::start(),
        startup: store::startup_worker(),
        icon_jobs: icons::start(),
        icon_cache: BTreeMap::new(),
        icon_pending: BTreeSet::new(),
        generation: Arc::new(AtomicU64::new(0)),
        paused: Arc::new(AtomicBool::new(false)),
        tickets: BTreeMap::new(),
        focus_from: BTreeMap::new(),
        buttons: vec![],
        tips: vec![],
        signature: String::new(),
        status: "Loading saved layouts…".into(),
        notice: String::new(),
        snap_notice: crate::snap_notice::Lifecycle::default(),
        loaded: false,
        ready: false,
        transition_received_us: None,
        dispatch_latency: Default::default(),
        startup_busy: false,
        theme_busy: false,
        report_busy: false,
        diagnostic_window_labels: BTreeMap::new(),
        diagnostic_desktop_labels: BTreeMap::new(),
        next_diagnostic_window: 0,
        next_diagnostic_desktop: 0,
        pre_foreground: None,
        hooks: vec![],
        taskbar_message: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        shutdown_ack: None,
        stopped: false,
        reveal: SlideIn::default(),
        reveal_offset: 0,
        shell_switcher: None,
    }));
    unsafe {
        let instance = GetModuleHandleW(null());
        let class_name = wide(CLASS);
        let class = WindowClass {
            size: size_of::<WindowClass>() as u32,
            style: 0,
            procedure: Some(procedure),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: app_icon(32),
            cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: app_icon(16),
        };
        if RegisterClassExW(&class) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let window = CreateWindowExW(
            0x08000080,
            class_name.as_ptr(),
            wide("Virtual Desktop Rail").as_ptr(),
            0x82000000,
            0,
            0,
            31,
            800,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        );
        if window.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        app.borrow_mut().window = window;
        app.borrow().update_window_frame();
        SetWindowLongPtrW(window, -21, &*app as *const RefCell<App> as isize);
        MAIN_WINDOW.store(window as usize, Ordering::Release);
        crate::appbar::register(window).map_err(|e| e.to_string())?;
        ShowWindow(window, 4);
        ShowWindow(window, 4);
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        for (first, last) in [
            (3, 3),
            (10, 11),
            (0x16, 0x17),
            (0x8001, 0x8003),
            (0x8017, 0x8018),
        ] {
            let hook = SetWinEventHook(first, last, None, Some(window_event), 0, 0, 2);
            if !hook.is_invalid() {
                app.borrow_mut().hooks.push(hook.0 as usize);
            }
        }
        app.borrow_mut().drain();
        app.borrow_mut().render(true);
        if settings {
            app.borrow_mut().show_settings();
        }
        let mut message: Message = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result <= 0 {
                break;
            }
            let tooltip = app.borrow().tooltip;
            if !tooltip.is_null() && (0x200..=0x20e).contains(&message.message) {
                SendMessageW(tooltip, 0x407, 0, &message as *const _ as isize);
            }
            let rename_window = app.borrow().rename.as_ref().map(|popover| popover.window);
            if let Some(rename_window) = rename_window {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{IsChild, IsWindowVisible};
                if IsWindowVisible(HWND(rename_window)).as_bool()
                    && matches!(message.message, 0x0201 | 0x0204 | 0x0207 | 0x00a1 | 0x00a4)
                    && message.window != rename_window
                    && !IsChild(HWND(rename_window), HWND(message.window)).as_bool()
                {
                    ShowWindow(rename_window, 0);
                }
                if IsWindowVisible(HWND(rename_window)).as_bool()
                    && (message.window == rename_window
                        || IsChild(HWND(rename_window), HWND(message.window)).as_bool())
                {
                    if message.message == 0x0100 && message.wparam == 0x1b {
                        ShowWindow(rename_window, 0);
                        continue;
                    }
                    if IsDialogMessageW(rename_window, &mut message) != 0 {
                        continue;
                    }
                }
            }
            if message.message == 0x0100 && message.wparam == 0x71 {
                app.borrow_mut().open_rename(message.window);
                continue;
            }
            let settings = app.borrow().settings;
            if settings.is_null()
                && ((message.message == 0x0100
                    && matches!(message.wparam, 0x09 | 0x20 | 0x0d | 0x25..=0x28)
                    && !KEYBOARD_FOCUS.swap(true, Ordering::AcqRel))
                    || (matches!(message.message, 0x0201 | 0x0203)
                        && KEYBOARD_FOCUS.swap(false, Ordering::AcqRel)))
            {
                let focused = GetFocus();
                if !focused.is_null() {
                    InvalidateRect(focused, null(), 0);
                }
            }
            if IsDialogMessageW(
                if settings.is_null() { window } else { settings },
                &mut message,
            ) == 0
            {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let stopped = app.borrow().stopped;
        if !stopped {
            DestroyWindow(window);
            app.borrow_mut().stop();
        }
        MAIN_WINDOW.store(0, Ordering::Release);
        if let Some(ack) = app.borrow_mut().shutdown_ack.take() {
            let _ = ack.recv_timeout(std::time::Duration::from_secs(2));
        }
        for icon in app.borrow_mut().icon_cache.values() {
            if *icon != 0 {
                DestroyIcon(*icon as Handle);
            }
        }
        for font in app.borrow_mut().settings_fonts.drain(..) {
            DeleteObject(font as Handle);
        }
        if !app.borrow_mut().font.is_null() {
            DeleteObject(app.borrow_mut().font);
        }
    }
    Ok(())
}

pub fn remote(command: &Command) -> Result<bool, String> {
    let window = unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            windows::core::PCWSTR(wide(CLASS).as_ptr()),
            windows::core::PCWSTR(wide("Virtual Desktop Rail").as_ptr()),
        )
    };
    let Ok(window) = window else {
        return Ok(false);
    };
    let bytes = serde_json::to_vec(command).map_err(|e| e.to_string())?;
    let data = CopyData {
        tag: 0x5241494c,
        size: bytes.len() as u32,
        data: bytes.as_ptr(),
    };
    let mut result = 0usize;
    let response = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
            window,
            0x4a,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(&data as *const _ as isize),
            windows::Win32::UI::WindowsAndMessaging::SMTO_ABORTIFHUNG,
            1000,
            Some(&mut result),
        )
    };
    if response.0 == 0 || result != 1 {
        return Err("The running rail did not accept the request within one second.".into());
    }
    Ok(true)
}

#[cfg(test)]
mod visual_tests {
    use super::{REPORT_OUTPUT_PATH_CAPACITY, desktop_color, report_output_path_buffer};
    #[test]
    fn reveal_restarts_for_new_switch_and_late_frames_finish_without_replay() {
        let mut reveal = super::SlideIn { started: Some(100) };
        assert_eq!(reveal.offset(100, 38), -38);
        let midway = reveal.offset(200, 38);
        assert!(midway > -38 && midway < 0);
        reveal.started = Some(280);
        assert_eq!(reveal.offset(280, 38), -38);
        assert!(reveal.offset(400, 38) < 0);
        assert_eq!(reveal.offset(10_000, 38), 0);
        assert!(reveal.started.is_none());
        assert_eq!(reveal.offset(10_016, 38), 0);
    }

    #[test]
    fn reveal_cancellation_restores_full_width_after_a_partial_frame() {
        let mut reveal = super::SlideIn { started: Some(0) };
        assert!(reveal.offset(100, 50) < 0);
        reveal.started = None;
        assert_eq!(reveal.offset(181, 50), 0);
        assert_eq!(reveal.offset(200, 25), 0);
    }

    #[test]
    fn external_switch_holds_hidden_until_settled_then_finishes() {
        let mut reveal = super::SlideIn { started: Some(500) };
        assert_eq!(reveal.offset(100, 31), -31);
        assert_eq!(reveal.offset(499, 31), -31);
        assert_eq!(reveal.offset(500, 31), -31);
        assert!(reveal.offset(600, 31) > -31);
        assert_eq!(reveal.offset(700, 31), 0);
        assert!(reveal.started.is_none());
    }

    #[test]
    fn desktop_palette_is_distinct_and_does_not_wrap_after_eight() {
        for dark in [false, true] {
            let colors: std::collections::BTreeSet<_> =
                (0..64).map(|i| desktop_color(i, dark)).collect();
            assert_eq!(colors.len(), 64);
            for a in 0..3 {
                for b in a + 1..3 {
                    let difference: u32 = [0, 8, 16]
                        .iter()
                        .map(|shift| {
                            ((desktop_color(a, dark) >> shift) & 255)
                                .abs_diff((desktop_color(b, dark) >> shift) & 255)
                        })
                        .sum();
                    assert!(difference > 150);
                }
            }
        }
    }

    #[test]
    fn desktop_name_raster_is_independent_of_gdi_batching() {
        use windows::Win32::Graphics::Gdi::{GdiFlush, GdiSetBatchLimit};

        struct RestoreBatchLimit(u32);
        impl Drop for RestoreBatchLimit {
            fn drop(&mut self) {
                unsafe {
                    let _ = GdiFlush();
                    GdiSetBatchLimit(self.0);
                }
            }
        }

        // Batch limits are thread-local; restore even if an assertion fails.
        let restore = RestoreBatchLimit(unsafe { GdiSetBatchLimit(1) });
        assert_ne!(restore.0, 0);
        for dpi in [96, 120, 144, 192] {
            let width = (24 * dpi / 96) as i32;
            unsafe { GdiSetBatchLimit(1) };
            let immediate = super::rasterize_desktop_name("Desktop 02", dpi, width);
            assert!(immediate.0 > 0);
            unsafe { GdiSetBatchLimit(4096) };
            for _ in 0..8 {
                let batched = super::rasterize_desktop_name("Desktop 02", dpi, width);
                assert_eq!(
                    batched, immediate,
                    "Raster changed with GDI batching at {dpi} DPI"
                );
            }
        }
    }

    #[test]
    fn desktop_name_rasterize_centers_and_fits_across_rail_widths() {
        let (empty_h, empty_mask) = super::rasterize_desktop_name("", 96, 24);
        assert_eq!(empty_h, 0);
        assert!(empty_mask.is_empty());

        for (dpi, rail_w) in [(96, 24), (120, 30), (144, 37), (192, 49)] {
            let (h, mask) = super::rasterize_desktop_name("Desktop 02", dpi, rail_w);
            assert!(h > 0);
            assert_eq!(mask.len(), (rail_w * h) as usize);

            // Verify alpha has smooth anti-aliased gradations:
            let has_intermediate = mask.iter().any(|&a| a > 20 && a < 235);
            assert!(
                has_intermediate,
                "Must contain smooth anti-aliased coverage"
            );

            // Verify optical centering:
            let mut weighted_x = 0.0;
            let mut total_weight = 0.0;
            for vy in 0..h {
                for vx in 0..rail_w {
                    let a = mask[(vy * rail_w + vx) as usize] as f64;
                    if a > 16.0 {
                        weighted_x += (vx as f64 + 0.5) * a;
                        total_weight += a;
                    }
                }
            }
            assert!(total_weight > 0.0);
            let centroid_x = weighted_x / total_weight;
            let target_center = rail_w as f64 * 0.5;
            assert!(
                (centroid_x - target_center).abs() < 0.75,
                "DPI {dpi}: Centroid {centroid_x:.2} should be close to rail center {target_center:.2}"
            );
        }
    }

    #[test]
    fn report_picker_buffer_holds_a_full_windows_path_and_suggested_name() {
        let suggested = "VirtualDesktopRail-diagnostic-123.zip";
        let buffer = report_output_path_buffer(suggested);
        assert_eq!(buffer.len(), REPORT_OUTPUT_PATH_CAPACITY);
        assert_eq!(
            String::from_utf16_lossy(&buffer[..suggested.encode_utf16().count()]),
            suggested
        );
        assert!(
            buffer[suggested.encode_utf16().count()..]
                .iter()
                .all(|unit| *unit == 0)
        );
    }

    #[test]
    fn settings_footer_boundary_matches_separator_line() {
        assert_eq!(super::SETTINGS_FOOTER_Y, 420);
        for dpi in [96, 120, 144, 192] {
            let scale = |dip: i32| (dip * dpi + 48) / 96;
            let erase_boundary = (super::SETTINGS_FOOTER_Y * dpi + 48) / 96;
            assert_eq!(
                scale(super::SETTINGS_FOOTER_Y),
                erase_boundary,
                "Footer erase top boundary must match scaled separator line position at {dpi} DPI"
            );
        }
    }

    #[test]
    fn desktop_name_layout_and_hit_testing() {
        let mut raster = super::DesktopNameRaster::default();

        // When show_desktop_name is false, layout is None:
        assert!(
            super::compute_desktop_name_layout(
                false,
                Some(("guid-1", "Desktop 1", 0)),
                96,
                24,
                100,
                500,
                &mut raster,
            )
            .is_none()
        );

        // When current desktop is None, layout is None:
        assert!(
            super::compute_desktop_name_layout(true, None, 96, 24, 100, 500, &mut raster,)
                .is_none()
        );

        // When space is insufficient, layout is None:
        assert!(
            super::compute_desktop_name_layout(
                true,
                Some(("guid-1", "Desktop 1", 0)),
                96,
                24,
                400,
                420,
                &mut raster,
            )
            .is_none()
        );

        // When space is sufficient, layout is Some:
        let layout = super::compute_desktop_name_layout(
            true,
            Some(("guid-1", "My Work", 0)),
            96,
            24,
            100,
            500,
            &mut raster,
        )
        .expect("Layout should succeed when space is sufficient");

        assert_eq!(layout.desktop_id, "guid-1");
        assert_eq!(layout.name, "My Work");
        assert_eq!(layout.rect.left, 0);
        assert_eq!(layout.rect.right, 24);
        assert!(layout.rect.top < layout.text_top);
        assert_eq!(layout.text_top, layout.rect.top + 8);
        assert_eq!(layout.rect.bottom - layout.text_top, raster.height + 8);

        // Hit testing:
        assert!(layout.rect.contains(0, layout.rect.top));
        assert!(
            layout
                .rect
                .contains(12, (layout.rect.top + layout.rect.bottom) / 2)
        );
        assert!(layout.rect.contains(24, layout.rect.bottom - 1));
        assert!(!layout.rect.contains(12, layout.rect.top - 1));
        assert!(!layout.rect.contains(12, layout.rect.bottom));
        assert!(!layout.rect.contains(-1, layout.rect.top));
        assert!(!layout.rect.contains(25, layout.rect.top));

        // When raw name is empty, fallback "Desktop {index + 1}" is formatted:
        let fallback_layout = super::compute_desktop_name_layout(
            true,
            Some(("guid-2", "", 2)),
            96,
            24,
            100,
            500,
            &mut raster,
        )
        .expect("Fallback layout should succeed");
        assert_eq!(fallback_layout.desktop_id, "guid-2");
        assert_eq!(fallback_layout.name, "Desktop 3");
    }
}
