# Initial compatibility evidence

Recorded 2026-11-01. Initial development target: Windows 11 Pro x64 **26300.9457**,
Explorer session 2, medium-integrity interactive tools, 1708 x 960, 120 DPI.
Microsoft lists this revision in the Windows 11 26H2 General Availability Channel:
[release information](https://learn.microsoft.com/en-us/windows/release-health/windows11-release-information).
This selects one development baseline; it does not certify other revisions.

The architecture's initial compatibility spike has passed for ordinary native
fixture windows and the passive AppBar prototype on this baseline. Feature work
may proceed. Product and release validation remain separate, including Obsidian,
fullscreen/display changes, shell recovery, privilege variations, resource goals
and physical hardware. Elevated applications are not certified by these checks.

| Check | Observed result | Local evidence under `artifacts/` |
| --- | --- | --- |
| Four desktop GUIDs, names, switches and old/new notification identity | Passed; original GUID/list/names restored | `compatibility-four-desktops.json` |
| Task View individual pin/unpin and app pin/unpin | Both complete cycles observed; operator reported completion | `compatibility-task-view-final.json` |
| Independent candidate-only reconciliation, one-second interval | API-triggered transitions detected in 795, 626, 480 and 333 ms, including actor/IPC overhead | `compatibility-pin-discovery.json` |
| Warmed idle process CPU | 0 ms reported over 29 seconds; finite counter resolution and small fixture inventory limit this result | `compatibility-pin-discovery.json` |
| AppBar reservation and native maximization | 31 physical pixels at 120 DPI; native maximized frame fits the work area | `compatibility-appbar.json` |
| AppBar presence across desktops | Visible, not cloaked or foreground on all four desktops, without explicit pinning | `compatibility-appbar.json` |
| AppBar cleanup | Original work area restored | `compatibility-appbar.json` |
| Owned modal minimize/restore and focus | Owner minimization hides dialog; explicit dialog restoration is needed; requested foreground subsequently verified | `compatibility-window-control.json` |
| Deliberately stalled target | Bounded probe timed out; async minimize dispatched in <1 ms and completed after the 1500 ms stall | `compatibility-window-control.json` |
| Duplicate-window tokens across helper exit | Distinct non-pointer values recovered with unchanged process lifetime, executable and session identity | `compatibility-identity-lifetime.json` |
| Window destruction and application restart | Normal fixture exit removed properties; new same-executable windows had no old tokens and a new process lifetime | `compatibility-identity-lifetime.json` |
| Lower-integrity access boundary | Sandbox discovery omitted ready medium-integrity windows; medium-integrity observation confirmed no property changes | `compatibility-low-integrity.json`, `compatibility-medium-integrity.json`, `compatibility-privilege-final.json` |

The lower-integrity result demonstrates omission in this sandbox, not a general
certificate for every UIPI/integrity combination. Product eligibility must reject
windows whose identity cannot be read, written and revalidated reliably.

The manual recording timestamps observations, not operator clicks. It establishes
Task View state transitions but does not independently measure click-to-discovery
latency. The independent API experiment supplies the measured polling bound.

Important implementation findings:

- `winvd::listen_desktop_events` starts registration asynchronously; its returned
  thread handle is not proof that registration succeeded. Matching events provide
  actual delivery evidence. Read-only report wording was corrected accordingly.
- Pin queries are not mutually exclusive. Application-pinned views may also return
  `true` for the window query. Task View can leave independent window bits off while
  the app bit remains on. Management must use window pin **OR** application pin.
- Tool-window AppBars have no shell ApplicationView on this build, yet the tested
  passive AppBar remains visible across virtual desktops. The product must preserve
  the validated style and continue excluding its own windows from discovery.
- `GetLastActivePopup` can return the owner after its modal is hidden. Owner-only
  `ShowWindowAsync(SW_RESTORE)` did not reveal the fixture dialog. Remember and
  revalidate the owned dialog; restore its visibility without independently saving
  or repositioning it. The spike's standard-dialog fallback is fixture-specific.
- Property values are opaque numbers, never pointers or allocated resource handles.
  Explicit removal works; normal destruction also removed the prototype values.
  Microsoft's [property-cleanup discussion](https://devblogs.microsoft.com/oldnewthing/20231030-00/?p=108939)
  describes system cleanup and the importance of avoiding owned-resource leaks.

Two earlier operator recordings were incomplete sequences, not ABI failures. They
are retained separately. The final recording passed. Its fixture and the known
temporary Task View desktop were cleaned up; the original desktop was restored.
