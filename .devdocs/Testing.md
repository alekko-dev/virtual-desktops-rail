# Testing and verification

## Verification layers

Run `./scripts/check-dev.ps1` for code changes: formatting, warnings-denied Clippy,
locked workspace tests and release builds. Pure controller tests use fake observations
to exercise races, handle reuse, geometry clamping, recovery and action ordering.
Storage/CLI/helper tests cover schema bounds, atomic replacement, corruption recovery,
session merging, isolated Startup links, subprocess crashes/timeouts and argument contracts.
Use meaningful regression scenarios; avoid asserting only implementation details.

Automated checks are necessary but cannot certify native appearance or interaction.
For UI and behavior changes, use computer use to inspect the actual running app and
exercise real pointer/keyboard input, then independently inspect foreground, placement,
visibility and integration state. A CLI `--click` passed while a real mouse-click focus
bug remained. Screenshots expose rendering defects that controller tests cannot.

Interactive tests run serially in the unlocked signed-in Explorer session, with owned
fixtures. Record OS build/revision, executable hash or source revision, privilege/session,
display/DPI, automation visibility and workload. Preserve original user windows/desktops
and settings. Stop only verified owned processes (PID plus executable and creation time).
Remove only known temporary desktops after a broad inventory confirms they are empty;
after ambiguous/time-out creation, report unexpected desktops instead of guessing ownership.
Real Explorer restart/session tests require a recorded recoverable checkpoint.

## Computer-use procedure

Read and follow the installed computer-use skill and its current API; do not copy
obsolete agent-specific automation code. Observe the app, inspect screenshots and
accessibility data, perform one meaningful input, then observe the result again.
Refresh targets after renders instead of retrying stale accessibility indexes.

The passive TOOLWINDOW rail is deliberately omitted by some automation inventories.
For real-button input checks, temporarily expose it using:

```powershell
./scripts/Invoke-Rail.ps1 --automation-ui on
# Select the observed rail window in the computer-use tool; exercise actual controls.
./scripts/Invoke-Rail.ps1 --automation-ui off
```

This nonpersistent mode changes discoverability while retaining no-activation behavior
and the same controls/controller. Restore it off even after failed tests. Check normal
passive style separately for cross-desktop presence, Task View exclusion and fullscreen.
Never construct an automation target from a diagnostic HWND when the tool cannot select it.
If the desktop locks or observation fails, stop input, report what remains unverified
and continue independent checks; a locked/RDP-disconnected run is not a UI pass.

## Interactive regression matrix

| Area | Essential scenarios |
| --- | --- |
| Rail rendering/input | Desktop colors and active shape, visibility indicators, settings layout, tooltips, real icon restore/focus/second-click minimize, keyboard pause/resume, unavailable warning, Done versus Exit |
| Desktop/pin integration | Four-desktop GUID/event identity; switches via rail and Windows; actual Task View window/app pin cycles; unpin/close removes entries; minimized windows stay eligible |
| Layouts | Move/resize A/B/A, normal/maximized state, fallback without memory, two identical windows, taskbar/external restoration with stable reachable geometry |
| Auto-restore | Mixed saved/unsaved windows, fresh desktop, passive normal/maximized restore preserving a separate foreground anchor, no memory rewrite, manual minimize retains memory |
| Ordering | Rapid switches including A→B→A, delayed minimize then restore click, partial dispatch, timeout quarantine, late completion, user drag, identity disappearance/reuse |
| Pause/restart | Pause across switches and rail restart; resume/setting changes do nothing immediately; rail restart preserves live duplicates; application restart gives fresh identities; second launch/settings addresses existing instance |
| Modal/fullscreen | Owner/dialog minimize/reveal/focus, stalled target with responsive rail, fullscreen entry/exit and restored topmost state in normal style |
| Recovery/display | Explicit retry, actual Explorer restart, sleep/resume, lock/unlock, session reconnect, primary-display changes, monitor disconnect/reconnect and mixed DPI |
| Accessibility/apps | Light/dark/high contrast, screen-reader and keyboard review, Obsidian disposable vault, other app types and privilege combinations |

The last two rows contain substantial unverified release work; see [Status](Status-and-Roadmap.md).
Fixtures do not substitute for real application workflows or physical display topology.

## Compatibility tools

Build first, then run relevant exercises serially, closing existing native fixtures
before owned-fixture runs. Each binary has `--help`:

