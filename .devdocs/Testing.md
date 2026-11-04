# Testing and verification

This document owns verification methods and completed evidence. Current behavior is
in [Architecture](Architecture.md); verification gaps and release work are maintained
in [Status and roadmap](Status-and-Roadmap.md).

## Verification layers

For code changes, run `./scripts/check-dev.ps1`: formatting, warnings-denied Clippy,
locked workspace tests, version contracts and release builds. Pure controller tests
exercise ordering, identity reuse, geometry, cancellation, recovery and late actions
with fake observations. Storage/helper tests cover bounds, atomic replacement,
corruption recovery, session merging, argument contracts and subprocess failures.
For provenance changes, also run `./scripts/test-versioning.ps1 -Provenance`.

For UI or interaction changes, inspect screenshots of the actual app and exercise
real pointer/keyboard input through the installed computer-use skill. Independently
observe foreground, visibility, placement and integration state. CLI controls and
controller tests cannot certify rendering, focus, tooltips or click behavior.

## Evidence standards and desktop safety

Run desktop tests serially in an unlocked, awake Explorer session. Use owned fixtures
and disposable app data. Record the OS build/revision, binary hash or source revision,
privilege/session type, display/DPI, workload, automation style and observation method.
Use explicit passed, failed, unavailable and not-run outcomes. State whether a result
came from a native harness, actual product CLI, real input or a physical device.

Preserve user windows, desktop identities/names, pins, preferences and data. Verify
PID, executable and creation time before stopping an owned process. After ambiguous
desktop creation, do not guess ownership; remove only verified empty test desktops.
Restore automation visibility off and any temporarily changed setting. Compare work
areas with display topology as well as reservations: a user display resize is not
permission to overwrite global work area. Explorer restart tests require a recorded
recoverable checkpoint. Stop input on lock/disconnection or failed observation.

Keep significant results below with enough context to interpret them without local
artifacts. Record material limitations, not every failed automation attempt. Do not
retain transient identifiers, delivery state or historical counts of passing tests.
No evidence on one Windows revision or VM certifies other machines or release readiness.

## Computer-use procedure

Observe, inspect, perform one meaningful input, then observe again. Refresh targets
after renders and follow the installed skill's current selection and recovery rules.
Do not construct automation targets from diagnostic HWNDs.

The normal passive TOOLWINDOW rail can be absent from automation inventories. For
real-button checks, temporarily expose it:

```powershell
./scripts/Invoke-Rail.ps1 --automation-ui on
# Select the observed rail and exercise its actual controls.
./scripts/Invoke-Rail.ps1 --automation-ui off
```

