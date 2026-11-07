# Contributor troubleshooting

User-facing recovery is in the [app README](../README.md). This document owns
reusable symptoms, known mechanisms and proven fixes. Current contracts are in
[Architecture](Architecture.md); validation procedures/results are in [Testing](Testing.md).
For user evidence, use the local support report and its [privacy contract](Architecture.md#stack-and-source-map).
It is not uploaded automatically; user review/sharing remains their decision.

## Snapped restoration reports success at the wrong visible size

`GetWindowPlacement` normal restore bounds can match a request while the actual
window stays snapped elsewhere. Conversely, an automatic placement sequence can
make visible bounds normal-sized while `IsWindowArranged` remains true. This caused
an actual-product false success. Verify visible bounds, normal restore bounds and
native state together; see the [current preservation path](Architecture.md#native-snap-preservation).

Native harness checks also established that placement writes do not invariably
destroy snapping. `SW_SHOWNA` left a minimized fixture minimized; `SW_SHOWNOACTIVATE`
revealed a minimized arrangement without focus, but could exit arranged state when
applied to an already-visible snapped window. Starting state matters: a visible
matching arrangement must receive no show/placement command. Arranged state can be
false while minimized, so saved visible evidence selects a candidate and post-reveal
observations establish success. Window placement uses workspace coordinates; convert
to screen coordinates before comparing visible geometry.

For an explicit normal-layout transition, a native harness successfully revealed a
minimized window, observed it, issued the no-activation show command on the visible
arranged window, observed arranged false, then applied normal placement. This is a
candidate for the remaining transition work, not the current automatic product path.
Do not issue unobserved show commands back-to-back or use activating `SW_RESTORE`
for automatic restoration.

Regression-check actual automatic and manual saved/unsaved round trips, both starting
states, original drag-away size and Task View Group membership. Matching rectangles
or an arranged flag do not establish native group membership or reconstruction.
An intermittent timeout is a separate diagnosis; do not infer its cause from these
geometry failures or release outstanding work without completion evidence.

## Action notices clip, persist or erase controls

Use separate status and measured read-only notice controls. Expand only while a notice
exists, move Version/Done below it, clamp to work area and allow selection/scrolling.
Child clipping and child redraw prevent parent background painting from erasing
checkbox labels during shrink. Clear an action-timeout notice only after all timed-out
slots resolve; preserve unrelated notices. Wording must describe unverified work,
not assert a hung target. Check compact, one-line, wrapped and oversized notices,
late completion, keyboard focus and DPI changes. Remaining coverage is in [Status](Status-and-Roadmap.md).

## Integration unavailable after Windows updates

Check the executable actually running, native build/revision and runtime diagnostics
against [current eligibility](Architecture.md#desktop-integration-and-eligibility).
Registry ProductName can misleadingly retain a Windows 10 label. Legacy revision
opt-ins no longer enable additional builds. Eligibility is not successful initialization
or release certification; startup/reconnect must fail safely and retry from a baseline.

## Transient pin-query failures

Only both pin queries returning `WindowNotFound` means no shell ApplicationView.
Mixed/other errors remain failures. Do not mask transient `0x80070490` as an unsupported
window. Revalidate candidate lifetime and attribute bounded observations to class,
handle/process/lifetime and both query results without titles. Regression checks must
distinguish unsupported candidates from mixed failures. Root-cause work belongs in [Status](Status-and-Roadmap.md).

## Fullscreen exit leaves the rail below normal windows

API acceptance did not guarantee the rail's topmost style survived. Reconcile actual
foreground/monitor bounds after shell notifications, foreground events and existing
one-second reconciliation. Exclude desktop shell classes from fullscreen inference.
Use z-order-only `SWP_NOOWNERZORDER | SWP_NOSENDCHANGING` with
`NOMOVE | NOSIZE | NOACTIVATE`; avoid owner reordering and recursive AppBar negotiation.
Report geometry changes to the shell only for actual geometry changes.
Check passive-style entry/exit, actual topmost/yielding state, application foreground
and released work-area reservation. Do not add a permanent frame timer.

## Rail disappears during animated desktop switches

On the locally tested Windows revision, `DWMWA_EXCLUDED_FROM_PEEK` removes rail pixels
from the shell transition as well as desktop thumbnails, while the window remains
uncloaked/topmost and its AppBar reservation persists. The [current presentation](Architecture.md#rail-presentation)
retains exclusion and reveals afterward. Do not diagnose the gap from cloak flags alone.

`DWMWA_DISALLOW_PEEK` did not remove the rail from desktop thumbnails. Combining it
or `DWMWA_TRANSITIONS_FORCEDISABLED` with Peek exclusion did not cure the omission.
Capture exclusion removed thumbnails locally but also changes screenshot/screen-sharing
behavior; it is not an equivalent replacement. Microsoft's documented Peek contract
does not guarantee Task View behavior: see [DWM attributes](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)
and [display affinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity).

The desktop-change event can arrive before the animation finishes. Task View can
also close without a desktop change. Hold a previously foreground Task View host until
it hides/cloaks; start its reveal then, without another desktop-event settling wait.
Respect the Windows animation preference. Test the shell's animated path, actual
thumbnails and composed pixels; an instant `winvd` switch did not reproduce the gap.
Current trigger limits, including cancelled gestures, belong in [Status](Status-and-Roadmap.md#known-defects-and-unmet-behavior).

## Real icon click restores without focus; second click fails to minimize

CLI click testing missed this physical-input defect: a passive rail click did not
make the rail foreground, so the old guard rejected intended restore focus. Assistive
activation can also replace the application's foreground with the rail itself.
Preserve the pre-click foreground in `Job::focus_from` / `WM_ACTIVATE`, return
`MA_NOACTIVATE` on `WM_MOUSEACTIVATE`, and validate the original intent before focus.
Mouse activation must not overwrite that saved foreground hint. Non-window actions
clear stale hints; minimized state wins over stale foreground.
Test actual two-icon restore/focus/second-click behavior, keyboard activation and late
completion after the user moves elsewhere. [Architecture](Architecture.md#layout-identity-storage-and-action-ordering)
owns the focus and action-ordering invariants.

## Auto-restored or snapped window appears above the focused foreground window without focus

Win32 `ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE)` and `SetWindowPlacement` (`SW_SHOWNOACTIVATE`)
insert an un-minimized window at `HWND_TOP` without activating it, leaving it visually on top
of the destination desktop's active foreground window. Capture the window's pre-minimize
`z_above` peer chain (`GW_HWNDPREV` across visible normal windows, allowing `DWM_CLOAKED_SHELL`
peers on other desktops) during snapshots and immediately before `Effect::Minimize`, retain
it across minimization, and on `Action::RestoreAutomatic` place the window behind the first
live uncloaked destination peer (`DWMWA_CLOAKED == 0`) with Z-order-only `SetWindowPos`
(`SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS`),
re-reconciling during post-reveal verification if asynchronous reveal resets Z-order.

## Owner restores but its modal remains hidden

`GetLastActivePopup` can return the owner once a dialog is hidden. Remember and
revalidate the active owned dialog, reveal/focus it with its owner when intended,
and keep it out of independent layouts/buttons. The fixture's hidden standard-dialog
lookup is not a general application-discovery rule. Check the entire owner/modal
interaction group through minimize, explicit restore and desktop switching.

## Blurred icons and dark transparent edges

Use the [shared icon size/selection pipeline](Architecture.md#stack-and-source-map).
Rail width minus fixed padding previously produced nonstandard sizes, and small-icon-
first selection enlarged undersized art. WIC HICON import exposes premultiplied bytes
as BGRA; converting those bytes to PBGRA multiplies alpha again. Re-label them PBGRA
before Fant filtering. Keep selection/filtering off the UI thread. Verify analytic
transparency/filtering regressions and actual icons across DPI settings.

## Passive rail absent from desktop automation

Normal TOOLWINDOW style can be omitted by automation inventories. Follow the
[temporary visibility procedure](Testing.md#computer-use-procedure), refresh observed
targets and restore visibility off. Switching with exposed APPWINDOW style can leave
a shell-cloaked view even after restoring tool style; restart only the verified owned
test instance if needed. Do not certify passive presence from exposed input tests.

Capturing/activating a target can change foreground, restore a snapped target or close
Task View before measurement starts. Use an independent owned observer and begin native
sampling before dismissal. Explorer's Task View shell entry opens the surface; it is
not a reliable close toggle, and its short-lived activation process can return 1 even
when the UI opened. Verify the actual surface. On a locked desktop, stop input and
refresh only after unlock; a black capture/no foreground is not a passing interval.

## Build, dependency and file-replacement failures

- If Cargo is absent from PATH, scripts locate the user's `.cargo/bin`; reopen the
  terminal after installation. Preserve the pinned toolchain and lockfile.
- If native cache access is sandbox-blocked, use supported execution approval;
  regenerating the lockfile does not repair permissions.
- Filter offline metadata for x64 as shown in [Development](Development.md#build-and-run)
  to avoid requesting uncached ARM dependencies.
- A running executable can block relinking. Exit the verified instance or set another
  `CARGO_TARGET_DIR`. Do not stop unrelated processes to clear a lock.
- If atomic replacement is denied in the default temp directory, use a workspace-owned
  temporary directory and isolate user state; do not weaken replacement/recovery checks.
- Packaging needs installed Rust documentation/license files. Install the missing
  component rather than omitting required notices.

## A manually minimized window returns on a saved desktop

Older builds restored every saved destination layout without independent minimized
intent. Support-archive/source analysis identified this behavior for Outlook; the
[evidence record](Testing.md#compatibility-and-completed-validation) states what it
establishes and what it cannot. Current geometry and minimized preferences are
separate; see [the intent contract](Architecture.md#product-contract-and-boundaries).
Check per-desktop intent and accepted/completed actions rather than treating this as
a Snap-group or focus diagnosis.

Three observation failures required distinct guards: drag snapshots discarded an
external-restore candidate while updating cached visibility; post-action suppression
discarded restoration before the first post-minimize snapshot; and equal-tick refresh
bursts invalidated otherwise valid candidates. Retain candidates through dragging and
restart stability at its boundaries. Retain completion provenance only for a current
verified minimize so its first fresh visible snapshot can seed a candidate. Separate
context validity from sample freshness: duplicate ticks neither advance stability nor
replace cached geometry/visibility, but pending actions, recovery and desktop changes
still invalidate context. Never extend the exception to late, cancelled, timed-out
or superseded work. Regression-check exact delivery orders deterministically and
verify actual visibility/intent independently through the minimized-intent procedure.

## Misleading geometry and performance observations

Transient minimized/off-screen geometry is not stable manual intent; apply the
observation guards in [Architecture](Architecture.md#layout-identity-storage-and-action-ordering).
Sequential snapshots are not atomic. Reject changed workloads and cleanup-overlapped
resource samples using [the measurement procedure](Testing.md#resource-and-latency-measurement).
