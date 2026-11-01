---
title: Virtual Desktop Rail — MVP
status: Draft
date: 2026-11-01
tags:
  - project
  - windows
  - productivity
---

# Virtual Desktop Rail — MVP

## Purpose

A lightweight Windows 11 utility that makes the current virtual desktop obvious and provides quick access to shared application windows, such as Obsidian, without leaving them visible across every desktop.

The core workflow is: **switch desktop → shared windows minimize → click a rail button → the selected window returns to its remembered position for this desktop.**

## First release

### Side rail

- A persistent, narrow vertical rail reserves space at the screen edge so ordinary maximized applications fit beside it.
- Desktop dots appear at the top, followed by managed-window buttons. Settings controls appear at the bottom. Windows pinning controls determine which windows appear; there is no add-window control.
- The active desktop dot uses a distinct shape and contrast, rather than color alone. Hovering reveals the desktop name; clicking a dot switches desktops.
- Application buttons display icons only, without labels or window-title tooltips in the MVP. Their appearance distinguishes visible and minimized windows. Closed or unavailable windows have no button.
- Start with a roughly 40–48-unit width, scaled for display settings, and light/dark appearance.
- Default placement: the left edge of the main display.
- Fullscreen applications can cover the rail. The rail should not take focus merely because the desktop changes.

### Managed windows

- All eligible application windows pinned to all virtual desktops through Windows are managed automatically. This includes individually pinned windows and windows covered by application-level pinning. Taskbar pinning does not count. Pinning or unpinning through Windows adds or removes the corresponding rail buttons automatically.
- Each button represents a particular window. Multiple windows from the same application may therefore display identical icons in the MVP.
- Switching desktops minimizes managed windows. Other windows remain unaffected.
- The tool remembers each managed window's position, size, and normal/maximized state separately for each desktop.
- Moving or resizing a restored window updates its remembered layout for the current desktop.
- Automatic minimization preserves that layout. Returning to a desktop leaves the window minimized until requested.
- With no saved layout on a desktop, restoration uses the last usable layout, adjusted to remain on an available display. Subsequent adjustments establish that desktop's layout.

### Button actions

| Current state | Click action |
| --- | --- |
| Minimized | Restore the saved layout for this desktop and focus the window |
| Visible behind other windows | Bring the window forward |
| Already foreground | Minimize the window |


The MVP has no context menu. Maximize and other standard window actions remain available through the application's regular Windows controls. A rail click restores the saved normal or maximized state.

Settings include pause/resume and an option to start with Windows. Saved layouts persist between utility restarts. Matching windows after an application restart should avoid guessing when multiple candidates exist. Closed or unavailable windows disappear from the rail; newly available pinned windows appear automatically.

### Automatic minimization and on-demand restoration

On every virtual-desktop switch, minimize all eligible managed windows that are currently visible, while preserving the layout last used on the desktop being left. Already minimized windows remain minimized. Entering any desktop, including one visited before, does not automatically reveal these windows. Clicking an icon requests restoration of that window's layout for the current desktop.

This behavior is called **minimize on desktop switch; restore on demand**. Minimization is automatic; restoration is user-requested. Pinning a window or starting the utility does not itself require minimizing it immediately; the next desktop switch applies the rule.

### Pinned-window scope and limitations

Manage ordinary application windows that can be reliably identified as pinned and controlled. Exclude the utility's own windows, Windows shell surfaces, and child/helper windows; associated dialogs should not get separate buttons. Minimized windows remain eligible. Unsupported or inaccessible windows are omitted instead of displaying nonfunctional buttons.

Automatic discovery must check both window-level and application-level pinning. Detecting this state depends on Windows-version-sensitive integration, so support must be validated on the targeted Windows 11 builds. Elevated applications can require matching privileges for reliable control and need explicit compatibility testing. Not every shell surface or special application window can be promised support.

## Example workflows

### Set up Obsidian

1. Set the Obsidian window to show on all desktops using Windows.
2. Its icon appears on the rail automatically.
3. On Development, restore it and place it beside the editor.
4. Switch to Infrastructure; Obsidian minimizes.
5. Click its rail button and arrange it for infrastructure work.
6. Return to Development and click the button: Obsidian returns to the Development layout.

### Quickly consult a shared tool

Click a minimized tool's icon to reveal it. Consult or edit it, then click its icon again while it is foreground to minimize it. Switching desktops also minimizes it.

### Identify and change context

Read the highlighted desktop dot, hover for its name, or click another dot to switch. The rail updates regardless of whether switching happens through the rail, Windows shortcuts, gestures, or Task View.

## Quality expectations

- Minimal idle CPU, memory use, and disk activity.
- Fast, predictable switching and window restoration.
- No layout drift, unwanted focus changes during switching, or stale actions after rapid desktop changes.
- Windows remain reachable after monitor or scaling changes.
- Clear status when desktop integration is unavailable, and straightforward recovery after Windows shell or application restarts.
- Local operation without a network dependency for the core workflow.

## Possible later releases

| Feature | Intended benefit |
| --- | --- |
| Per-window or per-desktop visibility policies | Automatically restore selected tools, leave others unchanged, or keep a tool visible on a particular desktop |
| Rail overflow controls | Scroll or otherwise access larger managed-window collections; the MVP assumes approximately two or three windows |
| Expanded control panel | Show window names, status, and larger action controls beside the rail |
| On-demand preview thumbnails | Inspect windows before restoring them, with an icon/title fallback where previews are unavailable |
| Open beside rail | Place the actual application window into a predefined side area for normal interaction |
| Optional reserved drawer | Expand the side area while allowing other applications to fit beside it |
| Vertical window labels | Help distinguish windows with identical application icons |
| Context menu and extra actions | Offer explicit restore-position, maximize, and other shortcuts |
| Pinning controls | Set or remove Windows “show on all desktops” from the tool |
| Rail placement options | Select another edge or display, or repeat controls across displays |
| Keyboard shortcuts and ordering | Summon tools quickly and organize larger managed-window collections |

These are candidates, not commitments. Rail overflow controls, live previews, an expanded drawer, labels, and context menus are outside the first release. Automatic management of eligible Windows-pinned windows is included in the MVP.

## Decisions to revisit

The rail starts on the left edge of the main display. All eligible Windows-pinned windows minimize on desktop switch and restore on demand. The exact rail width, icon treatment, and fullscreen behavior should be checked in a small visual prototype before implementation.

Implementation architecture and Windows API choices will be documented separately.

