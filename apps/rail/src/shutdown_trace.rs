//! Temporary [DEBUG-exit] probe. Buffer timings; write only after UI teardown.
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static ORIGIN: OnceLock<Instant> = OnceLock::new();
static EVENTS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

pub fn measure<T>(phase: &'static str, work: impl FnOnce() -> T) -> T {
    let origin = *ORIGIN.get_or_init(Instant::now);
    let start = Instant::now();
    let result = work();
    let end = Instant::now();
    if let Ok(mut events) = EVENTS.lock()
        && events.len() < 64
    {
        events.push(serde_json::json!({
            "phase": phase,
            "start_ms": start.duration_since(origin).as_secs_f64() * 1000.0,
            "duration_ms": end.duration_since(start).as_secs_f64() * 1000.0,
        }));
    }
    result
}

pub fn flush() {
    if let Ok(events) = EVENTS.lock() {
        let report = serde_json::json!({
            "probe": "DEBUG-exit", "build": crate::version::info(),
            "phases": *events,
        });
        if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
            let _ = std::fs::write(
                std::env::temp_dir().join("VirtualDesktopRail-shutdown.json"),
                bytes,
            );
        }
    }
}
