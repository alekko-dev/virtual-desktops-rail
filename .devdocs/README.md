# Contributor documentation

These documents serve contributors and coding agents. App installation and usage
belong in the [user README](../README.md).

| Document | Read for |
| --- | --- |
| [Architecture](Architecture.md) | Product contract, component map, invariants, decisions and compatibility policy |
| [Development](Development.md) | Environment, build/run commands, fixtures, packaging and CI |
| [Testing](Testing.md) | Verification methods, interactive procedures, evidence and release coverage |
| [Troubleshooting](Troubleshooting.md) | Recurring engineering issues, proven fixes and regression checks |
| [Status and roadmap](Status-and-Roadmap.md) | Implemented scope, verification gaps, known limitations and future ideas |

Read Architecture and Status first, then Development and Testing for the work at hand.
Root [AGENTS.md](../AGENTS.md) provides short agent instructions.

## Maintenance

Keep each policy in its owning document and link to it elsewhere. Write the current
design directly; integrate amendments rather than prepending updates to contradictory
text. Label alternatives and future ideas explicitly. Separate implemented behavior
from verified behavior and release certification.

Preserve significant evidence with its date, source revision or binary identity when
available, OS/session/workload, method, result, and limitations. Summarize findings in
version control even when raw artifacts are local and ignored. Do not retain session
PIDs, active desktop IDs, delivery notes, stale machine state, or agent handovers as
standing instructions. Git history preserves the superseded documents.
