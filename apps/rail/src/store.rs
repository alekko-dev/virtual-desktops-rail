use crate::{Event, notify};
use crossbeam_channel::Sender;
use rail_core::Saved;
use std::io::Read;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IPersistFile,
};
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Startup, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    ShellLink,
};
use windows::core::{Interface, PCWSTR};

pub enum Work {
    Save(Saved),
    Export(String, serde_json::Value),
    Trace(serde_json::Value),
    Report(ReportRequest),
    Log(String),
}
pub struct ReportWindow {
    pub label: String,
    pub window: rail_core::Window,
    pub cached_layout: rail_core::Layout,
    pub last_usable_layout: Option<rail_core::Layout>,
    pub saved_layouts: serde_json::Value,
    pub minimized_desktops: Vec<String>,
    pub pending: bool,
    pub blocked: bool,
}
pub struct ReportRequest {
    pub destination: PathBuf,
    pub context: serde_json::Value,
    pub windows: Vec<ReportWindow>,
}
pub struct Storage {
    pub sender: Sender<Work>,
    pub stop: Sender<Shutdown>,
    pub diagnostic_session: String,
}
pub type WriteAck = Sender<Result<(), String>>;
pub type Shutdown = (Option<Saved>, WriteAck);

fn known_folder(id: &windows::core::GUID) -> Result<PathBuf, String> {
    let pointer =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(|e| e.to_string())?;
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe { pointer.as_wide() }));
    unsafe {
        CoTaskMemFree(Some(pointer.0 as *const _));
    }
    Ok(path)
}

fn read(path: &Path) -> Result<Saved, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Saved::decode(&bytes)
}

fn write(
    path: &Path,
    saved: &Saved,
    session: u32,
    preferences_seen: &mut u64,
) -> Result<(), String> {
    let _guard = crate::singleton::coordination("State")?;
    let previous = read(path).ok();
    let mut merged = previous
        .clone()
        .or_else(|| read(&path.with_file_name("state.previous.json")).ok())
        .unwrap_or_default();
    for (key, record) in &saved.windows {
        if record.identity.session_id == session || !merged.windows.contains_key(key) {
            merged.windows.insert(key.clone(), record.clone());
        }
    }
    if saved.preferences_revision != *preferences_seen {
        merged.paused = saved.paused;
        merged.startup = saved.startup;
        merged.experimental_integration = saved.experimental_integration;
        merged.auto_restore = saved.auto_restore;
        merged.desktop_tint = saved.desktop_tint;
        merged.minimize_snapped_elsewhere = saved.minimize_snapped_elsewhere;
        merged.preferences_revision = merged.preferences_revision.saturating_add(1);
    }
    let bytes = serde_json::to_vec_pretty(&merged).map_err(|e| e.to_string())?;
    Saved::decode(&bytes)?;
    if let Some(previous) = previous {
        let previous = serde_json::to_vec_pretty(&previous).map_err(|e| e.to_string())?;
        rail_compatibility_probe::storage::replace(
            &path.with_file_name("state.previous.json"),
            &previous,
        )
        .map_err(|e| e.to_string())?;
    }
    rail_compatibility_probe::storage::replace(path, &bytes).map_err(|e| e.to_string())?;
    *preferences_seen = saved.preferences_revision;
    Ok(())
}

fn load(path: &Path) -> (Saved, Option<String>, bool) {
    let _guard = match crate::singleton::coordination("State") {
        Ok(guard) => guard,
        Err(error) => return (Saved::default(), Some(error), false),
    };
    if !path.exists() {
        return match read(&path.with_file_name("state.previous.json")) {
            Ok(saved) => (
                saved,
                Some("Recovered the previous configuration.".into()),
                true,
            ),
            Err(_) => (Saved::default(), None, true),
        };
    }
    match read(path) {
        Ok(saved) => (saved, None, true),
        Err(error) => {
            let saved = read(&path.with_file_name("state.previous.json")).unwrap_or_default();
            let nonce = windows::core::GUID::new()
                .map(|id| format!("{id:?}"))
                .unwrap_or_else(|_| std::process::id().to_string());
            let preserved = path.with_file_name(format!("state.corrupt-{nonce}.json"));
            match std::fs::rename(path, &preserved) {
                Ok(()) => (
                    saved,
                    Some(format!(
                        "Recovered layout memory where possible; the unreadable file was preserved as {}. {error}",
                        preserved.display()
                    )),
                    true,
                ),
                Err(_) => (
                    saved,
                    Some(format!(
                        "Saved layout memory is inaccessible; this run uses memory only and will preserve the original file. {error}"
                    )),
                    false,
                ),
            }
        }
    }
}

