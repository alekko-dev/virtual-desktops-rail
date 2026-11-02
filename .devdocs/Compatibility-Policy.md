# Desktop integration eligibility — 2026-11-02

The product now shares `OsVersion::meets_adapter_minimum` with the compatibility
probe: Windows version 10.0, build greater than 26100, or build 26100 with revision
at least 2605. This matches the pinned `winvd 0.0.49` minimum. Other major/minor
versions and older builds remain blocked regardless of the legacy preference.

This replaces the exact 26300.9457 allowlist and newer-26300 experimental opt-in.
The tested revisions remain evidence, not certification of all eligible systems.
Older validation notes describe the policy at the time they were recorded.

Existing `experimental_integration` saved state, status output and CLI commands
are retained for compatibility. The value no longer affects eligibility. The
settings checkbox has been removed; the ready status no longer
claims experimental mode. Settings groups window behavior, startup and integration controls, omits the
layout helper text, and closes through Done. Exit is a separate rail button
below Settings. Desktop and other rail tooltips receive explicitly relayed
mouse events.

Worker startup and reconnect both check eligibility before initialization.
Shell COM remains isolated in the owned subprocess with its watchdog and bounded
queues. Initialization/query failures, worker crashes/timeouts, unavailable-state
handling and fresh recovery baselines are unchanged. No version check alone
asserts that shell operations work.

Verification: formatting, Clippy with warnings denied and all 53 workspace tests
passed. Tests used a workspace-owned temporary directory because the default
temporary location rejected report replacement. All workspace release binaries
built successfully in `artifacts/compatibility-policy-build`; the ordinary
release path could not replace the running/locked application executable.
Interactive shell validation on
additional Windows revisions is separate from this eligibility-policy change.
