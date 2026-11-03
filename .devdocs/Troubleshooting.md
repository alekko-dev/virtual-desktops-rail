# Contributor troubleshooting

User-facing recovery steps are in the [user README](../README.md). These notes preserve
engineering findings and regression checks. Inspect bounded local diagnostics and
current process identity rather than relying on an old session snapshot.

For a user issue, Settings **Save diagnostic report…** creates a user-selected local ZIP.
It contains redacted build/OS/settings context, a fresh native observation for each
still-matching managed window, saved layouts and bounded structured event history. It
intentionally excludes the free-form local diagnostics log, titles, document contents,
handles, PIDs, full executable paths, desktop names/IDs and screenshots. The archive is
not uploaded. Ask the user to review it before choosing whether and where to share it.
Windows Snap Group membership is unavailable to the app; capture Task View separately
only if the user chooses to provide it.

## Native snapping and clipped action notices

See [Native Snap compatibility](Snapping.md) for verified mechanisms, the support
boundary and the implementation plan. A key verified defect is that placement can
update normal restore bounds while the window remains visibly snapped elsewhere;
checking normal bounds alone produces a false success. The app now models optional
arranged state and separate visible bounds; native group membership remains unmodeled. Some existing arrangements survive its
placement sequence, so do not claim every placement write destroys native snapping.

Snap-related intermittent timeout/quarantine remains a separate unproven diagnosis.
Preserve outstanding operations until verified; a responsive visible window is not
proof that all queued asynchronous native work has completed.

Settings control 6100 now contains status only. Notice control 6109 is a measured,
selectable read-only edit, hidden with zero reserved space when empty. It expands the
window and moves Version/Done below the message, with scrolling when height is capped.
Clip child painting and invalidate all children during relayout: parent background
repainting after shrink otherwise left partially erased checkbox text in a live screenshot.
Timeout wording describes unverified work rather than diagnosing a hung app; clear that
notice only after all quarantined slots resolve, without clearing unrelated notices.

## Integration unavailable after Windows updates

