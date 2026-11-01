---
title: Virtual Desktop Rail — Development and Test Environment
status: Proposed
date: 2026-11-01
tags:
  - project
  - windows
  - development
---

# Virtual Desktop Rail — Development and Test Environment

Companion to [the MVP](Virtual-Desktop-Rail-MVP.md) and [the architecture](Virtual-Desktop-Rail-Architecture.md).

## Recommendation

Use a dedicated **Windows 11 x64 Hyper-V VM** for development, native builds, and most interactive testing. Run the coding agent inside the guest if its Windows tooling supports this. Otherwise, use a remote command channel for builds and a separate agent-controlled helper running in the guest's logged-in desktop for UI testing.

A VM alone does not make a cloud coding agent able to operate Windows. The agent must have an explicit command execution connection and a desktop observation/control connection. This document defines the required capabilities; none are assumed to be installed or connected already.

## Host and VM requirements

| Item | Requirement or recommendation |
| --- | --- |
| Host OS | Windows 11 Pro or Enterprise with Hyper-V enabled; Windows Home cannot host the Hyper-V role |
| Host hardware | Supported 64-bit processor, SLAT, virtualization and DEP enabled in firmware |
| Host resources | Suggested: 32 GB RAM and SSD storage, leaving enough capacity for normal host work |
| Guest | Licensed Windows 11 x64, using the same build and update revision as the initial supported target |
| VM generation | Generation 2, with Secure Boot and virtual TPM 2.0 enabled |
| VM allocation | Suggested starting point: 4 virtual CPUs, 8 GB RAM, 100–128 GB expandable virtual disk |
| Display | Start at 1920×1080 and 100% scaling; add other resolutions and 125–200% scaling to the test matrix |
| Network | NAT/default switch is sufficient for package downloads and agent connectivity; no public inbound endpoint is required |

The allocations are development recommendations, not minimum Windows requirements or application runtime requirements. More guest memory may help large builds. For reproducible performance measurements, use a fixed allocation and record host load. Hyper-V and Windows 11 guest prerequisites are documented by Microsoft: [host hardware requirements](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/host-hardware-requirements), [Hyper-V installation](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/get-started/install-hyper-v?pivots=windows), and [Windows 11 VM requirements](https://learn.microsoft.com/en-us/windows/whats-new/windows-11-requirements#virtual-machine-support).

Provide a valid Windows entitlement for the guest; do not assume the host license automatically covers another installation. Confirm the rights associated with the chosen license. Nested virtualization is unnecessary when the Hyper-V host is physical; it becomes an additional prerequisite if the host is itself a VM.

## Guest toolchain

- Git and a repository checkout on the guest's local NTFS disk, for example `C:\src\virtual-desktop-rail`.
- Rust installed through rustup, using the `x86_64-pc-windows-msvc` target. Pin the tested toolchain in the repository and commit `Cargo.lock`.
- Visual Studio Build Tools with **Desktop development with C++**, the x64 MSVC compiler/linker, and a compatible Windows SDK. A full Visual Studio IDE is optional.
- PowerShell, an editor, and the selected coding agent's supported Windows runtime and authentication.
- Optional diagnostics: debugger, Process Explorer, and Windows Performance Recorder/Analyzer when investigating resource consumption or timing.
- Obsidian with a disposable test vault, plus a small native test-window fixture supporting multiple windows, move/resize, minimize/maximize, owned dialogs, and delayed responses.

Rust's Windows MSVC prerequisites are described in the [Rust installation guide](https://rust-lang.org/tools/install/). Build Tools workload identifiers and SDK choices are in [Microsoft's component catalog](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools).