```powershell
./target/release/rail-compatibility-probe.exe --output artifacts/compatibility.json
./target/release/rail-native-smoke.exe --windows 2 --delay-ms 1500
./target/release/rail-native-smoke.exe --probe
./target/release/rail-compatibility-exercise.exe --run-owned-fixture
./target/release/rail-compatibility-exercise.exe --run-pin-discovery
./target/release/rail-compatibility-exercise.exe --run-window-control
./target/release/rail-desktop-exercise.exe --run-desktop-fixture
./target/release/rail-desktop-exercise.exe --run-appbar-fixture
```

The read-only probe never switches desktops, changes pinning or controls windows.
`--observe-seconds 1..60` (default 2) collects GUIDs/names, event kinds and one-second
pin samples. Calls/listener teardown run in an owned subprocess with a duration-plus-ten-
second watchdog. Output, event queues and candidate counts are bounded; events may drop.
Reports contain desktop names, candidate classes and transient HWND/PID observations,
not titles or persistent identities. Schema 2 includes times/current desktop/event GUIDs.
`--output` atomically replaces a report in an existing directory.

Exit 0 means observations collected, **not compatibility passed**; 2 means unavailable;
1 means invocation/report I/O failure. No queryable candidates, a desktop change during
sampling or an event limit makes the report unavailable. Unsupported ApplicationViews
and query failures are distinguished as specified in [Architecture](Architecture.md).

The explicit exercise switches mutate only their owned fixtures. The owned-window
exercise tests window/app pins and non-pointer identity across helper processes, restores
pinning and stops its fixture. Keep `rail-native-smoke.exe` beside it. Individual shell
operations have ten-second subprocess watchdogs. Exit 0 means passed, 2 failed/unavailable,
1 invalid invocation.

Pin-discovery exercise polls two cached candidates once per second for about 42 seconds
and measures four API-triggered transitions with independent QPC timestamps. Measurements
include actor dispatch/IPC, not Task View click latency. Window-control exercise distinguishes
API acceptance, visible state, foreground and completion for a modal group and 1500 ms stall.
Its standard-dialog fallback is fixture-specific, not general discovery policy.

Desktop exercise creates three temporary desktops, names only those, verifies identity
and events, returns to the original, then removes only verified empty owned desktops.
AppBar exercise checks reservation, native maximization, DWM cloaking across desktops
and work-area restoration. `rail-native-smoke --appbar` runs its passive prototype alone;
close normally. A prototype pass is distinct from product-control verification.

For actual Task View pin origin:

```powershell
./target/release/rail-compatibility-exercise.exe --record-task-view > artifacts/compatibility-task-view.json
```

Wait for recorder readiness; through Task View, pin one fixture window, unpin it, pin
all fixture application windows, then unpin them. Leave at least two seconds between
changes. Recording lasts about two minutes; it does not mutate pins during observation.
Cleanup restores the initial state. Both cycles and operator confirmation of Task View
origin are required. Observed transitions do not timestamp the user's clicks.

## Resource and latency measurement

Provisional goals: under 20 MB private working set, effectively zero idle CPU, and an
automatic native action issued within 50 ms of desktop-event receipt. Pin discovery
has a provisional two-second idle-session bound. Goals require measurement.

For a warmed stable two-pinned-window workload, run `./scripts/Measure-RailIdle.ps1`.
It repeatedly checks inventory, geometry, visibility, desktop generation, pending work
and UI/worker lifetimes; changed workloads or unavailable integration reject the sample.
Include both processes. Distinguish private working set, total working set and private
bytes, record duration/counter resolution/host load, and avoid concurrent UI or cleanup.

`--status` exposes bounded `dispatch_latency` statistics in microseconds (count, max,
over-50-ms and latest 256 samples). Cross-process QPC measures worker desktop-event
receipt through snapshot/IPC/UI/action queues to accepted automatic minimization.
It excludes winvd notification-queue wait before receipt and app redraw; it does not
certify automatic-restore latency. Do not equate fixture script duration or API pin
discovery latency with product event-to-action latency.

## Consolidated validation evidence

These are historical observations, not a claim that the current binary passed all
cases. Raw JSON is local under ignored `artifacts/`; summaries remain useful without it.

