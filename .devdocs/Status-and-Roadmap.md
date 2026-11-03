# Implementation status and roadmap

Reviewed 2026-11-03. The native development MVP is implemented; general release
certification remains open. This file owns scope and outstanding work. Current behavior
and decisions are in [Architecture](Architecture.md); scoped results are in [Testing](Testing.md).

## Implemented capabilities

| Area | Implementation | Verification boundary |
| --- | --- | --- |
| Native rail | 25-DIP primary-left AppBar, full-height bar or centered responsive mode with rounded right edges (`centered_rail`), desktop dots/names, per-window icons, pause, settings and separate exit; app icon embedded at multiple sizes | Native fixture, real pointer/keyboard and fullscreen checks recorded for full-height bar; centered responsive mode verified by unit/store/controller tests, live desktop rendering/input unverified when desktop CUA is unavailable |
| Shared-window discovery | Window OR app pinning, minimized windows, cached one-second checks and 30-second safety rescans | Task View origin established on 26300.9457; API exercises also passed on 26300.9711 |
| Layout management | Independent normal/maximized desktop layouts, clamped fallback, default minimize-on-switch/restore-on-demand | Fixture A/B/A, maximized, modal and duplicate-window checks passed |
| Optional auto-restore | Default-off preference; saved destinations restore without focus, unsaved ones minimize; per-desktop user-minimized intent overrides restoration; manual/external restores establish layouts | Historical restore checks on 9711; minimized-intent verification is recorded in Testing |
| Controller ordering | Generations/sequences, one outstanding operation per identity, deadlines/quarantine, late-result guards | Pure regression scenarios and stalled-target checks; accepted native work cannot be retracted |
| Identity and persistence | Non-pointer token plus process lifetime/executable/session; atomic schema-1 state, backup/corrupt recovery, bounds and cross-session merges | Duplicate live windows survive rail restart; app restarts intentionally get fresh identities; real concurrent sessions remain open |
| Recovery and eligibility | Owned COM worker, bounded IPC/watchdog/JobObject, backoff, TaskbarCreated handling, retry and unavailable controls; shared minimum-build policy | Explicit retry passed; actual Explorer restart remains open. Eligibility is broader than tested revisions. |
| Version identity | Cargo-derived Windows/manifest version, compiled commit/dirty identity in CLI, Settings and status; package BUILD.json and clean tagged-release gates | Isolated incremental/provenance checks and extracted-package metadata/hash checks passed; live Settings checked at 125%, 100% remains open. |
| Preferences and distribution | Persisted pause/auto-restore, opt-in per-user Startup shortcut, session singleton, static CRT portable ZIP with licenses/checksum | Isolated shortcut tests; package hash/manifest/import checks recorded. Unsigned preview. |
| Switch presentation | Peek exclusion retained; bounded slide-in after desktop switches and Task View return; Windows reduced-motion preference respected | Scoped real input and native/pixel checks recorded in [Testing](Testing.md#completed-presentation-validation); physical cancelled gestures have no supported trigger |
| Appearance and diagnostics | Normal icons with under-icon indicators, light/dark and high-contrast palette handling, Peek-exclusion request, bounded local logs/structured event history, dispatch statistics, and Settings ZIP support report with fresh matching-window observations | Code checks passed. Live Settings/picker/archive verification could not be completed because the passive rail had no targetable CUA window and its status request timed out. Snap Group membership is unavailable. High-contrast implementation is not a completed visual or screen-reader review; Peek exclusion is not screenshot exclusion |

Application icons now use a shared 16-DIP size, dimension-based selection from window
icon variants, size-specific executable fallback and off-thread WIC Fant filtering.
Synthetic transparency/filtering regressions and a live 125% rendering/pointer check
passed; visual sharpness across other DPI settings remains unverified. See [Testing](Testing.md).

Latest recorded code verification before this documentation restructure: formatting,
warnings-denied Clippy, 53 tests and locked release builds for the minimum-build policy
change. This is dated evidence, not a permanent expected test count or a fresh test run.

## Known limitations and unmet goals

- Cancelled touchpad swipes without a desktop change have no supported reveal trigger;
  further gesture work is outside current scope.

- Layouts now store optional arranged state and visible screen bounds separately from
  normal restore bounds; matching live Snap arrangements use a no-placement reveal/no-op
  path and visible-result verification. Native group membership is not stored or recreated.
  Actual two-fixture saved/unsaved and both-saved automatic round trips now pass locally;
  different/lost arrangements, explicit normal/Snap transitions, changed topology,
  constrained-size completion and broader minimized-at-departure verification remain unfinished.
  Calculator duplicate frame/content eligibility and the user's exact intermittent conflict
  remain undiagnosed. Settings notices now expand conditionally and timeout notices clear
  after observed late completion. See [implementation and evidence](Snapping.md).
- Per-desktop user-minimized intent is now independently saved. External changes
  while stopped/unavailable and events that cannot be safely attributed across a
  desktop transition are not inferred. Exact last-visit restoration across all Snap,
  topology and concurrent-action cases remains a broader acceptance target.
- First observed desktop-event-to-automatic-minimize dispatch: **72.635 ms**, above the
  provisional 50 ms target. Representative distributions and optimization remain open.
- Transient `0x80070490` pin failures lack an established root cause; current behavior
  fails closed and reconnects. Do not reclassify them broadly as unsupported.
- About two or three managed windows are expected; there is no overflow UI. Storage
  limits are 512 identities / 64 layouts per identity, with new identities omitted at cap.
  No history-management UI exists.
- Application restart reassociation is deliberately conservative. Elevated and unusual
  application/window types are outside validated ordinary-window scope.
- Performance evidence is a small VM workload. The clean two-window sample was about
  4.73 MB combined private working set and 62.5 ms CPU over 30.103 s, with polling and
  counter-resolution caveats; it is not physical-machine certification.

## Prioritized follow-up

| Work | Completion evidence / prerequisites |
| --- | --- |
| Exact destination restoration and Snap verification | Finish explicit normal/Snap transitions and broader minimized-intent verification; cover mixed unsaved destinations, constrained-size completion, and Calculator duplicate eligibility. Preserve the verified same-arrangement visible-bounds/drag-away and notice-recovery fixes. See [Snapping](Snapping.md). |
| Diagnose transient pin errors | Reproducible candidate-attributed evidence without window titles; preserve strict query classification |
| Latency and realistic resource coverage | Stable populated-workload distributions, both processes, clear event boundary and counter definitions; larger inventories and physical hardware |
| Shell/session recovery | Record a recoverable checkpoint first; actual Explorer restart, sleep/resume, lock/unlock and reconnect without layout corruption or unsolicited actions |
| Real app workflow | Disposable Obsidian vault end-to-end plus additional application/modal types |
| Display and accessibility matrix | Physical machine, mixed DPI, unplug/replug, primary changes, normal fullscreen behavior, light/dark/high contrast, tooltip hover, keyboard and screen-reader review |
| Wider compatibility | Additional eligible OS revisions, actual Task View pin cycles on newer revisions, privilege combinations and real concurrent sessions |
| History bounds | Review cap behavior and recovery expectations before expanding scope |

An observed screenshot of physical PC revision 26300.9818 was not a successful live
test on that machine. Do not infer certification from that report or minimum eligibility.
Host checkpoint, VMConnect mode, guest entitlement and the full host configuration
were not recorded by earlier successful guest tests; establish them before relying on them.

## Future product ideas

These are candidates, not commitments or missing MVP requirements.

| Idea | Intended benefit |
| --- | --- |
| Per-window/per-desktop policies | Selective restore, leave unchanged, or keep a tool visible in a chosen context |
| Overflow controls | Access larger managed-window collections |
| Expanded control panel | Window names, status and larger controls |
| On-demand previews | Inspect before restoring, with icon/title fallback |
| Open beside rail | Place the real app in a predefined side area |
| Optional reserved drawer | Expand working space while other apps fit alongside |
| Vertical labels | Distinguish windows sharing an icon |
| Context menu / extra actions | Explicit restore-position, maximize and related shortcuts |
| Pinning controls | Change Windows all-desktop pinning from the rail |
| Placement options | Other edges/displays or repeated multi-display controls |
| Keyboard shortcuts / ordering | Faster access and larger-collection organization |
| Distribution/target expansion | Decide signing and installer approach; consider ARM64 after explicit validation |

## Decisions to revisit

Reassess rail width, left-primary placement, icon treatment and fullscreen behavior
only with current UI evidence. Keep the narrow MVP scope while completing release
coverage. Automatic restoration is already implemented as one global opt-in; it should
not remain listed as wholly future work. Broader per-window policies are still ideas.
