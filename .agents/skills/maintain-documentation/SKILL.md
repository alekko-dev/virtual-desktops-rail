---
name: maintain-documentation
description: Maintain Virtual Desktop Rail documentation when behavior, design, validation evidence, contributor workflows, or implementation status changes; consolidate reports into authoritative project docs without losing useful knowledge.
---

# Maintain Virtual Desktop Rail documentation

Keep user guidance in the root `README.md` and durable contributor knowledge in
`.devdocs`. Resolve all paths below from the repository root. This skill guides
documentation work; it does not authorize unrelated code changes or desktop tests.

## Start with the current project

Read `AGENTS.md`, `.devdocs/README.md`, `.devdocs/Architecture.md`, and
`.devdocs/Status-and-Roadmap.md` before changing behavior descriptions. Then read the
owning documents and relevant implementation, tests, or evidence for the task.
Inspect the working tree first and preserve unrelated changes, including staged work.

Use this ownership map to decide where each fact belongs. All contributor filenames
in the table live under `.devdocs/`.

| File | Authoritative contents |
| --- | --- |
| Root `README.md` | App description, features, Windows requirements, installation and updating, getting started with Task View pinning, rail controls, layout behavior, optional auto-restore, pause/startup/exit, local data, limitations, and common user troubleshooting. End with a contributor link to `.devdocs/README.md`. |
| `.devdocs/README.md` | Short contributor and coding-agent entry point: reading order, document responsibilities, and documentation maintenance rules. |
| `Architecture.md` | Current product contract, component/source map, threading and worker boundaries, identity and persistence, action ordering, focus handling, recovery, compatibility policy, and resource constraints. Include concise decision records with the choice, rationale, rejected alternatives, and consequences. |
| `Development.md` | Toolchain and environment setup, build/run/control commands, repository layout, fixture setup, dependency management, packaging, and CI workflow. Distinguish required tools from recommended VM configuration. |
| `Testing.md` | Automated and interactive strategy, repeatable procedures, fixture capabilities, performance measurement definitions, compatibility/release matrix, and evidence standards. Keep a compact record of significant completed validation with its environment and limitations. |
| `Troubleshooting.md` | Recurring contributor problems and proven resolutions: shell query classification, hidden modal restoration, fullscreen z-order recovery, passive-rail focus handling, automation visibility, locked desktops, locked executables, and dependency-cache issues. Describe symptoms, known causes, fixes, and regression checks; label unknown causes. |
| `Status-and-Roadmap.md` | Authoritative implementation status and remaining work. Separate implemented capabilities, verification gaps, known defects or unmet targets, planned work, and speculative ideas. Preserve MVP boundaries and later-release candidates. |

Existing focused documents, such as Snap investigations or refactoring plans, can
hold substantial subject-specific detail. Inspect their purpose and inbound links
before consolidating them. Keep their current contracts, top-level status, and
significant validation summaries in the owning documents above and link to detail;
do not turn a focused plan into a competing status authority.

## Maintain one coherent account

- Give each fact one authoritative home. Elsewhere, use a short audience-appropriate
  explanation and link to the owner instead of copying policies or status lists.
- Write architecture as the current design. Integrate auto-restore into the relevant
  contract, layout, ordering, focus, persistence, and recovery sections; remove
  superseded statements rather than placing an amendment above contradictions.
  Retain historical policy changes only when their rationale still informs decisions.
- Separate implementation from verification. Code or controller coverage can establish
  implementation without proving live UI behavior, physical-hardware coverage, or
  release certification. API acceptance does not establish verified completion.
- Reconcile claims with source and scoped evidence. If evidence is missing or
  contradictory, state the uncertainty and record the verification gap rather than
  inventing a result or silently promoting a target into supported behavior.
- Keep architecture invariants explicit: window identity, focus, action ordering,
  fresh-baseline recovery, and potentially blocking work off the UI thread.
- Describe auto-restore as implemented where supported by current source and evidence;
  do not keep it wholly in future work. Keep incomplete exact restoration or broader
  policy ideas distinct from the implemented opt-in behavior.

## Merge knowledge before removing reports

When consolidation is in scope, inventory handovers, readiness reports, compatibility
evidence, dated validation files, and references to them. Read each candidate before
deciding whether it is a disposable report or an actively maintained subject document.

For each candidate, map useful content to its owner: current design and rationale,
repeatable procedures, proven fixes, significant results, verification limits, and
remaining work. Reconcile duplicated or superseded guidance during the merge.
Compare the merged result with the original before removal; every durable finding
must be retained, linked to an existing authoritative account, or deliberately
discarded as obsolete or transient. Repair inbound links, then remove redundant
reports within the authorized documentation scope. Git retains their history;
do not create replacement dated handovers or agent reports.

Discard historical PIDs, active desktop GUIDs, “app left running” notes,
machine-specific checkout paths, delivery instructions, and stale test-count summaries.
This does not forbid documenting portable paths, identity concepts, or commands that
discover live state when they are needed for repeatable procedures.

## Preserve evidence that can be understood later

Summarize significant results in `Testing.md` with:

- Date and tested build, revision, or binary identity.
- Environment: relevant OS revision, hardware/VM and session, display/DPI, and settings.
- Workload and measurement or interaction method, including metric definitions.
- Observed result, scope, caveats, and what remains unverified.

Retain available context; mark missing provenance or environment details as unknown
instead of reconstructing them from assumptions. Ignored local artifact paths may
support a result, but the checked-in account must remain understandable without
access to those artifacts. Keep historical evidence scoped to the tested build;
do not present it as a fresh check of later changes. Link outstanding verification
and unmet targets from `Status-and-Roadmap.md` rather than duplicating evidence tables.

## Review and validate the change

Check document links, source paths, command accuracy, ownership, contradictory claims,
and the distinction between current behavior, observed results, targets, and ideas.
Review the diff for lost knowledge and unrelated edits. Keep the entry point short
and the root README focused on users, ending with its contributor link.

Use checks appropriate to the actual change. Documentation-only edits need link and
content review; do not launch intrusive desktop experiments solely to tidy prose.
For code changes, follow `AGENTS.md` and run `./scripts/check-dev.ps1`, preserving
the pinned toolchain and lockfile. For UI or interaction changes, also follow the
installed computer-use skill and `.devdocs/Testing.md`: inspect the actual app and
exercise real pointer/keyboard input. Serialize desktop tests, use owned fixtures,
preserve user windows/desktops/settings, verify process identity before stopping
fixtures, and restore automation visibility off. Report unavailable desktop access
and any behavior left unverified instead of treating automated checks as UI evidence.

Finish by stating what documentation changed, what was checked, and any unresolved
contradictions or verification gaps. Record durable findings in their owning docs,
not in a new session report.
