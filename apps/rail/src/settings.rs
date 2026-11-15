//! On-demand companion supervision. Pipe I/O and process waits never run on the UI thread.
use crate::{Event, notify};
use crossbeam_channel::{Sender, bounded};
use rail_settings_protocol::{self as wire, Request, State};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

pub struct Companion {
    pub token: u64,
    pub acknowledged: u64,
    pid: Arc<AtomicU32>,
    created: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    changed: Sender<()>,
    activate: Sender<()>,
    stop: Sender<()>,
}
impl Companion {
    pub fn start(token: u64, state: State) -> Self {
        let pid = Arc::new(AtomicU32::new(0));
        let created = Arc::new(AtomicU64::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        let state = Arc::new(Mutex::new(state));
        let (changed, updates) = bounded(1);
        let (activate, activations) = bounded(1);
        let (stop, stopping) = bounded(1);
        let result = Self {
            token,
            acknowledged: 0,
            pid: pid.clone(),
            created: created.clone(),
            alive: alive.clone(),
            state: state.clone(),
            changed,
            activate,
            stop,
        };
        std::thread::spawn(move || {
            let run = || -> Result<(), String> {
                use std::os::windows::process::CommandExt;
                let path = std::env::current_exe()
                    .map_err(|e| e.to_string())?
                    .with_file_name("settings")
                    .join("rail-settings.exe");
                // The runtime stays in its own directory, away from the resident executable.
                let mut child = rail_compatibility_probe::subprocess::OwnedChild(
                    Command::new(path)
                        .arg("--pipe")
                        .creation_flags(0x08000000)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|e| format!("Cannot start Settings: {e}."))?,
                );
                let _job = crate::bridge::own_process(&child.0)?;
                created.store(
                    rail_compatibility_probe::platform::process_created(child.0.id())
                        .ok_or("Cannot establish Settings process identity")?,
                    Ordering::Release,
                );
                pid.store(child.0.id(), Ordering::Release);
                let mut input = child.0.stdin.take().ok_or("Missing Settings input")?;
                let output = child.0.stdout.take().ok_or("Missing Settings output")?;
                let (failed, failures) = bounded::<Option<String>>(2);
                let writer_failed = failed.clone();
                let (ready, initialized) = bounded(1);
                let started = Instant::now();
                let initial = state.lock().unwrap().clone();
                let (_writer_stop, writer_stopping) = bounded::<()>(0);
                // Separate writers let supervision terminate a child even if it stops reading.
                let writer = std::thread::spawn(move || {
                    let mut next = initial;
                    loop {
                        if let Err(error) = wire::write(&mut input, &next) {
                            let _ = writer_failed.try_send(Some(error));
                            break;
                        }
                        crossbeam_channel::select! {
                            recv(writer_stopping) -> _ => break,
                            recv(updates) -> message => if message.is_err() { break; },
                        }
                        next = state.lock().unwrap().clone();
                    }
                });
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(output);
                    loop {
                        if reader.fill_buf().is_ok_and(|bytes| bytes.is_empty()) {
                            let _ = failed.try_send(None);
                            break;
                        }
                        match wire::read::<Request>(&mut reader) {
                            Ok(request) if request.version == wire::VERSION => {
                                if request.id == 0 && request.action == wire::Action::Ready {
                                    let _ = ready.try_send(());
                                } else if request.id > 0 && request.action != wire::Action::Ready {
                                    notify(Event::SettingsRequest { token, request });
                                } else {
                                    let _ = failed.try_send(Some(
                                        "Invalid Settings request sequence.".into(),
                                    ));
                                    break;
                                }
                            }
                            _ => {
                                let _ = failed.try_send(Some(
                                    "Settings connection closed or sent an invalid request.".into(),
                                ));
                                break;
                            }
                        }
                    }
                });
                let mut booted = false;
                loop {
                    if stopping.recv_timeout(Duration::from_millis(100)).is_ok() {
                        break;
                    }
                    booted |= initialized.try_recv().is_ok();
                    if !booted && started.elapsed() > Duration::from_secs(15) {
                        return Err("Modern Settings did not initialize within 15 seconds.".into());
                    }
                    if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
                        if status.success() {
                            break;
                        }
                        return Err(format!("Modern Settings exited unexpectedly ({status})."));
                    }
                    if let Ok(error) = failures.try_recv() {
                        if let Some(error) = error {
                            return Err(error);
                        }
                        let mut exited = false;
                        for _ in 0..25 {
                            if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
                                if !status.success() {
                                    return Err(format!(
                                        "Modern Settings exited unexpectedly ({status})."
                                    ));
                                }
                                exited = true;
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        if exited {
                            break;
                        }
                        return Err("Modern Settings lost its connection.".into());
                    }
                    if activations.try_recv().is_ok() {
                        activate_process(child.0.id());
                    }
                }
                drop(child);
                // The writer stop channel disconnects when supervision ends.
                drop(writer);
                Ok(())
            };
            if let Err(error) = run() {
                notify(Event::SettingsFailed { token, error });
            }
            alive.store(false, Ordering::Release);
        });
        result
    }
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn owns(&self, window: &rail_core::Window) -> bool {
        window.pid == self.pid.load(Ordering::Acquire)
            && window.identity.process_created == self.created.load(Ordering::Acquire)
    }
    pub fn activate(&self) {
        let _ = self.activate.try_send(());
    }
    pub fn publish(&self, state: State) {
        let mut current = self.state.lock().unwrap();
        if *current != state {
            *current = state;
            let _ = self.changed.try_send(());
        }
    }
}
impl Drop for Companion {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
    }
}

fn activate_process(pid: u32) {
    use windows::Win32::Foundation::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> BOOL {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
        }
        if pid == data.0 as u32
            && unsafe { IsWindowVisible(window) }.as_bool()
            && unsafe { GetWindow(window, GW_OWNER) }
                .unwrap_or_default()
                .is_invalid()
        {
            unsafe {
                let _ = ShowWindow(window, SW_RESTORE);
                let _ = SetForegroundWindow(window);
            }
            return BOOL(0);
        }
        BOOL(1)
    }
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(pid as isize)) };
}
