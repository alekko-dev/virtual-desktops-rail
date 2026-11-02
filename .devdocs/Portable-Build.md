# Virtual Desktop Rail — development build

Extract this directory and run `virtual-desktop-rail.exe` in an unlocked Windows
Explorer session. No installer or administrator privileges are required. The
Microsoft C runtime is statically linked. This local build is unsigned.

Shell integration attempts initialization on Windows 11 x64 **24H2 build
26100.2605 or newer**, using `winvd 0.0.49`'s minimum rather than an exact-revision
allowlist. Successful initialization and runtime queries are still required;
meeting the minimum does not certify every later revision.

If there are no desktop dots or pinned-window controls, read the integration
status in Settings. Versions below the minimum are blocked; initialization or
runtime failures disable dependent controls and can be retried. The old
experimental checkbox is disabled. Its saved preference and CLI flag remain
accepted for compatibility but no longer affect eligibility.

The left rail reserves 25 DIP on the primary monitor. Desktop dots switch
desktops; hovering shows names. Dots receive automatic distinct colors, with a larger filled circle for the active desktop and colored outlines for the others. App icons represent eligible windows pinned
through Windows Task View, including application-level pinning. Pin discovery
uses cached candidates once per second. Expect approximately two or three
shared windows; this MVP has no overflow controls.

Application icons retain their normal appearance when minimized. Only the
indicator underneath changes: solid when visible, outlined when minimized.

Switching desktops minimizes visible shared windows. Clicking an icon restores
its layout for the current desktop, raises a visible background window, or
minimizes a foreground window. By default, returning leaves windows minimized.
Move, resize, and maximize using the application's normal controls. Each window
remembers its own desktop layouts. Utility restarts preserve duplicate identities;
application restarts create new identities rather than guessing old matches.

For automatic restoration, enable **Restore saved layouts on desktop switch** in
Settings. This preference defaults to off and saves automatically. Each window
with a layout saved for the destination restores there without taking focus;
each window without one gets minimized, including on newly created desktops.
Manually restoring through the rail or taskbar saves the resulting desktop layout.
External restores wait for stable, reachable geometry before saving. Automatic
restores do not overwrite that memory. Minimizing a window keeps its layouts.
Startup, recovery, resume and enabling the setting do not move or reveal windows;
the next desktop switch applies the policy.

The bottom pause button suspends automatic window actions; manual controls continue
working. The sliders button opens settings, including start with Windows, integration
retry, and exit. `virtual-desktop-rail.exe --settings` also opens settings.
Startup is opt-in and uses a per-user Startup shortcut. Moving this directory
after enabling startup requires turning startup off and back on at the new path.

Configuration, previous-state backup, and bounded diagnostics are local under
`%LOCALAPPDATA%\VirtualDesktopRail`. The executable needs no network connection.
No HWND is persisted as an identity. History is bounded to 512 window identities
and 64 desktop layouts per identity. Inaccessible, elevated, unusual shell or
dialog surfaces are outside the validated ordinary-window scope.

This is a development MVP, not a broadly certified release. Physical hardware,
mixed DPI/monitor changes, real Explorer restart, Obsidian workflows, and the
remaining support matrix require release validation. See the repository's
Implementation-Status and Live-Validation documents for measured evidence.

Third-party notices and license texts accompany this package.
