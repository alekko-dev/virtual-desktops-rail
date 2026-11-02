# Virtual Desktop Rail

A lightweight Windows utility for keeping shared application windows useful across
virtual desktops without leaving them visible everywhere. A narrow rail shows your
desktops and shared windows, and remembers a separate window layout for each desktop.

**Switch desktop → shared windows minimize → click an icon to restore that desktop's layout.**

This is an unsigned development preview. Broader release validation is still in progress.

## Features

- A 25-DIP rail on the primary display's left edge reserves space beside maximized apps.
- Desktop dots show the current desktop, reveal names on hover, and switch desktops on click.
- Windows pinned through Task View appear automatically, including application-level pins.
- Each window remembers its position, size, and normal/maximized state per desktop.
- Optional automatic restoration restores saved destination layouts without taking focus.
- Pause/resume, optional startup with Windows, and local settings and layout storage.
- Native Windows UI, no installer, no administrator privileges, and no network requirement for use.

## Requirements

- Windows 11 x64, 24H2 build **26100.2605 or newer**.
- A signed-in Windows Explorer desktop session.
- Ordinary application windows that Windows can show on all desktops.

Meeting the minimum Windows build allows integration to initialize; it does not
guarantee compatibility with every later Windows revision or application. Elevated
applications and unusual shell/dialog surfaces are outside the validated scope.
The C runtime is statically linked, so no separate Visual C++ runtime installation is needed.

## Installation and updates

1. Obtain a portable Windows x64 ZIP from the project's release assets, when available,
   or a build provided by the maintainer. Source archives are not runnable app packages.
2. Extract the whole package into a permanent folder you can write to.
3. Run `virtual-desktop-rail.exe`. Startup with Windows is off by default.

To update, exit the running app using the button below Settings, extract the new
package, and launch its executable. Settings and layouts remain in your local app-data
folder. If you move the executable after enabling startup, turn startup off and back
on in Settings at the new location.

To remove the app, turn startup off, exit, and delete its extracted folder. Optionally
remove `%LOCALAPPDATA%\VirtualDesktopRail` to delete saved settings, layouts, and diagnostics.

## Getting started

1. Open Windows Task View using the taskbar's Task View button.
2. Right-click an application window and choose **Show this window on all desktops**, or
   **Show windows from this app on all desktops**. Pinning to the taskbar is different.
3. Its icon appears on the rail, normally within about a second.
4. On one desktop, click the icon and arrange the window using its usual Windows controls.
5. Switch to another desktop. The shared window minimizes. Click its icon and arrange
   it for this desktop.
6. Return to the first desktop and click the icon to restore its remembered layout.

Unpinning or closing a window removes its rail button. Each button represents one
window, so multiple windows from the same application can have identical icons.

## Using the rail

| Control or state | Behavior |
| --- | --- |
| Desktop dot | Click to switch; hover for its name. The current desktop is a larger filled circle; others are colored outlines. |
| Minimized window icon | Restore its current-desktop layout and request focus. |
| Visible background window icon | Bring the window forward. |
| Foreground window icon | Minimize the window. |
| Under-icon indicator | Solid means visible; outlined means minimized. App icons keep their normal appearance. |
| Play/Pause | Suspend or resume automatic window actions. Manual controls continue working. |
| Sliders button | Open Settings. Done closes Settings. |
| Button below Settings | Exit the app and release the reserved screen space. |

Fullscreen applications can cover the rail. The rail requests exclusion from DWM Peek
previews but remains visible in screenshots and screen sharing.

### Layouts and automatic restoration

By default, switching desktops minimizes shared windows; returning does not reveal
them until requested. With no saved layout, a manual rail restore uses the last usable
layout adjusted to stay on an available display.

Enable **Restore saved layouts on desktop switch** in Settings to restore windows
that have a saved destination layout, without taking focus. Windows without a layout
there still minimize. Manual rail restores and stable external Windows restores
(such as taskbar restores) save the resulting layout for that desktop. Move, resize,
and maximize normally to update it. Automatic restoration preserves saved geometry;
minimizing does not forget layouts.

Pause suspends both automatic minimization and restoration. Starting the app,
resuming, recovering integration, or changing the restoration setting does not
immediately move windows; the next desktop switch applies the policy.

Layouts survive rail restarts while their application windows remain running.
Restarting an application gives its windows fresh identities rather than guessing
which old layout belongs to which window.

## Troubleshooting and limitations

- **No desktop dots or window controls:** read the integration status in Settings.
  Below-minimum Windows builds are blocked. For initialization or runtime failures,
  use **Retry desktop integration**. An amber warning triangle replaces the normal
  Play/Pause icon while integration is unavailable; its tooltip explains the status.
- **Missing window:** check Task View pinning, including app-level pinning. Unsupported
  or inaccessible windows are omitted. This preview expects about two or three shared
  windows and has no scrolling or overflow controls.
- **Layout not reused after an app restart:** this is the conservative identity policy
  described above. Arrange the new window to establish new layouts.
- **Startup uses an old path:** disable startup and enable it again from the new location.

Configuration, previous-state backup, and bounded diagnostics live under
`%LOCALAPPDATA%\VirtualDesktopRail`. Layout history is limited to 512 window identities
and 64 desktop layouts per identity; new identities are omitted at the identity limit.
There is no history-management UI yet.

Physical hardware, mixed DPI and monitor changes, actual Explorer restart,
screen-reader/high-contrast review, and the full Obsidian workflow still need broader
validation. The preview does not promise support for every application or privilege level.
Third-party notices and license texts accompany portable packages.

<!-- repository-links:start -->
## Contributing

For building from source, architecture, testing, and implementation status, start with
the [contributor documentation](.devdocs/README.md).
<!-- repository-links:end -->
