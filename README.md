# Virtual Desktop Rail

Windows 11 x64 utility in the design/compatibility-spike stage. Product requirements
and architecture live in `.devdocs`. There is no rail implementation yet.

## Development

Install Git, Rust through rustup, and Visual Studio C++ tools with a Windows SDK.
The repository pins Rust and its formatting/lint components in `rust-toolchain.toml`.
Open a new PowerShell session after installation so PATH changes take effect.

From this repository:

```powershell
./scripts/check-dev.ps1
./tools/native-smoke/target/release/rail-native-smoke.exe
./tools/native-smoke/target/release/rail-native-smoke.exe --probe
```

The script runs formatting, Clippy (warnings denied), Cargo tests, and a locked
release build of the dependency-free native fixture. It is a toolchain check;
there are no product tests yet. The window supports ordinary move/resize and
minimize/maximize. Launch another instance for multiple windows. `--probe` prints
JSON containing the foreground HWND, owning PID, screen rectangle, window DPI,
and primary display dimensions. Close fixture windows to exit.

Commands needing the Rust cache or interactive desktop may require execution
outside an agent's repository-only sandbox. Run UI checks as the ordinary signed-in
user in the Explorer session, separately from privileged provisioning.

Create a disposable Obsidian vault by copying `fixtures/obsidian-vault` to
`artifacts/obsidian-vault`, then open that folder as a vault in Obsidian.
`artifacts/` and Cargo build outputs are ignored by Git.

See [.devdocs/Environment-Readiness.md](.devdocs/Environment-Readiness.md) for the
verified machine baseline and outstanding integration-test prerequisites.
