# Live MVP validation

> Updated evidence is recorded in [Validation-2026-11-02](Validation-2026-11-02.md).
> The results below describe the original revision 9457 validation. The VM has
> since updated to 9711; newer same-build revisions now have an explicit
> experimental opt-in without changing the validated default matrix.

2026-11-01; Windows 11 Pro x64 10.0.26300.9457; Explorer session 2;
1708 x 960 primary display, 120 DPI. Ordinary native fixtures run at medium
integrity. Tests were serialized on the unlocked input desktop. Local JSON
artifacts are ignored by Git; this document preserves their findings.

| Product behavior | Result | Evidence under artifacts/ |
| --- | --- | --- |
| Pause across switches; resume without immediate minimize | Passed | rail-live-workflow.json |
| Unpin removes entry; app pin discovers two windows | Passed | rail-live-workflow.json |
| Actual drag on desktop A and B; restore A/B/A | Exact normal rectangles preserved | rail-layout-roundtrip.json |
| Minimized HWND still reported as foreground | Explicit click restores correctly | rail-layout-roundtrip.json |
| Paused utility restart; external move while stopped | Pause recovered; saved layout not overwritten | rail-restart-check.json |
| Two identical windows through utility restart | Independent layout records; no startup minimize | rail-duplicate-restart-check.json |
| Second utility launch | Exits successfully; no duplicate AppBar/controller | rail-duplicate-restart-check.json |
| Maximized minimize/restore | Maximized state restored with normal rectangle retained | rail-maximized-before.json, rail-maximized-after.json |
| Active owned modal | Two owner entries total; minimize hides group, restore focuses actual dialog HWND | rail-modal-roundtrip.json |
| Same executable restarted with two windows | Both fresh keys; zero inherited desktop layouts | rail-app-restart.json |
| Explicit integration retry | Ready again; both visible windows remain visible | rail-retry-recovery.json |
| Five-second target UI stall | Pending operation quarantined after two seconds; status/pause responsive; late completion clears without backlog | rail-stall-live.json |
| Fullscreen entry/exit | Passed after actual-foreground reconciliation and safe z-order flags; foreground remains with fixture | rail-fullscreen-during.json, rail-fullscreen-after.json, rail-fullscreen-pass.jsonl |
| Actual rail buttons through computer use | Both independent icons restore/focus then minimize; mouse pause and keyboard Space resume pass | rail-cua-first-click.json, rail-cua-second-click.json, rail-cua-second-window-restored.json, rail-cua-second-window-minimized.json, rail-cua-keyboard.json |
| Final portable build, empty managed inventory | 3,993,600 bytes private working set; 0 ms reported CPU in 30.007 seconds at finite counter resolution | rail-final-performance.json |

The stall script took 2282 ms including its explicit 2200 ms wait and two control
commands. This is responsiveness evidence, not event-to-native-action latency.
Private working set was read from Win32_PerfFormattedData_PerfProc_Process,
matched by process ID; private bytes were recorded separately. The earlier
rail-performance.json sample reported 4,145,152 bytes and 125 ms CPU but overlapped
the interactive test period; it is not a clean final idle certification. The
populated rail-managed-idle-performance.json experiment was interrupted and its
fixture cleaned up; do not use its declared inventory as stable-idle evidence.
Repeat a clean warmed sample with two pinned windows. See Agent-Handover for details.

The initial fullscreen fixture yielded the rail correctly but exposed missing
closing-notification recovery. The product now reconciles actual
fullscreen state on foreground events, shell notifications, and the existing one-second reconciliation, checking the foreground owner
rectangle against its monitor without extra timer wakeups or application messages. Z-order changes suppress owner reordering and reentrant position-changing callbacks. This
supplements the AppBar callback protocol illustrated in Microsoft's
[AppBar sample](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/Win7Samples/winui/shell/legacysamples/appbar/AppBar.cpp).

The Startup shortcut test uses an isolated temporary directory and verifies
creation/loading/removal; the real user Startup setting was left off. Corrupt
state and cross-session merge tests also use isolated directories. Real concurrent
interactive sessions remain a release-matrix case.

Obsidian, physical display changes, and real Explorer
restart remain listed explicitly in Implementation-Status. Controller diagnostics
must not be mistaken for certification of those user interactions.


The operator's initial physical test found restoration without the expected
second-click minimization. Computer use reproduced it. The focus guard now
retains the click's original foreground, allows focus when it has not changed,
and remembers pre-rail activation across keyboard/assistive input. Mouse
activation uses MA_NOACTIVATE. A late completion still cannot focus after the
user moves elsewhere.

Computer use clicked the real native rail buttons and independently verified
HWND state/foreground after each action for both fixture windows, then clicked
pause and used keyboard Space to resume. These checks temporarily enabled
`--automation-ui on`, exposing the same AppBar/button implementation as an
application window while preserving WS_EX_NOACTIVATE. The normal TOOLWINDOW
style is excluded by the automation helper (even with a previously selected
handle). `--automation-ui off` restores it; the mode is never persisted. Desktop
presence checks use the normal validated style, not this diagnostic mode.
