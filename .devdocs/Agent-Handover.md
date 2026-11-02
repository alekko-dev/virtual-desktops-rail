> **Latest: optional auto-restore implemented.** Read
> [Auto-Restore-Validation.md](Auto-Restore-Validation.md) first. The approved
> setting restores destination layouts without focus and minimizes windows with
> no destination layout. Manual rail and stable external restores now establish
> layouts. There are 51 passing tests. Latest portable ZIP:
> `artifacts/VirtualDesktopRail-nightly-2026-11-02-autorestore-windows-x64.zip`.
> The packaged app is left running, ready, auto-restore off (default), experimental
> integration on for the VM, no test windows and only the original two desktops.
> See `artifacts/auto-package-verification.json` and `auto-cleanup-final.json`.

> **2026-11-02 update:** Read [Validation-2026-11-02.md](Validation-2026-11-02.md)
> before using this historical handover. The VM is now 26300.9711; the user's
> physical PC screenshot is 26300.9818. The default validated revision is unchanged,
> but Settings now has a persisted experimental opt-in for newer 26300 revisions.
> The product shares strict pin-query classification with the probe, records
> bounded dispatch timings, uses normal app icons with solid/outlined indicators
> underneath, and supports the high-contrast system palette. The full suite has
> 41 tests. A clean two-window VM idle sample and real pointer checks passed;
> a first latency sample was 72.635 ms, exceeding the provisional target.
> Current deliverable: `artifacts/VirtualDesktopRail-nightly-2026-11-02-windows-x64.zip`.
> Earlier PIDs, resource figures and live-state claims below are historical.
# Agent handover — 2026-11-01

## Objective and current stopping point

The user's original request was to read the project documentation and finish
implementation. They explicitly required preserving the architecture's
compatibility gate. That scoped gate passed before feature work. A native MVP
has now been implemented, exercised, and delivered as a **nightly**, not a
generally certified release. The user interrupted further validation to request
the nightly, then requested this handover. Do not silently restart a lengthy
validation campaign merely to prepare the handover.

The user prefers autonomous progress and specifically said to use computer use
to execute UI tests independently. A manual operator check found a real
restore/focus/second-click bug; it was subsequently reproduced and fixed using
computer use. Avoid asking the user to repeat routine fixture actions.

Workspace: `C:\Users\aleks\source\local\virtual-desktops-rail`, PowerShell.
All implementation changes are **uncommitted**. Many important source directories
are untracked; `git diff` alone does not show them. Preserve this working tree.
No commit, public release, upload, or publication was performed.

Read first:

1. `Virtual-Desktop-Rail-MVP.md` and `Virtual-Desktop-Rail-Architecture.md`.
2. `Implementation-Status.md` for implemented scope and remaining release work.
3. `Compatibility-Evidence.md` for the completed initial gate.
4. `Live-Validation.md`, `Environment-Readiness.md`, and the root `README.md`.

## Delivered nightly

ZIP, relative to the workspace:

`artifacts/VirtualDesktopRail-nightly-2026-11-01-windows-x64.zip`

Extracted copy:

`artifacts/VirtualDesktopRail-nightly-2026-11-01-windows-x64/`

Executable SHA-256:

`C6EBB42014570A05F20C05D936E309D29CB8904B7D710BA29C41586360CFAD85`

The ZIP contains the current executable, README, NIGHTLY notes, SHA256.json,
third-party notices and 66 license files. Static CRT; unsigned; no installer.
The build's PE imports were inspected: no redistributable VCRUNTIME dependency.
The embedded manifest was extracted and verified: PerMonitorV2, asInvoker and
longPathAware. The executable hash matches `target/release/virtual-desktop-rail.exe`.

The nightly was copied from the final package directory
`artifacts/VirtualDesktopRail-0.1.0-windows-x64-20261101-224855`.
Older ZIPs in artifacts predate the final focus fix; use the nightly above.
`scripts/package.ps1` creates timestamped portable packages with licenses;
the friendly nightly name and NIGHTLY.md were added when delivering this snapshot.

## Host, compatibility and authorization constraints

- Windows 11 Pro x64 **10.0.26300.9457**, session 2, 1708 x 960 at 120 DPI.
  Native OS checks use the build/revision, not the misleading registry ProductName.
- The product enables shell integration on **that exact revision only**, in
  `apps/rail/src/platform.rs::supported`. Do not broaden it merely because a
  later build exceeds upstream's minimum. Validate additional revisions first.
- `winvd` is pinned to 0.0.49; its upstream minimum is 26100.2605. The read-only
  compatibility tools use that minimum, while the product uses the exact matrix.
