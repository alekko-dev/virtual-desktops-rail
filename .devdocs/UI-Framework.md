# Modern native UI options

## Status and recommendation

**Settings trial implemented; interactive and resource acceptance pending.** Sources
checked on 2026-11-15. The user subsequently authorized implementation. The workspace
pins published `windows-reactor` and `windows-reactor-setup` 0.100.0; its downloaded
source, rather than development-branch examples, supplied the tested APIs. The setup
helper stages Windows App SDK Runtime 2.4.0 and WebView2 projection 1.0.4078.44.
[Architecture](Architecture.md#settings-companion) owns the implemented design and
[Testing](Testing.md#modern-settings-companion-validation) owns results. The following
research explains the choice and remaining gates; the resource target is unchanged.

For this personal, actively developed app, **trial Rust `windows-reactor` / WinUI 3
in a separate Settings executable, started on demand and terminated when Settings
closes**. This is a useful newest-technology experiment: native Fluent controls and
layout without introducing a second application language. Keep the AppBar/controller
and the precisely anchored modeless name popups in Win32 initially. Promote the
experiment only after measuring startup, resource cost and actual interaction.

This recommendation is an engineering inference from the capabilities below, not
evidence that WinUI meets the app's resource or interaction requirements. If the
combined below-20-MB target must also hold while Settings is open, WinUI is not yet
an acceptable choice without a measurement proving it. A companion isolates steady
state cost only after it exits; it does not make open Settings free.

## Rust WinUI is feasible today, with a preview boundary

Microsoft's [Reactor README](https://github.com/microsoft/windows-rs/blob/master/crates/libs/reactor/readme.md)
explicitly describes native WinUI 3 Rust applications and labels the library a
preview with changeable public APIs. It contains a complete Cargo/component example;
this is a native control framework, not a browser UI or just an aspirational binding.
The [published API documentation](https://docs.rs/windows-reactor/latest/windows_reactor/)
exposes text inputs, checkboxes, radio controls, lists, scrolling, flyouts and
accessibility metadata suitable for Settings.

The release and development tracks need care: the fetched docs.rs `latest` page
identified **0.100.0**, while Microsoft's fetched
[development manifest](https://github.com/microsoft/windows-rs/blob/master/crates/libs/reactor/Cargo.toml)
identified **0.101.0**, Rust edition 2024 and minimum Rust **1.95**. These observations
do not establish that 0.101.0 is published. The repository's pinned Rust 1.99.1 is
above that development minimum; no toolchain upgrade is implied. Pin one tested
published version or explicit upstream revision, and consult documentation for that
exact version instead of mixing master samples with released APIs.

The existing `windows = 0.58.0` dependency is not a drop-in Reactor UI layer.
Reactor's manifest has its own Windows support-crate dependencies and UI lifecycle.
A separate executable reduces integration pressure between these API generations.
Direct hand-generated WinUI bindings are another technical route, but require owning
metadata generation, activation, lifetime and deployment integration that Reactor
already packages. Merely enabling another feature on the existing `windows` crate
does not supply a complete WinUI application framework.

Reactor's example constructs the UI in Rust, so a C# project or XAML compiler is not
required for that path. Conventional C#/C++ WinUI/XAML tooling remains an alternative
if mature designer/tooling workflows become more important than staying in Rust.
A C# companion introduces .NET deployment as well as Windows App SDK deployment;
neither it nor a C++ companion is needed just to make native WinUI possible.

## Hosting and deployment tradeoffs

| Option | Fit here | Cost or limitation |
| --- | --- | --- |
| Existing Win32 controls | Lowest-risk baseline for all current surfaces | Continued layout, painting and theme work; modern frame styling does not turn client controls into WinUI |
| On-demand Rust Reactor Settings companion | Recommended experiment for the richest occasional surface | Startup and IPC complexity; preview API; runtime payload/dependency; open-window resource cost |
| Reactor inside the always-running rail process | Possible later integration | WinUI lifecycle enters the resident process; closing a window does not prove all framework allocations unload |
| WinUI XAML Islands in existing HWNDs | Incremental option when preserving exact host-window semantics is essential | Explicit dispatcher, XAML lifetime, native child bounds, input/focus and shutdown work |
| Rewrite the rail and all popups in WinUI | Broad consistency | Largest regression surface; no established resource or AppBar benefit for the tiny resident rail |

[Current WinUI XAML Islands guidance](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/host-controls-existing-desktop-apps)
supports retaining a Win32 top-level window and message loop while hosting
`Microsoft.UI.Xaml` content through `DesktopWindowXamlSource`. The host remains
responsible for dispatcher/XAML initialization, keyboard traversal, focus, bounds/DPI
and cleanup order. These are the current Windows App SDK islands, distinct from
legacy `Windows.UI.Xaml` UWP islands. Islands are a legitimate incremental route,
but no ready-made Reactor-to-existing-HWND embedding path was established by this
research. Do not assume Reactor's application host can simply attach to the current
message loop.

[AppWindow](https://learn.microsoft.com/en-us/windows/apps/develop/ui/manage-app-windows)
is a window-management abstraction over a top-level HWND, usable alongside Win32;
it is not a control toolkit, XAML host or replacement for AppBar registration.
Its sizing APIs also distinguish physical pixels from XAML DIPs. Choosing it alone
does not modernize the client area or establish correct popup activation/ownership.

[Microsoft's deployment comparison](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/deploy-overview)
distinguishes a shared installed runtime from self-contained deployment. Shared
runtime deployment reduces shipped files and shares code pages, but introduces an
installation dependency. Self-contained deployment preserves folder-copy distribution
with larger downloads and servicing responsibility; Microsoft also documents load
and memory disadvantages for unpackaged self-contained apps.

The [Reactor setup helper](https://github.com/microsoft/windows-rs/blob/master/crates/libs/reactor-setup/readme.md)
stages a matching private Windows App Runtime from `build.rs`, embeds its manifest,
and requires distributing the staged DLLs/subdirectories alongside the executable.
The first build needs network access and Windows download/extraction tools. Its
WebView2 projection file is not the WebView2 browser runtime, and ordinary Settings
does not require adding a browser control. Do not equate the current static-CRT ZIP
or MSBuild's .NET single-file publishing feature with a self-contained Rust WinUI
single executable. Audit the actual output and test a machine lacking the runtime.

## Why postpone the modeless popups

The rename/picker contract includes 280-DIP work-area-clamped anchoring, lightweight
tool-window behavior, precise deactivation dismissal, second-click toggle, preserved
keyboard focus, no AppBar resizing and GUID/token-based asynchronous commands.
Framework controls do not establish those top-level window behaviors.

The inspected [Reactor window source](https://github.com/microsoft/windows-rs/blob/master/crates/libs/reactor/src/window.rs)
has theme, backdrop, sizing and initial placement declarations; initial positioning
is not ongoing position state. Microsoft's
[window-policy discussion](https://github.com/microsoft/windows-rs/issues/5091)
also distinguishes custom title bars from borderless windows and discusses missing
declarative hide/restore, frame and switcher policies. Its closed status is not proof
that every requested behavior shipped. Recheck the pinned version's native-handle
escape hatch and lifecycle APIs before choosing it for an owned popup.

If later migrating popup **content**, a preserved Win32 host plus WinUI island is
worth evaluating against a fully Reactor-owned window. Native focus and pointer
tests must decide, rather than screenshots alone. Starting a new WinUI process on
every desktop-picker click also adds cold-start latency and coordination to a
frequent, very small interaction; there is no measured benefit yet.

## Experiment scope and acceptance boundary

1. Build only the Settings companion. The initial retained Win32 Settings fallback
   was subsequently removed at the user's request; Git retains its implementation.
   Send bounded, versioned preference commands to the resident
   process; retain one controller/storage authority and existing asynchronous work.
   Do not have two UIs independently overwrite the settings file. A repeat open
   should activate one existing Settings window rather than spawn duplicates.
2. Keep the companion out of managed-window inventory and define parent shutdown,
   crash, disconnected IPC and pending-command behavior. Do not initialize WinUI in
   the resident rail just to launch the helper. Exit the helper after its final
   window closes instead of retaining a hidden prewarmed instance.
3. Compare release builds: resident baseline before first open and after close,
   open-but-idle Settings, cold/warm open-to-interactive latency, repeated open/close,
   combined process private working set and idle CPU, and distribution size. Use
   the definitions in [Testing](Testing.md#resource-and-latency-measurement). No
   numerical WinUI memory estimate is justified by the inspected sources. The
   current idle script counts only the rail and integration worker, so an open
   Settings companion must be explicitly added to a separate total. Capture a
   Win32 Settings-open before baseline before implementation; the existing closed
   Settings baseline cannot establish the open-window cost delta.
4. Exercise real pointer/keyboard interaction, theme changes, high contrast, text
   scaling, mixed DPI, focus/activation, immediate setting effects and error states.
   Only then decide whether the native visual improvement warrants runtime/build
   complexity and whether the resource target needs an explicit open-Settings budget.

The Settings trial is authorized and implemented; acceptance measurements and wider
interactive validation remain open. Results belong in Testing; current design belongs
in Architecture; remaining work belongs in Status and roadmap.