fn log(path: &Path, message: &str) -> Result<(), String> {
    let _guard = crate::singleton::coordination("Diagnostics")?;
    let path = path.with_file_name("diagnostics.log");
    let mut bytes = Vec::new();
    if let Ok(file) = std::fs::File::open(&path) {
        let _ = file.take(65536).read_to_end(&mut bytes);
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    text.push_str(&format!(
        "{timestamp} {}\r\n",
        message
            .replace(['\r', '\n'], " ")
            .chars()
            .take(2048)
            .collect::<String>()
    ));
    if text.len() > 65536 {
        let start = text.len() - 65536;
        let start = text.as_bytes()[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| start + i + 1)
            .unwrap_or(text.len());
        text = text[start..].to_owned();
    }
    rail_compatibility_probe::storage::replace(&path, text.as_bytes()).map_err(|e| e.to_string())
}

fn trace(path: &Path, value: &serde_json::Value, session: &str) -> Result<(), String> {
    let _guard = crate::singleton::coordination("Diagnostics")?;
    let path = path.with_file_name("diagnostics-events.jsonl");
    let mut bytes = Vec::new();
    if let Ok(file) = std::fs::File::open(&path) {
        let _ = file.take(131072).read_to_end(&mut bytes);
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    let mut entry = value.clone();
    if let Some(object) = entry.as_object_mut() {
        object.insert("run".into(), serde_json::json!(session));
        object.insert(
            "time_unix_ms".into(),
            serde_json::json!(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
        );
    }
    text.push_str(&serde_json::to_string(&entry).map_err(|e| e.to_string())?);
    text.push('\n');
    if text.len() > 131072 {
        let start = text.len() - 131072;
        let start = text.as_bytes()[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| start + i + 1)
            .unwrap_or(text.len());
        text = text[start..].to_owned();
    }
    rail_compatibility_probe::storage::replace(&path, text.as_bytes()).map_err(|e| e.to_string())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let name = name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| "Report entry name is too long")?;
        let size = u32::try_from(data.len()).map_err(|_| "Report is too large")?;
        let offset = u32::try_from(out.len()).map_err(|_| "Report is too large")?;
        let crc = crc32(data);
        out.extend_from_slice(&0x04034b50_u32.to_le_bytes());
        out.extend_from_slice(&20_u16.to_le_bytes());
        out.extend_from_slice(&0x0800_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        central.extend_from_slice(&0x02014b50_u32.to_le_bytes());
        central.extend_from_slice(&20_u16.to_le_bytes());
        central.extend_from_slice(&20_u16.to_le_bytes());
        central.extend_from_slice(&0x0800_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u16.to_le_bytes());
        central.extend_from_slice(&0_u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let count = u16::try_from(entries.len()).map_err(|_| "Too many report entries")?;
    let central_offset = u32::try_from(out.len()).map_err(|_| "Report is too large")?;
    let central_size = u32::try_from(central.len()).map_err(|_| "Report is too large")?;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50_u32.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    Ok(out)
}

fn create_report(request: ReportRequest, state_path: Option<&Path>) -> Result<PathBuf, String> {
    let mut context = request.context;
    let windows = request
        .windows
        .into_iter()
        .map(|item| {
            let native = if crate::platform::validate(&item.window) {
                crate::platform::layout(crate::platform::hwnd(item.window.hwnd))
                    .map(|layout| {
                        let handle = crate::platform::hwnd(item.window.hwnd);
                        let visible = unsafe {
                            windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(handle)
                                .as_bool()
                        };
                        let minimized = unsafe {
                            windows::Win32::UI::WindowsAndMessaging::IsIconic(handle).as_bool()
                        };
                        let mut cloaked = 0_i32;
                        let cloak_query = unsafe {
                            windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                                handle,
                                windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(14),
                                &mut cloaked as *mut _ as *mut _,
                                std::mem::size_of::<i32>() as u32,
                            )
                        };
                        serde_json::json!({"available":true,"layout":layout,"visible":visible,"minimized":minimized,"cloaked":cloak_query.is_ok().then_some(cloaked != 0)})
                    })
                    .unwrap_or_else(|_| serde_json::json!({"available":false}))
            } else {
                serde_json::json!({"available":false,"reason":"identity_changed"})
            };
            let executable = std::path::Path::new(&item.window.identity.executable)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");
            let executable = executable
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
                })
                .take(128)
                .collect::<String>();
            serde_json::json!({
                "label":item.label,
                "application_executable":executable,
                "responsive":item.window.responsive,
                "cached_minimized":item.window.minimized,
                "cached_model_layout":item.cached_layout,
                "last_usable_fallback":item.last_usable_layout,
                "pending":item.pending,
                "blocked":item.blocked,
                "fresh_native_observation":native,
                "saved_layouts":item.saved_layouts,
                "minimized_desktops":item.minimized_desktops
            })
        })
        .collect::<Vec<_>>();
    if let Some(object) = context.as_object_mut() {
        object.insert("windows".into(), serde_json::json!(windows));
        object.insert(
            "windows_version".into(),
            serde_json::json!(rail_compatibility_probe::platform::os_version().ok()),
        );
    }
    let mut events = String::new();
    let mut event_history_available = false;
    if let Some(path) = state_path {
        let path = path.with_file_name("diagnostics-events.jsonl");
        if let Ok(file) = std::fs::File::open(path) {
            event_history_available = true;
            let _ = file.take(131072).read_to_string(&mut events);
        }
    }
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "event_history".into(),
            serde_json::json!({"available":event_history_available,"bytes":events.len(),"bounded_bytes":131072,"contains_tail":true}),
        );
        object.insert(
            "observation_note".into(),
            serde_json::json!("Fresh native window observations are gathered sequentially after the UI snapshot and may not represent one atomic instant."),
        );
    }
    let report = serde_json::to_vec_pretty(&context).map_err(|e| e.to_string())?;
    let readme = b"Virtual Desktop Rail diagnostic report\r\n\r\nThe report includes application/build and Windows version context, settings, desktop counts, fresh native observations for still-matching managed windows, saved geometry/state and recent redacted event history. Window titles, document contents, window handles, process IDs, full executable paths, desktop names/IDs and screenshots are excluded. Windows does not expose native Snap Group membership to the app, so this report cannot include or verify group membership. No data is uploaded. Review the files before sharing.\r\n";
    let archive = zip(&[
        ("README.txt", readme),
        ("report.json", &report),
        ("events.jsonl", events.as_bytes()),
    ])?;
    rail_compatibility_probe::storage::replace(&request.destination, &archive)
        .map_err(|error| error.to_string())?;
    Ok(request.destination)
}