- Missing ApplicationViews on some ApplicationFrameWindow surfaces are classified
  unsupported only when both pin queries return WindowNotFound. Mixed or other
  errors remain failures. Do not convert all COM failures to unsupported.
- Window pin and app pin are an OR, not mutually exclusive query states.
- The native test processes were medium-integrity. Tool sandbox escalation was
  needed for ordinary Rust caches and in-session GUI tests; it did not establish
  elevated-application support. Lower-integrity discovery omitted medium windows.
- A recoverable host checkpoint, VMConnect mode, entitlement and physical test
  machine have not been recorded. Real Explorer restart was not performed.
- Use existing user authorization for ordinary reversible implementation/test work.
  Do not send messages to third parties or publish the build without instructions.

## Live state at handover

Rechecked for this handover:

- Product UI PID **6848**, running from the timestamped package directory above.
- Integration worker PID **9304**, same executable, parent 6848, command
  `--integration-worker 6848`.
- `artifacts/rail-session.json` contains the UI PID, executable and creation time.
  PIDs are observations, not durable identities: recheck before any process action.
- `artifacts/rail-handover-status.json` is the latest status snapshot.
- Normal passive style; automation visibility off; startup off; pause off;
  integration ready; no managed fixture windows. The application is left running.
- No native-smoke or fullscreen-smoke processes remain. The last performance
  fixture PID 2028 was unpinned and stopped after checking executable/creation
  time. Cleanup evidence: `artifacts/rail-nightly-cleanup.json`.
- Current/original user desktop: `7CC33836-BC98-4DEB-9005-E971C422CC93`.
- Preserve user desktop `179B0161-0B03-4668-BDF9-6E9015C98711`.
- Our temporary desktop `DCE4AFA1-6DC5-46B3-881D-2E166D1AC77E` was removed after
  returning to the original desktop, using the empty-desktop guard. Evidence:
  `rail-cleanup-switch.json` and `rail-cleanup-desktop.json`.
- `%LOCALAPPDATA%\VirtualDesktopRail` contains real local app state and diagnostics,
  including old test-window identities. They are intentionally bounded historical
  records. Do not wipe user configuration as a routine cleanup step.
- The real Startup shortcut was never enabled. Shortcut tests used isolated dirs.

## Implementation map

| Path | Responsibility |
| --- | --- |
| `crates/rail-core/src/lib.rs` | Pure controller, identity records, layout adjustment, sequencing, pause/recovery policy |
| `crates/rail-core/tests/controller.rs` | 16 meaningful controller regression scenarios |
| `apps/rail/src/main.rs` | GUI/CLI entry, bounded event delivery, helper mode |
| `apps/rail/src/ui.rs` | Native buttons, rendering, settings, input/activation routing, WinEvents, orchestration |
| `apps/rail/src/appbar.rs` | Primary AppBar reservation, fullscreen/z-order reconciliation, recovery |
| `apps/rail/src/platform.rs` | Exact support gate, stable window properties, candidate capture, geometry and identity validation |
| `apps/rail/src/bridge.rs` | Shell COM/listener subprocess, bounded protocol/watchdog, reconnect/backoff and JobObject |
| `apps/rail/src/actions.rs` | Async native actions, verification, quarantine, guarded user-requested focus |
| `apps/rail/src/store.rs` | Atomic local state, backup/recovery, cross-session merging, diagnostics, Startup link worker/tests |
| `apps/rail/src/icons.rs` | Off-UI bounded icon extraction; identity/DPI cache delivery |
| `apps/rail/src/singleton.rs` | Per-user/session instance mutex; cross-session file/link coordination |
| `apps/rail/src/native.rs` | Small raw Win32 UI declarations and structures |
| `apps/rail/build.rs`, `rail.manifest` | Embedded PMv2/asInvoker/long-path manifest |
| `.cargo/config.toml` | Static CRT for x86_64-pc-windows-msvc |
| `tools/compatibility-probe` | Read-only probe, shared atomic replacement/watchdogs, opt-in owned fixture and desktop exercises |
| `tools/native-smoke` | Dependency-free duplicate normal windows, owned modal, configurable UI stall, foreground probe, AppBar prototype |
| `tools/fullscreen-smoke` | Explicit owned borderless fullscreen fixture and read-only rail topmost inspection |

Core invariants to preserve:

- Startup, recovery, pin discovery, and resume establish baselines without
  unsolicited minimization. Confirmed desktop changes minimize visible windows;
  returning never restores automatically.
- Persist opaque random token + process creation time + executable + session.
  Never persist HWND as identity or guess duplicate matches after app restart.
