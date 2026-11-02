# Implementation status

Updated 2026-11-02. The native Windows MVP is implemented. The documented
compatibility gate was preserved and passed on **Windows 11 x64 10.0.26300.9457**
before rail feature work began. See [Compatibility-Evidence](Compatibility-Evidence.md).
This is a development MVP; broader release certification remains open. See
[2026-11-02 follow-up](Validation-2026-11-02.md) for the newer-revision opt-in,
measured resource results and deferred checks.

## Implemented

- A 25-DIP primary-monitor left AppBar with shape-distinguished desktop dots,
  name tooltips, icon-only shared-window buttons, pause and settings. Native
  keyboard controls and accessible button captions; no title tooltips or labels.
- Discovery of eligible window-pinned OR application-pinned ordinary windows,
  including minimized windows. Cached candidates reconcile once per second;
  shell/window events trigger rescans and a 30-second safety rescan bounds drift.
- Minimize on confirmed desktop transitions, restore only on explicit request.
  Pause persists and cancels queued automatic work; resume, startup, pin discovery,
  and integration recovery establish baselines without unsolicited minimization.
- Independent normal/maximized layouts per window and desktop. Move/resize end
  events commit geometry; utility-generated or external restoration does not
  overwrite saved desktop layouts. Monitor/DPI adjustment clamps restored bounds.
- Stable opaque window properties combined with process lifetime, executable,
  and session. Duplicate windows retain independent identities through utility
  restarts. New application lifetimes receive new identities; no heuristic match.
- Asynchronous window actions, per-window sequencing, generation cancellation,
  two-second verification deadlines and one outstanding operation per window.
  Timed-out operations remain quarantined until verified or the identity dies;
  late completion cannot focus the window or commit geometry. Owned modal dialogs
  stay grouped with their owner and are explicitly revealed on restoration.
- Shell COM isolated in an owned worker with bounded protocol/queues, watchdog,
  kill-on-parent-close JobObject, and reconnect backoff. TaskbarCreated handling
  re-registers the AppBar and reconnects with a fresh controller baseline.
- Visible unavailable status and retry controls. The exact validated OS revision
  enables integration by default. A persisted experimental opt-in allows newer
  revisions of build 26300, while other builds remain blocked. Inaccessible and
  unsupported surfaces are excluded; mixed pin-query failures remain failures.
- Per-user/session single instance, per-user opt-in Startup shortcut, atomic local
  JSON state, previous-state recovery, corrupt-file preservation, debounced writes,
  bounded local diagnostics and coordinated cross-session merges. No HWND is
  persisted as an identity. No network dependency for the core workflow.
- Per-Monitor V2/asInvoker manifest, identity/DPI icon cache, light/dark rendering,
  high-contrast system palette, unchanged normal icons with under-icon state
  indicators, AppBar fullscreen yielding and foreground-event recovery of z-order. Static CRT
  release configuration and a portable ZIP packaging script with license notices.

## Verification

`scripts/check-dev.ps1` runs formatting, warnings-denied Clippy, **41 tests**, and
locked release builds. Tests cover the pure controller's race/recovery behavior,
monitor clamping, schema validation, atomic storage, corrupt-file recovery,
cross-session merging, isolated Startup shortcut creation/removal, diagnostic
bounds, subprocess failures/watchdogs, and CLI contracts.

[Live-Validation](Live-Validation.md) records the interactive product checks and
local evidence. They include real move/resize and A/B/A layout restoration,
maximized restore, owner/modal focus, duplicate utility restart, conservative
application restart, pause/resume, unpin/app-pin discovery, second launch,
fresh-baseline integration retry, and a five-second stalled application with a
responsive rail. A final portable-build sample with an empty managed inventory
reported **3,993,600 combined private working-set bytes** and **0 ms CPU over
30.007 seconds**, at finite counter resolution. A clean populated-workload sample
is now recorded in [the follow-up](Validation-2026-11-02.md): 4,730,880 combined
private working-set bytes and 62.5 ms CPU over 30.103 seconds with two stable
managed windows. See that document for measurement caveats. This is not
physical-hardware certification.

## Remaining release validation

- Real Explorer restart under a recorded recoverable host checkpoint, sleep/resume,
  lock/unlock and session reconnect coverage; explicit retry is already tested.
- Obsidian's disposable-vault end-to-end workflow and additional application types.
- Physical hardware, mixed DPI, monitor disconnect/reconnect, primary-display
  changes, screen-reader review, and light/dark/high-contrast visual coverage.
- Additional supported OS revisions and privilege combinations. The version gate
  intentionally stays closed after an unvalidated Windows update.
- Dedicated event-receipt-to-action-dispatch latency instrumentation against the
  provisional 50 ms target: instrumentation is now implemented, but a first VM
  sample was 72.635 ms. Broader realistic-workload measurements remain open.
- Desktop-name hover and wider native-style input coverage. Real icon clicks and
  keyboard pause/resume passed through computer use in temporary automation
  visibility mode; see Live-Validation for its scope and the fixed focus regression.

The MVP assumes approximately two or three managed windows and has no overflow
UI. State is bounded to 512 identities and 64 desktop layouts per identity;
reaching the identity limit omits new identities. History management and broader
application/elevated-window support are outside this development build.

## Run and package

See [README](../README.md) for launch/control commands. `scripts/package.ps1`
builds a local unsigned portable ZIP under `artifacts/`, including the executable,
usage notes, dependency licenses and SHA-256. No automatic startup is enabled by
building or launching the app. No public release or publication is performed.
