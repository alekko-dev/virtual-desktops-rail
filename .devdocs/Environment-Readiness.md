> Current 2026-11-02 VM: Windows 26300.9711, 2560 x 1440 at 120 DPI.
> The following 2026-11-01 baseline is historical. See [follow-up validation](Validation-2026-11-02.md).

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

The compatibility spike passed on the exact baseline above before feature work.
See [Compatibility-Evidence](Compatibility-Evidence.md) for four-desktop identity
and notifications, both Task View pin cycles, candidate-only polling measurements,
passive AppBar reservation/presence, modal focus, delayed actions, and duplicate
window identity lifetime.

The native MVP is now implemented. Formatting, warnings-denied Clippy, 38 tests,
and locked release builds are the repeatable check suite. Product checks include
real fixture dragging and per-desktop geometry restoration, maximized and modal
restore, duplicate utility restart, conservative application restart, pause/resume,
integration retry and a five-second stalled target. See
[Live-Validation](Live-Validation.md) and [Implementation-Status](Implementation-Status.md).

Pointer/keyboard control and foreground geometry were verified on the unlocked
desktop. A locked VM returned GetCursorPos access denied and an empty foreground
probe; testing stopped until the operator unlocked it. That interval is not a
passing UI run. The passive AppBar is excluded from the automation inventory;
real icon and keyboard interaction was subsequently exercised through computer
use with temporary automation visibility. The mode preserves no-activation input
behavior and is disabled afterward; see Live-Validation for the precise scope.

## Outstanding release environment checks

- Record a recoverable host checkpoint, guest entitlement, Generation 2 / Secure
  Boot / TPM configuration, and VMConnect mode. These host facts are not inferred
  from successful guest compilation.
- Dedicated standard-user account and privilege matrix; observed test processes
  were medium-integrity, which does not certify elevated application support.
- Recommended 1920 x 1080 / 100% baseline plus mixed DPI, primary-display changes,
  monitor unplug/reconnect, lock/sleep/session transitions and physical hardware.
- Disposable Obsidian-vault end-to-end workflow and real Explorer restart under
  the recorded checkpoint. Explicit adapter reconnect already passed.
- Broader supported Windows revisions and physical-machine resource/latency
  measurements before a general release.

No reboot or real Explorer restart was performed. Existing user desktops are
preserved; only known empty test desktops are eligible for fixture cleanup.
