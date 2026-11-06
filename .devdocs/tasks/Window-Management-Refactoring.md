# Window-management refactoring plan

## Status and baseline

Start from main after the per-desktop minimized-intent changes. Preserve that behavior,
including its persistence compatibility and conservative rejection of ambiguous
external events. Progress is tracked per stage below; [Status and roadmap](../Status-and-Roadmap.md)
remains the authority for product scope.

Read [Architecture](../Architecture.md), [Status and roadmap](../Status-and-Roadmap.md),
and [Testing](../Testing.md) before implementation. Those documents remain authoritative
for product behavior and verification requirements. This document owns the refactoring
sequence and completion criteria; update it as stages are implemented.

## Assessment and objective

The existing foundation is sound: a pure controller, strong live-window identity,
process-isolated shell integration, asynchronous execution, verified completion,
and bounded quarantine are worth preserving. A wholesale rewrite is not justified.

The main maintenance risk is distributed ownership of rules. The UI mutates controller
runtime state; the controller and action worker represent overlapping operation
lifecycles; native evidence and snapshots follow separate admission paths. Contributors
must understand several modules to establish whether a transition is safe.

The objective is to make these rules explicit and deterministically testable while
preserving externally observable behavior. File splitting alone does not achieve this.
Keep changes small enough to review and merge independently.

### Review of the original plan

The original sequence was sound; the following changes make it more checkable:

- **Concrete inventory.** Stage 1 names every UI mutation point instead of describing
  them generally, so completion can be checked by visibility rather than by review.
- **Characterize before redesigning.** The executor was originally made testable only
  after its protocol changed, leaving the protocol change untested on the executor
  side. Stage 2 now extracts the executor mechanically with a fake native interface;
  Stage 3 changes the protocol with both sides under test.
- **Derived state removed, not renamed.** The controller's `blocked` flag always equals
  "pending operation timed out". Encode it once in the operation state.
- **Behavior changes isolated.** Stage 4 separates the mechanical admission extraction
  from the freshness-domain change; the latter needs its own reviewed decision.
- **No new dependencies for generated tests.** A small seeded generator and
  removal-based shrinking keep the pinned lockfile unchanged.

## Invariants to preserve

- Validate live identity before native effects; HWND reuse must never transfer memory
  or outstanding operations to a different window.
- Permit at most one unverified native operation per identity. Superseding intent may
  replace queued work but cannot retract an accepted native effect.
- API acceptance is not completion. Partial dispatch and timeout retain quarantine
  until observation or identity death permits release.
- Keep one latest desired intent while outstanding work settles; do not create retry
  backlogs or fight a drag.
- Automatic work never modifies manual minimized intent or saved layout memory.
- Stale and late results cannot commit destination geometry or acquire focus.
- Startup, recovery, resume, and preference changes establish a fresh baseline without
  unsolicited visibility changes.
- Preserve native Snap arrangements through the existing evidence-based paths; native
  group reconstruction remains outside this refactor.
- Keep potentially blocking work outside the UI thread and keep queues bounded.

## Implementation sequence

### 1. Encapsulate controller transitions — implemented

UI mutation points in `apps/rail/src/ui.rs` that move behind controller methods:

| Former UI code | Controller entry point |
| --- | --- |
| Click writes `runtime.window.minimized` from `IsIconic` | `Model::click(key, Click { minimized, was_foreground })` |
| `Accepted` rewrites `pending.request.action` with the adjusted target | `Model::accepted(key, sequence, Acceptance)` |
| `ActionResult` reads pending action, then completes | `Model::settle(...)` returns a `Settled` summary |
| Timeout notice scans pending operations | `Model::has_quarantined()` |
| WinEvent minimize builds `MinimizeObservation` from runtime fields | `Model::minimize_probe(hwnd, tick)` |
| Move end looks up runtime for a capture request | `Model::capture_request(hwnd)` |
| `Captured` resolves an HWND, then reports a move end | `Model::captured(key, generation, tick, layout)` (by identity key) |
| Ticket synchronization reads runtime sequences | `Model::tickets()` |
| Startup / legacy integration preferences and `dirty` | `set_startup`, `set_experimental_integration`, `is_dirty`, `mark_clean`, `mark_dirty` |

Model and runtime fields become private; rendering, status, and diagnostics use
read-only accessors. **Intent versus execution:** the original request is never
rewritten. The executor reports the actual effect (restore, raise after an external
reveal, or an adjusted/observed target). The *intent* governs whether memory and focus
may change (explicit versus automatic, minimize versus restore); the verified
*execution target* governs which layout is committed. Without an acceptance report,
the requested layout is used.

Completion criteria:

- UI code cannot directly mutate observed state or pending operations.
- Controller tests cover click-time observations, adjusted targets, and restore-to-raise
  conversion with the intended memory semantics.
- Existing behavior and regression scenarios remain intact.

### 2. Make execution deterministic under test — implemented

Extract executor progression from the thread loop in `apps/rail/src/actions.rs` into a
pure state machine over an injected `Native` interface (clock plus the window queries
and commands it actually uses). The thread loop and Win32 bindings become thin
adapters. Avoid a broad platform abstraction unrelated to tested behavior.

Build a fake native implementation that can accept a request before applying its
effect, reject a later stage, change identity, report intermediate geometry, and move
foreground focus independently. Connect it to the controller through the production
protocol so tests exercise both sides of acceptance and completion.

Completion criteria:

- Deterministic tests cover delayed effects, partial automatic restore failure,
  timeout/late completion, disappearance versus identity death, and focus movement.
