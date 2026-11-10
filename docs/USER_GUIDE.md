# User Guide

This guide covers detailed controls, layout and Snap behavior, troubleshooting, and build/diagnostic details for Virtual Desktop Rail. For a quick overview and installation steps, see the [main README](../README.md).

## Using the rail

A narrow 25-DIP rail sits on the left edge of your primary display, reserving space beside maximized apps. Application icons use a consistent 16-DIP size with DPI-aware source selection and filtered resizing.

| Control or state | Behavior |
| --- | --- |
| Desktop dot | Click to switch; hover for its name. The current desktop is a larger filled circle; others are colored outlines. |
| Minimized window icon | Restore its current-desktop layout and request focus. |
| Visible background window icon | Bring the window forward. |
| Foreground window icon | Minimize the window. |
| Under-icon indicator | Solid means visible; outlined means minimized. App icons keep their normal appearance. |
| Light switch above Pause | Toggle Windows light/dark colors for both apps and the system. The tooltip names the next mode; from a custom mixed mode, the app color mode determines the next mode. Apps with their own theme override may keep their chosen appearance. |
| Pause / Resume | Suspend or resume automatic window actions. Manual controls continue working. |
| Sliders button | Open Settings. Done closes Settings. |
| Save diagnostic report… | Choose a local ZIP destination for the redacted support report. |
| Button below Settings | Exit the app and release the reserved screen space. |

In Settings, enable **Tint rail with active desktop color** to give the whole rail a subtle tint matching the current desktop dot. The option is off by default, applies immediately, and survives restart. Light and dark themes use a muted blend; Windows high-contrast mode keeps system colors.

## Layouts and automatic restoration

By default, switching desktops minimizes shared windows; returning does not reveal them until requested. With no saved layout, a manual rail restore uses the last usable layout adjusted to stay on an available display.

Enable **Restore saved layouts on desktop switch** in Settings to restore windows that have a saved destination layout, without taking focus. Windows without a layout there still minimize. Manual rail restores and stable external Windows restores (such as taskbar restores) save the resulting layout for that desktop, except for the temporary Snap reveals described below. Move, resize, and maximize normally to update it. Automatic restoration preserves saved geometry; minimizing does not forget layouts.

### Minimized preferences

Minimizing a shared window yourself, using its normal controls or the rail, remembers it as minimized on that desktop. It stays minimized when you return, including with automatic restoration enabled. Other desktops keep their own state. Restore it manually to allow automatic restoration there again.

Rail's automatic minimization on switching desktops does not change this preference. These preferences survive rail restarts for the same live windows. Changes while Rail is stopped or desktop integration is unavailable cannot be reliably attributed to a desktop and are not inferred on recovery.

### Pause and window identity

Pause suspends both automatic minimization and restoration. Starting the app, resuming, recovering integration, or changing either window restoration setting does not immediately move windows; the next desktop switch applies the policy.

Layouts survive rail restarts while their application windows remain running. Restarting an application gives its windows fresh identities rather than guessing which old layout belongs to which window.

## Windows Snap behavior

Native Windows Snap support is limited:

- New layouts save snapped visible bounds separately from the original drag-away size.
- The rail can preserve a matching live arrangement by minimizing and revealing it without rewriting its placement, and verifies the visible result. Automatic reveals preserve focus.
- Recreating lost or different native Snap groups is unsupported; incompatible saved arrangements and changed displays can be declined. Application-specific constraints remain unverified. Pause automatic actions when using unsupported arrangements.
- The rail leaves a known minimized Snap arrangement untouched when an ordinary saved layout would replace it, and declines saved Snap restoration from a minimized maximized state.
- **Minimize snapped windows on other desktops** is enabled by default in Settings, including after upgrading an existing configuration. With automatic restoration on, a snapped window stays minimized on desktops whose saved layout does not match its live Snap arrangement, even if they have an ordinary saved position. A matching saved Snap layout can still restore. Turn the option off to retain the snapped window's visibility when an incompatible automatic restore is declined.
- You can explicitly reveal a preserved snapped window with its rail button on another desktop. This keeps its Snap placement and that desktop's existing saved layout; it does not make the temporary reveal an automatic restore preference there.
- A declined restore's Snap notice clears after that window successfully restores or raises, or a fresh observation confirms the saved target is compatible again. Merely becoming visible does not clear an unresolved mismatch; removing the window clears its notice.
- Exact last-visit restoration, including windows left minimized, is still in progress.

## Troubleshooting and limitations

- **No desktop dots or window controls:** read the integration status in Settings. Below-minimum Windows builds are blocked. For initialization or runtime failures, use **Retry desktop integration**. An amber warning triangle replaces the normal Play/Pause icon while integration is unavailable; its tooltip explains the status.
- **Missing window:** check Task View pinning, including app-level pinning. Unsupported or inaccessible windows are omitted. This preview expects about two or three shared windows and has no scrolling or overflow controls.
- **Layout not reused after an app restart:** this is the conservative identity policy described above. Arrange the new window to establish new layouts.
- **Startup uses an old path:** disable startup and enable it again from the new location.

### Local storage and limits

Configuration, previous-state backup, and bounded diagnostics live under `%LOCALAPPDATA%\VirtualDesktopRail`. Layout history is limited to 512 window identities and 64 remembered desktops per identity (layouts and minimized preferences combined); new identities are omitted at the identity limit. There is no history-management UI yet.

### Diagnostic reports

**Save diagnostic report…** in Settings creates a local ZIP with the app/build context, saved and freshly observed managed-window geometry/state, and a bounded event history of desktop transitions, accepted and verified window actions, integration readiness, and selected setting changes.

It excludes window titles, document contents, process IDs, full executable paths, desktop names/IDs, and screenshots. It does not include native Snap Group membership because Windows does not expose that state to the app. Nothing is uploaded; review the archive before sharing.

### Validation scope

Meeting the minimum Windows build (`26100.2605`) allows integration to initialize; it does not guarantee compatibility with every later Windows revision or application. Elevated applications and unusual shell/dialog surfaces are outside the validated scope.

Physical hardware, mixed DPI and monitor changes, actual Explorer restart, screen-reader/high-contrast review, and the full Obsidian workflow still need broader validation. The preview does not promise support for every application or privilege level. Third-party notices and license texts accompany portable packages.

## Version and build identity

Settings shows the application version and source identity in small gray text below the integration status. To inspect an executable without starting the app, capture its output with PowerShell:

```powershell
$p = Start-Process ./virtual-desktop-rail.exe -ArgumentList '--version' -NoNewWindow -Wait -PassThru -RedirectStandardOutput version.txt
Get-Content ./version.txt
```

Use `--version --json` for machine-readable version, full Git commit, and dirty state. Windows file Properties also includes File version and Product version. A `dirty` marker means source changes were present at build time; `unknown source` means Git provenance was unavailable. Neither marker changes the application release number.

Portable packages include `BUILD.json` and `SHA256.json` to identify the exact executable. Diagnostic `--status` reports the running instance's build, whereas `--version` describes the executable being invoked.
