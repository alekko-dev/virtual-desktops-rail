//! Persisted per-identity memory and preferences (schema 1).
use crate::Layout;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub identity: crate::Identity,
    pub last_usable: Layout,
    pub layouts: BTreeMap<String, Layout>,
    /// User intent, independent of Rail's automatic switch minimization.
    #[serde(default)]
    pub minimized_desktops: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Saved {
    pub schema_version: u32,
    pub paused: bool,
    pub startup: bool,
    #[serde(default)]
    pub experimental_integration: bool,
    #[serde(default)]
    pub auto_restore: bool,
    #[serde(default)]
    pub preferences_revision: u64,
    pub windows: BTreeMap<String, Record>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            schema_version: 1,
            paused: false,
            startup: false,
            experimental_integration: false,
            auto_restore: false,
            preferences_revision: 0,
            windows: BTreeMap::new(),
        }
    }
}

impl Saved {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Configuration exceeds 4 MiB".into());
        }
        let saved: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if saved.schema_version != 1
            || saved.windows.len() > 512
            || saved.windows.iter().any(|(key, record)| {
                *key != record.identity.key()
                    || record
                        .layouts
                        .keys()
                        .chain(record.minimized_desktops.iter())
                        .collect::<BTreeSet<_>>()
                        .len()
                        > 64
                    || !valid_layout(&record.last_usable)
                    || record.layouts.values().any(|layout| !valid_layout(layout))
            })
        {
            return Err("Unsupported or invalid configuration schema/limits".into());
        }
        Ok(saved)
    }

    pub(crate) fn bump_preferences(&mut self) {
        self.preferences_revision = self.preferences_revision.saturating_add(1);
    }
}

fn valid_layout(layout: &Layout) -> bool {
    [layout.normal, layout.monitor_rect, layout.work_area]
        .into_iter()
        .chain(layout.visible)
        .all(|rect| {
            rect.iter().all(|value| value.unsigned_abs() <= 1_000_000)
                && rect[2] > rect[0]
                && rect[3] > rect[1]
        })
        && (48..=768).contains(&layout.dpi)
}