Use a dedicated guest test account with an Explorer desktop. Run the application and ordinary UI tests at standard user privilege. Keep privileged setup operations separate. Elevated application tests are a distinct compatibility case: Windows UIPI can restrict lower-integrity automation of higher-integrity applications. See [Microsoft's UI Automation security explanation](https://learn.microsoft.com/en-us/windows/security/threat-protection/security-policy-settings/user-account-control-allow-uiaccess-applications-to-prompt-for-elevation-without-using-the-secure-desktop).

## Required agent capabilities

| Capability | Purpose |
| --- | --- |
| Read/write repository files | Implement changes and inspect diffs |
| Execute native PowerShell commands | Build, run checks, launch and stop owned test processes |
| Read process output, logs, and exit codes | Diagnose compilation and runtime failures |
| Observe the guest desktop | Verify rail layout, icons, focus, and maximized-window boundaries |
| Send guest keyboard and pointer input | Exercise Task View, Windows pinning controls, desktop shortcuts, and rail clicks |
| Run an in-session test helper | Query window state and geometry and produce reproducible assertions |
| Collect artifacts | Preserve build logs, screenshots, timings, and failure context |

Prefer running the agent's execution component in the guest's signed-in user session. If the reasoning/controller runs elsewhere, connect it to that component through an authenticated local/private channel. Git can move source changes between machines; the desktop helper should expose a narrow test interface rather than unrestricted unauthenticated remote control.

PowerShell Direct is useful for provisioning, file transfer, and build commands from the Hyper-V host, without configuring guest networking. Its “interactive PowerShell session” means an interactive **shell**, not access to Explorer's visible desktop. It is not the UI test channel. See [PowerShell Direct](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/powershell-direct).

## Interactive desktop conditions

Window integration tests must run in the same logged-in Windows session as Explorer, the rail, and the test applications. Use an unlocked, awake guest while driving pointer/keyboard or capturing screenshots. Serialize these tests: parallel tests must not fight over the current virtual desktop or foreground window.

A Windows service or a generic service-based CI runner is suitable for compilation and pure logic tests, but should not directly run the desktop suite. Services run in a different noninteractive context; use a separate helper launched in the signed-in user's session. See [Interactive Services](https://learn.microsoft.com/en-us/windows/win32/services/interactive-services).

Use **VMConnect Basic Session Mode as the baseline**. Enhanced Session Mode uses RDP, which can change display dimensions and session behavior; use it as a separate test configuration. Record mode, resolution, scaling, session identity, and Windows build in results. Do not treat an RDP disconnect or locked desktop as a valid passing UI run. Microsoft documents the distinction in [Enhanced Session Mode](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/enhanced-session-mode).

## Build and test workflow

1. Provision the guest and install the toolchain. Create a clean checkpoint before adding the test applications and project-specific setup.
2. Verify `rustc -Vv`, `cargo -V`, the MSVC target, SDK availability, Windows build, and user session. Build and run a minimal native window before developing shell integration.
3. Keep the Rust toolchain, dependency lockfile, build steps, and test commands in version control. Use the repository's pinned dependencies and `--locked` for reproducible builds.
4. Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked`, adjusted if the repository later defines a workspace or additional targets.
5. Sign in to the test account, create four virtual desktops, start the fixture and Obsidian, and pin windows through Windows Task View.
6. Run the interactive suite and save structured results plus screenshots on failures. Reset fixture windows and desktops between scenarios.
7. Take a known-good environment checkpoint. Evaluate Windows updates on a cloned VM or checkpoint branch before accepting them into the tested baseline.

## Minimum interactive coverage

- Left-edge rail reserves space on the main display; ordinary maximized windows fit beside it.
- Four desktop dots track switches initiated through Windows and through the rail.
- Window-level and application-level pinning add buttons automatically; unpinning and closure remove them. Minimized windows remain listed.
- Switching desktops minimizes managed windows without corrupting layouts; restoring returns each window to the current desktop's saved geometry/state.
- Click behavior covers minimized, obscured, and foreground windows. Multiple identical application icons still target the correct windows.
- Rapid switching, Explorer restart, application restart, fullscreen entry/exit, and unsupported integration states behave predictably.
- Resolution/scaling changes keep windows reachable. Resource checks measure idle activity and switch-to-action latency in a release build.

## What the VM cannot establish by itself

The VM is the primary development laboratory, not the entire release test matrix. Validate on a physical Windows 11 machine before release, especially mixed-DPI monitors, monitor disconnect/reconnect, sleep/resume, graphics/fullscreen behavior, and realistic performance. RDP or simulated displays are useful additional cases, but do not substitute for real display topology.

Do not describe Linux, WSL, containers, or a hosted build runner as sufficient to verify native Windows shell behavior. They can help edit, build selected targets, or test platform-independent logic. The Windows Explorer desktop and supported OS build are essential for the product's integration tests.

## Ready-to-develop checklist

- [ ] Hyper-V host and licensed Windows 11 guest are provisioned.
- [ ] Guest toolchain builds and runs a native x64 executable.
- [ ] Agent can edit, execute commands, observe, and control the guest desktop.
- [ ] In-session helper can report the correct foreground window and window rectangles.
- [ ] Four desktops and test pinned windows are reproducibly available.
- [ ] Checkpoint, dependency versions, OS build, and display configuration are recorded.
- [ ] A physical-machine validation path is available before release.
