const ACTION_TIMEOUT_NOTICE: &str = "A window action could not be verified in time. Its button remains disabled while that action is outstanding.";

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
const SET_PAUSE: usize = 6000;
const STARTUP: usize = 6001;
const RETRY: usize = 6002;
const EXIT: usize = 6003;
const DONE: usize = 6004;
const AUTO_RESTORE: usize = 6005;
const SAVE_REPORT: usize = 6105;
const REPORT_PICKER_NOTICE: &str = "Choose a location to save the diagnostic report…";
const REPORT_OUTPUT_PATH_CAPACITY: usize = 32768;

#[derive(Clone, PartialEq, Eq)]
enum ButtonKind {
    Desktop(String),
    Window(String),
    Pause,
    Settings,
    Exit,
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
    loaded: bool,
    ready: bool,
    transition_received_us: Option<u64>,
    dispatch_latency: crate::metrics::DispatchLatency,
    startup_busy: bool,
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
}

struct ReportDraft {
    context: serde_json::Value,
    windows: Vec<store::ReportWindow>,
    previous_notice: String,
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
            Command::AutoRestore { enabled } => {
                self.model.set_auto_restore(enabled);
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"auto_restore_changed","enabled":enabled}),
                ));
            }
            Command::ExperimentalIntegration { enabled } => {
                if self.model.saved.experimental_integration != enabled {
                    self.model.saved.experimental_integration = enabled;
                    self.model.saved.preferences_revision =
                        self.model.saved.preferences_revision.saturating_add(1);
                    self.model.dirty = true;
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
                let _ = self.storage.sender.try_send(store::Work::Trace(
                    serde_json::json!({"event":"integration_retry_requested"}),
                ));
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
                let value = serde_json::json!({"schema_version":1,"build":crate::version::info(),"pid":std::process::id(),"status":self.status,"ready":self.ready,"notice":self.notice,
                    "paused":self.model.saved.paused,"startup":self.model.saved.startup,"generation":self.model.generation,"current":self.model.current,
                    "appbar":crate::appbar::diagnostics(self.window),
                    "dispatch_latency":self.dispatch_latency,
                    "experimental_integration":self.model.saved.experimental_integration,
                    "auto_restore":self.model.saved.auto_restore,
                    "desktops":self.model.desktops,"windows":self.model.windows.iter().filter(|(_,r)|r.present).map(|(key,r)|serde_json::json!({
                        "key":key,"hwnd":r.window.hwnd,"pid":r.window.pid,"minimized":r.window.minimized,"blocked":r.blocked,
                        "responsive":r.window.responsive,"pending":r.pending.is_some(),"layout":r.window.layout,
                        "saved_layouts":self.model.saved.windows.get(key).map(|r|&r.layouts),
                        "minimized_desktops":self.model.saved.windows.get(key).map(|r|&r.minimized_desktops),
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
                    if self.model.current.as_ref().is_some_and(|current| {
                        *current != frame.current
                            || self.model.transition_serial != frame.transition_serial
                    }) {
                        let from = self
                            .model
                            .current
                            .as_deref()
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
                    } else if self.model.current.is_none() {
                        self.transition_received_us = None;
                    }
                    self.model.snapshot(frame);
                    for (key, runtime) in &self.model.windows {
                        if runtime.present
                            && !self.diagnostic_window_labels.contains_key(key)
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
                    if self.model.observe_minimized(observation) {
                        let label = self.diagnostic_window_label(&key);
                        let desktop = self
                            .model
                            .current
                            .as_ref()
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
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"user_notice","kind":if self.notice==ACTION_TIMEOUT_NOTICE{"action_timeout"}else{"other"}}),
                    ));
                }
                Event::Accepted(key, sequence, target, action) => {
                    let label = self.diagnostic_window_label(&key);
                    let target_summary = target.as_ref().map(|layout| {
                        serde_json::json!({
                            "normal":layout.normal,"maximized":layout.maximized,
                            "visible":layout.visible,"arranged":layout.arranged
                        })
                    });
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"window_action_accepted","window":label,"sequence":sequence,"target":target_summary,"action":action.kind(),"source":action.source()}),
                    ));
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
                    let action = self
                        .model
                        .windows
                        .get(&key)
                        .and_then(|r| r.pending.as_ref())
                        .filter(|p| p.request.sequence == sequence)
                        .map(|p| (p.request.action.kind(), p.request.action.source()));
                    self.model
                        .completed(&key, sequence, tick, verified, cancelled);
                    let observed = self.model.windows.get(&key).map(|runtime| {
                        serde_json::json!({
                            "normal":runtime.window.layout.normal,
                            "visible":runtime.window.layout.visible,
                            "maximized":runtime.window.layout.maximized,
                            "arranged":runtime.window.layout.arranged,
                            "minimized":runtime.window.minimized,
                            "pending":runtime.pending.is_some(),
                            "blocked":runtime.blocked
                        })
                    });
                    let _ = self.storage.sender.try_send(store::Work::Trace(
                        serde_json::json!({"event":"window_action_completed","window":self.diagnostic_window_label(&key),"sequence":sequence,"verified":verified,"cancelled":cancelled,"tick_ms":tick,"observed":observed,"action":action.map(|a|a.0),"source":action.map(|a|a.1)}),
                    ));
                    if !verified && !cancelled {
                        self.notice = ACTION_TIMEOUT_NOTICE.into();
                    } else if self.notice == ACTION_TIMEOUT_NOTICE
                        && !self.model.windows.values().any(|runtime| {
                            runtime
                                .pending
                                .as_ref()
                                .is_some_and(|pending| pending.timed_out)
                        })
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
                    0x16 => {
                        if let (Some(integration), Some(desktop), Some(runtime)) = (
                            &self.integration,
                            &self.model.current,
                            self.model
                                .windows
                                .values()
                                .find(|r| r.present && r.window.hwnd == hwnd),
                        ) && runtime.pending.is_none()
                            && !runtime.blocked
                            && !runtime.dragging
                        {
                            let _ = integration.try_send(bridge::CommandMessage::ObserveMinimize(
                                Box::new(rail_core::MinimizeObservation {
                                    window: runtime.window.clone(),
                                    desktop: desktop.clone(),
                                    generation: self.model.generation,
                                    sequence: runtime.sequence,
                                    event_tick: tick,
                                    tick,
                                    transition_serial: self.model.transition_serial,
                                }),
                            ));
                        }
                    }
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
            let mut y = scale(42);
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
                &format!(
                    "{}. {}",
                    if self.model.saved.paused {
                        "Resume automatic window actions"
                    } else {
                        "Pause automatic window actions"
                    },
                    self.status
                ),
                bounds.bottom - scale(122),
                scale(34),
                ButtonKind::Pause,
            );
            self.button(
                SETTINGS,
                &format!("Settings. {}", self.status),
                bounds.bottom - scale(82),
                scale(34),
                ButtonKind::Settings,
            );
            self.button(
                EXIT,
                "Exit application",
                bounds.bottom - scale(42),
                scale(34),
                ButtonKind::Exit,
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
                scale(380),
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
            (6103, "STATIC", "Startup", 0x50000000, 24, 126, 496, 22),
            (
                STARTUP,
                "BUTTON",
                "Start with Windows",
                0x50010003,
                24,
                154,
                496,
                28,
            ),
            (
                6104,
                "STATIC",
                "Desktop integration",
                0x50000000,
                24,
                198,
                496,
                22,
            ),
            (
                RETRY,
                "BUTTON",
                "Retry desktop integration",
                0x50010000,
                24,
                226,
                212,
                32,
            ),
            (
                SAVE_REPORT,
                "BUTTON",
                "Save diagnostic report…",
                0x50010000,
                248,
                226,
                272,
                32,
            ),
            (
                6110,
                "STATIC",
                "Local ZIP: no titles or content. Nothing is uploaded; review before sharing.",
                0x50000000,
                24,
                258,
                496,
                14,
            ),
            (6106, "STATIC", "", 0x50000010, 0, 274, 560, 1),
            (DONE, "BUTTON", "Done", 0x50010001, 432, 290, 88, 34),
            (6107, "STATIC", "●", 0x50000000, 24, 285, 10, 18),
            (6100, "STATIC", "", 0x50000000, 36, 286, 372, 28),
            // Selectable, scrollable details; it occupies no space while hidden.
            (6109, "EDIT", "", 0x40a10844, 24, 310, 496, 0),
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
                    6102..=6104 => 1,
                    6100 | 6107 | 6109 => 2,
                    6110 => 3,
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
                scale(314),
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
                let available = (work[3] - work[1] - chrome - px(380)).max(px(24));
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
                (6109, px(24), px(310), px(496), notice_height),
                (6108, px(36), px(314) + extra, px(372), px(14)),
                (
                    DONE as i32,
                    px(432),
                    if extra == 0 { px(290) } else { px(314) + extra },
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
                px(380)
            } else {
                px(380) + notice_height + chrome
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
        self.layout_settings_notice();
        unsafe {
            let light = GetDlgItem(self.settings, 6107);
            SetWindowLongPtrW(light, -21, self.ready as isize);
            InvalidateRect(light, null(), 1);
            let text = GetDlgItem(self.settings, 6100);
            SetWindowTextW(text, wide(&self.status).as_ptr());
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
                GetDlgItem(self.settings, AUTO_RESTORE as i32),
                0xf1,
                self.model.saved.auto_restore as usize,
                0,
            );
            EnableWindow(
                GetDlgItem(self.settings, AUTO_RESTORE as i32),
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
            .desktops
            .iter()
            .filter_map(|desktop| {
                self.diagnostic_desktop_labels
                    .get(&desktop.id)
                    .map(|label| (desktop.id.clone(), label.clone()))
            })
            .collect();
        let desktops = self
            .model
            .desktops
            .iter()
            .filter_map(|desktop| {
                let label = self.diagnostic_desktop_labels.get(&desktop.id)?;
                Some(serde_json::json!({
                    "label":label,
                    "current":self.model.current.as_deref()==Some(desktop.id.as_str())
                }))
            })
            .collect::<Vec<_>>();
        let windows = self
            .model
            .windows
            .iter()
            .filter(|(_, runtime)| runtime.present)
            .map(|(key, runtime)| {
                let record = self.model.saved.windows.get(key);
                let layouts = self
                    .model
                    .saved
                    .windows
                    .get(key)
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
                    window: runtime.window.clone(),
                    cached_layout: runtime.window.layout.clone(),
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
                    pending: runtime.pending.is_some(),
                    blocked: runtime.blocked,
                }
            })
            .collect();
        let current_desktop = self
            .model
            .current
            .as_ref()
            .and_then(|id| self.diagnostic_desktop_labels.get(id));
        let context = serde_json::json!({
            "schema_version":1,
            "diagnostic_run":self.storage.diagnostic_session,
            "generated_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
            "application":crate::version::info(),
            "integration":{"status":self.status,"ready":self.ready},
            "settings":{"paused":self.model.saved.paused,"auto_restore":self.model.saved.auto_restore,"startup":self.model.saved.startup},
            "desktops":desktops,
            "model":{"generation":self.model.generation,"transition_serial":self.model.transition_serial,"current_desktop":current_desktop},
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
            self.notice = draft.previous_notice;
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
                    let size = icons::pixel_size(GetDpiForWindow(app.window).max(96));
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
                ButtonKind::Pause | ButtonKind::Settings | ButtonKind::Exit => {
                    let color = if matches!(button.kind, ButtonKind::Pause) && !app.ready {
                        unavailable
                    } else {
                        text
                    };
                    smooth_shape(dc, width, height, background, color, |x, y| {
                        if matches!(button.kind, ButtonKind::Pause) && !app.ready {
                            // A full warning symbol stays legible in the narrow rail and
                            // distinguishes unavailable integration without relying on color.
                            let triangle = (x.abs() * 0.866 - y * 0.5 - 3.5).max(y - 6.0);
                            let outline = triangle.abs() - 0.7;
                            let stem = (x.abs() - 0.75).max((y + 0.5).abs() - 2.4);
                            let dot = (x * x + (y - 4.0).powi(2)).sqrt() - 0.85;
                            outline.min(stem).min(dot)
                        } else if matches!(button.kind, ButtonKind::Settings) {
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
                        } else if matches!(button.kind, ButtonKind::Exit) {
                            let ring = ((x * x + y * y).sqrt() - 6.0).abs() - 0.8;
                            let ring = if y < -2.0 && x.abs() < 3.0 {
                                100.0
                            } else {
                                ring
                            };
                            ring.min((x.abs() - 0.8).max((y + 4.0).abs() - 4.0))
                        } else if app.model.saved.paused {
                            (-x - 4.0).max(y.abs() * 1.4 + x - 4.0)
                        } else {
                            ((x.abs() - 3.0).abs() - 0.9).max(y.abs() - 6.0)
                        }
                    });
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
    // Color/erase messages can reenter while ShowWindow holds the model borrow.
    if !main {
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
                rect.top = (274 * dpi + 48) / 96;
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
                let size = scale(18);
                DrawIconEx(
                    dc,
                    (client.right - 1 - size) / 2,
                    scale(12),
                    app_icon(size),
                    size,
                    size,
                    0,
                    null_mut(),
                    3,
                );
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
                let y = scale(42 + 33 * app.model.desktops.len() as i32 + 3);
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
                    AUTO_RESTORE => {
                        let enabled = unsafe {
                            SendMessageW(GetDlgItem(window, AUTO_RESTORE as i32), 0xf0, 0, 0)
                        } != 0;
                        app.command(Command::AutoRestore { enabled });
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
        tooltip: null_mut(),
        font: null_mut(),
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
        loaded: false,
        ready: false,
        transition_received_us: None,
        dispatch_latency: Default::default(),
        startup_busy: false,
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
        // Task View composes desktop previews through DWM, independently of
        // screen-capture affinity. Exclude the rail from that composition.
        let excluded = windows::Win32::Foundation::BOOL(1);
        if let Err(error) = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            windows::Win32::Foundation::HWND(window),
            windows::Win32::Graphics::Dwm::DWMWA_EXCLUDED_FROM_PEEK,
            &excluded as *const _ as *const std::ffi::c_void,
            size_of::<windows::Win32::Foundation::BOOL>() as u32,
        ) {
            let _ = app
                .borrow()
                .storage
                .sender
                .try_send(store::Work::Log(format!(
                    "Could not exclude the rail from desktop previews: {error}"
                )));
        }
        SetWindowLongPtrW(window, -21, &*app as *const RefCell<App> as isize);
        MAIN_WINDOW.store(window as usize, Ordering::Release);
        crate::appbar::register(window).map_err(|e| e.to_string())?;
        ShowWindow(window, 4);
        ShowWindow(window, 4);
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        for (first, last) in [(3, 3), (10, 11), (0x16, 0x17), (0x8001, 0x8002)] {
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
}
