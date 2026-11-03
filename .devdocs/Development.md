# Development

## Toolchain and environment

Use Windows 11 x64 with Git, PowerShell, Rust through rustup, and Visual Studio C++
Build Tools (Desktop development with C++, x64 MSVC linker and a Windows SDK).
`rust-toolchain.toml` pins Rust 1.99.1, rustfmt, Clippy and the MSVC x64 target;
`Cargo.lock` and exact workspace dependency versions pin inputs. Open a new terminal
after installation so PATH updates apply. Scripts locate Cargo under the user's `.cargo/bin`.

A dedicated Windows VM is recommended for shell tests. Suggested Hyper-V setup:
Generation 2, Secure Boot, TPM 2.0, 4 vCPUs, 8 GB RAM and a 100–128 GB expanding disk;
a host with 32 GB RAM and SSD provides headroom. These are development recommendations,
not app runtime requirements. Record guest entitlement and a recoverable checkpoint.
Use VMConnect Basic Session as the baseline; Enhanced Session/RDP is a separate case.
Start coverage at 1920×1080 / 100%, then add scaling and topology variants.

Native shell tests require an unlocked, awake Explorer session shared by the rail,
fixtures and desktop automation. Standard-user operation is the baseline. Keep elevated
provisioning separate; medium-integrity tests do not establish elevated-app support.
PowerShell Direct provides a shell, not access to Explorer's input desktop. Service-based
CI, WSL or containers cannot verify native virtual-desktop behavior.

An agent needs repository access, native command execution with outputs/exit codes,
desktop observation and pointer/keyboard control, and an in-session geometry/foreground
probe. A remote controller needs an authenticated private connection to that session;
do not expose an unrestricted unauthenticated desktop helper.

## Build and run

From the repository root:

```powershell
./scripts/check-dev.ps1
Start-Process ./target/release/virtual-desktop-rail.exe
Start-Process ./target/release/virtual-desktop-rail.exe -ArgumentList '--settings'
./scripts/Invoke-Rail.ps1 --status
./scripts/Invoke-Rail.ps1 --pause
./scripts/Invoke-Rail.ps1 --resume
./scripts/Invoke-Rail.ps1 --auto-restore on
./scripts/Invoke-Rail.ps1 --auto-restore off
./scripts/Invoke-Rail.ps1 --centered-rail on
./scripts/Invoke-Rail.ps1 --centered-rail off
./scripts/Invoke-Rail.ps1 --retry
./scripts/Invoke-Rail.ps1 --exit
```

`check-dev.ps1` runs formatting, warnings-denied Clippy, workspace tests and locked
release builds of all workspace binaries. The CLI helper captures GUI-subsystem stdout
and real exit codes and addresses an already running instance, including a packaged one.
Stop the app before relinking its executable, or use another Cargo target directory.
All binaries accept `--help`. Diagnostic `--click WINDOW_KEY` and `--switch DESKTOP_GUID`
support instrumentation; real input tests remain required for interaction changes.

Keep the lockfile locked. For offline dependency metadata, filter the actual target:

```powershell
cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc --locked --offline
```

Omitting the platform filter can request uncached ARM dependencies even when x64 builds
succeed. See [Troubleshooting](Troubleshooting.md) for cache and executable-lock issues.

## Fixtures and artifacts

Fixture capabilities and exercise procedures are owned by
[Testing](Testing.md#compatibility-tools). The source/component map is in
[Architecture](Architecture.md#stack-and-source-map).

Copy `fixtures/obsidian-vault` into `artifacts/obsidian-vault` and open that copy in
Obsidian for disposable-vault tests. Never use the user's working vault for fixtures.
`artifacts/` and Cargo outputs are ignored. Keep durable findings in the documentation;
raw local artifact filenames alone are not portable evidence. Do not commit local
run reports or machine-specific handovers.

## Packaging and CI

```powershell
./scripts/package.ps1
./scripts/package.ps1 -Version 0.1.0 -BuildLabel preview
```

Cargo is the version source; a mismatched explicit `-Version` is rejected.
`-BuildLabel` changes only the package name. Local dirty/unknown builds are allowed
and marked in embedded identity and `BUILD.json`. Packaging reads the executable's
`--version --json` output and checks Windows version resources before copying it.
It respects Cargo's `CARGO_TARGET_DIR`, allowing isolated builds while the app runs.
`BUILD.json` records version, full commit, dirty state, optional label and SHA-256.
The script builds the app with static CRT and creates a portable directory and ZIP under `artifacts/`.
It includes the executable, user README, SHA-256, third-party notices, dependency
license texts and Rust standard-library licenses. Installed Rust documentation licenses
are required. Existing package destinations are rejected instead of overwritten.

The root README is the single user-guide source. Packaging removes only its marked
repository-only contributor section so the shipped README needs no source checkout.
When changing those markers or documentation paths, check packaging and CI references.
Verify packaged/extracted executable hashes, manifest and runtime imports for release
changes; then launch the actual packaged executable for relevant live checks.

`.github/workflows/ci.yml` runs checks on pull requests and main pushes. Version tags
must match the application Cargo version. CI passes `-ReleaseTag vMAJOR.MINOR.PATCH`;
packaging requires its own clean Git checkout, a tag pointing to HEAD, and known clean
embedded provenance matching that commit. This gate runs before and after building.
Successful pushes to `main` build the portable package and upload its directory as a
GitHub Actions artifact named with the commit SHA. Upload from the `artifacts/` parent
and exclude the locally generated ZIP so the downloaded artifact ZIP contains one
`VirtualDesktopRail-<version>-windows-x64/` folder, with no nested archives.
The artifact is retained for 90 days and can be downloaded from that workflow run.
Tag jobs package assets and create a draft
prerelease with generated notes and the remaining-validation link; release assets are
the versioned downloads to keep. CI success does not replace interactive certification.
Local packaging does not upload or publish anything.

## Environment references

- [Rust installation](https://rust-lang.org/tools/install/)
- [Visual Studio component catalog](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools)
- [Hyper-V hardware requirements](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/host-hardware-requirements)
- [Windows VM requirements](https://learn.microsoft.com/en-us/windows/whats-new/windows-11-requirements#virtual-machine-support)
- [PowerShell Direct](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/powershell-direct)
- [Enhanced Session Mode](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/enhanced-session-mode)

## Build provenance

The build script derives fixed Windows and manifest versions as MAJOR.MINOR.PATCH.0
(each component at most 65535), and ProductVersion text as version+gCOMMIT[.dirty].
CLI, Settings and status share compiled identity; no runtime Git calls are made.
No timestamps or machine paths are embedded. The manifest source is a template;
resolved resources and identity constants are generated only under Cargo OUT_DIR.

An intentionally absent Cargo build-script input refreshes provenance on every build,
including changes elsewhere in the workspace, new nonignored files and commit changes.
This trades a small rebuild cost for reliable identity. Exported source trees and
unavailable Git produce null commit/dirty fields, never enclosing-repository identity.

Run ./scripts/test-versioning.ps1 -Provenance for isolated incremental-build and release
validation. It copies current nonignored source into a disposable ignored fixture and
creates commits/tags only there; the user's repository history is untouched.