- Tests verify emitted native calls and state transitions without wall-clock sleeps.
- Native desktop tests still verify actual Windows behavior; fakes do not certify it.

### 3. Define the operation lifecycle and worker protocol — implemented

Replace Boolean combinations with explicit states and typed outcomes:

| Controller operation state | Meaning |
| --- | --- |
| `Queued` | Sent to the executor; no native effect accepted yet. Supersession can still cancel it through the ticket. |
| `Accepted` | A native effect was accepted; awaiting verification within the deadline. |
| `Quarantined` | Deadline passed without verification; the slot stays held. |

| Executor outcome | Slot | Intent / desired | Memory | Focus |
| --- | --- | --- | --- | --- |
| `Verified` (current) | Released | Kept | Explicit intent commits | Executor may request guarded focus |
| `Verified` after quarantine (late) | Released | Kept; reconciled next | None | None (executor gates on its own timeout) |
| `Rejected` (no effect applied) | Released | Kept | None | None |
| `IdentityLost` (outstanding identity died) | Released | Kept | None | None |
| `TimedOut` | Quarantined | Kept | None | None |
| `Busy` (executor still owns an older operation for the identity) | Quarantined | Kept | None | None |

The controller owns policy and latest intent; the executor owns native dispatch
progress and verification evidence. A failure after a native stage was accepted is
reported as acceptance (plus a notice) so that partial dispatch never releases the slot.
Cancellation tickets and generation checks remain at the native boundary.

Completion criteria:

- Each outcome has a documented effect on the operation slot, intent, memory, and focus.
- Partial failure cannot accidentally release an outstanding operation.
- Timeout and late verification are represented explicitly.
- Tests exercise supersession before dispatch, during execution, and after timeout.

### 4. Consolidate observation admission and freshness

**4a. Mechanical extraction — implemented.** Snapshot, native minimize, geometry
capture, and action-result admission share a documented context (identity, generation,
desktop transition serial, operation sequence, event time, sampling time). Snapshot
per-window predicates are named and documented in one place instead of inline flags.

**4b. Freshness domains — open decision.** Accepted native minimize evidence advances
the global snapshot watermark. Per-window state is already protected by each window's
`state_tick`, so the global advance mainly discards older whole snapshots, delaying
desktop and other-window information until the next snapshot. A proposed split keeps
the global watermark for desktop transitions while letting per-window `state_tick`
alone guard window state. This is a behavior change: review it separately with tests
for native evidence racing queued snapshots and explicit switches. Old desktop evidence
must never authorize an action or layout attribution.

Completion criteria:

- Reordered, duplicate, stale, and equal-timestamp evidence has specified behavior.
- Tests cover multiple windows, coalesced desktop transitions, recovery, and native
  evidence racing queued snapshots and explicit switches.
- Every accepted memory update has attributable identity and desktop context.
- Any intentional behavior change is reviewed separately from mechanical extraction.

### 5. Add generated event-sequence coverage and organize modules — implemented

Retain readable named regression scenarios and add generated sequences covering clicks,
switches, drags, pause/resume, recovery, identity replacement, and delayed outcomes.
Assert the invariants above after each step. Use reproducible seeds and minimized
failure traces; avoid a reference model that merely duplicates implementation branches.
Use an in-repository seeded generator; do not add a property-testing dependency.

After ownership is clear, split modules by responsibility: identity/layout data,
persisted memory, observation admission, controller policy, operation lifecycle, and
native execution. Keep crate-root re-exports so callers do not depend on file layout.
Keep the existing crate and subprocess boundaries unless concrete evidence justifies
changing them.

Completion criteria:

- Generated failures can be reproduced as small named regression scenarios.
- UI orchestration translates input and renders views without owning domain rules.
- Module dependencies make policy, evidence, and effects easy to locate and test.

## Remaining work and known limits

- Stage 5's seeded generator surfaced two controller edge cases during implementation,
  both now fixed and covered by named tests:
  - Drag-end geometry captures (`Model::moving` with `started = false` and a layout)
    now check `layout.reachable()`, matching the snapshot learning path so minimized
    parking coordinates can never be saved from a late capture.
  - Settling an operation for an already-absent window now drops its `Runtime` entry
    immediately instead of retaining an idle absent entry until the next snapshot.
- Stage 4b freshness domains need a reviewed behavior decision (see above).
- `Busy` is reachable only when the controller is replaced while the executor still owns
  an operation (model reload). The new request remains quarantined until its identity
  dies because the older operation reports under its own sequence. This preserves prior
  behavior; it is not reachable after startup in current code.
- Interactive verification of the refactor (see below) has not been recorded yet.

## Verification and delivery

Use separate reviewable changes for the stages; do not combine them with new product
features, Snap reconstruction, or unrelated storage-format changes. Preserve the pinned
toolchain and lockfile. Run `./scripts/check-dev.ps1` for code changes.

For interaction-affecting changes, follow the installed computer-use skill and
[Testing](../Testing.md). Exercise actual pointer/keyboard behavior with owned fixtures:
manual minimize/restore, automatic A/B/A round trips, immediate switches after external
minimize, delayed-operation recovery, focus movement, dialogs, and supported Snap
preservation. Serialize desktop tests, preserve user state, verify fixture identity
before stopping processes, and restore automation visibility off.

Record scoped evidence and unverified behavior in the relevant subject documents.
Update Architecture as ownership changes land, and Status and roadmap as stages finish.
A passing pure-controller suite does not certify event delivery, native timing, focus,
rendering, or Snap behavior.