- One outstanding operation per identity, even across temporary disappearance.
  Timeouts quarantine; late verification cannot commit layout or steal focus.
- Drag and snapshot timestamps, generations and per-window sequences reject stale
  geometry/actions. Coalesced A→B→A transitions still count as a transition.
- Own restores and external restoration do not overwrite saved desktop geometry.
  Minimized HWND takes precedence even if Windows still reports it foreground.
- Minimized maximized windows retain WPF_RESTORETOMAXIMIZED semantics.
- Modal dialogs remain grouped; verify/reveal a reliable popup rather than creating
  an independent entry or issuing actions against an ambiguous disabled owner.
- UI thread does not run shell COM, file I/O, icon extraction or blocking target
  messages. Actions are async; foreground/USER geometry reads are nonblocking.
- State is schema 1, max 4 MiB, 512 identities and 64 desktop layouts per identity.
  New identities are omitted at the history cap; no history-management UI exists.

## Two important bugs fixed late in the session

### Fullscreen/z-order

The initial check yielded the AppBar during fullscreen but it could remain below
normal windows afterward. A successful SetWindowPos did not imply the topmost
style was retained. The working fix combines actual-foreground/monitor-rectangle
reconciliation with z-order-only flags **SWP_NOOWNERZORDER | SWP_NOSENDCHANGING**,
in addition to NOMOVE/NOSIZE/NOACTIVATE. Preserve these flags. Do not regress to
the older raw 0x13-only callback flags.

Foreground events, shell notifications and the existing one-second reconciliation
check the actual topmost state; there is no extra permanent frame timer. Shell
desktop classes are excluded from fullscreen inference. Geometry-only AppBar
position reporting avoids z-order feedback. The final packaged executable passed
fullscreen entry/exit in normal style: `rail-packaged-fullscreen-during.json` and
`rail-packaged-fullscreen-after.json` show yielding true/topmost false, followed by
yielding false/topmost true, automation_ui false. Foreground remained outside rail.

### Real icon click restore/focus/minimize

The operator reported that the first click revealed a window but a second did not
minimize it. The original focus guard rejected explicit restore focus when the
passive rail had not become foreground. Computer use also revealed that assistive
activation could replace the pre-click foreground with the rail itself.

The fix remembers the click's original foreground in `Job::focus_from`, permits
focus when that original foreground remains unchanged, preserves pre-rail
activation from WM_ACTIVATE, and returns **MA_NOACTIVATE** on WM_MOUSEACTIVATE.
Mouse activation does not overwrite the saved foreground with the rail itself.
Non-window rail button actions clear the stale pre-foreground hint. Late or
superseded completion still cannot focus after the user moves elsewhere.

Both independent icons passed real computer-use clicks: restore/focus then
second-click minimize. Mouse pause and keyboard Space resume passed as well.
See the `rail-cua-*.json` artifacts and Live-Validation. Do not replace this with
only CLI --click testing; CLI did not expose the original physical input bug.

## Repeatable build and control commands

```powershell
./scripts/check-dev.ps1
./scripts/package.ps1
Start-Process ./target/release/virtual-desktop-rail.exe
Start-Process ./target/release/virtual-desktop-rail.exe -ArgumentList '--settings'
./scripts/Invoke-Rail.ps1 --status
./scripts/Invoke-Rail.ps1 --pause
./scripts/Invoke-Rail.ps1 --resume
./scripts/Invoke-Rail.ps1 --retry
./scripts/Invoke-Rail.ps1 --exit
```

The helper captures GUI-subsystem stdout and real exit codes. It addresses the
running instance even if that instance is from a package directory. Normal
second launch exits cleanly; --settings opens settings on the existing instance.
Stop the instance before relinking the same executable path. Do not kill an
arbitrary process by name; verify recorded PID + creation time + executable.

Cargo is not always on PATH. Use `C:/Users/aleks/.cargo/bin/cargo.exe`; the scripts
already use this explicit path. Current Rust is pinned to 1.99.1, MSVC x64.
Sandboxed build/cache failures were resolved with ordinary tool escalation.

Keep Cargo.lock locked. Its diff adds only local workspace packages, not new
versions of third-party dependencies. Do not run generate-lockfile merely to fix
a cache error. For offline dependency metadata, use:

`cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc --locked --offline`

Omitting filter-platform attempts to download uncached ARM packages even though
the x64 build succeeds. Packaging requires installed Rust documentation licenses.

## Computer-use workflow that works

Read the installed computer-use skill and its guidance/API/confirmations before
UI automation. It is under the bundled plugin's computer-use skill directory.
Use the persistent node_repl tool with `@oai/sky`, not custom UI automation.

