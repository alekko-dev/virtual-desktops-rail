# Optional auto-restore — 2026-11-02

The approved behavior is implemented behind **Restore saved layouts on desktop
switch** in Settings. It defaults to off; schema-1 files without `auto_restore`
continue using minimize-on-switch/restore-on-demand. The preference is persisted,
merged with other sessions, exposed in `--status`, and controllable through
`scripts/Invoke-Rail.ps1 --auto-restore on|off`.

## Behavior

- On each confirmed switch, independently for each eligible pinned window:
  restore its saved destination layout without requesting foreground if one
  exists; otherwise minimize it. A new desktop gets no fallback auto-restoration.
- Manual rail restoration saves the verified resulting layout, including the
  normal rectangle underlying maximized state. A stable external Windows restore
  (such as taskbar restoration) establishes or updates that desktop layout too.
  Require matching reachable observations at least 100 ms apart for external
  restores, avoiding transient minimized/off-screen coordinates.
- Automatic restoration never rewrites saved layout memory or last-usable
  fallback. A manual minimize retains layouts, so a later switch back restores
  the window again. User move/resize/maximize continues updating desktop memory.
- Pause suspends automatic minimize and restore. Startup, discovery, recovery,
  resume and preference changes do not immediately move or reveal windows.
- Automatic restores share generation/sequence cancellation, per-window single
  outstanding operation and timeout quarantine. A late restore is minimized when
  the latest destination has no layout. Already accepted native work cannot be
  retracted; do not release its slot after a partial dispatch failure.
- The validated OS matrix and explicit experimental newer-26300 opt-in are
  unchanged. This does not certify the user's physical PC or revision 9818.

Native passive restoration sets destination placement and restore-to-maximized
state while minimized, then reveals with `SW_SHOWNOACTIVATE`. It does not issue
`SetForegroundWindow` for automatic work. Manual restoration retains the existing
guarded focus behavior. Ordinary icons and their under-icon indicators remain.

## Automated checks

`scripts/check-dev.ps1`: formatting, warnings-denied Clippy, **51 tests** and all
locked release binaries passed. The controller now has 27 regression scenarios,
including mixed saved/unsaved windows, first manual and external restoration,
stable geometry, automatic memory preservation, startup/recovery, pause/resume,
setting cancellation, in-flight minimize/restore ordering and late verification.
Legacy settings and cross-session preference merging also cover `auto_restore`.

## Live verification

Windows 11 x64 **26300.9711**, Explorer session 2, 2560 × 1440 at 120 DPI, using
the existing explicit experimental-integration option. Tests used owned native
windows and preserved the user's original two desktops. JSON below is under the
ignored local `artifacts/` directory.

| Scenario | Result / evidence |
| --- | --- |
| Two pinned windows on an unsaved destination | Both get minimized, no layouts invented; `auto-no-layout.json` |
| Restore one through rail, return to destination | Exactly that window restores, unsaved window stays minimized; `auto-rail-manual.json`, `auto-return-normal.json` |
| Newly created temporary desktop | Both get minimized with no layouts for that GUID; `auto-fresh-desktop-final.json` |
| External Windows restoration without a rail click | Resulting normal/maximized layout saved after stable observations; `auto-external-windows-restore.json`, `auto-external-saved-final.json` |
| Normal and maximized destination layouts | Both verified, no timeout/quarantine; `auto-normal-after-max.json`, `auto-max-restored-attempt.json` |
| Automatic maximized restoration preserves foreground | A separate unmanaged fixture window remains actual foreground across the roundtrip; saved geometry unchanged; `auto-anchor-foreground-before-final.json`, `auto-anchor-foreground-after-final.json`, `auto-focus-max-final.json` |
| Pause and resume | Switch while paused preserves placement and visibility; resume does not reveal or minimize; `auto-paused-final.json`, `auto-resumed-final.json` |
| Settings interaction | Real pointer toggles off/on, policy updates without immediate window changes; `auto-settings-off.json`, `auto-settings-on.json` |
| Persistence, startup and integration retry | Same live identity, saved preference, minimized window remains minimized; `auto-startup-final.json`, `auto-recovery-final.json` |
| Final executable: real native rail-icon click | Maximized restore and destination layout verified; `auto-real-rail-restore-final.json` |
| Exact packaged executable | Automatic maximized restore, opt-out default minimize behavior, opt-in without immediate reveal, then restoration on next switch; `auto-packaged-restored.json`, `auto-packaged-disabled.json`, `auto-packaged-reenabled.json` |

The external restoration check uses the owned-fixture worker's Windows
`ShowWindowAsync(SW_RESTORE)` path, rather than a literal taskbar-button click.
It verifies the same externally observed minimize-to-visible state transition
used by taskbar restoration. Pointer input independently verified the Settings
checkbox and actual rail icon. A separate copied anchor executable encountered
a computer-use app-approval timeout; the focus test instead used the already
available fixture's second, independently unmanaged window.

Physical-machine, screen-reader/high-contrast, Obsidian, monitor changes and
recoverable-checkpoint shell/session tests remain the release-matrix follow-ups
recorded in [Validation-2026-11-02](Validation-2026-11-02.md).

## Portable build

`artifacts/VirtualDesktopRail-nightly-2026-11-02-autorestore-windows-x64.zip`
contains the executable, updated README, feature notes, SHA-256 and third-party
licenses. `auto-package-verification.json` records the release, extracted and ZIP
entry checksum match and live packaged-process identity. No public upload or Git
push is part of this work.

Final executable SHA-256:
`12959A9BB20FFB580947A09396522BEF36BDB6E627E461443D0F8209C5F19378`.
The package contains 66 license files. Owned fixtures were unpinned and stopped
after executable/creation-time verification. The known temporary desktop was
removed through the existing empty-desktop guard after returning to the original
desktop. `auto-cleanup-desktop.json` and `auto-cleanup-final.json` record cleanup.
The packaged app remains running, ready, normal passive style, no managed test
windows, startup/pause off, experimental integration on, and auto-restore off to
preserve the pre-test default. Both original user desktops are intact.