Earlier builds used an exact revision allowlist, then a newer-26300 opt-in. They could
show no desktop dots because the worker never initialized on a different revision.
Both restrictions are superseded by [current eligibility](Architecture.md#desktop-integration-and-eligibility).
Check the executable actually running, native OS build/revision and runtime diagnostics.
The registry ProductName can misleadingly retain a Windows 10 label.
Legacy experimental flags no longer enable additional builds. Test startup and reconnect
against the shared minimum; do not confuse an eligible build with a tested one.

## Transient pin-query failures

`ComError(HRESULT(0x80070490))` was observed around transient windows, but its cause
has not been reproduced sufficiently to call it harmless. Preserve strict classification:
only two `WindowNotFound` pin results mean unsupported. Mixed/other failures stay failures
and trigger unavailable/reconnect handling. Revalidate lifetime after querying and
attribute bounded diagnostics to class, HWND, PID, process creation time and both results;
do not collect titles. Regression coverage must distinguish unsupported from mixed failures.

## Fullscreen exit leaves the rail below normal windows

The AppBar initially yielded correctly but missed recovery after fullscreen closed.
Successful `SetWindowPos` did not mean topmost style remained set. The fix reconciles
actual foreground/monitor bounds on foreground events, shell notifications and existing
one-second reconciliation, and checks actual topmost state. Exclude shell desktop classes
from fullscreen inference. Keep z-order-only
`SWP_NOOWNERZORDER | SWP_NOSENDCHANGING` with `NOMOVE | NOSIZE | NOACTIVATE`;
the earlier raw `0x13` callback flags regressed behavior. Geometry-only AppBar position
reporting avoids z-order feedback. Do not add a permanent frame timer.

Verify fullscreen entry and exit in normal passive style, unchanged application
foreground, yielding/topmost state and work-area restoration. Reference observations:
`rail-packaged-fullscreen-during.json` and `rail-packaged-fullscreen-after.json`.

## Real icon click restores without focus; second click fails to minimize

CLI click testing missed this physical-input bug. The passive rail had not become
foreground, so the original guard rejected explicit restore focus. Assistive activation
could also replace the pre-click foreground with the rail itself.

Preserve `Job::focus_from`, allow explicit focus only while original foreground remains
unchanged, retain pre-rail activation from `WM_ACTIVATE`, and return `MA_NOACTIVATE`
on `WM_MOUSEACTIVATE`. Mouse activation must not overwrite the saved foreground hint;
non-window rail actions clear stale hints. Late or superseded completion cannot focus
after the user moves elsewhere. Minimized state takes precedence over stale foreground.
Regression-test actual pointer restore/focus/second-click minimize on two separate icons
and keyboard activation; also verify late work never steals focus.

## Owner restores but its modal remains hidden

`GetLastActivePopup` can return the owner after a dialog is hidden. Owner-only
`ShowWindowAsync(SW_RESTORE)` did not reveal the fixture modal. Remember and revalidate
the owned dialog, reveal it and request user-initiated focus there. Do not save/reposition
it independently or create another rail entry. The spike's hidden standard-dialog lookup
was fixture-specific and is not a general application-discovery policy.
Verify the whole owner/modal group through minimize, explicit restore and desktop switching.

## Blurred application icons and dark transparent edges

Use the same 16-DIP pixel calculation for worker extraction and painting. Deriving
icon size from scaled rail width minus fixed pixel padding produced unusual sizes
such as 22 pixels at 125%, while small-icon-first extraction could enlarge a smaller
source. Compare available window icon dimensions before resizing and keep all
extraction/filtering off the UI thread.

WIC HICON import exposes premultiplied HICON bytes as BGRA. Converting that import
to PBGRA multiplies alpha again and darkens edges. Re-label its copied bytes as
PBGRA before Fant filtering instead. The synthetic checkerboard regression verifies
filtered alpha and color across two resizes, including a partially transparent source.

## Passive rail absent from desktop automation

Normal TOOLWINDOW style is intentionally omitted by some inventories; previously selected
targets can become unusable when that style returns. Use the temporary `--automation-ui`
mode described in [Testing](Testing.md#computer-use-procedure), select an observed target,
refresh after each render and restore off. Input checks in this mode do not certify
normal-style cross-desktop presence. Follow the current installed computer-use skill,
not historical library snippets or restrictions copied from old agent sessions.

`GetCursorPos` access denied plus no foreground window occurred when the input desktop
locked. Stop UI input and request unlock if needed; continue independent work. Do not
automate lock/security screens or classify that interval as a passing run.

## Build/cache/report replacement failures

- If Cargo is absent from PATH, scripts use the user `.cargo/bin/cargo.exe`; reopen the
  terminal after installing tools. Preserve the pinned toolchain and `Cargo.lock`.
- If native cache access is sandbox-blocked, use the environment's supported execution
  approval mechanism. Regenerating the lockfile does not repair access permissions.
- Offline metadata without `--filter-platform x86_64-pc-windows-msvc` can try fetching
  uncached ARM packages. Use the filtered command in [Development](Development.md).
- A running executable can block relinking. Exit the verified instance or choose another
  Cargo target directory; the policy change used `artifacts/compatibility-policy-build`.
- Atomic report replacement failed in the default temporary location during a previous
  run. A workspace-owned temporary directory resolved it; keep real user state isolated.
- Packaging needs installed Rust documentation license files. Do not omit notices to
  make packaging succeed; install the missing component and retry.

## Misleading layout or performance observations

### Outlook reappears after returning to a saved desktop

The user-supplied `VirtualDesktopRail_diagnostic_1793701509_Outlook_popedup_on_desktop.zip`
provides direct action evidence on 2026-11-03: report version 0.1.0, clean commit
`0b593b1131a8fa297b06cfc150271c4495bf1a18`, Windows 26300.9711. Auto-restore was enabled,
pause was off, and Outlook had a saved Desktop 3 layout. At 11:24:53.248 CET an Outlook
action completed verified with `minimized: true` on Desktop 4. Returning to Desktop 3
was observed at 11:24:55.299; Rail accepted sequence 34 with the saved Desktop 3
geometry at 11:24:55.428 and verified it unminimized at 11:24:55.438. The report's
fresh native observation also found Outlook visible and unminimized, without pending
or blocked work. Earlier returns to Desktop 3 show the same restoration pattern.

This identifies Rail restoration on desktop return, consistent with that build's
auto-restore policy. Source at the reported commit confirms that its controller
chooses `RestoreAutomatic` whenever auto-restore is enabled and the destination has
a layout, even for a minimized window. That record had no independent minimized intent.
The fix adds `minimized_desktops` separately from geometry: current verified manual
minimization or trusted external minimize evidence sets it; manual restoration clears
it. Automatic minimization/restoration does neither. Destination intent overrides a
saved layout, including after a late restore completes. The timeout path preserves
the single latest desired action while quarantined so late work can reconcile.

Evidence limits: the event tail does not establish when the user manually minimized
Outlook on Desktop 3 or whether foreground focus changed. Action entries do not label
automatic versus explicit intent; their timing and saved targets support the automatic
path. This was archive/source analysis, not a live Outlook reproduction or Snap-group
diagnosis.

External restoration can briefly report minimized/off-screen geometry. Require stable,
reachable observations at least 100 ms apart; exclude drags and pending/late own actions.
Automatic restoration must not learn geometry. Preserve outstanding operation slots after
partial dispatch failure and quarantine timeouts until observation or identity death.

Do not call an interrupted or changing fixture workload warmed idle, equate API acceptance
with completion, or treat process counters reporting 0 ms as literal zero CPU. Use
[the measurement procedure and scoped evidence](Testing.md#resource-and-latency-measurement).
