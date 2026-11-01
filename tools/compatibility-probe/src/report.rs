use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const MINIMUM_BUILD: u32 = 26100;
pub const MINIMUM_REVISION: u32 = 2605;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct OsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub revision: u32,
}

impl OsVersion {
    /// This is an upstream minimum, never a product support certification.
    pub fn meets_adapter_minimum(self) -> bool {
        self.major == 10
            && self.minor == 0
            && (self.build > MINIMUM_BUILD
                || self.build == MINIMUM_BUILD && self.revision >= MINIMUM_REVISION)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Desktop {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Candidate {
    /// Transient observation only, not a persistent identity.
    pub hwnd: usize,
    pub process_id: u32,
    pub process_created: u64,
    pub minimized: bool,
    pub window_pinned: Option<bool>,
    pub application_pinned: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Sample {
    pub elapsed_ms: u64,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Observation {
    pub elapsed_ms: u64,
    pub kind: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdapterReport {
    pub current_desktop: String,
    pub desktops: Vec<Desktop>,
    pub samples: Vec<Sample>,
    pub events: Vec<Observation>,
    pub event_limit_reached: bool,
    pub reconciliation_interval_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unavailable,
    Observed,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub status: Status,
    pub os: OsVersion,
    pub adapter: String,
    pub explanation: String,
    pub observation: Option<AdapterReport>,
    pub remaining_release_gates: Vec<String>,
}

impl Report {
    pub fn new(os: OsVersion) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            status: Status::Unavailable,
            os,
            adapter: "winvd 0.0.49".into(),
            explanation: format!(
                "The adapter requires Windows 11 build {MINIMUM_BUILD}.{MINIMUM_REVISION} or newer. No shell COM calls were made."
            ),
            observation: None,
            remaining_release_gates: [
                "Select a serviced Windows build and validate its exact revision.",
                "Verify desktop GUIDs, names, switch requests and notifications in four desktops.",
                "Measure idle pin/unpin discovery for both window and application pinning (<= 2 seconds).",
                "Verify AppBar presence across desktops, DPI, fullscreen and Explorer recovery.",
                "Validate duplicate-window identity across utility restart and HWND reuse.",
                "Validate focus, active owned dialogs and unresponsive targets.",
                "Implement and test the rail/controller after compatibility gates pass.",
                "Measure release CPU, private working set and switch latency; test physical hardware.",
            ]
            .map(String::from)
            .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_boundary_is_exact_and_not_windows_10_or_unknown_major() {
        let version = |major, build, revision| OsVersion {
            major,
            minor: 0,
            build,
            revision,
        };
        assert!(!version(10, 22621, 6060).meets_adapter_minimum());
        assert!(!version(10, 26100, 2604).meets_adapter_minimum());
        assert!(version(10, 26100, 2605).meets_adapter_minimum());
        assert!(version(10, 26200, 1).meets_adapter_minimum());
        assert!(!version(11, 26200, 1).meets_adapter_minimum());
        assert!(!version(10, 19045, 9999).meets_adapter_minimum());
    }

    #[test]
    fn unavailable_report_roundtrips_and_does_not_claim_product_readiness() {
        let report = Report::new(OsVersion {
            major: 10,
            minor: 0,
            build: 22621,
            revision: 6060,
        });
        let encoded = serde_json::to_vec(&report).unwrap();
        let decoded: Report = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.schema_version, SCHEMA_VERSION);
        assert_eq!(decoded.status, Status::Unavailable);
        assert!(decoded.observation.is_none());
        assert!(!decoded.remaining_release_gates.is_empty());
    }
}