pub fn export_path(nonce: &str) -> Result<PathBuf, String> {
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid diagnostic request identity".into());
    }
    Ok(std::env::temp_dir().join(format!("VirtualDesktopRail-{nonce}.json")))
}

pub fn start() -> Storage {
    let diagnostic_session = windows::core::GUID::new()
        .map(|id| format!("{id:?}"))
        .unwrap_or_else(|_| format!("run-{}", std::process::id()));
    let trace_session = diagnostic_session.clone();
    let (sender, incoming) = crossbeam_channel::bounded(4);
    let (stop, shutdown) =
        crossbeam_channel::bounded::<(Option<Saved>, Sender<Result<(), String>>)>(1);
    std::thread::spawn(move || {
        let root = known_folder(&FOLDERID_LocalAppData).map(|path| path.join("VirtualDesktopRail"));
        let (mut path, mut initial_warning) = match root.and_then(|root| {
            std::fs::create_dir_all(&root)
                .map(|_| root.join("state.json"))
                .map_err(|e| e.to_string())
        }) {
            Ok(path) => (Some(path), None),
            Err(error) => (
                None,
                Some(format!(
                    "Saved layout storage is unavailable; this run uses memory only. {error}"
                )),
            ),
        };
        let (saved, warning, writable) = if let Some(path) = &path {
            load(path)
        } else {
            (Saved::default(), initial_warning.take(), false)
        };
        if !writable {
            path = None;
        }
        let session = rail_compatibility_probe::platform::process_context(std::process::id())
            .map(|context| context.1)
            .unwrap_or(0);
        let mut preferences_seen = saved.preferences_revision;
        notify(Event::Loaded {
            saved,
            path: path.clone().unwrap_or_default(),
            warning,
        });
        let mut pending = None;
        let mut first = None;
        let mut due = Instant::now();
        let mut retry_after = Instant::now();
        loop {
            let timer = if pending.is_some() {
                crossbeam_channel::after(due.saturating_duration_since(Instant::now()))
            } else {
                crossbeam_channel::never()
            };
            crossbeam_channel::select_biased! {
                recv(shutdown) -> message => {
                    if let Ok((saved,ack)) = message { let result = if let Some(saved)=saved{path.as_ref().ok_or_else(||"Layout storage is unavailable".to_string()).and_then(|path|write(path,&saved,session,&mut preferences_seen))}else{Ok(())}; let _ = ack.send(result); }
                    return;
                },
                recv(timer) -> _ => {
                    if let Some(saved) = pending.take() {
                        let result=path.as_ref().ok_or_else(||"Layout storage is unavailable".to_string()).and_then(|path|write(path,&saved,session,&mut preferences_seen));
                        if result.is_err(){pending=Some(saved);retry_after=Instant::now()+Duration::from_secs(5);due=retry_after;}
                        notify(Event::Stored(result));
                    }
                    first = None;
                },
                recv(incoming) -> message => match message {
                    Ok(Work::Save(saved)) => {
                        let now = Instant::now();
                        let first = *first.get_or_insert(now);
                        due = (now + Duration::from_millis(300)).min(first + Duration::from_secs(2)).max(retry_after);
                        pending = Some(saved);
                    }
                    Ok(Work::Log(message))=>{if let Some(path)=&path{let _=log(path,&message);}}
                    Ok(Work::Trace(value))=>{if let Some(path)=&path{let _=trace(path,&value,&trace_session);}}
                    Ok(Work::Report(request))=>{
                        let result=create_report(request,path.as_deref());
                        notify(Event::Report(result));
                    }
                    Ok(Work::Export(nonce,value)) => {
                        let result = export_path(&nonce).and_then(|path| serde_json::to_vec_pretty(&value).map_err(|e| e.to_string()).and_then(|bytes|
                            rail_compatibility_probe::storage::replace(&path,&bytes).map_err(|e| e.to_string())));
                        if let Err(error) = result { notify(Event::Notice(error)); }
                    }
                    Err(_) => return,
                }
            }
        }
    });
    Storage {
        sender,
        stop,
        diagnostic_session,
    }
}

