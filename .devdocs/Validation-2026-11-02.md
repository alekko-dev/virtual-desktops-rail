# Implementation follow-up — 2026-11-02

The native MVP feature implementation is complete. General release certification
remains separate. No public release, upload or Git push was performed.

## Windows update and the user's physical-PC report

The current VM is Windows 11 x64 **10.0.26300.9711**, session 2, primary display
2560 × 1440 at 120 DPI. The user's screenshot reports **26300.9818** on a separate
physical PC. The previous nightly intentionally enabled integration only on
26300.9457. On other revisions it never started the desktop worker, so no desktop
dots or automatic minimization could appear. This explains the reported symptoms
without requiring remote access to the user's PC.

The validated default remains 26300.9457. Settings now offers **Try experimental
integration on newer Windows 26300 revisions**. Opting in immediately retries,
persists across restarts, and passes the policy to the isolated worker. Newer
26300 revisions can run experimentally; other build numbers, older revisions and
different major versions stay blocked. Opting out immediately invalidates queued
work and rejects frames from the old policy. Reconnection establishes a fresh
baseline without minimizing visible windows.

The user must replace their old nightly, then enable this checkbox on their PC.
This action is postponed to delivery; no claim of testing revision 9818 is made.
Legacy configuration defaults to validated-only mode. The internal worker still
requires a same-image, same-session parent and retains watchdog/JobObject bounds.

## Changes and automated verification

- Product and probe share strict pin-query classification: only two
  `WindowNotFound` results mean unsupported. Mixed results and arbitrary COM
  errors fail closed. Revalidate candidate lifetime after queries, and attribute
  failures using bounded class, HWND, PID, creation time and both results. No
  titles are collected. Historical 0x80070490 failures have not been reproduced
  sufficiently to establish their cause; they are not reclassified as harmless.
- Normal app icons are used for visible and minimized windows. Only the
  indicator underneath changes: solid visible, outlined minimized. High-contrast
  rendering uses the Windows foreground/background palette and reacts to system
  color changes. Actual high-contrast and screen-reader review remain deferred.
- Bounded dispatch metrics include worker receipt through snapshot/IPC/UI/action
  queues to an accepted native automatic minimize call. They use cross-process
  QPC microseconds; count/max/over-50-ms and the latest 256 samples are exported by
  `--status`. Notification-queue wait before worker receipt and application redraw
  are excluded. No continuous logging or additional timer was introduced.
- `scripts/Measure-RailIdle.ps1` checks a stable warmed workload and UI/worker
  lifetimes repeatedly. Unavailable integration, pending operations, geometry,
  visibility, inventory, desktop or process changes reject the sample.
- Formatting, warnings-denied Clippy, 41 tests and locked release builds passed.
  Tests cover default gating, explicit 9711/9818 eligibility, refusal of other
  builds, legacy settings, cross-session preference merging, bounded metrics and
  worker-parent rejection in both policies.

## Live evidence on revision 9711

Local JSON files under ignored `artifacts/` preserve full observations.

| Check | Result / evidence |
| --- | --- |
| Read-only shell queries | Observed; `compatibility-20261102.json` |
| Individual and application pins | Passed; `compatibility-owned-20261102.json` |
| Owned modal and stalled target | Passed; `compatibility-control-20261102.json` |
| Duplicate tokens and application lifetime | Passed; `compatibility-identity-20261102.json` |
| Four desktops, events, passive AppBar and maximized reservation | Passed; `compatibility-appbar-20261102.json`; original desktops restored, no cleanup errors |
| Independent API pin reconciliation | Passed; 786, 630, 483, 316 ms conservative latencies; `compatibility-discovery-20261102.json` |
| Checkbox opt-in with real pointer input | Ready, two desktop dots and two pinned-window controls; `rail-preview-enabled-20261102.json`, `rail-preview-managed-20261102.json` |
| Normal-style desktop roundtrip | Both fixture windows minimized; `rail-preview-switch-20261102.json` |
| Actual icon restore/focus and second-click minimize | Passed through screenshot-based computer-use clicks; `rail-preview-click-restored-20261102.json`, `rail-preview-click-minimized-20261102.json` |
| Saved opt-in and duplicate identities after restart | Passed, visible windows remain visible; `rail-preview-persistence-20261102.json` |
| Opt-out and re-enable | Passed; disabled controls, then ready with a fresh baseline and no unsolicited minimize; `rail-preview-disabled-20261102.json`, `rail-preview-reenabled-20261102.json` |
| Warmed, stable two-window idle | 4,730,880 combined private working-set bytes; 62.5 ms combined CPU over 30.103 seconds, six stable checks; `rail-preview-populated-idle-20261102.json` |

The resource sample includes diagnostic polling overhead and finite CPU-counter
resolution. Total working set and private bytes are recorded separately. It is
a small VM workload, not physical-machine certification. The executable hash in
that sample identifies the build before the subsequent icon-only rendering edit.

A first event-to-dispatch observation was **72.635 ms**, above the provisional
50 ms goal. This is a measured limitation, not a passing latency certificate.
The independent observer used 140.625 ms CPU over 28.999 seconds; its resource
figures are not product measurements.

## Deferred work and prerequisites

1. The user's physical PC: install the updated nightly and explicitly enable the
   experimental checkbox. Revision 9818 is intentionally not called validated.
2. Task View pin cycles on each newer revision. Current automated tools verify
   shell API mutations; they do not prove Task View origin. Keep the exact
   validated matrix unchanged until the remaining interaction checks pass.
3. Populated latency distribution and optimization of the >50 ms observation;
   larger application inventories and physical-hardware resource measurements.
4. Disposable Obsidian-vault end-to-end, screen-reader/high-contrast review,
   mixed DPI, monitor removal, primary changes and privilege combinations.
5. Host owner: record a recoverable VM checkpoint before destructive shell/session
   tests. Actual Explorer restart, sleep/resume and session reconnect remain
   postponed; do not infer the host checkpoint from guest API success.

None of these tasks warrants asking the user to repeat the completed fixture
checks, and none blocks delivering the implementation or updated portable ZIP.

## Delivered package and final state

`artifacts/VirtualDesktopRail-nightly-2026-11-02-windows-x64.zip` contains the final
icon fix, experimental revision option, README, nightly notes, checksum and 66
license files. Its executable SHA-256 is
`21709ECED41261E9814FEAECF8A53F984355E780598765C6BE2AE8031C1C49AA`.
The ZIP entry, extracted executable and release target hashes match.
`rail-final-package-verification-20261102.json` records verification and the
packaged executable was launched and observed ready.

Computer use visually inspected the final rendering with one minimized and one
visible fixture: both icons retain their ordinary appearance, with outlined and
solid indicators underneath. `rail-final-icons-20261102.json` records the states.
The fixture was then unpinned and stopped after PID, executable and creation-time
verification. The packaged app remains running, experimental mode enabled for
this VM, normal passive style, automation visibility off, startup off, pause off,
no managed test windows. Original desktop
`7CC33836-BC98-4DEB-9005-E971C422CC93` is restored. The other user desktop is
preserved. See `rail-final-cleanup-status-20261102.json` for the final snapshot.

All source changes remain uncommitted, including implementation inherited from
the prior agent; no existing user changes were discarded.
