# Window-management refactoring plan

## Status and baseline

This is a proposed implementation plan, not a description of completed refactoring.
Start from main after the per-desktop minimized-intent changes have been committed
and merged. Preserve that behavior, including its persistence compatibility and
conservative rejection of ambiguous external events.

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

### 1. Encapsulate controller transitions

Inspect `crates/rail-core/src/lib.rs` and the command, dispatch, and event handling in
`apps/rail/src/ui.rs`. Move direct UI mutations of observed minimized state and pending
execution targets into controller methods with explicit input types.

Provide read-only controller views for rendering, status, and diagnostics. Make runtime
and pending-operation internals private once callers have migrated. Distinguish the
original user intent from the actual execution plan: a restore may become a raise at
dispatch, or its layout may be adjusted to the current display topology. Specify which
information governs memory updates rather than implicitly rewriting the request.

Completion criteria:

- UI code cannot directly mutate observed state or pending operations.
- Controller tests cover click-time observations, adjusted targets, and restore-to-raise
  conversion with the intended memory semantics.
- Existing behavior and regression scenarios remain intact.

### 2. Define the operation lifecycle and worker protocol

Replace ambiguous Boolean combinations with explicit lifecycle states and typed
outcomes where they clarify legal transitions. Candidate concepts include queued,
accepted, quarantined, verified, verified late, rejected before effects, and identity
lost. These names are design suggestions, not a requirement for one large enum.

Document controller versus executor ownership. The controller owns policy and latest
intent; the executor owns native dispatch progress and verification evidence. Their
protocol must distinguish failure before any effect from partial accepted dispatch.
Keep cancellation tickets and generation checks where they guard the native boundary.

Completion criteria:

- Each outcome has a documented effect on the operation slot, intent, memory, and focus.
- Partial failure cannot accidentally release an outstanding operation.
- Timeout and late verification are represented explicitly.
- Tests exercise supersession before dispatch, during execution, and after timeout.

### 3. Make execution deterministic under test

Extract executor progression from the thread loop in `apps/rail/src/actions.rs`.
Inject a clock and a small native-window interface for the operations and observations
it actually needs. Keep scheduling and Win32 bindings in thin production adapters.
Avoid a broad platform abstraction unrelated to tested behavior.

Build a fake native implementation that can accept a request before applying its
effect, reject a later stage, change identity, report intermediate geometry, and move
foreground focus independently. Connect it to the controller through the production
protocol so tests exercise both sides of acceptance and completion.

Completion criteria:

- Deterministic tests cover delayed effects, partial automatic restore failure,
  timeout/late completion, disappearance versus identity death, and focus movement.
- Tests verify emitted native calls and state transitions without wall-clock sleeps.
- Native desktop tests still verify actual Windows behavior; fakes do not certify it.

### 4. Consolidate observation admission and freshness

Review snapshot, native minimize, geometry capture, and action-result admission as a
single design problem. Define a common context for identity, generation, desktop
transition, operation sequence, event time, and sampling time where applicable.
Document why each field is required and where its validity is established.

The merged implementation advances a global snapshot watermark after accepting native
minimize evidence. This protects newer state but also discards older whole snapshots,
including desktop and other-window information. Assess separate freshness domains for
desktop transitions, inventory, and per-window state. Do not simply remove the global
guard: old desktop evidence must never authorize an action or layout attribution.

Separate evidence admission from the policy decision to remember external intent.
Preserve conservative rejection when the source desktop cannot be established safely.

Completion criteria:

- Reordered, duplicate, stale, and equal-timestamp evidence has specified behavior.
- Tests cover multiple windows, coalesced desktop transitions, recovery, and native
  evidence racing queued snapshots and explicit switches.
- Every accepted memory update has attributable identity and desktop context.
- Any intentional behavior change is reviewed separately from mechanical extraction.

### 5. Add generated event-sequence coverage and organize modules

Retain readable named regression scenarios and add generated sequences covering clicks,
switches, drags, pause/resume, recovery, identity replacement, and delayed outcomes.
Assert the invariants above after each step. Use reproducible seeds and minimized
failure traces; avoid a reference model that merely duplicates implementation branches.

After ownership is clear, split modules by responsibility: identity/layout data,
observation admission, controller policy, operation lifecycle, and native execution.
Choose the final structure from the resulting dependencies, not an arbitrary file-size
limit. Keep the existing crate and subprocess boundaries unless concrete evidence
justifies changing them.

Completion criteria:

- Generated failures can be reproduced as small named regression scenarios.
- UI orchestration translates input and renders views without owning domain rules.
- Module dependencies make policy, evidence, and effects easy to locate and test.

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
