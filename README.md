# Virtual Desktop Rail

Windows 11 x64 utility in the compatibility-spike stage. Product requirements
and architecture live in `.devdocs`. The workspace includes a read-only shell
compatibility checker and a native integration-test fixture. There is no rail
implementation yet: the architecture gates feature work on shell compatibility.

## Development

Install Git, Rust through rustup, and Visual Studio C++ tools with a Windows SDK.
The repository pins Rust and its formatting/lint components in `rust-toolchain.toml`.
Open a new PowerShell session after installation so PATH changes take effect.

From this repository:

```powershell
./scripts/check-dev.ps1
./target/release/rail-compatibility-probe.exe --output artifacts/compatibility.json
./target/release/rail-native-smoke.exe --windows 2 --delay-ms 1500
./target/release/rail-native-smoke.exe --probe
```

The script runs formatting, Clippy (warnings denied), Cargo tests, and a locked
release build of both workspace binaries using the root `Cargo.lock`. The tests
cover build rejection, subprocess timeouts/crashes, report replacement, and CLI
contracts. They do not establish product correctness or supported-build shell ABI.

The fixture supports ordinary move/resize and minimize/maximize. `--windows 1..4`
creates identical windows sharing one process; closing one keeps the others alive.
Its dialog button opens an owned modal, and its delay button intentionally stalls
the fixture UI thread for `--delay-ms 0..10000` (default 2000). `--probe` prints
JSON containing the foreground HWND, owning PID, screen rectangle, window DPI,
and primary display dimensions, including in release builds. Close all fixture
windows to exit. Both binaries accept `--help`.

The compatibility checker requires at least Windows build **26100.2605** for
`winvd 0.0.49`. The VM runs **26300.9457** and clears the version gate, but its
live probe returned incomplete pin-query observations. See
[environment readiness](.devdocs/Environment-Readiness.md) for setup and evidence.
Exit status **2** means unavailable, **1** means invocation/report I/O failed, and
**0** means read-only observations were collected, not that compatibility passed.
There is deliberately no build-gate bypass.

On an eligible build, `--observe-seconds 1..60` (default 2) records desktop GUIDs/names,
notification kinds, and one-second samples of window/app pin queries. Calls and
listener teardown run in an owned subprocess with a duration-plus-ten-second
watchdog. Event queues, observations, candidates, and output size are bounded.
The checker does not switch desktops, change pinning, or move/minimize windows.
Reports contain local desktop names and transient HWND/PID observations; these
are diagnostics, not persistent window identities. No window titles are captured.
`--output` atomically replaces the report in an existing directory.

See [.devdocs/Implementation-Status.md](.devdocs/Implementation-Status.md) for
verified behavior and the remaining compatibility and implementation gates.

Commands needing the Rust cache or interactive desktop may require execution
outside an agent's repository-only sandbox. Run UI checks as the ordinary signed-in
user in the Explorer session, separately from privileged provisioning.

Create a disposable Obsidian vault by copying `fixtures/obsidian-vault` to
`artifacts/obsidian-vault`, then open that folder as a vault in Obsidian.
`artifacts/` and Cargo build outputs are ignored by Git.

See [.devdocs/Environment-Readiness.md](.devdocs/Environment-Readiness.md) for the
verified machine baseline and outstanding integration-test prerequisites.
