use crate::{
    EVENTS, Event, MAIN_WINDOW, OVERRUN, WAKE, actions, bridge, icons, native::*, notify, platform,
    store,
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
const PAUSE: usize = 5000;
const SETTINGS: usize = 5001;
const SET_PAUSE: usize = 6000;
const STARTUP: usize = 6001;
const RETRY: usize = 6002;
const EXIT: usize = 6003;
const EXPERIMENTAL: usize = 6004;
const AUTO_RESTORE: usize = 6005;

#[derive(Clone, PartialEq, Eq)]
enum ButtonKind {
    Desktop(String),
    Window(String),
    Pause,
    Settings,
}
struct Button {
    hwnd: Handle,
    id: usize,
    kind: ButtonKind,
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
}

struct App {
    window: Handle,
    settings: Handle,
    tooltip: Handle,
    font: Handle,
    font_dpi: u32,
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
    loaded: bool,
    ready: bool,
    transition_received_us: Option<u64>,
    dispatch_latency: crate::metrics::DispatchLatency,
    startup_busy: bool,
    pre_foreground: Option<usize>,
    hooks: Vec<usize>,
    taskbar_message: u32,
    shutdown_ack: Option<Receiver<Result<(), String>>>,
    stopped: bool,
}

impl App {
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
            Command::AutoRestore { enabled } => self.model.set_auto_restore(enabled),
            Command::ExperimentalIntegration { enabled } => {
                if self.model.saved.experimental_integration != enabled {
                    self.model.saved.experimental_integration = enabled;
                    self.model.saved.preferences_revision =
                        self.model.saved.preferences_revision.saturating_add(1);
                    self.model.dirty = true;
                    self.command(Command::Retry);
                }
            }
            Command::Pause => self.model.pause(true),
            Command::Resume => self.model.pause(false),
            Command::Retry => {
                self.model.unavailable();
                self.ready = false;
                self.status = "Reconnecting to Windows desktops…".into();
                if let Some(integration) = &self.integration {
                    let _ = integration.try_send(bridge::CommandMessage::Retry {
                        experimental: self.model.saved.experimental_integration,
                    });
                } else {
                    match platform::supported(self.model.saved.experimental_integration) {
                        Ok(()) => {
                            self.integration =
                                Some(bridge::start(self.model.saved.experimental_integration))
                        }
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
                let value = serde_json::json!({"schema_version":1,"pid":std::process::id(),"status":self.status,"ready":self.ready,"notice":self.notice,
                    "paused":self.model.saved.paused,"startup":self.model.saved.startup,"generation":self.model.generation,"current":self.model.current,
                    "appbar":crate::appbar::diagnostics(self.window),
                    "dispatch_latency":self.dispatch_latency,
                    "experimental_integration":self.model.saved.experimental_integration,
                    "auto_restore":self.model.saved.auto_restore,
                    "desktops":self.model.desktops,"windows":self.model.windows.iter().filter(|(_,r)|r.present).map(|(key,r)|serde_json::json!({
                        "key":key,"hwnd":r.window.hwnd,"pid":r.window.pid,"minimized":r.window.minimized,"blocked":r.blocked,
                        "responsive":r.window.responsive,"pending":r.pending.is_some(),"layout":r.window.layout,
                        "saved_layouts":self.model.saved.windows.get(key).map(|r|&r.layouts),
                        "layout_count":self.model.saved.windows.get(key).map(|r|r.layouts.len()).unwrap_or(0)})).collect::<Vec<_>>()});
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
                    (self.ready, &self.integration, &self.model.current)
                    && self.model.desktops.iter().any(|d| d.id == guid)
                {
                    let _ = integration.try_send(bridge::CommandMessage::Switch {
                        guid,
                        expected_current: current.clone(),
                        expected_serial: self.model.transition_serial,
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
                if let Some(runtime) = self.model.windows.get_mut(&key) {
                    self.focus_from.insert(key.clone(), foreground);
                    // This is a nonblocking USER state query, not an application message.
                    runtime.window.minimized = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::IsIconic(platform::hwnd(
                            runtime.window.hwnd,
                        ))
                    }
                    .as_bool();
                    let root = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::GetAncestor(
                            platform::hwnd(foreground),
                            windows::Win32::UI::WindowsAndMessaging::GA_ROOTOWNER,
                        )
                    }
                    .0 as usize;
                    let was = foreground == runtime.window.hwnd
                        || root == runtime.window.hwnd
                        || Some(foreground) == runtime.window.popup;
                    self.model.click(&key, was);
                }
            }
        }
        self.dispatch();
        self.render(false);
    }

    fn dispatch(&mut self) {
        self.focus_from
            .retain(|key, _| self.model.windows.contains_key(key));
        self.generation
            .store(self.model.generation, Ordering::Release);
        self.paused
            .store(self.model.saved.paused, Ordering::Release);
        for (key, runtime) in &self.model.windows {
            self.tickets
                .entry(key.clone())
                .or_insert_with(|| Arc::new(AtomicU64::new(runtime.sequence)))
                .store(runtime.sequence, Ordering::Release);
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
                self.model
                    .completed(&key, sequence, platform::tick(), false, true);
                self.notice = "Window action queue is busy; try again.".into();
            }
        }
        if self.loaded
            && self.model.dirty
            && self
                .storage
                .sender
                .try_send(store::Work::Save(self.model.saved.clone()))
                .is_ok()
        {
            self.model.dirty = false;
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
                    if self.model.saved.startup {
                        self.startup_busy = true;
                        let _ = self.startup.try_send(true);
                    }
                    match platform::supported(self.model.saved.experimental_integration) {
                        Ok(()) => {
                            self.integration =
                                Some(bridge::start(self.model.saved.experimental_integration))
                        }
                        Err(error) => self.status = error,
                    }
                }
                Event::Frame(frame, received, experimental) => {
                    // Ignore queued frames from a worker superseded by a legacy
                    // preference change while its replacement is reconnecting.
                    if experimental != self.model.saved.experimental_integration {
                        continue;
                    }
                    crate::appbar::foreground_changed(self.window);
                    if !self.ready {
                        let _ = self
                            .storage
                            .sender
                            .try_send(store::Work::Log("Desktop integration is ready.".into()));
                    }
                    self.ready = true;
                    self.status = "Desktop integration is ready.".into();
                    if self.model.current.as_ref().is_some_and(|current| {
                        *current != frame.current
                            || self.model.transition_serial != frame.transition_serial
                    }) {
                        self.transition_received_us = received;
                    } else if self.model.current.is_none() {
                        self.transition_received_us = None;
                    }
                    self.model.snapshot(frame);
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
                    if !ready {
                        self.transition_received_us = None;
                        self.model.unavailable();
                    }
                }
                Event::Notice(notice) => {
                    let _ = self
                        .storage
                        .sender
                        .try_send(store::Work::Log(notice.clone()));
                    self.notice = notice;
                }
                Event::Accepted(key, sequence, target) => {
                    if let Some(target) = target
                        && let Some(runtime) = self.model.windows.get_mut(&key)
                        && let Some(pending) = &mut runtime.pending
                        && pending.request.sequence == sequence
                    {
                        pending.request.action = match pending.request.action {
                            rail_core::Action::RestoreAutomatic(_) => {
                                rail_core::Action::RestoreAutomatic(target)
                            }
                            rail_core::Action::Restore(_) => rail_core::Action::Restore(target),
                            ref action => action.clone(),
                        };
                    }
                    self.model.dispatched(&key, sequence);
                }
                Event::ActionResult(key, sequence, tick, verified, cancelled) => {
                    self.model
                        .completed(&key, sequence, tick, verified, cancelled);
                    if !verified && !cancelled {
                        self.notice="A window did not respond in time. Its button is disabled until the outstanding action is observed.".into();
                    }
                    if let Some(integration) = &self.integration {
                        let _ = integration.try_send(bridge::CommandMessage::Refresh);
                    }
                }
                Event::Captured {
                    key,
                    generation,
                    tick,
                    layout,
                } => {
                    if generation == self.model.generation
                        && let (Some(runtime), Ok(layout)) = (self.model.windows.get(&key), layout)
                    {
                        let hwnd = runtime.window.hwnd;
                        self.model.moving(hwnd, false, tick, Some(layout));
                    }
                }
                Event::Stored(result) => {
                    if let Err(error) = result {
                        self.notice = format!("Saved layouts could not be written: {error}");
                        self.model.dirty = true;
                    }
                }
                Event::Startup(value, result) => {
                    self.startup_busy = false;
                    match result {
                        Ok(()) => {
                            if self.model.saved.startup != value {
                                self.model.saved.startup = value;
                                self.model.saved.preferences_revision =
                                    self.model.saved.preferences_revision.saturating_add(1);
                                self.model.dirty = true;
                            }
                        }
                        Err(error) => {
                            self.notice =
                                format!("Windows startup preference could not be changed: {error}")
                        }
                    }
                }
                Event::Icon(key, dpi, icon) => {
                    self.icon_pending.remove(&(key.clone(), dpi));
                    if self.model.windows.get(&key).is_some_and(|r| r.present)
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
                    3 => {
                        crate::appbar::foreground_changed(self.window);
                        if let Some(integration) = &self.integration {
                            let _ = integration.try_send(bridge::CommandMessage::Refresh);
                        }
                    }
                    10 => self.model.moving(hwnd, true, tick, None),
                    11 => {
                        self.model.moving(hwnd, false, tick, None);
                        if let Some((key, runtime)) = self
                            .model
                            .windows
                            .iter()
                            .find(|(_, r)| r.present && r.window.hwnd == hwnd)
                        {
                            let _ = self.actions.try_send(actions::Work::Capture {
                                key: key.clone(),
                                window: runtime.window.clone(),
                                generation: self.model.generation,
                                tick,
                            });
                        }
                    }
                    0x7e | 0x2e0 => {
                        self.signature.clear();
                    }
                    _ => {
                        if let Some(integration) = &self.integration {
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
            "{:?}|{:?}|{}|{}|{:?}|{}|{}",
            self.model.desktops,
            self.model.current,
            self.ready,
            self.model.saved.paused,
            self.model
                .windows
                .iter()
                .filter(|(_, r)| r.present)
                .map(|(key, r)| (key, r.window.minimized, r.blocked, r.window.responsive))
                .collect::<Vec<_>>(),
            dpi,
            self.status
        );
        if force || signature != self.signature {
            self.signature = signature;
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
                for button in self.buttons.drain(..) {
                    DestroyWindow(button.hwnd);
                }
            }
            self.tooltip = null_mut();
            self.tips.clear();
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
                if !self.settings.is_null() {
                    for id in [
                        6100,
                        SET_PAUSE as i32,
                        STARTUP as i32,
                        RETRY as i32,
                        EXIT as i32,
                    ] {
                        unsafe {
                            SendMessageW(
                                GetDlgItem(self.settings, id),
                                0x30,
                                self.font as usize,
                                1,
                            );
                        }
                    }
                }
                if !old_font.is_null() {
                    unsafe {
                        DeleteObject(old_font);
                    }
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
            let mut y = scale(12);
            let desktops = self.model.desktops.clone();
            for (index, desktop) in desktops.iter().enumerate() {
                let name = if desktop.name.is_empty() {
                    format!("Desktop {}", index + 1)
                } else {
                    desktop.name.clone()
                };
                let caption = if self.model.current.as_ref() == Some(&desktop.id) {
                    format!("{name} (current)")
                } else {
                    name.clone()
                };
                let button = self.button(
                    100 + index,
                    &caption,
                    y,
                    scale(30),
                    ButtonKind::Desktop(desktop.id.clone()),
                );
                unsafe {
                    EnableWindow(button, self.ready as i32);
                }
                self.tips.push(wide(&name));
                let text = self.tips.last_mut().unwrap().as_mut_ptr();
                let mut info = ToolInfo {
                    size: size_of::<ToolInfo>() as u32,
                    flags: 0x11,
                    window: self.window,
                    id: button as usize,
                    rect: Rect::default(),
                    instance: null_mut(),
                    text,
                    parameter: 0,
                    reserved: null_mut(),
                };
                unsafe {
                    SendMessageW(self.tooltip, 0x432, 0, &mut info as *mut _ as isize);
                }
                y += scale(33);
            }
            y += scale(15);
            let windows = self
                .model
                .windows
                .iter()
                .filter(|(_, r)| r.present)
                .map(|(k, r)| (k.clone(), r.window.clone(), r.blocked))
                .collect::<Vec<_>>();
            for (index, (key, window, blocked)) in windows.iter().enumerate() {
                let name = std::path::Path::new(&window.identity.executable)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let button = self.button(
                    1000 + index,
                    &format!("Shared {name} window {}", index + 1),
                    y,
                    scale(39),
                    ButtonKind::Window(key.clone()),
                );
                unsafe {
                    EnableWindow(button, (self.ready && !blocked && window.responsive) as i32);
                }
                let icon_key = (key.clone(), dpi);
                if !self.icon_cache.contains_key(&icon_key)
                    && !self.icon_pending.contains(&icon_key)
                    && self.icon_jobs.try_send((window.clone(), dpi)).is_ok()
                {
                    self.icon_pending.insert(icon_key);
                }
                y += scale(45);
            }
            let mut bounds = Rect::default();
            unsafe {
                GetClientRect(self.window, &mut bounds);
            }
            self.button(
                PAUSE,
                if self.model.saved.paused {
                    "Resume automatic window actions"
                } else {
                    "Pause automatic window actions"
                },
                bounds.bottom - scale(82),
                scale(34),
                ButtonKind::Pause,
            );
            self.button(
                SETTINGS,
                &format!("Settings. {}", self.status),
                bounds.bottom - scale(42),
                scale(34),
                ButtonKind::Settings,
            );
            let live = self
                .model
                .windows
                .iter()
                .filter(|(_, r)| r.present)
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
            if unsafe { GetForegroundWindow() } == self.window
                && let Some(focused) = focused
                && let Some(button) = self.buttons.iter().find(|button| button.kind == focused)
            {
                unsafe {
                    SetFocus(button.hwnd);
                }
            }
            unsafe {
                InvalidateRect(self.window, null(), 1);
            }
        }
        self.update_settings();
    }

    fn button(&mut self, id: usize, name: &str, y: i32, cell: i32, kind: ButtonKind) -> Handle {
        let button = unsafe {
            CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide(name).as_ptr(),
                0x5001000b,
                0,
                y,
                ((25 * GetDpiForWindow(self.window).max(96) + 48) / 96) as i32 - 1,
                cell,
                self.window,
                id as Handle,
                GetModuleHandleW(null()),
                null_mut(),
            )
        };
        self.buttons.push(Button {
            hwnd: button,
            id,
            kind,
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
        let pointer = unsafe { GetWindowLongPtrW(self.window, -21) };
        self.settings = unsafe {
            CreateWindowExW(
                0x40000,
                wide(CLASS).as_ptr(),
                wide("Virtual Desktop Rail — Settings").as_ptr(),
                0x10c80000,
                200,
                160,
                640,
                470,
                self.window,
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            )
        };
        unsafe {
            SetWindowLongPtrW(self.settings, -21, pointer);
        }
        for (id, class, label, style, y, height) in [
            (6100, "STATIC", "", 0x50000000, 18, 140),
            (
                SET_PAUSE,
                "BUTTON",
                "Pause automatic window actions",
                0x50010003,
                164,
                30,
            ),
            (STARTUP, "BUTTON", "Start with Windows", 0x50010003, 200, 30),
            (
                AUTO_RESTORE,
                "BUTTON",
                "Restore saved layouts on desktop switch",
                0x50010003,
                236,
                30,
            ),
            (
                6101,
                "STATIC",
                "Windows without a saved layout get minimized.",
                0x50000000,
                270,
                28,
            ),
            (
                EXPERIMENTAL,
                "BUTTON",
                "Experimental integration (legacy preference; no longer required)",
                0x50010003,
                304,
                36,
            ),
            (
                RETRY,
                "BUTTON",
                "Retry desktop integration",
                0x50010000,
                364,
                34,
            ),
            (EXIT, "BUTTON", "Exit", 0x50010000, 364, 34),
        ] {
            let (x, width) = if id == EXIT {
                (440, 120)
            } else {
                (20, if id == RETRY { 260 } else { 580 })
            };
            let child = unsafe {
                CreateWindowExW(
                    0,
                    wide(class).as_ptr(),
                    wide(label).as_ptr(),
                    style,
                    x,
                    y,
                    width,
                    height,
                    self.settings,
                    id as Handle,
                    GetModuleHandleW(null()),
                    null_mut(),
                )
            };
            unsafe {
                SendMessageW(child, 0x30, self.font as usize, 1);
            }
        }
        self.update_settings();
    }

    fn update_settings(&mut self) {
        if self.settings.is_null() {
            return;
        }
        let signature = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.status,
            self.notice,
            self.model.saved.paused,
            self.model.saved.startup,
            self.startup_busy,
            self.loaded,
            self.model.saved.experimental_integration,
            self.model.saved.auto_restore
        );
        if signature == self.settings_signature {
            return;
        }
        self.settings_signature = signature;
        unsafe {
            let text = GetDlgItem(self.settings, 6100);
            SetWindowTextW(
                text,
                wide(&format!("{}\r\n{}", self.status, self.notice)).as_ptr(),
            );
            SendMessageW(
                GetDlgItem(self.settings, SET_PAUSE as i32),
                0xf1,
                self.model.saved.paused as usize,
                0,
            );
            SendMessageW(
                GetDlgItem(self.settings, STARTUP as i32),
                0xf1,
                self.model.saved.startup as usize,
                0,
            );
            SendMessageW(
                GetDlgItem(self.settings, EXPERIMENTAL as i32),
                0xf1,
                self.model.saved.experimental_integration as usize,
                0,
            );
            SendMessageW(
                GetDlgItem(self.settings, AUTO_RESTORE as i32),
                0xf1,
                self.model.saved.auto_restore as usize,
                0,
            );
            EnableWindow(
                GetDlgItem(self.settings, AUTO_RESTORE as i32),
                self.loaded as i32,
            );
            EnableWindow(GetDlgItem(self.settings, EXPERIMENTAL as i32), 0);
            EnableWindow(
                GetDlgItem(self.settings, STARTUP as i32),
                (!self.startup_busy && self.loaded) as i32,
            );
            EnableWindow(
                GetDlgItem(self.settings, SET_PAUSE as i32),
                self.loaded as i32,
            );
        }
    }

    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if let Some(integration) = &self.integration {
            let _ = integration.try_send(bridge::CommandMessage::Stop);
        }
        let _ = self.actions.try_send(actions::Work::Stop);
        let (ack, result) = crossbeam_channel::bounded(1);
        let _ = self
            .storage
            .stop
            .try_send((self.loaded.then(|| self.model.saved.clone()), ack));
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

fn dark() -> bool {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut u32,
            size: *mut u32,
        ) -> i32;
    }
    let mut value = 1u32;
    let mut size = 4;
    unsafe {
        RegGetValueW(
            0x80000001u32 as i32 as isize,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            wide("AppsUseLightTheme").as_ptr(),
            0x10,
            null_mut(),
            &mut value,
            &mut size,
        );
    }
    value == 0
}

unsafe fn fill(dc: Handle, rect: &Rect, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, rect, brush);
        DeleteObject(brush);
    }
}

