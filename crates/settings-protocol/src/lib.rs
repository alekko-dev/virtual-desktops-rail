//! Private, versioned, bounded pipe protocol. Only the resident rail writes preferences.
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

pub const VERSION: u32 = 1;
pub const MAX_BYTES: usize = 16384;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u32,
    pub acknowledged: u64,
    pub loaded: bool,
    pub paused: bool,
    pub startup: bool,
    pub startup_busy: bool,
    pub report_busy: bool,
    pub auto_restore: bool,
    pub minimize_snapped_elsewhere: bool,
    pub desktop_tint: bool,
    pub show_desktop_name: bool,
    pub light_switch_target: rail_core::LightSwitchTarget,
    pub status: String,
    pub notice: String,
    pub build: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Ready,
    Pause {
        enabled: bool,
    },
    Startup {
        enabled: bool,
    },
    AutoRestore {
        enabled: bool,
    },
    MinimizeSnappedElsewhere {
        enabled: bool,
    },
    DesktopTint {
        enabled: bool,
    },
    ShowDesktopName {
        enabled: bool,
    },
    LightSwitchTarget {
        target: rail_core::LightSwitchTarget,
    },
    Retry,
    Report {
        destination: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: u64,
    pub action: Action,
}

pub fn read<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> Result<T, String> {
    let mut bytes = Vec::new();
    let count = std::io::Read::take(&mut *reader, (MAX_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if count == 0 || count > MAX_BYTES || bytes.last() != Some(&b'\n') {
        return Err("Settings connection closed or exceeded its message limit.".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub fn write<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() >= MAX_BYTES {
        return Err("Settings message too large.".into());
    }
    writer
        .write_all(&bytes)
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_rejects_truncated_oversized_and_unknown_requests() {
        for bytes in [
            b"{}".to_vec(),
            vec![b'x'; MAX_BYTES + 1],
            b"{\"version\":1,\"action\":{\"action\":\"exit\"}}\n".to_vec(),
        ] {
            assert!(read::<Request>(&mut bytes.as_slice()).is_err());
        }
    }
    #[test]
    fn requests_round_trip_without_consuming_next_message() {
        let mut bytes = Vec::new();
        for enabled in [true, false] {
            write(
                &mut bytes,
                &Request {
                    version: VERSION,
                    id: 1,
                    action: Action::Pause { enabled },
                },
            )
            .unwrap();
        }
        let mut input = bytes.as_slice();
        for enabled in [true, false] {
            assert_eq!(
                read::<Request>(&mut input).unwrap().action,
                Action::Pause { enabled }
            );
        }
    }
}