pub fn startup_worker() -> Sender<bool> {
    let (sender, incoming) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        for enabled in incoming {
            notify(Event::Startup(enabled, startup(enabled)));
        }
    });
    sender
}

fn startup(enabled: bool) -> Result<(), String> {
    let _guard = crate::singleton::coordination("Startup")?;
    let path = known_folder(&FOLDERID_Startup)?.join("VirtualDesktopRail.lnk");
    startup_at(&path, enabled)
}
fn startup_at(path: &Path, enabled: bool) -> Result<(), String> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|e| e.to_string())?;
    let result = (|| {
        if !enabled {
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| e.to_string())?;
        let wide = |path: &Path| {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        unsafe { link.SetPath(PCWSTR(wide(&executable).as_ptr())) }.map_err(|e| e.to_string())?;
        let persist: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        unsafe { persist.Save(PCWSTR(wide(path).as_ptr()), true) }.map_err(|e| e.to_string())?;
        Ok(())
    })();
    unsafe {
        CoUninitialize();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rail_core::{Identity, Layout, Record};
    use std::collections::BTreeMap;
    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rail-store-test-{:?}",
                windows::core::GUID::new().unwrap()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn state(&self) -> PathBuf {
            self.0.join("state.json")
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            for entry in std::fs::read_dir(&self.0).unwrap().flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    fn record(token: &str, session: u32) -> Record {
        Record {
            identity: Identity {
                token: token.into(),
                process_created: 100,
                executable: "fixture.exe".into(),
                session_id: session,
            },
            last_usable: Layout {
                normal: [100, 100, 600, 500],
                maximized: false,
                visible: None,
                arranged: None,
                dpi: 96,
                monitor: "test".into(),
                monitor_rect: [0, 0, 1920, 1080],
                work_area: [25, 0, 1920, 1040],
            },
            layouts: BTreeMap::new(),
            minimized_desktops: Default::default(),
        }
    }
    #[test]
    fn saves_merge_other_sessions_without_overwriting_their_newer_layouts_or_preferences() {
        let directory = Temporary::new();
        let path = directory.state();
        let first = record("one", 7);
        let first_key = first.identity.key();
        let mut a = Saved {
            paused: true,
            preferences_revision: 1,
            ..Saved::default()
        };
        a.windows.insert(first_key.clone(), first);
        let mut seen_a = 0;
        write(&path, &a, 7, &mut seen_a).unwrap();
        let mut b = read(&path).unwrap();
        let mut seen_b = b.preferences_revision;
        let second = record("two", 8);
        let second_key = second.identity.key();
        b.windows.insert(second_key.clone(), second);
        a.paused = false;
        a.experimental_integration = true;
        a.auto_restore = true;
        a.desktop_tint = true;
        a.minimize_snapped_elsewhere = false;
        a.preferences_revision += 1;
        a.windows.get_mut(&first_key).unwrap().last_usable.normal = [250, 100, 750, 500];
        a.windows
            .get_mut(&first_key)
            .unwrap()
            .minimized_desktops
            .insert("desktop-a".into());
        b.windows
            .get_mut(&second_key)
            .unwrap()
            .minimized_desktops
            .insert("desktop-b".into());
        write(&path, &a, 7, &mut seen_a).unwrap();
        write(&path, &b, 8, &mut seen_b).unwrap();
        let merged = read(&path).unwrap();
        assert!(!merged.paused);
        assert!(merged.experimental_integration);
        assert!(merged.auto_restore);
        assert!(merged.desktop_tint);
        assert!(!merged.minimize_snapped_elsewhere);
        assert_eq!(merged.windows[&first_key].last_usable.normal[0], 250);
        assert!(merged.windows.contains_key(&second_key));
        assert!(
            merged.windows[&first_key]
                .minimized_desktops
                .contains("desktop-a")
        );
        assert!(
            merged.windows[&second_key]
                .minimized_desktops
                .contains("desktop-b")
        );
        b.paused = true;
        b.experimental_integration = false;
        b.auto_restore = false;
        b.desktop_tint = false;
        b.minimize_snapped_elsewhere = true;
        b.preferences_revision += 1;
        write(&path, &b, 8, &mut seen_b).unwrap();
        write(&path, &a, 7, &mut seen_a).unwrap();
        assert!(read(&path).unwrap().paused);
        assert!(!read(&path).unwrap().experimental_integration);
        assert!(!read(&path).unwrap().auto_restore);
        assert!(!read(&path).unwrap().desktop_tint);
        assert!(read(&path).unwrap().minimize_snapped_elsewhere);
    }
    #[test]
    fn corruption_recovers_previous_state_and_keeps_the_unreadable_bytes() {
        let directory = Temporary::new();
        let path = directory.state();
        let mut seen = 0;
        let mut saved = Saved {
            paused: true,
            preferences_revision: 1,
            ..Saved::default()
        };
        write(&path, &saved, 2, &mut seen).unwrap();
        saved.paused = false;
        saved.preferences_revision += 1;
        write(&path, &saved, 2, &mut seen).unwrap();
        std::fs::write(&path, b"{").unwrap();
        let (recovered, warning, writable) = load(&path);
        assert!(recovered.paused && writable && warning.is_some());
        assert!(!path.exists());
        let corrupt = std::fs::read_dir(&directory.0)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("state.corrupt-")
            })
            .unwrap();
        assert_eq!(std::fs::read(corrupt.path()).unwrap(), b"{");
        let mut seen = recovered.preferences_revision;
        write(&path, &recovered, 2, &mut seen).unwrap();
        assert!(read(&path).unwrap().paused);
    }
    #[test]
    fn invalid_geometry_cannot_replace_the_last_good_state() {
        let directory = Temporary::new();
        let path = directory.state();
        let mut seen = 0;
        let mut saved = Saved::default();
        write(&path, &saved, 2, &mut seen).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut broken = record("broken", 2);
        broken.last_usable.dpi = 0;
        saved.windows.insert(broken.identity.key(), broken);
        assert!(write(&path, &saved, 2, &mut seen).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn diagnostics_are_bounded_valid_utf8_and_request_paths_reject_traversal() {
        let directory = Temporary::new();
        for _ in 0..12 {
            log(&directory.state(), &"𝄞".repeat(3000)).unwrap();
        }
        let bytes = std::fs::read(directory.0.join("diagnostics.log")).unwrap();
        assert!(bytes.len() <= 65536);
        assert!(String::from_utf8(bytes).is_ok());
        for nonce in ["../state", "", &"z".repeat(32)] {
            assert!(export_path(nonce).is_err());
        }
    }
    #[test]
    fn diagnostic_zip_uses_valid_stored_entries_and_crc32() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let archive = zip(&[("report.json", b"{}"), ("events.jsonl", b"")]).unwrap();
        assert_eq!(&archive[..4], &0x04034b50_u32.to_le_bytes());
        assert_eq!(
            &archive[archive.len() - 22..archive.len() - 18],
            &0x06054b50_u32.to_le_bytes()
        );
        assert_eq!(
            &archive[archive.len() - 14..archive.len() - 10],
            &[2, 0, 2, 0]
        );
    }
    #[test]
    fn support_report_contains_redacted_state_and_never_copies_the_free_form_log() {
        let directory = Temporary::new();
        let state_path = directory.state();
        std::fs::write(directory.0.join("diagnostics.log"), "private-path-marker").unwrap();
        trace(
            &state_path,
            &serde_json::json!({"event":"desktop_transition_observed","from":"desktop-001","to":"desktop-002"}),
            "run-test",
        )
        .unwrap();
        let destination = directory.0.join("support.zip");
        let mut fixture = record("private-path-marker", 7);
        fixture.identity.executable = "C:\\private-path-marker\\fixture.exe".into();
        create_report(
            ReportRequest {
                destination: destination.clone(),
                context: serde_json::json!({"diagnostic_run":"run-test","desktops":[]}),
                windows: vec![ReportWindow {
                    label: "window-001".into(),
                    window: rail_core::Window {
                        identity: fixture.identity,
                        hwnd: 0,
                        pid: 0,
                        minimized: true,
                        responsive: true,
                        layout: fixture.last_usable.clone(),
                        popup: None,
                        z_above: Vec::new(),
                    },
                    cached_layout: fixture.last_usable,
                    last_usable_layout: None,
                    saved_layouts: serde_json::json!({}),
                    minimized_desktops: vec!["desktop-001".into()],
                    pending: false,
                    blocked: false,
                }],
            },
            Some(&state_path),
        )
        .unwrap();
        let archive = std::fs::read(destination).unwrap();
        for member in [b"README.txt".as_slice(), b"report.json", b"events.jsonl"] {
            assert!(archive.windows(member.len()).any(|bytes| bytes == member));
        }
        assert!(archive.windows(8).any(|bytes| bytes == b"run-test"));
        assert!(archive.windows(9).any(|bytes| bytes == b"time_unix"));
        assert!(
            archive
                .windows(b"minimized_desktops".len())
                .any(|bytes| bytes == b"minimized_desktops")
        );
        assert!(
            archive
                .windows(b"fresh_native_observation".len())
                .any(|bytes| bytes == b"fresh_native_observation")
        );
        assert!(
            !archive
                .windows(b"private-path-marker".len())
                .any(|bytes| bytes == b"private-path-marker")
        );
    }
    #[test]
    fn startup_shortcut_can_be_saved_loaded_and_removed_in_an_isolated_directory() {
        let directory = Temporary::new();
        let path = directory.0.join("isolated.lnk");
        startup_at(&path, true).unwrap();
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .unwrap();
        {
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            let wide = crate::native::wide(path.to_str().unwrap());
            unsafe {
                persist.Load(
                    PCWSTR(wide.as_ptr()),
                    windows::Win32::System::Com::STGM_READ,
                )
            }
            .unwrap();
        }
        unsafe {
            CoUninitialize();
        }
        startup_at(&path, false).unwrap();
        assert!(!path.exists());
    }
}