```javascript
if (!globalThis.sky) {
  const { sky } = await import('@oai/sky');
  globalThis.sky = sky;
}
nodeRepl.write(JSON.stringify(await sky.list_windows()));
```

The normal passive TOOLWINDOW AppBar is deliberately absent from the usable app
window inventory. A previously returned handle also becomes unusable after its
normal tool style is restored. Never manufacture a Window from a diagnostic HWND.

Use the explicit, nonpersistent diagnostic visibility mode:

```powershell
./scripts/Invoke-Rail.ps1 --automation-ui on
# Select the returned Virtual Desktop Rail window from sky.list_windows().
# Observe its current accessibility tree; click the real native child buttons.
./scripts/Invoke-Rail.ps1 --automation-ui off
```

It temporarily adds APPWINDOW and removes TOOLWINDOW, preserving NOACTIVATE and
the same UI/controller/button procedures. This enables independent input tests;
it does not certify normal-style presence across desktops. Desktop-presence and
fullscreen checks use normal mode. Do not leave diagnostic visibility enabled.

Observe, stop and inspect; then one input and refresh. Accessibility indexes can
change after a render. If an index is unavailable, re-observe instead of retrying
stale input. Screenshots must be forwarded as image content, never serialized.
Do not use Windows-key shortcuts, terminal/Run-dialog UI automation, security
settings, or authentication dialogs. Task View checks earlier required the user
because the skill forbids Windows-key shortcuts; that gate already passed.

The input desktop occasionally locks: GetCursorPos access denied plus no native
foreground. Stop UI input and request unlock if needed; continue independent work.
Do not automate LockApp or change security/lock settings. The user unlocked it
during this session, but that does not prove it remains unlocked later.

## Validation evidence and remaining work

38 tests pass: compatibility/storage/watchdog/CLI tests, 16 pure controller
scenarios, native fixture argument bounds, five product storage/shortcut tests,
and two product CLI tests. Formatting, warnings-denied Clippy and locked release
builds passed. The final help/diagnostics-only source additions were linted and
packaged afterward; the binary hash above identifies exactly what was delivered.

Live checks passed for A/B/A dragged geometry, maximized restore, modal group
restore/focus, pause/resume, app/window pin eligibility, duplicate utility restart,
conservative app restart, second instance, explicit integration retry and a
five-second target stall with a responsive UI/quarantined operation. The initial
compatibility gate separately passed Task View cycles, four-desktop notifications,
passive AppBar presence/reservation and pin polling discovery bounds.

Resource evidence needs careful wording:

- `rail-final-performance.json`: final static-CRT portable build, **empty managed
  inventory**, 30.007 seconds, 3,993,600 combined private working-set bytes, 0 ms
  CPU reported at finite process-counter resolution. Not a claim of literal zero
  CPU or a realistic populated-workload measurement.
- `rail-performance.json`: earlier 4,145,152-byte / 125-ms sample. Interactive
  activity occurred around that experiment; do not treat it as a clean final idle
  certification. Older status documentation called it warmed idle provisionally.
- `rail-managed-idle-performance.json` exists, but the user interrupted this
  experiment and the nightly cleanup removed its fixture. Despite its declared
  managed_windows:2, **do not certify stable populated idle from this artifact**.
  Repeat a clean warmed measurement with two pinned windows and no concurrent UI,
  creation/destruction or cleanup, rechecking child lifetimes during the sample.

Priorities if the user asks to continue implementation/release work:

1. Reproduce and investigate occasional `ComError(HRESULT(0x80070490))` pin-query
   failures around transient windows. Current behavior fails closed, disables
   controls and reconnects; do not mask arbitrary errors as unsupported. Inspect
   `%LOCALAPPDATA%\VirtualDesktopRail\diagnostics.log` and consider bounded
   diagnostic attribution to the failing candidate without collecting titles.
2. Complete populated idle/resource and event-receipt-to-action-dispatch latency
   measurement against provisional <20 MB and <50 ms targets. Distinguish private
   working set, private bytes, total RSS, controller delay and application redraw.
3. Validate the disposable Obsidian vault workflow, actual Explorer restart under
   a recorded recoverable checkpoint, sleep/resume and session recovery.
4. Physical hardware, monitor removal/primary changes/mixed DPI, high contrast,
   screen-reader review, desktop tooltip hover and privilege/support matrix.
5. Review history-cap behavior and edge cases without expanding MVP scope into
   overflow UI, labels, app pinning controls or heuristic duplicate reassociation.

No further user input is currently pending. The user has a usable nightly and
the app is left running in normal mode. Further support should start from these
facts, not redo the completed compatibility gate or reinstall the toolchain.
