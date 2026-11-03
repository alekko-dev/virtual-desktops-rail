use crate::{Event, notify, platform};
use crossbeam_channel::{Receiver, Sender};
use rail_compatibility_probe::{platform as diagnostic, subprocess::OwnedChild};
use rail_core::Snapshot;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum CommandMessage {
    Switch {
        guid: String,
        expected_current: String,
        expected_serial: u64,
    },
    Refresh,
    ObserveMinimize(Box<rail_core::MinimizeObservation>),
    Retry {
        experimental: bool,
    },
    Stop,
}

#[derive(Deserialize, Serialize)]
enum Wire {
    Frame(Snapshot, Option<u64>),
    Failure(String),
    Minimized(rail_core::MinimizeObservation),
}

struct KillJob(windows::Win32::Foundation::HANDLE);
impl Drop for KillJob {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}
fn own_process(child: &std::process::Child) -> Result<KillJob, String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::System::JobObjects::*;
    let job = KillJob(unsafe { CreateJobObjectW(None, None) }.map_err(|e| e.to_string())?);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        )
    }
    .map_err(|e| e.to_string())?;
    unsafe {
        AssignProcessToJobObject(
            job.0,
            windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(job)
}

fn emit(message: Wire) -> Result<(), String> {
    let bytes = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Integration report exceeded 1 MiB".into());
    }
    let mut output = std::io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}

fn parse_guid(value: &str) -> Option<windows::core::GUID> {
    (value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        }))
    .then(|| windows::core::GUID::from(value))
}

pub fn worker(parent: u32, experimental: bool) -> Result<(), String> {
    platform::supported(experimental)?;
    let (parent_image, parent_session) =
        diagnostic::process_context(parent).map_err(|_| "Invalid integration parent process")?;
    let (image, session) = diagnostic::process_context(std::process::id())?;
    if parent == std::process::id()
        || parent_session != session
        || !parent_image.eq_ignore_ascii_case(&image)
    {
        return Err("Invalid integration parent process".into());
    }
    let (commands, incoming) = crossbeam_channel::bounded(32);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            // Native evidence includes the executable identity, which may use
            // a full Windows long path. Keep IPC bounded without rejecting it.
            if line.len() > 256 * 1024 {
                break;
            }
            let Ok(command) = serde_json::from_str::<CommandMessage>(&line) else {
                break;
            };
            if matches!(command, CommandMessage::Stop) {
                let _ = commands.send(command);
                return;
            }
            let _ = commands.try_send(command);
        }
        let _ = commands.send(CommandMessage::Stop);
    });
    let (sender, events) = crossbeam_channel::bounded::<winvd::DesktopEvent>(128);
    let _listener = winvd::listen_desktop_events(sender).map_err(|e| format!("{e:?}"))?;
    let mut candidates = diagnostic::candidates()?;
    let mut last_scan = Instant::now();
    let mut serial = 0;
    let mut transition_received_us = None;
    loop {
        match platform::snapshot(parent, &candidates, serial) {
            Ok(frame) => {
                emit(Wire::Frame(frame, transition_received_us))?;
                transition_received_us = None;
            }
            Err(error) if error.starts_with("Desktop changed during") => {}
            Err(error) => {
                emit(Wire::Failure(error))?;
                return Ok(());
            }
        }
        let mut rescan = last_scan.elapsed() >= Duration::from_secs(30);
        let mut pending = None;
        crossbeam_channel::select! {
            recv(incoming) -> message => match message { Ok(message) => pending = Some(message), Err(_) => return Ok(()) },
            recv(events) -> event => if let Ok(event) = event {
                if matches!(event, winvd::DesktopEvent::DesktopChanged { .. }) {
                    serial += 1;
                    if transition_received_us.is_none() {
                        transition_received_us = crate::metrics::clock_us();
                    }
                }
                rescan = true;
            },
            default(Duration::from_secs(1)) => {}
        }
        for event in events.try_iter() {
            if matches!(event, winvd::DesktopEvent::DesktopChanged { .. }) {
                serial += 1;
                // Coalesced transitions are measured from the earliest receipt.
                if transition_received_us.is_none() {
                    transition_received_us = crate::metrics::clock_us();
                }
            }
            rescan = true;
        }
        let mut messages: Vec<_> = pending.into_iter().chain(incoming.try_iter()).collect();
        if messages.iter().any(|m| matches!(m, CommandMessage::Stop)) {
            return Ok(());
        }
        // Confirm native intent before processing a rail-requested switch. Shell
        // queries and identity validation belong here, never in the UI callback.
        for message in &messages {
            if let CommandMessage::ObserveMinimize(observation) = message {
                let mut observation = *observation.clone();
                let current = winvd::get_current_desktop().and_then(|d| d.get_id());
                if observation.transition_serial == serial
                    && current
                        .as_ref()
                        .is_ok_and(|id| format!("{id:?}") == observation.desktop)
                    && platform::validate(&observation.window)
                {
                    observation.window.minimized = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::IsIconic(platform::hwnd(
                            observation.window.hwnd,
                        ))
                    }
                    .as_bool();
                    let after = winvd::get_current_desktop().and_then(|d| d.get_id());
                    if observation.window.minimized
                        && current.as_ref().ok() == after.as_ref().ok()
                        && platform::validate(&observation.window)
                    {
                        observation.tick = platform::tick();
                        emit(Wire::Minimized(observation))?;
                    }
                }
            }
        }
        // Collapse rapid explicit desktop requests to the latest intent.
        if let Some(command) = messages
            .iter()
            .rposition(|m| matches!(m, CommandMessage::Switch { .. }))
        {
            let command = messages.swap_remove(command);
            if let CommandMessage::Switch {
                guid,
                expected_current,
                expected_serial,
            } = command
            {
                let actual = winvd::get_current_desktop()
                    .and_then(|d| d.get_id())
                    .map_err(|e| format!("{e:?}"))?;
                if expected_serial == serial && expected_current == format!("{actual:?}") {
                    let target = parse_guid(&guid).ok_or("Invalid desktop identity")?;
                    winvd::switch_desktop(target)
                        .map_err(|e| format!("Switch request failed: {e:?}"))?;
                }
                rescan = true;
            }
        }
        if messages
            .iter()
            .any(|m| matches!(m, CommandMessage::Refresh))
        {
            rescan = true;
        }
        if rescan {
            candidates = diagnostic::candidates()?;
            last_scan = Instant::now();
        }
    }
}

