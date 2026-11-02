# Working on Virtual Desktop Rail

- Start with [.devdocs/README.md](.devdocs/README.md); read architecture and current
  status before changing behavior. Keep user guidance in the root README and durable
  contributor knowledge in `.devdocs`; update those docs when behavior or decisions change.
- Run checks appropriate to the change. For code changes, use `./scripts/check-dev.ps1`
  (formatting, Clippy, tests, and release builds). Preserve the pinned toolchain and lockfile.
- For UI or interaction changes, **verify the actual app's appearance and behavior
  through computer use in addition to automated testing**. Inspect screenshots and
  exercise real pointer/keyboard input; CLI commands and passing controller tests do
  not establish correct rendering, focus, tooltips, or click behavior. Follow the installed
  computer-use skill and [.devdocs/Testing.md](.devdocs/Testing.md). Report any unverified
  behavior and the reason when desktop access is unavailable.
- Preserve window identity, focus, action-ordering, and fresh-baseline recovery invariants.
  Keep potentially blocking work off the UI thread. API acceptance is not verified completion.
- Serialize desktop tests. Use owned fixtures, preserve user windows/desktops/settings,
  verify process identity before stopping fixtures, and restore automation visibility off.
- Record reusable decisions, fixes, evidence limitations, and remaining work in the
  relevant subject document; do not add dated agent reports or handovers.
