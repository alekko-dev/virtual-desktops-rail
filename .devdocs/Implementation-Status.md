# Implementation status

Verified 2026-11-01 on Windows x64 build **10.0.26300.9457**. This records
completed work, not a claim that the MVP is implemented. The current toolchain
and environment checks are in [Environment-Readiness.md](Environment-Readiness.md).

## Compatibility gate

The pinned `winvd 0.0.49` dependency requires at least **26100.2605**, as stated
in its [upstream documentation](https://github.com/Ciantic/VirtualDesktopAccessor/tree/rust).
The current OS clears that version gate. The live probe reaches the shell APIs,
but returns incomplete pin-query observations, so compatibility is not established.
The architecture's gate remains:

> Failures here or in pin-state discovery must be resolved before feature work proceeds.

Investigate candidate eligibility and pin-query failures, then complete the
four-desktop compatibility scenarios before implementing the rail shell/controller.
Meeting the minimum does not certify this exact shell revision.

## Completed

- Cargo workspace and root lockfile with exact direct dependency versions;
  the native fixture remains dependency-free.
- `rail-compatibility-probe`: manifest-independent Windows version query plus
  update revision; unsupported-build rejection in both parent and worker;
  schema-versioned JSON and same-directory atomic report replacement.
- On eligible builds, read-only desktop enumeration, names/GUIDs, current desktop,
  bounded notification recording, and both window/app pin queries. Candidates
  include minimized windows and exclude owned/helper windows, known shell classes,
  and the probe. Observations revalidate PID and process creation time after
  queries. Candidate enumeration is preliminary, not a product eligibility policy
  or a solution to same-process HWND reuse.
- Shell integration confined to an owned subprocess; bounded output and a
  duration-plus-ten-second watchdog cover COM calls and listener teardown.
  A timeout terminates/reaps only the probe subprocess. The notification channel
  uses bounded `try_send`; event overflow may drop events, so this diagnostic does
  not certify lossless notification coverage or precise pin-change latency.
- Native fixture duplicate windows, an owned modal dialog, and a configurable
  bounded UI stall. Release `--probe` now reliably writes redirected stdout.
- Workspace check script runs formatting, Clippy with warnings denied, all tests,
  and locked release builds.

## Verification

- Formatting, Clippy with warnings denied, all ten automated tests, and locked
  release builds pass. Tests cover the minimum build boundary, report schema and
  replacement, CLI validation, worker crashes/timeouts, worker file-write
  rejection, and bounded fixture options. The unsupported-host CLI test returns
  early on this build, so it does not exercise live build rejection here.
- Release probe returns exit **2**, `status: unavailable`, build **26300.9457**.
  It enumerates one desktop and obtains several window/application pin states,
  including both fixture windows. Two candidates return `Pin query: WindowNotFound`.
  Report: `artifacts/compatibility-new-vm-rebuilt.json` (ignored).
- Release fixture launches with two windows requested and a 1500 ms stall
  configuration, exposing a nonzero main-window handle and expected title in
  Explorer session 2. The owned process is cleaned up after verification.
- Foreground probe reports 120 DPI and a 1708 x 960 display. Visual interaction,
  modal behavior, delayed-response behavior, and foreground fixture matching
  remain unverified on this environment.

The supported-build COM branch has run, but incomplete observations prevent a
compatibility pass. Query failures alone do not establish an ABI failure.
No product performance or support certification follows from these checks.

## Remaining work

1. On a serviced compatible guest, verify GUID/event identity, desktop switching,
   both pin modes and idle discovery latency, and stable duplicate-window tokens
   across utility restart. The checker can collect query/notification evidence;
   it deliberately does not exercise mutating shell operations.
2. Validate AppBar reservation and presence across desktops, owned dialogs/focus,
   privilege boundaries, and late operations on delayed or hung windows.
3. Implement the native rail, accessible controls, controller/action ordering,
   per-desktop layouts, persistence/reassociation, pause/startup settings, recovery,
   DPI/fullscreen behavior, and single-instance enforcement as documented.
4. Run the documented serialized integration matrix, release resource/latency
   measurements, and physical-machine validation.
