# Environment readiness

Verified 2026-11-01. This report records the current development VM.

## Development baseline

| Item | Observed |
| --- | --- |
| Machine | Microsoft Virtual Machine; 11 logical processors; 9,431,830,528 bytes RAM |
| OS | Windows Professional, x64; DisplayVersion 26H2; 10.0.26300.9457 |
| Git | 2.56.0.4 |
| Rust | rustup 1.29.1; rustc and Cargo 1.99.1; x86_64-pc-windows-msvc |
| Version pin | rust-toolchain.toml: 1.99.1, rustfmt, Clippy, x64 MSVC target |
| Native tools | Visual Studio Build Tools 2022 17.14.44; MSVC 14.44.35207 |
| SDK | Windows SDK 10.0.26100.0 |
| Desktop | Command process and Explorer in session 2 |
| Display | 1708 x 960 primary display; 120 DPI (125% scaling) |
| Test app | Obsidian 1.14.5; disposable vault at artifacts/obsidian-vault |

The registry ProductName retains a Windows 10 label; the native build number
is used for the compatibility gate. Tools were installed through winget with
package hash verification. Open a new terminal to pick up installer PATH changes;
`scripts/check-dev.ps1` also uses the explicit user Cargo path.

## Verification

- `scripts/check-dev.ps1` passed formatting, Clippy with warnings denied,
  10 tests, and locked release builds of both workspace binaries. The
  unsupported-host integration test returns early on this build; passing tests
  do not certify the shell adapter.
- The release native fixture launched with two windows requested and a 1500 ms
  stall configuration. It exposed a nonzero main-window handle and the expected
  title in session 2, and was cleaned up afterward. This confirms native launch;
  visual, pointer/keyboard, modal-dialog and stall behavior remain unverified.
- The foreground probe returned display dimensions and DPI. Matching its
  foreground HWND against a selected fixture remains an interactive check.
- The release compatibility probe enumerated one desktop and obtained several
  window/application pin states, including both fixture windows. Two candidates
  returned `Pin query: WindowNotFound`; exit 2 / `status: unavailable` correctly
  indicates incomplete observations. Evidence:
  `artifacts/compatibility-new-vm-rebuilt.json` (ignored by Git).

The OS exceeds the pinned winvd 0.0.49 minimum of 26100.2605, but shell
compatibility remains an open gate. Query failures alone do not establish an ABI
failure; candidate eligibility needs investigation. See
[Implementation-Status.md](Implementation-Status.md).

## Outstanding environment checks

- Record a recoverable host checkpoint, guest entitlement, and Generation 2 /
  Secure Boot / TPM configuration.
- Establish a dedicated standard-user test account and record VMConnect mode.
- Validate the recommended 1920 x 1080 / 100% display baseline and additional DPI
  configurations.
- Open the disposable Obsidian vault; establish four desktops and test both
  window and application pinning.
- Verify desktop observation, pointer/keyboard control, and fixture geometry.
- Identify physical hardware for pre-release display and performance validation.

No reboot was performed. Pending file-rename entries include installer temporary
files and Chrome paths. Compilation and native launch succeed in this session.