| Date / environment | Findings and scope | Evidence references |
| --- | --- | --- |
| 2026-11-01, Windows 11 x64 26300.9457, session 2, medium integrity, 1708×960 / 120 DPI | Initial compatibility spike passed before product work: four desktops/GUID events; actual Task View window/app pin cycles; passive AppBar visible, uncloaked and nonforeground on four desktops; 31 physical-pixel reservation and restored work area; modal focus, bounded stalled actions, duplicate-token lifetime. Lower-integrity sandbox omitted medium windows; elevated support not established. | `compatibility-four-desktops.json`, `compatibility-task-view-final.json`, `compatibility-appbar.json`, `compatibility-window-control.json`, `compatibility-identity-lifetime.json`, `compatibility-privilege-final.json` |
| Same initial baseline | One-second cached pin checks observed four API changes in 795/626/480/333 ms including actor/IPC. Observer reported 0 ms CPU over 29 s at finite resolution; small fixture scope only. | `compatibility-pin-discovery.json` |
| 2026-11-01, same baseline, product | Exact A/B/A geometry, maximized state, modal focus, duplicate rail restart, fresh app-restart identities, pause, second instance, retry baseline, five-second target stall with two-second quarantine and responsive rail. Fullscreen recovery and real two-icon click/keyboard tests passed after fixes. Input used temporary automation visibility; desktop/fullscreen checks used normal style. | `rail-layout-roundtrip.json`, `rail-maximized-after.json`, `rail-modal-roundtrip.json`, `rail-duplicate-restart-check.json`, `rail-app-restart.json`, `rail-retry-recovery.json`, `rail-stall-live.json`, `rail-fullscreen-pass.jsonl`, `rail-cua-*.json` |
| 2026-11-01, final portable build, empty inventory | Combined private working set 3,993,600 bytes; 0 ms reported CPU over 30.007 s. Not literal zero CPU or a populated workload. | `rail-final-performance.json` |
| 2026-11-02, Windows 26300.9711, session 2, 2560×1440 / 120 DPI | API pin, identity, modal/stall, four-desktop/AppBar and normal-style minimization checks passed; real pointer restore/focus/second-click passed. Cached API pin transitions: 786/630/483/316 ms. New-revision Task View origin not established. Historical experimental opt-in used; that policy is now superseded. | `compatibility-owned-20261102.json`, `compatibility-control-20261102.json`, `compatibility-identity-20261102.json`, `compatibility-appbar-20261102.json`, `compatibility-discovery-20261102.json`, `rail-preview-click-*.json` |
| 2026-11-02, 9711, warmed stable two-window VM workload | Combined private working set 4,730,880 bytes; 62.5 ms CPU over 30.103 s with six stability checks. Includes diagnostic polling; finite counter resolution. Hash recorded in artifact predates subsequent icon-only edit. A first product dispatch observation was 72.635 ms, exceeding the 50 ms goal. | `rail-preview-populated-idle-20261102.json`; bounded dispatch metrics |
| 2026-11-02, 9711, optional auto-restore | Saved/unsaved destinations, new desktop, manual/external learning, normal/maximized restore, preserved foreground anchor and memory, pause/resume, real settings/rail input, persistence/retry and packaged executable passed. External restore used fixture `ShowWindowAsync(SW_RESTORE)`, not a literal taskbar click. | `auto-no-layout.json`, `auto-fresh-desktop-final.json`, `auto-external-saved-final.json`, `auto-focus-max-final.json`, `auto-paused-final.json`, `auto-startup-final.json`, `auto-recovery-final.json`, `auto-real-rail-restore-final.json`, `auto-packaged-*.json` |
| 2026-11-02, minimum-build policy change | Formatting, warnings-denied Clippy, 53 workspace tests and all release binaries passed in an alternate target directory. This validates policy code, not additional OS revisions interactively. | Historical build directory `artifacts/compatibility-policy-build` |

Auto-restore package verification identified executable SHA-256
`12959A9BB20FFB580947A09396522BEF36BDB6E627E461443D0F8209C5F19378`;
release/extracted/ZIP entries matched (`auto-package-verification.json`). Its 51-test
result predates the 53-test policy change and does not identify the current binary.

Do not reuse `rail-managed-idle-performance.json` as a populated-idle pass: its run was
interrupted and fixture cleanup overlapped. Earlier `rail-performance.json` also overlapped
interactive activity. The clean 2026-11-02 sample supersedes them. Neither observer CPU
nor virtual-machine performance certifies physical-machine behavior. Original incomplete
Task View recordings were incomplete sequences, not evidence of ABI failure.

## Evidence required for release

Keep a concise capability/build matrix with passed, failed, not-run and blocked results,
exact environment, binary identity and measurement limitations. Preserve important
raw artifacts with the release when available; missing local files must not erase
the checked-in finding. Update [Status and roadmap](Status-and-Roadmap.md) for outstanding
work instead of starting a new dated report. Tests on one revision, API mutations,
or successful read-only queries do not certify all eligible Windows builds.
