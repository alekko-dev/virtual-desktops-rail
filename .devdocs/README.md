# Contributor documentation

User installation and quick-start usage belong in the [app README](../README.md), with
detailed controls, Snap behavior, and user troubleshooting in the [User Guide](../docs/USER_GUIDE.md).
For code or behavior changes, read Architecture and Status first, then the relevant
development, testing and troubleshooting sections. Root [AGENTS.md](../AGENTS.md)
contains the working instructions.

| Document | Authoritative responsibility |
| --- | --- |
| [Architecture](Architecture.md) | Current product contract, source map, worker boundaries, invariants, resource constraints and concise design decisions |
| [Development](Development.md) | Required toolchain, recommended environment, build/control commands, repository layout, fixtures, dependencies, packaging and CI |
| [Testing](Testing.md) | Repeatable verification procedures, measurement definitions, evidence standards and compact contextual validation records |
| [Troubleshooting](Troubleshooting.md) | Recurring contributor symptoms, known causes, proven resolutions and regression checks |
| [Status and roadmap](Status-and-Roadmap.md) | Implemented capabilities, verification gaps, known defects, unmet targets, planned work and speculative ideas |

## Maintenance

- Give each technical fact one authoritative home; link to it elsewhere. Keep the
  app README understandable without contributor documents or a source checkout.
- Write Architecture as the current design. Keep decisions concise: choice, relevant
  alternatives and consequences. Put reproducible failure mechanisms in Troubleshooting,
  validation results in Testing and remaining work in Status.
- Separate implemented behavior from verified behavior. An API return, harness result,
  eligible Windows build or passing automated check does not certify an interactive workflow.
- Merge useful knowledge before deleting a report. Reconcile superseded guidance;
  Git preserves the old narrative. Do not add dated agent reports or handovers.
- Preserve significant evidence with date, build/revision when available, environment,
  workload, method, result and caveats. Ignored local artifacts are supporting material;
  the checked-in record must explain the finding without them.
- Discard session PIDs, active desktop GUIDs, machine-specific checkout paths, delivery
  instructions, “app left running” notes and historical test-count summaries.
