# Virtual Desktop Rail

Native Windows 11 x64 desktop rail MVP. It reserves 25 DIP on the primary
display, switches desktops, discovers Windows-pinned application windows, and
minimizes them on desktop changes. Click an icon to restore that window's saved
layout for the current desktop. Pause, startup preference, and independent window
layouts persist locally.

Enable **Restore saved layouts on desktop switch** in Settings for automatic
restoration. On each switch, a window with a saved layout for the destination
restores there without taking focus; a window with no layout gets minimized.
The option is off by default. Restoring manually through the rail or Windows
(including the taskbar) saves the resulting layout for that desktop. External
restoration waits for stable, reachable geometry before saving. Minimizing keeps
the saved layout, so the window auto-restores on a later switch back. Startup,
integration recovery, resume and enabling the option do not move windows.
Pause suspends both automatic minimization and restoration. To change the option
on a running instance from PowerShell, use
`./scripts/Invoke-Rail.ps1 --auto-restore on` (or `off`).

The documented compatibility gate passed before feature implementation. Shell
integration defaults to the validated **10.0.26300.9457** revision. On newer
revisions of build **26300** (including 9711 and 9818), Settings offers **Try
experimental integration on newer Windows 26300 revisions**. Enabling it retries
immediately and persists across restarts. Other builds stay unavailable. This is
an unsigned development MVP, with broader release
validation still pending. See [implementation status](.devdocs/Implementation-Status.md),
[live evidence](.devdocs/Live-Validation.md), and [portable usage](.devdocs/Portable-Build.md).

## Run

After building, launch the GUI from PowerShell:

```powershell
Start-Process ./target/release/virtual-desktop-rail.exe
Start-Process ./target/release/virtual-desktop-rail.exe -ArgumentList '--settings'
./scripts/Invoke-Rail.ps1 --status
./scripts/Invoke-Rail.ps1 --pause
./scripts/Invoke-Rail.ps1 --resume
./scripts/Invoke-Rail.ps1 --retry
./scripts/Invoke-Rail.ps1 --exit
```

Pin windows with Windows Task View. Individual and application-level pins are
both supported; cached discovery runs once per second. Startup is opt-in through
settings. State and bounded diagnostics live in `%LOCALAPPDATA%\VirtualDesktopRail`.
The MVP expects approximately two or three shared windows and has no overflow UI.
Application restarts receive fresh identities rather than guessed layout matches.

If the rail has no desktop dots and Settings says Windows has not passed the
compatibility matrix, integration was deliberately disabled before discovery.
On a newer 26300 revision, use the experimental checkbox to try it. This does
not certify that revision; runtime query failures still disable dependent controls.
The same preference can be changed on a running instance with
`./scripts/Invoke-Rail.ps1 --experimental-integration on` (or `off`).

Desktop dots receive automatic contrasting colors (blue, orange, purple, then other hue families). The current desktop is a larger filled circle; inactive desktops use colored outlines. The rail uses smooth dots, spaced window buttons, and a drawn sliders control for Settings.

App icons keep their regular appearance when minimized. The indicator underneath
is solid for visible windows and outlined for minimized windows.

For desktop automation only, `./scripts/Invoke-Rail.ps1 --automation-ui on`
temporarily exposes the rail as a targetable application window while retaining
its no-activation input behavior. Restore `--automation-ui off` after testing.
This mode is not persisted; normal launches use the validated passive tool style.

`scripts/package.ps1` produces a portable ZIP under `artifacts/` with static CRT,
usage notes, third-party license texts and executable SHA-256. No installer or
administrator privileges are required to run the rail.

## Development

Install Git, Rust through rustup, and Visual Studio C++ tools with a Windows SDK.
The repository pins Rust and its formatting/lint components in `rust-toolchain.toml`.
Open a new PowerShell session after installation so PATH changes take effect.

From this repository:

```powershell
./scripts/check-dev.ps1
./target/release/rail-compatibility-probe.exe --output artifacts/compatibility.json
./target/release/rail-native-smoke.exe --windows 2 --delay-ms 1500
./target/release/rail-native-smoke.exe --probe
./target/release/rail-compatibility-exercise.exe --run-owned-fixture
./target/release/rail-compatibility-exercise.exe --run-pin-discovery
./target/release/rail-compatibility-exercise.exe --run-window-control
./target/release/rail-desktop-exercise.exe --run-desktop-fixture
./target/release/rail-desktop-exercise.exe --run-appbar-fixture
```

The script runs formatting, Clippy (warnings denied), Cargo tests, and a locked
release build of all workspace binaries using the root `Cargo.lock`. The tests
cover controller races and recovery, identity reuse, monitor clamping, atomic
storage and corruption recovery, cross-session merging, isolated startup links,
subprocess timeouts/crashes, and CLI contracts. Interactive and release-matrix
checks are recorded separately.

The fixture supports ordinary move/resize and minimize/maximize. `--windows 1..4`
creates identical windows sharing one process; closing one keeps the others alive.
Its dialog button opens an owned modal, and its delay button intentionally stalls
the fixture UI thread for `--delay-ms 0..10000` (default 2000). `--probe` prints
JSON containing the foreground HWND, owning PID, screen rectangle, window DPI,
and primary display dimensions, including in release builds. Close all fixture
windows to exit. All binaries accept `--help`.

