# Implementation status and roadmap

This document owns current scope and remaining work. [Architecture](Architecture.md)
defines implemented behavior; [Testing](Testing.md#compatibility-and-completed-validation)
owns completed validation and its limits. A scoped pass is not general release certification.

## Implemented capabilities

| Area | Implemented scope | Verification boundary |
| --- | --- | --- |
| Native rail | Primary-left AppBar, full-height bar or centered responsive floating pill (`centered_rail`), desktop dots/tooltips, icon-only shared-window controls, pause, Settings and Exit | Real input/rendering and passive fullscreen checks exist for the full-height bar; centered responsive mode verified by unit/store/controller tests and user desktop inspection; broader display/accessibility coverage remains open |
| Shared-window discovery | Individual OR app-level pins, minimized inventory, bounded cached reconciliation/rescans | Task View pin origin established on the initial tested revision; later API tests do not establish origin |
| Layout management | Independent normal/maximized desktop layouts, clamped fallback, default minimize-on-switch and manual restore/focus | Owned A/B/A, duplicate, modal and restart cases passed locally |
| Auto-restore | Optional saved-destination restoration without focus; independent per-desktop minimized intent; stable manual/external learning | Scoped real-input and deterministic minimized-intent regressions exist; external changes while stopped/unavailable and ambiguous transitions are not inferred |
| Native Snap preservation | Separate visible/restore bounds and arranged evidence; no-placement reveal for a matching live arrangement; visible-result verification | Two-member saved/unsaved and both-saved product round trips passed locally; arbitrary reconstruction and different/lost arrangements are unsupported |
| Ordering/identity/recovery | Generations/sequences, one outstanding action per identity, verification/quarantine, late-result guards, strong live identity, isolated COM worker and fresh baselines | Pure regression, stalled-target and retry checks exist; broader shell/session and real concurrent-session coverage remain open |
| Persistence/distribution | Bounded versioned storage, backup/corrupt recovery, session merges, optional Startup shortcut, singleton, static-CRT portable ZIP and build provenance | Storage/version/package contracts checked; preview is unsigned and installer/signing choices remain open |
| Presentation | DPI-aware icons, color/contrast handling, DWM corner rounding/border framing, Task View Peek exclusion, double-buffered GDI/persistent button painting, bounded slide-in after Task View return, conditional notices | Scoped live checks exist; physical gesture timing, broad DPI/contrast/accessibility and relayout focus are not certified |
| Diagnostics | Bounded local logs/event history and a redacted support ZIP with fresh matching-window observations | Code checks and Settings rendering checked; end-to-end picker/archive workflow remains unverified. Native Snap Group membership is unavailable to the app |

## Known defects and unmet behavior

- Exact last-visit restoration across all Snap, topology and concurrent-action cases
  remains incomplete. Minimized intent is implemented; broader interactive race and
  physical-hardware verification remain open.
- Different/lost saved Snap arrangements and deliberate Snap-to-normal transitions do
  not have complete exact-restoration/fallback handling. Changed topology can discard
  arrangement evidence and use the ordinary clamped normal fallback.
- Calculator frame/content duplicate eligibility and its reproduced timeout remain
  undiagnosed. They do not establish the cause of the user's exact intermittent edge/Snap conflict.
- Transient pin-query `0x80070490` failures have no established root cause. Integration
  fails closed and reconnects; no broad unsupported-window reclassification is justified.
- Cancelled touchpad swipes without a desktop change have no supported reveal trigger;
  further gesture work is outside current scope.
- Application restart reassociation is intentionally conservative. Elevated/unusual
  windows and inventories needing overflow/history-management UI are outside validated MVP scope.

## Performance targets

| Provisional target | Current assessment |
| --- | --- |
| Combined private working set below 20 MB; effectively zero idle CPU | Small VM samples only; physical/populated coverage and representative distributions remain open |
| Automatic minimize accepted within 50 ms of worker desktop-event receipt | A recorded sample exceeded the target; measurement boundaries/results are in [Testing](Testing.md#resource-and-latency-measurement) |
| Cached pin discovery within two seconds in an idle session | Local API-triggered samples exist; real interaction and broader workloads need coverage |

## Planned work

| Priority | Work and acceptance evidence |
| --- | --- |
| Exact destination restoration | Implement explicit normal/Snap transitions and fallback reporting; broaden minimized-intent verification. Verify saved same/different/unsaved destinations with an independent foreground anchor; preserve the matching-arrangement path. |
| Complete two-member Snap coverage | Finish mixed saved/unsaved visible/minimized combinations, constrained placements, held-drag switching, pending-click/race focus, shell taskbar/Alt+Tab group actions, pause/retry while pending and member close/unpin/replacement. Use [the repeatable procedure](Testing.md#native-snap-procedure). |
| Diagnose real-app failures | Attribute Calculator duplicates/timeouts and transient pin errors to matching live identities without collecting titles. Extend disposable ordinary-app edge/shared-divider/drag-away workflows. |
| Resource/latency coverage | Stable populated distributions for both processes, explicit event/completion boundaries, larger inventories and physical hardware. |
| Shell/session recovery | Actual Explorer restart after a recoverable checkpoint, sleep/resume, lock/unlock and reconnect without layout corruption or unsolicited actions. |
| Display and accessibility | Mixed DPI, primary changes, monitor unplug/replug, physical machines, normal fullscreen-during-reveal, high contrast, screen readers, tooltips, long-notice scrolling and focus/repaint across relayout. |
| Real workflows and compatibility | Disposable Obsidian workflow, additional eligible Windows revisions, actual Task View pin cycles there, privilege combinations and concurrent sessions. |
| Support report workflow | Real picker save/cancel/overwrite/long-path checks, archive privacy/fresh observations and continued event processing during the dialog. |
| History bounds | Decide history-management behavior before expanding the existing [storage limits](Architecture.md#layout-identity-storage-and-action-ordering). |

## Speculative ideas

These are later-release candidates, not commitments or missing MVP requirements.

| Idea | Intended benefit |
| --- | --- |
| Per-window/per-desktop policies | Selective restore, leave unchanged or keep a tool visible in a chosen context |
| Overflow controls and larger control panel | Access larger inventories and distinguish windows with labels |
| On-demand previews | Inspect a window before restoring it |
| Open beside rail / optional drawer | Provide an explicit side work area |
| Context actions and pinning controls | Direct restore/maximize and all-desktop pin management |
| Placement options | Other edges/displays or repeated multi-display rails |
| Keyboard shortcuts and ordering | Faster access and inventory organization |
| Distribution/target expansion | Signing/installer choice; ARM64 after explicit validation |

Keep the narrow MVP while completing release coverage. Revisit appearance/placement
only with current UI evidence; elaborate animation infrastructure is not planned.