fn palette() -> (u32, u32, u32, u32) {
    use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor};
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    if unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some(&mut contrast as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
        && contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
    {
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
    background: u32,
    color: u32,
    distance: impl Fn(f64, f64) -> f64,
) {
    use windows::Win32::{
        Foundation::COLORREF,
        Graphics::Gdi::{HDC, SetPixelV},
    };
    let scale = width as f64 / 25.0;
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
            let mut blended = 0;
            for shift in [0, 8, 16] {
                let a = ((background >> shift) & 255) as f64;
                let b = ((color >> shift) & 255) as f64;
                blended |= ((a + (b - a) * coverage).round() as u32) << shift;
            }
            unsafe {
                let _ = SetPixelV(HDC(dc), x, y, COLORREF(blended));
            }
        }
    }
}

fn draw(app: &App, item: &DrawItem) {
    let width = item.rect.right - item.rect.left;
    let height = item.rect.bottom - item.rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    let (background, text, accent, unavailable) = palette();
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
        let center = width / 2;
        if let Some(button) = app.buttons.iter().find(|b| b.hwnd == item.window) {
            match &button.kind {
                ButtonKind::Desktop(id) => {
                    let active = app.model.current.as_ref() == Some(id);
                    let index = app
                        .model
                        .desktops
                        .iter()
                        .position(|d| &d.id == id)
                        .unwrap_or(0);
                    let color = if accent == text {
                        text
                    } else {
                        desktop_color(index, dark())
                    };
                    smooth_shape(dc, width, height, background, color, |x, y| {
                        let distance = (x * x + y * y).sqrt();
                        if active {
                            distance - 5.0
                        } else {
                            (distance - 2.75).abs() - 0.75
                        }
                    });
                }
                ButtonKind::Window(key) => {
                    let icon = app
                        .icon_cache
                        .get(&(key.clone(), GetDpiForWindow(app.window).max(96)))
                        .copied()
                        .filter(|icon| *icon != 0)
                        .map(|icon| icon as Handle)
                        .unwrap_or_else(|| LoadIconW(null_mut(), 32512usize as *const u16));
                    let size = (width - 8).max(8);
                    let minimized = app
                        .model
                        .windows
                        .get(key)
                        .is_some_and(|r| r.window.minimized);
                    // Preserve the application's normal icon in every state.
                    // Only the underline changes: outlined minimized, solid visible.
                    DrawIconEx(
                        dc,
                        (width - size) / 2,
                        (height - size) / 2,
                        icon,
                        size,
                        size,
                        0,
                        null_mut(),
                        3,
                    );
                    let indicator = Rect {
                        left: center - 5,
                        top: height - 4,
                        right: center + 5,
                        bottom: height - 1,
                    };
                    fill(dc, &indicator, if minimized { text } else { accent });
                    if minimized {
                        fill(
                            dc,
                            &Rect {
                                left: indicator.left + 1,
                                top: indicator.top + 1,
                                right: indicator.right - 1,
                                bottom: indicator.bottom - 1,
                            },
                            background,
                        );
                    }
                }
                ButtonKind::Pause | ButtonKind::Settings => {
                    smooth_shape(dc, width, height, background, text, |x, y| {
                        if matches!(button.kind, ButtonKind::Settings) {
                            let track = |yy: f64| (y - yy).abs().max(x.abs() - 7.0) - 0.8;
                            let ring = |xx: f64, yy: f64| {
                                (((x - xx).powi(2) + (y - yy).powi(2)).sqrt() - 2.5).abs() - 0.8
                            };
                            let upper = if (x + 3.0).abs() < 3.0 {
                                100.0
                            } else {
                                track(-4.0)
                            };
                            let lower = if (x - 3.0).abs() < 3.0 {
                                100.0
                            } else {
                                track(4.0)
                            };
                            upper.min(lower).min(ring(-3.0, -4.0)).min(ring(3.0, 4.0))
                        } else if app.model.saved.paused {
                            (-x - 4.0).max(y.abs() * 1.4 + x - 4.0)
                        } else {
                            ((x.abs() - 3.0).abs() - 0.9).max(y.abs() - 6.0)
                        }
                    });
                    if matches!(button.kind, ButtonKind::Settings) {
                        fill(
                            dc,
                            &Rect {
                                left: 2,
                                top: 2,
                                right: 5,
                                bottom: 5,
                            },
                            if app.ready { accent } else { unavailable },
                        );
                    }
                }
            }
        }
        if item.state & 0x10 != 0 {
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
                    experimental: app.model.saved.experimental_integration,
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
    let Ok(mut app) = cell.try_borrow_mut() else {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    match message {
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
            if wparam & 0xffff != 0 && lparam != 0 {
                app.pre_foreground = Some(lparam as usize);
            }
            0
        }
        0x0005 if main => {
            app.render(true);
            0
        }
        0x0015 | 0x001a | 0x031a => {
            app.signature.clear();
            app.render(true);
            0
        }
        0x0014 if main => 1,
        0x000f if main => {
            let mut paint: Paint = unsafe { zeroed() };
            unsafe {
                let dc = BeginPaint(window, &mut paint);
                fill(dc, &paint.rect, palette().0);
                let dpi = GetDpiForWindow(window).max(96) as i32;
                let scale = |dip: i32| (dip * dpi + 48) / 96;
                let mut client = Rect::default();
                GetClientRect(window, &mut client);
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
                let y = scale(12 + 33 * app.model.desktops.len() as i32 + 3);
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
                            let command = if app.model.saved.paused {
                                Command::Resume
                            } else {
                                Command::Pause
                            };
                            app.command(command);
                        }
                        ButtonKind::Settings => app.command(Command::Settings),
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
                    EXPERIMENTAL => {
                        let enabled = unsafe {
                            SendMessageW(GetDlgItem(window, EXPERIMENTAL as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(Command::ExperimentalIntegration { enabled });
                    }
                    AUTO_RESTORE => {
                        let enabled = unsafe {
                            SendMessageW(GetDlgItem(window, AUTO_RESTORE as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(Command::AutoRestore { enabled });
                    }
                    EXIT => app.command(Command::Exit),
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
        tooltip: null_mut(),
        font: null_mut(),
        font_dpi: 0,
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
        loaded: false,
        ready: false,
        transition_received_us: None,
        dispatch_latency: Default::default(),
        startup_busy: false,
        pre_foreground: None,
        hooks: vec![],
        taskbar_message: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        shutdown_ack: None,
        stopped: false,
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
            icon: LoadIconW(null_mut(), 32512usize as *const u16),
            cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: null_mut(),
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
        SetWindowLongPtrW(window, -21, &*app as *const RefCell<App> as isize);
        MAIN_WINDOW.store(window as usize, Ordering::Release);
        crate::appbar::register(window).map_err(|e| e.to_string())?;
        ShowWindow(window, 4);
        ShowWindow(window, 4);
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        for (first, last) in [(3, 3), (10, 11), (0x8001, 0x8002)] {
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
            let settings = app.borrow().settings;
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
    use super::desktop_color;
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
}