For a warmed populated idle sample, keep two pinned windows stable and run
`./scripts/Measure-RailIdle.ps1`. It checks inventory, geometry, visibility,
desktop generation and both process lifetimes throughout the sample. It rejects
unavailable integration or a changed workload. `--status` also exposes bounded
`dispatch_latency` statistics in microseconds from integration-worker desktop-event
receipt to an accepted automatic minimize call. These include snapshot/IPC/queue
delay, exclude application redraw, and exclude time waiting in winvd's notification
queue before worker receipt.

The compatibility checker requires at least Windows build **26100.2605** for
`winvd 0.0.49`. The VM runs **26300.9457** and clears the version gate, but its
read-only probe now collects observations while distinguishing unsupported shell
views from actual query failures. The separate compatibility spike passed on
this exact baseline; a read-only report alone cannot clear that gate. See
[environment readiness](.devdocs/Environment-Readiness.md) for setup and evidence.
Exit status **2** means unavailable, **1** means invocation/report I/O failed, and
**0** means read-only observations were collected, not that compatibility passed.
There is deliberately no build-gate bypass.

On an eligible build, `--observe-seconds 1..60` (default 2) records desktop GUIDs/names,
notification kinds, and one-second samples of window/app pin queries. Calls and
listener teardown run in an owned subprocess with a duration-plus-ten-second
watchdog. Event queues, observations, candidates, and output size are bounded.
The checker does not switch desktops, change pinning, or move/minimize windows.
Reports contain local desktop names and transient HWND/PID observations; these
are diagnostics, not persistent window identities. No window titles are captured.
`--output` atomically replaces the report in an existing directory.

Schema version 2 records candidate class names, per-candidate observation times,
sample duration/current desktop, and GUIDs on desktop events. Both pin queries
returning `WindowNotFound` means no shell ApplicationView exists for that HWND;
the candidate remains in diagnostics as `unsupported`. Mixed results and other
errors remain `query_failed`. A sample with no queryable candidates, an event
limit, or a desktop change during sampling still makes the report unavailable.
The bounded notification queue may drop events; this is not a lossless recorder.

The separate **compatibility exercise changes fixture state** and requires the
explicit `--run-owned-fixture` switch. Close existing native fixtures first. It
launches two owned fixture windows, tests individual/application pinning, checks
that distinct non-pointer identity properties survive separate helper processes,
removes the properties, restores pinning, and terminates its fixture. Each shell
operation runs in a subprocess with a ten-second watchdog. This experiment does
not switch desktops or control other applications. It prints JSON; exit 0 means
the experiment passed, 2 means failed/unavailable, and 1 means invalid invocation.
Keep `rail-native-smoke.exe` beside it. Run interactive checks serially.

`--run-pin-discovery` adds a separate observer that polls two cached candidates
once per second. It measures warmed idle process CPU and four API-triggered pin
transitions over about 42 seconds. QPC timestamps come from independent actor and
observer processes. Recorded latency includes actor dispatch/IPC overhead; it is
not Task View click latency. CPU counters have finite resolution and this short
fixture run does not certify product idle performance or a larger inventory.

`--run-window-control` tests minimizing/restoring an owned modal group and async
minimization during a 1500 ms fixture UI stall. It separately records API
acceptance, observed visibility, foreground verification, and action completion.
The hidden standard-dialog lookup is fixture-specific; it is not a general
application identity or dialog-discovery policy.

`rail-desktop-exercise --run-desktop-fixture` creates three temporary desktops,
names only those desktops, switches through them and the original, and verifies
GUIDs and notification identities. It then restores the original desktop and
removes created desktops only after a broad window inventory finds no associated
windows. If a user moves/opens a window there, cleanup preserves the desktop and
reports the failure. Each operation and listener teardown has a ten-second
subprocess watchdog. If creation times out after a shell-side effect, the final
inventory reports unexpected desktops rather than guessing which ones to delete.

`--run-appbar-fixture` adds a passive 25-unit left-edge AppBar, checks a native
maximized window against the reserved work area, observes DWM cloaking across
all four desktops, then verifies work-area restoration. This is a compatibility
prototype, with no product rail controls. `rail-native-smoke --appbar` can also
launch that fixture alone; close it normally to release its reservation.

For a manual Task View recording, run:

```powershell
./target/release/rail-compatibility-exercise.exe --record-task-view > artifacts/compatibility-task-view.json
```

Once the recorder says it is ready, use Windows Task View to pin one fixture
window to all desktops, unpin it, pin all windows from the fixture application,
then unpin them. Leave at least two seconds between changes. Recording lasts
about two minutes; it does not mutate pinning during that period. Cleanup restores
the fixture's initial pin state and closes it. The recording checks both observed
state cycles, requires operator confirmation of their Task View origin, and does
not infer click-to-discovery latency from untimestamped manual actions.

The identity experiment validates a standard-user native fixture only. It does
not establish persistence/reassociation for other applications, privilege levels,
HWND reuse, or application restarts. Neither successful tool certifies the MVP.

See [.devdocs/Implementation-Status.md](.devdocs/Implementation-Status.md) for
verified behavior and the remaining compatibility and implementation gates.

Commands needing the Rust cache or interactive desktop may require execution
outside an agent's repository-only sandbox. Run UI checks as the ordinary signed-in
user in the Explorer session, separately from privileged provisioning.

Create a disposable Obsidian vault by copying `fixtures/obsidian-vault` to
`artifacts/obsidian-vault`, then open that folder as a vault in Obsidian.
`artifacts/` and Cargo build outputs are ignored by Git.

See [.devdocs/Environment-Readiness.md](.devdocs/Environment-Readiness.md) for the
verified machine baseline and outstanding integration-test prerequisites.