pub fn start(experimental: bool) -> Sender<CommandMessage> {
    let (sender, commands) = crossbeam_channel::bounded(32);
    std::thread::spawn(move || supervise(commands, experimental));
    sender
}

fn supervise(commands: Receiver<CommandMessage>, mut experimental: bool) {
    use std::os::windows::process::CommandExt;
    let mut backoff = 1u64;
    loop {
        notify(Event::Status(
            "Reconnecting to Windows desktops…".into(),
            false,
        ));
        let result = (|| -> Result<bool, String> {
            platform::supported(experimental)?;
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            command.args(["--integration-worker", &std::process::id().to_string()]);
            if experimental {
                command.arg("--experimental-integration");
            }
            let mut child = OwnedChild(
                command
                    .creation_flags(0x08000000)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| e.to_string())?,
            );
            let _job = own_process(&child.0)?;
            let stdout = child.0.stdout.take().ok_or("Missing integration output")?;
            let mut input = child.0.stdin.take().ok_or("Missing integration input")?;
            let (output, frames) = crossbeam_channel::bounded(4);
            let reader = std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut bytes = Vec::new();
                    let result = reader
                        .by_ref()
                        .take(1024 * 1024 + 1)
                        .read_until(b'\n', &mut bytes);
                    if result.is_err() || bytes.is_empty() || bytes.len() > 1024 * 1024 {
                        let _ = output.send(Err(
                            "Integration process ended or exceeded its output limit".into(),
                        ));
                        break;
                    }
                    let message = serde_json::from_slice::<Wire>(&bytes)
                        .map_err(|e| format!("Invalid integration response: {e}"));
                    if output.send(message).is_err() {
                        break;
                    }
                }
            });
            let mut latest = Instant::now();
            let mut stale = false;
            let outcome = loop {
                let deadline = latest + Duration::from_secs(if stale { 10 } else { 2 });
                crossbeam_channel::select! {
                    recv(commands) -> command => match command {
                        Ok(CommandMessage::Stop) | Err(_) => break Ok(true),
                        Ok(CommandMessage::Retry { experimental: enabled }) => { experimental = enabled; break Ok(false); },
                        Ok(command) => {
                            let bytes = serde_json::to_vec(&command).map_err(|e| e.to_string())?;
                            input.write_all(&bytes).and_then(|_| input.write_all(b"\n")).and_then(|_| input.flush()).map_err(|e| e.to_string())?;
                        }
                    },
                    recv(frames) -> frame => match frame {
                        Ok(Ok(Wire::Frame(frame, received))) => { latest = Instant::now(); stale = false; backoff = 1; notify(Event::Frame(frame, received, experimental)); },
                        Ok(Ok(Wire::Minimized(observation))) => notify(Event::Minimized(observation)),
                        Ok(Ok(Wire::Failure(error))) | Ok(Err(error)) => break Err(error),
                        Err(_) => break Err("Integration output disconnected".into()),
                    },
                    default(deadline.saturating_duration_since(Instant::now())) => {
                        if stale { break Err("Desktop integration exceeded its ten-second watchdog".into()); }
                        stale = true;
                        notify(Event::Status("Desktop integration is not responding; controls are temporarily disabled.".into(), false));
                    }
                }
            };
            drop(child);
            drop(frames); // Release a reader waiting on the bounded queue.
            let _ = reader.join();
            outcome
        })();
        match result {
            Ok(true) => return,
            Ok(false) => continue,
            Err(error) => notify(Event::Status(error, false)),
        }
        match commands.recv_timeout(Duration::from_secs(backoff)) {
            Ok(CommandMessage::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return;
            }
            Ok(CommandMessage::Retry {
                experimental: enabled,
            }) => {
                experimental = enabled;
            }
            _ => {}
        }
        backoff = (backoff * 2).min(30);
    }
}
