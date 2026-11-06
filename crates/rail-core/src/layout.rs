//! Identity, layout and snapshot data shared by the controller and native adapters.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Identity {
    pub token: String,
    pub process_created: u64,
    pub executable: String,
    pub session_id: u32,
}

impl Identity {
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.token,
            self.process_created,
            self.session_id,
            self.executable.to_lowercase()
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Layout {
    /// WINDOWPLACEMENT workspace coordinates, never silently treated as screen coordinates.
    pub normal: [i32; 4],
    pub maximized: bool,
    /// Screen bounds, absent for minimized parking rectangles and legacy records.
    #[serde(default)]
    pub visible: Option<[i32; 4]>,
    /// None means arranged-state detection was unavailable or the record predates it.
    #[serde(default)]
    pub arranged: Option<bool>,
    pub dpi: u32,
    pub monitor: String,
    pub monitor_rect: [i32; 4],
    pub work_area: [i32; 4],
}

impl Layout {
    pub(crate) fn reachable(&self) -> bool {
        let x = self.work_area[0] - self.monitor_rect[0];
        let y = self.work_area[1] - self.monitor_rect[1];
        self.normal[2].saturating_add(x) > self.work_area[0]
            && self.normal[0].saturating_add(x) < self.work_area[2]
            && self.normal[3].saturating_add(y) > self.work_area[1]
            && self.normal[1].saturating_add(y) < self.work_area[3]
    }
    /// Convert saved workspace coordinates to a reachable workspace rectangle on a current monitor.
    pub fn adjusted(
        &self,
        monitor: &str,
        monitor_rect: [i32; 4],
        work: [i32; 4],
        dpi: u32,
    ) -> Self {
        let scale = |value: i32| {
            (i64::from(value) * i64::from(dpi.max(1)) / i64::from(self.dpi.max(1))) as i32
        };
        let old_offset = [
            self.work_area[0] - self.monitor_rect[0],
            self.work_area[1] - self.monitor_rect[1],
        ];
        let new_offset = [work[0] - monitor_rect[0], work[1] - monitor_rect[1]];
        let width = scale(self.normal[2].saturating_sub(self.normal[0]))
            .clamp(100, (work[2] - work[0]).max(100));
        let height = scale(self.normal[3].saturating_sub(self.normal[1]))
            .clamp(80, (work[3] - work[1]).max(80));
        let left_screen = work[0] + scale(self.normal[0] + old_offset[0] - self.work_area[0]);
        let top_screen = work[1] + scale(self.normal[1] + old_offset[1] - self.work_area[1]);
        let left = left_screen.clamp(work[0], (work[2] - width).max(work[0])) - new_offset[0];
        let top = top_screen.clamp(work[1], (work[3] - height).max(work[1])) - new_offset[1];
        Self {
            normal: [left, top, left + width, top + height],
            maximized: self.maximized,
            visible: if self.monitor == monitor
                && self.monitor_rect == monitor_rect
                && self.work_area == work
                && self.dpi == dpi
            {
                self.visible
            } else {
                None
            },
            arranged: if self.monitor == monitor
                && self.monitor_rect == monitor_rect
                && self.work_area == work
                && self.dpi == dpi
            {
                self.arranged
            } else {
                None
            },
            dpi,
            monitor: monitor.into(),
            monitor_rect,
            work_area: work,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Desktop {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Window {
    pub identity: Identity,
    pub hwnd: usize,
    pub pid: u32,
    pub minimized: bool,
    pub responsive: bool,
    pub layout: Layout,
    pub popup: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub desktops: Vec<Desktop>,
    pub current: String,
    pub windows: Vec<Window>,
    pub tick: u64,
    pub transition_serial: u64,
}