This changes discoverability, so test normal passive-style presence, fullscreen and
Task View exclusion separately. Exposed windows have become shell-cloaked after
switching; a passed exposed-style input check is not a passive-style presence pass.
See [automation recovery](Troubleshooting.md#passive-rail-absent-from-desktop-automation).

## Interactive regression matrix

| Area | Repeatable scenarios and observations |
| --- | --- |
| Rendering/input | Active desktop shape/colors, icon visibility indicators, Settings/notices, hover tooltips, actual restore/focus/second-click minimize, keyboard focus, Done and Exit |
| Desktop/pin integration | GUID/event identity across desktops; rail and Windows switches; actual Task View window/app pin/unpin cycles; closing/unpinning removes entries; minimized windows remain eligible |
| Layouts/auto-restore | A/B/A normal and maximized layouts; saved/unsaved destinations; clamped fallback; duplicate windows; manual and stable external learning; unchanged automatic layout memory and independent foreground anchor |
| Ordering | Rapid/coalesced A/B/A, pending minimize followed by restore, stalled/partial actions, timeout quarantine, late completion, held drag, destruction and fresh/reused identities |
| Baselines/recovery | Startup, pause/resume, preference changes, retry and restart produce no immediate window actions; live duplicates retain memory; app restarts receive fresh identities; singleton/settings delivery |
| Modal/fullscreen | Owner/dialog restore and minimize together; stalled targets leave rail responsive; passive-style fullscreen yield/recovery preserves application foreground |
| Presentation | Animated shell switches, instant rail switches, rapid replacement, same-desktop Task View dismissal, Windows animations off, no residual clipping/timer, fixed AppBar bounds and Task View thumbnails |
| Display/session/accessibility | Mixed DPI, primary changes, unplug/replug, sleep/resume, lock/unlock, Explorer restart, light/dark/high contrast, screen-reader/keyboard review and ordinary app workflows |

### Native Snap procedure

Use two group members and a separate owned unsnapped observer. Larger-group tests
require explicit user scope; historical larger-group results below remain valid.
Create a genuine group and confirm an explicit Task View Group thumbnail before
testing. Adjacent rectangles or `IsWindowArranged` alone do not establish membership.
Automation activation can alter an arrangement or dismiss the Snap flyout; inspect
from the independent observer and reject ambiguous starting states.

For each saved/unsaved destination and pinned/unpinned combination, test visible and
independently verified minimized starts, auto-restore on/off, actual rail restore and
an A/B/A trip. Observe normal restore bounds, visible screen bounds, arranged and
minimized/maximized state, foreground, memory and pending/quarantine state. Check
unpinned members separately. Verify the original drag-away size and shared-edge resize.
Confirm group membership afterward. Native reveal tests do not substitute for product
automatic switches or shell group actions. Remaining combinations belong in
[Status](Status-and-Roadmap.md#planned-work).

### Minimized-intent procedure

With auto-restore enabled and owned app-pinned fixtures, minimize before any geometry
is saved, restore manually, then establish distinct A/B layouts and minimize only on
B. Verify return to B preserves its preference while A restores independently.
Exercise title-bar and rail minimize, keyboard restore, restart and paused manual
changes. Automatic actions must not change intent. Observe stored preferences,
actual visibility, pending/quarantine state and independent foreground.

For external restoration, cover restore-then-drag and the first snapshot after a
verified minimize. Require fresh reachable matching post-drag observations; duplicate
ticks must not advance stability or replace cached state. Deterministic tests cover
exact delivery races; separate input calls cannot certify those races. External
taskbar restore, native minimize/switch races and a live Snap regression remain open.

### Presentation procedure

Exercise the shell's animated path as well as `winvd::switch_desktop`, which is an
immediate path. Cloak/topmost flags can stay unchanged while DWM omits the rail's
pixels; observe composed screen pixels and actual Task View thumbnails too. Finite
pixel samples establish detected transitions, not every-frame smoothness.
Task View entry alone is insufficient: confirm actual dismissal without a desktop
change, then check full width, buttons, foreground and absence of a residual hold/timer.
An Explorer shell activation process exit code alone does not establish the UI result.
Respect reduced motion and distinguish a skipped effect from failed presentation.
Mouse drags or completed switches do not reproduce a cancelled multi-finger gesture.

### Diagnostic report procedure

Open Settings through real input and inspect **Save diagnostic report…** and the local
save picker. Test save, cancel, overwrite and a long absolute destination. Verify the
archive has `README.txt`, `report.json` and bounded `events.jsonl`; check the
[privacy contract](Architecture.md#stack-and-source-map) and explicit unavailable
fresh-observation results when identity revalidation fails. Do not upload the archive
as part of a test. While the picker is open, cause an owned-fixture event and verify
the nested UI loop continues draining work without unintended actions or queue overflow.

## Compatibility tools

Build first, read each tool's `--help`, and close old owned fixtures before another
exercise. Build/run setup is in [Development](Development.md).

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

| Tool | Capability and boundary |
| --- | --- |
| Read-only compatibility probe | Does not switch, pin or control windows. `--observe-seconds 1..60` collects GUIDs/names, events and one-second pin samples in an owned subprocess with a duration-plus-ten-second watchdog. Schema-2 reports contain transient candidate metadata, not titles or persistent window identities. Queues/output are bounded; dropped events, a changing desktop or no queryable candidates make the sample unavailable. Exit 0 means collected, 2 unavailable, 1 invocation/report I/O failure. |
| Native smoke | One to four identical windows in one process; closing one preserves the others. Supports move/resize, minimize/maximize, an owned modal and `--delay-ms 0..10000` UI-thread stall. `--probe` reads foreground, bounds, DPI and display size. |
| Owned/pin/control exercises | Mutate only owned fixtures and restore their pins. Pin discovery measures four API-triggered transitions over about 42 seconds; it does not measure Task View click latency. Window control distinguishes acceptance, visibility, focus and completion, including modal/stalled cases. Individual shell workers have ten-second deadlines. Exit 0 passed, 2 failed/unavailable, 1 invalid invocation. Keep native smoke beside the exercise. |
| Desktop/AppBar exercise | Creates and names only temporary desktops, checks GUID events, returns to the original, removes verified empty owned desktops and checks work-area cleanup. AppBar mode also observes maximization and DWM cloaking. The `rail-native-smoke --appbar` prototype is not the actual product. |
| Fullscreen smoke | `--run-owned-fullscreen` opens a disposable borderless fullscreen fixture; `--inspect-rail` reads topmost state. Tests yield/recovery separately from desktop switching. |

To establish actual Task View pin origin, run:

```powershell
./target/release/rail-compatibility-exercise.exe --record-task-view > artifacts/compatibility-task-view.json
```

Wait for readiness, then use Task View to pin one fixture window, unpin it, pin all
fixture app windows and unpin them, leaving at least two seconds between changes.
Recording lasts about two minutes and does not mutate pins while observing. Both
cycles and operator confirmation of Task View origin are required; API changes alone
do not establish origin. Probe formats are not the privacy-filtered support ZIP.

## Resource and latency measurement

For a warmed stable two-pinned-window workload, run `./scripts/Measure-RailIdle.ps1`.
Check inventory, geometry, visibility, generations, pending work and UI/worker lifetimes
throughout; reject changed/unavailable workloads and overlapping cleanup. Include
both processes and distinguish private working set, total working set, private bytes,
duration, counter resolution and host load. Performance targets are owned by
[Status](Status-and-Roadmap.md#performance-targets).

`--status` reports bounded `dispatch_latency` statistics in microseconds: count, max,
over-50-ms count and the latest 256 samples. Cross-process QPC measures integration
worker receipt of a desktop event through snapshot/IPC/UI/action queues to accepted
automatic minimization. It excludes winvd's preceding notification-queue wait and
redraw, and does not measure automatic-restore completion. Script duration is not this
latency; reported zero CPU is not proof of no work. Use representative distributions
and independently verified outcomes before claiming the target is met.

## Compatibility and completed validation

These are scoped historical results, not certification of the current binary.
Hash prefixes below identify the full executable fingerprints in the next table.
Unknown original binary identity is stated rather than inferred. Local artifact
families under ignored `artifacts/` are supporting references only.

| Date / environment / build | Method, result and limitations |
| --- | --- |
| 2026-11-01; Windows 26300.9457 x64, medium integrity, 1708×960 / 120 DPI; original binary identity not recorded | Four-desktop GUID/events, actual Task View window/app pins, passive AppBar presence/uncloaking/nonactivation and 31-pixel reservation/cleanup passed. Native modal, stalled-target and duplicate-identity checks passed. Product A/B/A normal/maximized, modal, duplicate restart/fresh app identities, pause/singleton/retry and late-action quarantine passed; real pointer/focus/second-click and keyboard checks followed fixes. Input used temporary automation style; desktop/fullscreen checks used passive style. Lower-integrity discovery omitted medium windows; elevated support was not established. Supporting families: `compatibility-*`, `rail-layout-*`, `rail-cua-*`, `rail-fullscreen-*`. |
| 2026-11-02; Windows 26300.9711, 2560×1440 / 120 DPI; original exercise binary identity not recorded | API pin, identity, modal/stall, four-desktop/AppBar and passive minimization checks passed; real pointer restore/focus/second-click passed. Actual Task View pin origin was not established on this revision. Eligibility does not certify later revisions. Supporting families: `compatibility-*-20261102`, `rail-preview-click-*`. |
| 2026-11-02; Windows 26300.9711, 1708×960 / 120 DPI; app `1C60C2D7925A` | Icon rendering inspected with Notepad, Obsidian and an owned fixture; real minimize/restore and independent focus observations passed. A Space attempt did not establish keyboard activation. Synthetic filtering/transparency regressions passed. Other DPI sharpness was not tested. Supporting family: `icon-ui-*`. |
| 2026-11-02; Windows 26300.9711, local owned/packaged auto-restore workload; package `12959A9BB20F` | Saved/unsaved/fresh destinations, manual/external learning, normal/maximized restoration with independent foreground anchor, unchanged memory, pause/resume, persistence/retry and real rail/Settings input passed. External restore was fixture `SW_RESTORE`, not a literal taskbar click. Package/release/extracted executable hashes agreed. Supporting family: `auto-*`. |
| 2026-11-02; Windows 26300.9711, 1708×960 / 120 DPI, Per-Monitor V2; reviewed source `b0f497592b2d4ccdeec65eaee16d3f01ac398433`, fixture `1BE87A9ED54A` | Owned native harness distinguished visible/restore bounds, arranged state and foreground. Minimized reveal preserved snapping; placement could update normal bounds without changing visible snapped bounds, while visible-start automatic placement produced normal-sized visible bounds with arranged true. An observed no-activation unsnap followed by placement reached requested normal geometry with the 31-pixel work-area offset. These explain the troubleshooting guidance; they are not end-to-end product transition passes. Supporting family: `snap-*`. |
| 2026-11-02; Windows 26300.9711, 1708×960 then separately 2560×1440 / 120 DPI; app `1C60C2D7925A`, fixture `1BE87A9ED54A` | Native two-member all/mixed-pin A/B/A and a user-prepared genuine three-member group with one/two/all pinned members retained visible/normal bounds and explicit Task View Group membership. Actual rail pointer restores retained membership and requested focus; drag-away restored original 640×360 size. A controlled rail restart retained the group. These were native preservation/manual-product checks, not product automatic-switch certification or group reconstruction. Unconfirmed groups were excluded. Supporting families: `snap-result-*`, `snap-switch-*`. |
| 2026-11-02; Windows 26300.9711, 1918×1140 / 120 DPI; app `1C60C2D7925A` | Actual real-dot and product-CLI automatic A/B/A exposed normal-size visible restoration despite arranged true and reported success, in both-pinned and mixed saved-layout cases. Auto-off/manual restores, shared-edge resize, drag-away, B normal/maximized memory and pause/retry/fresh identities had scoped passes. Notepad 11.2607.14.0 and Calculator 11.2607.0.0 formed a real group; Calculator also reproduced duplicate frame/content eligibility and timeout. A synchronized five-second stall reached quarantine and late completion cleared slots, but stale/clipped notices remained. No independent foreground anchor covered the full race; mixed minimized-start evidence reused an earlier arrangement. Supporting family: `snap-next-*`. |
| 2026-11-03; Windows 26300.9711, 1708×960 / 120 DPI; preservation `67AC94D9666B`, refinement `BD68EC29E299` | Real rail saved-A/unsaved-B/A and both-saved repeated product trips retained snapped visible bounds and 640×360 restore sizes. An independent observer kept foreground on the final diagnostic repeat; manual diagnostic focus denials are not pointer-focus passes. Task View confirmed two-member Group survival; real drag-away restored original size. Synchronized real five-second stalls reached pending/quarantine and observed late completion cleared slots/notices. Wrapped notices were inspected on the preservation build; the final refinement repeated quarantine/recovery and compact Settings/Done/Tab, but its expanded screenshot missed the transient notice. Broader repaint, relayout focus and topology coverage remain open. Supporting family: `snap-restore-*`. |
| 2026-11-02; versioning source `0ec6474d72906d788f81a83e808e3306beb3b204` plus changes; packages `B98A1FB5C110`, footer refinement `55C47CEF724F`; Windows 26300.9711, 1708×960 / 120 DPI for UI | Isolated clean/dirty/untracked/incremental/exported-source provenance and tagged-release rejection/acceptance checks passed. Release/package/extracted/ZIP hashes, Windows resources and manifest identity agreed. CLI distinguished invoked and running builds. Real packaged Settings footer, Tab/reverse-Tab and Done passed at 125%; 100% and populated notice states were not covered by the footer refinement. |
| 2026-11-03; presentation source `5070793f989362d0765f395985d91dda2a99688d` plus changes; packages `9A6D41EA4097`, `FBF1D2846E9A`, `8CA6D62CD038`; Windows 26300.9711, 1708×960 / 120 DPI | Actual animated/instant/rapid switch traces showed intermediate rail regions/button positions with fixed AppBar bounds and no residual hold/timer. Real desktop-dot click and Shift+Tab/Space round trip passed using a temporarily exposed, individually pinned owned rail; passive style was tested separately. Same-desktop Task View dismissal and desktop changes while it stayed open passed. The timing refinement sampled a partial 7-pixel rail in the same sample as shell disappearance, then full width about 154 ms later; finite sampling is not frame-perfect certification. Packaged exclusion/Settings and reduced motion were checked. A later removal of background discovery passed code checks and observed return-to-full-width/input checks; its sampling attempts did not re-establish intermediate-frame timing. No physical cancelled-touchpad pass exists. Supporting families: `slide-experiment`, `taskview-experiment`, `gesture-experiment`, `return-only-experiment`. |
| 2026-11-03; user support archive, Windows 26300.9711; clean source `0b593b1131a8fa297b06cfc150271c4495bf1a18` | Archive/source analysis found verified Outlook minimization on the away desktop, followed by a saved-destination restore accepted 129 ms after return and verified visible 10 ms later. The old controller restored saved layouts without independent minimized intent. The archive does not establish when the user minimized on the destination or whether focus changed; this was not a live Outlook/Snap reproduction. Supporting archive: `VirtualDesktopRail_diagnostic_1793701509_Outlook_popedup_on_desktop.zip`. |
| 2026-11-03; Windows 26300.9711, unlocked Explorer, 2048×1152 / 144 DPI; dirty `0b593b1` builds `06EDCCA22C67`, `C3E13F2F4DCA` | Two owned app-pinned fixtures with auto-restore passed real title-bar minimize before geometry, manual/keyboard restore and desktop-specific saved-layout intent. Rail restart preserved preference/layouts. The final build repeated minimized A/B/A with no restore request and no pending/blocked work; departure used a real dot, return was observed after interruption. Temporary automation visibility required refresh/restart after some switches; passive presence, live Snap, taskbar restore, independent focus and exact native races were not certified. Code checks passed. |
| 2026-11-03; same OS/display/workload; dirty source `d182cc8`, executable `0607B4AB18EB` | External activation restore followed by real title-bar drag cleared intent and saved moved bounds; real-dot departure/CLI return restored those bounds with no pending/blocked work. Not a literal taskbar click or exact sub-poll race. Deterministic regression first failed on reviewed code, then covered before/during-drag candidates, 100-ms post-drag stability, invalidation, unreachable bounds and late capture. Code checks passed. |
| 2026-11-03; same OS/display/workload; dirty source `0f1f501`, executable `4A0AA4DD5E16` | Real rail minimize/external activation cleared intent and established a layout; real-dot departure/CLI return restored visibility without pending/blocked work. Separate input calls do not establish restore before the first snapshot. Deterministic regressions cover that failure for explicit/automatic minimize, stale frames, drag and excluded recovery/completion contexts. Code checks passed. |
| 2026-11-03; same OS/display/workload; dirty source `cdc536d`, executable `6F04526DC828` | Deterministic equal-tick regression first failed on reviewed code, then covered identical/conflicting duplicate observations, unchanged stability timing, context invalidation, drag and post-minimize restoration. Real rail minimize/external activation verified visible state, cleared intent and no pending/blocked work; exact same-tick delivery was not live-certified. Code checks passed. |
| 2026-11-03; Windows 11 x64; source `ad1d7c5` | Centered responsive floating bar option (`centered_rail`, `--centered-rail on\|off`, and Settings checkbox **Center rail and size to content** applied on **Done**) passed `check-dev.ps1` (88 workspace tests, version contracts and locked release builds). Visual appearance beside maximized windows was inspected via user-provided screenshots; automated CUA pointer/keyboard exercises were unavailable because no computer-use tool was installed in the session. |

The actual-product `snap-next-*` run also passed pause/resume, post-completion retry,
member close/unpin and fresh replacement identity without inherited layouts. Its
shared-divider drag moved both boundaries by 125 pixels. These are scoped recovery
and geometry checks, not pending-action retry or group reconstruction passes. The
final `BD68EC29E299` build declined an incompatible saved arrangement after a member
was dragged away, rather than claiming reconstruction.

The initial native toolchain run used Build Tools 2022 17.14.44, MSVC 14.44.35207 and
SDK 10.0.26100.0. These are evidence context, not additional setup version pins.
A reported screenshot of physical Windows 26300.9818 did not establish a live pass.
The initial VM's checkpoint, host/guest entitlement and connection mode were not fully
recorded; those results do not distinguish Basic Session from Enhanced Session/RDP.

### Executable fingerprints

| Prefix / use | SHA-256 |
| --- | --- |
| `1C60C2D7925A` product icon/manual Snap/automatic-failure baseline; embedded source `68509b02268786c0b4ac1c66a129ed145997b5a9`, dirty | `1C60C2D7925A198AF049E15BAE4E2C7081CA2A722BC079FF819329F1A81A7CC1` |
| `1BE87A9ED54A` native Snap fixture | `1BE87A9ED54A8E3F29A94CEE68A75779177CDAA8479DCD2FE28B700A48303973` |
| `12959A9BB20F` auto-restore package | `12959A9BB20FFB580947A09396522BEF36BDB6E627E461443D0F8209C5F19378` |
| `67AC94D9666B` matching-arrangement preservation | `67AC94D9666B448DBDDFD58508FE1FA92C1173A4D9AB65AB081E291930CAAD54` |
| `BD68EC29E299` visibility verification/notice redraw refinement | `BD68EC29E2990E814846959A05FEC5FE389D18AF41312BA891F0B05CCE56C949` |
| `B98A1FB5C110` traceability package | `B98A1FB5C110081DE8AF92A102312B22074F0217BBC848F5D8FDA9DE167D4864` |
| `55C47CEF724F` compact Settings footer | `55C47CEF724F51CD162B15C158B1170554D1CBD43D94722D3C31DBE3E38EE3B6` |
| `9A6D41EA4097` slide-in package | `9A6D41EA409775617A90BF44F893F3F8D02F5F64605B4F569C555E8F6997CCB1` |
| `FBF1D2846E9A` Task View return package | `FBF1D2846E9A25F0C2B0317E2C6A7A71433F6CD799FA9C2E9C200B2AF2986604` |
| `8CA6D62CD038` visibility timing package; background experiment subsequently removed | `8CA6D62CD038C34F49C02FA376A3A6B79D6BCA8C0985034544B09AA52A8C4A21` |
| `06EDCCA22C67` initial minimized-intent check | `06EDCCA22C67514A4ED2AE4453676133CA7DFA55CE69C07C02D65234FCAE42B5` |
| `C3E13F2F4DCA` final minimized-intent check | `C3E13F2F4DCA9797273EB8648C4DDAF1EED6F76C14008A076CA16D577FB96AB6` |
| `0607B4AB18EB` restore-then-drag refinement | `0607B4AB18EB4ECC0F5802BF56BC1EE75FF4149890D1139F77720D17AE02E3E1` |
| `4A0AA4DD5E16` first-post-minimize-snapshot refinement | `4A0AA4DD5E16C6491D39B3E5E2694B019391ACF43FBFC307948637F32D93700B` |
| `6F04526DC828` equal-timestamp refinement | `6F04526DC828A2E561830867C4837FE3EA3A6FD1413807E552F0D74EBA1BF969` |

### Performance evidence

On 2026-11-01, Windows 26300.9457 / 120 DPI, an empty-inventory portable sample used
3,993,600 bytes combined private working set and reported 0 ms CPU over 30.007 s.
One-second cached pin checks saw four API changes in 795/626/480/333 ms. Neither
result measures real Task View click timing or proves zero CPU. Binary identity was
not recorded for these initial samples.

On 2026-11-02, Windows 26300.9711, a warmed stable two-window VM workload used
4,730,880 bytes combined private working set and 62.5 ms CPU over 30.103 s, with six
stability checks. Diagnostic polling and counter resolution limit interpretation;
the local artifact's hash predates the subsequent icon edit. First desktop-event-to-
minimize acceptance was 72.635 ms. API pin samples were 786/630/483/316 ms.
The clean `rail-preview-populated-idle-20261102.json` supersedes interrupted or
cleanup-overlapped `rail-managed-idle-performance.json` / `rail-performance.json`.
Representative physical-machine distributions and completion timing remain unmeasured.

### Integration smoke check

On 2026-11-03, the presentation changes combined with main's minimized-intent feature
(`140f2b16686d` plus changes; executable SHA-256
`2ABEF49CBB20DA54F0AE052061A458557FC9B6AD524ABCB47F339026F7E5EAAA`)
passed `check-dev.ps1` including locked release builds. In the unlocked local Explorer
session on Windows 26300.9711, 1708×967 / 120 DPI, Computer Use inspected the actual
Settings screenshot and exercised its real Done button; status reported integration
ready. Automation visibility was off, the owned process exited and original app files
were restored byte-for-byte. No desktop switch or physical gesture was repeated in
this smoke check; detailed motion evidence above belongs to its recorded builds.
