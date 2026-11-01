# Implementation status

Verified 2026-11-01 on the existing Windows 11 x64 guest. This records completed
work, not a claim that the MVP is implemented.

## Compatibility gate

The live OS is **10.0.22621.6060 (22H2)**. The pinned `winvd 0.0.49` dependency
requires at least **26100.2605**, as stated in its published package and
[upstream documentation](https://github.com/Ciantic/VirtualDesktopAccessor/tree/rust).
The architecture's first gate cannot pass on this guest. Its instruction is:

> Failures here or in pin-state discovery must be resolved before feature work proceeds.

No OS upgrade, reboot, desktop switch, or pinning change was made. Use a serviced
Windows 11 guest meeting the adapter minimum, record the exact revision and a
recoverable host checkpoint, and complete the compatibility scenarios before
implementing the rail shell/controller. Meeting the minimum does not certify
later shell revisions.

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

- Ten automated tests pass: exact minimum boundary, unavailable report schema,
  CLI validation, worker crash and timeout handling, complete report replacement,
  worker file-write rejection, current-host build rejection in both processes,
  help/exit behavior, and bounded fixture options.
- Release probe returns exit **2**, `status: unavailable`, build **22621.6060**,
  and no adapter observations. Report: `artifacts/compatibility.json` (ignored).
- Computer-use observation showed both identical fixture windows. Closing one
  retained the other. A launcher visibility issue found during this check was
  fixed: hiding the console no longer leaves the first fixture window hidden.
- Owned modal showed a disabled owner and an accessible OK button. The 1500 ms
  delay button returned to a responsive fixture; observed click-to-snapshot time
  was approximately 1665 ms, including automation overhead.
- Release foreground probe matched the selected fixture PID/HWND and reported
  120 DPI, 1708 x 960 display, and its expected window rectangle.

These checks exercise the fixture and unavailable path. The supported-build COM
branch compiles but has **not run on a compatible shell**. No product performance
or support certification follows from these results.

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
