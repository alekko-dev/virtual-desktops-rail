# Environment readiness

Verified 2026-11-01. The proposed environment guide is reference material; this
report records observations and does not mark unverified requirements as complete.

## Working development baseline

| Item | Observed |
| --- | --- |
| Machine | Microsoft Virtual Machine; Hyper-V heartbeat service running |
| OS | Windows 11 Enterprise Evaluation 22H2, x64, 22621.6060 |
| Resources | 11 logical processors; 8,345,550,848 bytes RAM |
| Storage | Local NTFS C:, 133,661,978,624 bytes total; about 57 GB free at check |
| Git | Installed, checkout writable; initial working tree clean |
| Rust | rustc 1.99.1, Cargo 1.99.1; x86_64-pc-windows-msvc |
| Version pin | rust-toolchain.toml: 1.99.1, rustfmt, Clippy, x64 MSVC target |
| Native tools | Visual Studio Community 2022 17.10.4; MSVC 14.40.33807 |
| SDK | Windows SDK 10.0.22621.0 libraries |
| Desktop | Explorer, fixture and execution process in session 2 |
| Display probe | 1708 x 960 primary display; 120 DPI (125% scaling) |
| Test app | Obsidian 1.14.5 installed; disposable vault seed in fixtures/obsidian-vault |

## Verification completed

- `scripts/check-dev.ps1` passed formatting, Clippy with warnings denied, Cargo
  tests, and locked release compilation. The fixture has zero unit tests; this
  establishes compilation and test-runner operation, not product correctness.
- Native x64 window launched in the interactive session. Screenshot and accessible
  text were observed through the connected computer-use helper.
- Pointer input maximized the fixture. A later keyboard check was interrupted
  by user input and the fixture was subsequently minimized; keyboard injection
  has not been verified. The owned fixture process was cleaned up through the
  command channel.
- In-session `--probe` returned foreground HWND 67026 and PID 12368, matching the
  selected fixture. Normal rectangle: [150,150,790,510]. After maximize:
  [-9,-9,1717,909]. These HWND/PID values are historical and must not be reused.
- Repository file editing and native command execution work. Rust cache access and
  foreground-window queries required execution outside the restricted sandbox.

The native fixture intentionally remains small. Owned-dialog and delayed-response
scenarios still need a richer fixture during integration-test development.

## Outstanding before the full shell integration baseline is ready

- Upgrade or replace the guest with a serviced Windows 11 build, then select and
  validate that exact build for the desktop adapter. Enterprise 22H2 ended updates
  on 2025-10-14: [Microsoft lifecycle notice](https://learn.microsoft.com/en-us/lifecycle/announcements/windows-11-22h2-end-of-updates-enterprise-education).
  No OS upgrade or reboot was performed in this setup pass.
- Confirm guest entitlement/evaluation validity, dedicated standard-user test
  account, and host-side Generation 2 / Secure Boot / TPM configuration.
- Take and record a clean host-side checkpoint. The guest cannot establish that
  a recoverable host checkpoint exists.
- Record VMConnect Basic versus Enhanced/RDP session mode; current mode was not
  established. The observed display is not the guide's 1920x1080/100% baseline.
- Open the disposable vault in Obsidian and establish four virtual desktops with
  pinned test windows. Desktop count and pinning were not verified or changed.
- Develop a serialized desktop integration suite, including owned dialogs,
  delayed responses, and reset/cleanup. No desktop adapter dependencies are pinned
  yet; select them after the compatibility spike.
- Identify a physical Windows 11 machine for pre-release display/hardware checks.

Coding and native builds can begin. Full readiness under the proposed checklist
remains pending the host, OS-baseline, and interactive test setup above.
