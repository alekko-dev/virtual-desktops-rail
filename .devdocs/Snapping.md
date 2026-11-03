# Native Snap compatibility

## Scope and decision

The intended user outcome is to return to a desktop exactly as it was left: visible
geometry, normal/maximized/snapped and minimized state, native grouping where possible,
and the original unsnapped drag-away size. This is an acceptance target, **not the
current implementation contract**. Per-desktop user-minimized intent now overrides
auto-restore independently of geometry. Native group membership
is not stored. Layouts now include optional snapped visible bounds separately from
normal restore bounds. Minimized-intent observation and persistence are described in
[Architecture](Architecture.md); broader live Snap/minimize combinations remain part of verification.

There are three different capabilities:

| Capability | Engineering decision |
| --- | --- |
| Preserve an existing live window's native arrangement/group during minimize/reveal | Implementable with documented APIs; verified locally for genuine groups and actual manual rail restores. The original automatic-switch defect is fixed for the verified matching-arrangement cases below; broader support remains gated. |
| Restore visible geometry independently per desktop | Visible and unsnapped restore bounds are now separate and verified. Different or lost arrangements and explicit fallback behavior remain unfinished. |
| Reconstruct arbitrary Windows Snap groups, including different groups/layouts per desktop | Keep as a documented limitation. The reviewed documented APIs expose arranged-state detection, not group enumeration/creation or a general snap setter. Preservation of an existing group does not establish reconstruction support. |

Do not equate a matching rectangle, `IsWindowArranged`, or two adjacent snapped windows
with verified native group membership. Conversely, do not claim that every placement
write necessarily destroys snapping: the local experiment below disproved that claim.

## Implemented preservation path

The first implementation increment adds optional visible screen-coordinate bounds
and arranged evidence to schema-1 layouts. Missing fields deserialize as unknown;
visible rectangles are validated under existing storage limits. Minimized parking
rectangles are never recorded as visible bounds. IsWindowArranged is resolved once
from user32; missing support remains unknown. Stable external geometry/Snap changes
are learned after matching observations at least 100 ms apart, outside drags,
baselines and pending/late operations. Drag completion retains existing guards.

For a saved live arrangement with matching display context and restore bounds,
minimized windows receive only SW_SHOWNOACTIVATE; already-visible matching windows
receive no show/placement command. **This Windows revision can report arranged false
while minimized.** Saved evidence selects a reveal candidate even then; success still
requires observed visible bounds, original restore bounds and arranged state to match.
Normal-sized visible bounds with a stale arranged flag fail verification. Incompatible
saved arrangements in the same display context are declined before dispatch. Ordinary
placement behavior is otherwise unchanged. Identity, generation, ordering, focus and
quarantine guards remain intact.

This is preservation, not arbitrary restoration. Minimized departure intent, deliberate
observed unsnap/normal transitions, different saved Snap geometry, native group
reconstruction and constrained-placement completion classification remain unfinished.
Display changes discard arrangement evidence and still use the existing clamped normal
fallback; that path is not exact Snap restoration and needs explicit fallback reporting.
A visible mismatch after accepted asynchronous work can still quarantine; elapsed timeout
alone never releases its slot. Initially minimized windows and legacy records without
prior visible Snap evidence need separate coverage.

Settings status and notice are separate. A font/DPI-measured read-only edit expands
Settings only while populated, moves Version/Done below the text and allows selectable,
scrollable oversized details. The window is clamped to the work area. Child clipping
and child redraw prevent background erasure during shrink. Timeout wording describes
unverified outstanding work; it clears after all timed-out slots resolve while unrelated
notices remain. Long-message scrolling, other DPI values and focus during relayout
still need broader verification.

### Rebuilt-product evidence

On 2026-11-03, serialized owned-fixture checks ran on Windows 11 x64 26300.9711,
session 2, one 1708×960 display at 120 DPI; rail work area was [31,0,1708,900].
The starting managed inventory was empty. Only two owned group members were pinned;
a separate unpinned observer supported Task View inspection. The original two desktops
and their names were retained.

The preservation implementation (executable SHA-256
67AC94D9666B448DBDDFD58508FE1FA92C1173A4D9AB65AB081E291930CAAD54)
passed these actual-product checks:

- Real rail icon restores and real desktop-dot saved-A/unsaved-B/A switching restored
  both snapped rectangles and both 640×360 normal restore sizes, with no pending/blocked
  action. Neither member became foreground on automatic return; the real-dot run left
  the rail foreground, not an independent application anchor.
- Product diagnostic switching with both destinations saved, including visible-start
  repeated A/B/A, retained the same bounds and two saved layouts per member. A separate
  observer remained foreground on the final repeat. Diagnostic manual restore attempts
  could be denied foreground permission; those are not pointer focus passes.
- Task View displayed an explicit two-member Group before and after both-saved switching.
  This establishes membership survival, not group action or recreation.
- A real rail raise exposed the snapped fixture for screenshot inspection. A real
  title-bar drag then cleared arranged state and restored the original 640×360 size.
- A gated real five-second delay-button click established pending actions, then both
  pending and blocked at about 2.6 seconds. Slots stayed quarantined until late minimized
  completion was observed. The timeout notice then cleared and Settings collapsed.
  Earlier poorly synchronized attempts never established quarantine and are excluded.
  No independent foreground anchor was maintained throughout the stall race.
- Populated one-line and wrapped Settings notices were readable with Version/Done below
  them. Real Done clicks closed Settings. Shrink exposed a child-paint artifact, fixed
  in the subsequent UI refinement; see final-build verification below.

Raw evidence is local under artifacts/snap-restore-*.json, particularly
status-return-corrected, status-both-saved-repeat, drag-away, and
gated-pending/quarantine/complete. Snapshot/probe observations are sequential, not atomic.
Some captures showed an occluding window; only the exposed real-raise screenshot and
explicit Task View Group captures support visual conclusions. Automation visibility
needed refreshing after switches.

### Final build and cleanup

The final executable SHA-256 is
BD68EC29E2990E814846959A05FEC5FE389D18AF41312BA891F0B05CCE56C949.
It adds child clipping/redraw during Settings relayout and requires IsWindowVisible
during restore verification. The required check-dev.ps1 passed formatting,
warnings-denied Clippy, all 63 workspace tests, version contracts and locked release
builds. The first sandboxed run hit the known atomic-replacement access denial;
native execution passed without changing the pinned toolchain or lockfile.

The final build independently repeated gated real-input pending → quarantine → late
completion. Both slots cleared only after observations, the timeout notice became
empty, and actual Settings collapsed with Version and Done readable. Compact Settings
and real Done/Tab input were exercised. The earlier final-gated attempt used a rejected
cached input target and is excluded; final-gated2 contains the conclusive records.
The final build also declined the incompatible saved Snap arrangement after the left
member had deliberately been dragged away; it did not claim successful reconstruction.

Populated wrapped-notice appearance was inspected on the preservation build; the final
build's timeout expansion was recorded in native status but had already collapsed by
the screenshot. Some small text raster artifacts remain ambiguous at capture scaling;
do not treat child repaint correctness across all controls/DPI as certified. A final
focus-preservation-across-expansion attempt lost the automation surface during desktop
switching, so only the observed Tab focus itself passed. No input was sent through the
missing surface. Restarting the owned rail recovered discoverability on the original
desktop. Normal-style cross-desktop presence and broader focus/DPI coverage remain open.

Cleanup removed both fixture pins, restored auto-restore on/pause off/automation
visibility off, returned to the original current desktop, and exited the test rail.
Owned fixture PIDs were stopped only after path and exact UTC creation-tick checks;
PowerShell JSON's timestamp conversion initially stopped cleanup safely, then the
typed tick comparison confirmed the same identities. Both original state files were
restored byte-for-byte from backups after rail exit, preserving user layout memory
and preferences. Final native inventory had no fixtures and the original desktop
identity; the two desktops were neither created, removed nor renamed. Raw final
artifacts use the snap-restore-final-* prefix.

## Verified mechanisms

On 2026-11-02, source review of `b0f497592b2d4ccdeec65eaee16d3f01ac398433`
and owned-fixture experiments ran on Windows 11 x64 26300.9711, session 2,
1708 by 960 physical pixels at 120 DPI. Measurements used Per-Monitor V2 awareness;
native calls were checked separately from UI screenshots. Only owned fixtures were
controlled. The Windows-key restriction in the installed computer-use skill was
explicitly overridden by the user for this verification.

The native fixture executable SHA-256 was
`1BE87A9ED54A8E3F29A94CEE68A75779177CDAA8479DCD2FE28B700A48303973`.
Computer use established native left/right snapping with approved Windows shortcuts.
Read-only measurements checked process lifetime/path, `IsWindowArranged`, minimized
and maximized state, foreground, `GetWindowPlacement`, and `GetWindowRect`.

| Experiment | Result |
| --- | --- |
| Native snap left | Arranged true; normal restore bounds differed from visible bounds. |
| Minimize, then `ShowWindowAsync(SW_SHOWNA)` | Window remained minimized; this is not a restore solution. |
| Minimize, then `ShowWindowAsync(SW_SHOWNOACTIVATE)` | Returned to arranged state at the original snapped bounds without foreground activation. |
| Minimize, then the app's automatic placement sequence (`WPF_ASYNCWINDOWPLACEMENT`, minimized intermediate, `SW_SHOWNOACTIVATE`) | Retained native arranged state and visible snapped bounds while updating the normal restore rectangle. Placement rewriting does not invariably unsnap a window. |
| Minimize, then the app's manual placement sequence requesting normal bounds `[100,100,740,460]` | Normal bounds matched the request, but visible bounds stayed `[-8,0,862,908]` and arranged remained true. The existing verifier would report success despite the visibly different layout. |
| Automatic placement sequence starting from a visible snapped window | Visible bounds became the normal-size rectangle while arranged remained true, still present in a later observation. The minimized-start result must not be generalized to visible-start automatic work. |
| `SW_SHOWNOACTIVATE` on an already visible arranged window | Left arranged state, returned to the original normal size, and did not become foreground. The same show command has different useful outcomes depending on starting state. |
| Explicit no-activation unsnap, observe, then apply normal placement | Arranged false; requested workspace bounds `[100,100,740,460]` produced screen bounds `[131,100,771,460]` with the rail's 31-pixel left reservation, without changing foreground. |

The app sequences above were reproduced in a fixture-only native harness; this
is not an end-to-end product automatic-switch pass. Raw records are local under
`artifacts/snap-*.json`; fixture scripts explicitly check PID, executable and creation
time before mutations. The supplied Settings screenshot independently confirms
clipping of the populated timeout notice.

The first Task View attempt showed separate fixture thumbnails because the input
tool activates its target and can restore an arranged target before sending keys.
Retargeting a Snap flyout also dismissed it. Do not attribute automation-induced
state changes to the rail. Use a separate owned foreground anchor for Task View.
A black-capture/no-foreground interval stopped UI work; after the user confirmed
the desktop was unlocked, fresh fixtures completed these additional checks:

| Live check | Result |
| --- | --- |
| Two freshly snapped fixture windows, independent anchor invokes Task View | Shell displayed an explicit Group thumbnail containing both windows. |
| Pin both group members individually, minimize both, switch A/B/A, reveal each with `SW_SHOWNOACTIVATE` | Both arranged, original snapped bounds and normal restore sizes retained, separate anchor remained foreground; Group thumbnail still present. Existing desktop GUIDs/events independently verified each switch. |
| Launch actual rail, minimize both group members, click each actual rail button | Both restored arranged, foreground matched the explicitly clicked member, pending/blocked false and saved layout established. Task View still displayed the Group thumbnail. |
| Freshly snap a member and perform a real title-bar drag away | Arranged false and original 640-by-360 physical-pixel size restored. |
| Mixed two-window group: only the left member individually pinned; minimize only that member, switch A/B/A, reveal without activation | Both members retained their native arranged bounds on return and Task View still showed the two-member Group thumbnail. The unpinned member was not minimized or repositioned by the test. |
| Mixed two-window group: actual rail pointer restore of the single pinned member | Restored arranged, requested foreground obtained, pending/blocked false; the unpinned member remained arranged and the Group thumbnail survived. |

The actual product executable SHA-256 was
`1C60C2D7925A198AF049E15BAE4E2C7081CA2A722BC079FF819329F1A81A7CC1`,
embedded source `68509b02268786c0b4ac1c66a129ed145997b5a9` with dirty provenance.
The action/platform/controller files have no committed differences between that
revision and the reviewed source. The automatic sequence experiments used a harness;
the actual product was not switched automatically over the user's managed windows.
The reported intermittent timeout was not reproduced in these ordinary native fixtures.
This does not certify all applications, every pinned/unpinned combination, rapid switches,
different group membership on different desktops, other Windows revisions or monitors.

Cleanup verified original managed-window minimized states, saved layouts and all
preferences against the actual rail's startup baseline. Automation visibility was
restored off and the test rail exited. Temporary fixture pins were restored to false;
owned processes were stopped only after executable/lifetime checks. Final inventory
contained no fixtures or rail processes, the same two desktop identities/current
desktop, and the original `[0,0,1708,900]` work area. No user pinning or desktop names
were changed and no desktops were created or removed.

## Mixed and three-member group coverage

Additional serialized checks used the same Windows revision and fixture/product
binary identities. The mixed two-member case ran at 1708 by 960 / 120 DPI. For the
three-member phase the user prepared a genuine three-column group at 2560 by 1440;
the measured work area before rail startup was `[0,0,2560,1380]`. Do not combine these
phases as an unchanged display configuration. The observer remained outside the group.

Both available computer-use APIs activated their target and dismissed the shell Snap
flyout before layout selection. Corner snapping produced three arranged windows but
only a two-member shell group; that attempt is not a three-member group pass. The
user manually prepared the three-column group. Computer use independently confirmed
the explicit Task View label "Snap test B and 2 other windows" and three previews.

| Native desktop round trip | Pinned members | Result |
| --- | --- | --- |
| Mixed two-member group | A only | Passed; group thumbnail survived native reveal and a separate actual rail pointer restore. |
| Three-member group | A only | Passed; A alone minimized/revealed, B/C remained unminimized, all snapped/normal bounds unchanged, three-member Group thumbnail survived. |
| Three-member group | A and B | Passed; only A/B minimized/revealed, C remained unminimized, all bounds unchanged, three-member Group thumbnail survived. |
| Three-member group | A, B and C | Passed; all minimized/revealed, all bounds unchanged, three-member Group thumbnail survived. |

Each native case switched the existing desktop A/B/A with GUID/event checks and used
`SW_SHOWNOACTIVATE` to reveal only the pinned members. No member became foreground.
In the mixed three-member cases, DWM cloaking independently confirmed that unpinned
members were hidden on the other desktop while preserving their native arranged state.
These are native preservation tests, not product automatic-switch certification.
Raw cases are `artifacts/snap-result-three-*-pinned.json`, corresponding before/away/after
observations, and `artifacts/snap-switch-three-*.json`.

Actual product checks at the three-column display configuration also passed:

| Product check with a confirmed existing three-member group | Result |
| --- | --- |
| Only A pinned; minimize A externally, click its actual rail button | A restored arranged with its original snapped/normal bounds, requested foreground obtained, no pending/blocked operation; B/C untouched and three-member Group thumbnail retained. |
| All three individually pinned; minimize all externally, click each actual rail icon | All three restored arranged with unchanged snapped/normal bounds, last explicitly clicked A foreground, no pending/blocked operations; Group thumbnail retained. |
| Exit rail, inspect group; restart rail without window actions, inspect again | Group survived both work-area changes. This controlled case did not reproduce a startup/AppBar-induced group loss. |

Confirm membership before calling a restore test conclusive. An initial product
attempt combined pin changes, startup/work-area changes and restoration; no Group
thumbnail was present afterward, so its cause was not isolated. A subsequent visually
matching three-window arrangement also lacked a Group thumbnail and was excluded.
Once the user established a confirmed Group while the rail was already running,
the isolated mixed and all-pinned manual-restore checks passed. Do not classify the
earlier ambiguous setup as a reproduced rail restore failure or general AppBar defect.
No timeout was reproduced. Actual product automatic desktop switching and wider
application/topology coverage remain outside this evidence.

Cleanup restored every fixture pin to false, disabled automation visibility and exited
the test rail, then stopped the owned fixture process after PID/path/creation-time
verification. Final inventory had no fixtures or rail processes, the original two
desktop identities/current desktop, and work area `[0,0,2560,1380]`, preserving the
user's display configuration for this phase. Original managed-window minimized states,
saved layouts and pause/auto-restore/startup/legacy preferences matched the phase's
startup baseline. No production code changed; documentation diff checks passed.

## Actual product switching, application and recovery evidence

The follow-up interactive run on 2026-11-02 used Windows 11 x64 26300.9711,
session 2, one display at **1918 by 1140 physical pixels / 120 DPI**, with rail work
area `[31,0,1918,1080]`. This differs from both earlier display configurations.
Product and native-fixture hashes were the same as those recorded above
(`1C60C2D7…1A7CC1` and `1BE87A9E…03973`, respectively); no product code changed.
The user explicitly allowed Windows shortcuts and closure of the leftover test
Notepad/Obsidian windows. New tests used two group members and an independent observer.

Real pointer clicks exercised the rail's desktop dots, window buttons, Settings Pause,
Retry and Done. Additional serialized cases used the **running product's** diagnostic
`--switch` and `--click` commands, not the native switch/reveal harness. Native read-only
observations independently checked visible and normal bounds, arrangement, minimization,
cloaking and foreground. Task View explicitly displayed native Group thumbnails.
Automation visibility was enabled for input; the diagnostic rail surface sometimes
became untargetable after switching. Refreshing its temporary style or restarting the
test instance was needed. This does not certify normal-style cross-desktop rendering.

| Scenario | Observed result and boundary |
| --- | --- |
| Both pinned, auto-restore on, no saved layouts, real rail A/B/A | Both minimized on unsaved B and remained minimized on unsaved A. No pending/blocked operations. Real rail restore clicks subsequently established A memory. |
| Both pinned, auto-restore on, saved A / unsaved B | B minimized both; return to A revealed both at their **640-by-360 normal bounds**, rather than the original half-screen snapped bounds. `IsWindowArranged` remained true and the product reported success with no pending/blocked action. Foreground remained the rail in the real-dot case. Task View still showed the Group afterward. **Visible restoration failed despite retained group metadata.** |
| Both pinned, auto-restore on, both destinations saved, visible and independently verified minimized starts | The same normal-size/arranged-true false success occurred. Automatic placement from visible state also failed; this is now an actual-product result, not merely a harness finding. |
| Only one pinned, auto-restore on, saved destinations, visible and verified minimized starts | Pinned member returned at normal-size bounds; unpinned partner retained its snapped bounds, was cloaked on B and visible again on A. The earlier attempt labeled `on-mixed-min-saved` had not actually minimized its starting window and is excluded; use `on-mixed-min-verified`. |
| Auto-restore off, both pinned or only one pinned | Visible members minimized on switching and stayed minimized on return; minimized-start round trips stayed minimized. Both-pinned unsaved-B and saved-B cases, and mixed saved-B cases were exercised. Real rail pointer restores after automatic minimization retained snapped bounds. |
| Snapped A, deliberately moved normal member and maximized partner on B | B's distinct normal position and maximized state were learned and restored on revisit. Returning to A produced stored normal rectangles, not the former snapped geometry; after deliberately leaving snapping, native arranged state was false. This is a current-model fallback/limitation, not exact restoration or group reconstruction. |
| Shared-edge resize, rail minimize/restore, real drag-away | Real divider drag moved both members together: their shared physical boundary moved by 125 pixels. Auto-off product A/B/A plus product restore retained the resized split. A real title-bar drag of the restored foreground member then cleared arranged state and restored its original 640-by-360 size. Earlier background-member drag attempts that changed geometry during tool activation are excluded. |
| Disposable Notepad 11.2607.14.0 and Calculator 11.2607.0.0 | Native left/right snapping formed an explicit Task View Group. After product automatic minimization, real rail buttons restored both visible frames at snapped bounds. Calculator had a different 422-by-676 normal size; this does not constitute a minimum-size constraint sweep. |
| Calculator application pinning and minimize across desktops | An additional Calculator content-window entry appeared beside the ApplicationFrameHost frame. The additional entry remained pending and a timeout notice followed. Removing temporary app pins removed the entries but left the notice; retry did not clear it. Pinning the visible frame individually also briefly exposed the second entry; restoring the frame removed the duplicate. This is a real-app duplicate-eligibility/timeout reproduction, **not proof that the user's exact edge-snap conflict or strict geometry mismatch caused it**. The extra process was independently identified as CalculatorApp.exe, not an unrelated user app. |
| Five-second owned UI-thread stall, pending click, rapid product switches, late completion | A synchronized real delay-button click preceded switching. Both same-thread fixture actions were pending on B, then pending/blocked at about 2.7 seconds. A product click was submitted while pending. After the stall, both completed minimized and pending/blocked cleared; the rapid switch burst settled on A. CLI control remained responsive. No independent visible foreground anchor was maintained throughout this race, so it is not a no-focus-theft pass. |
| Timeout recovery UI | The timeout notice persisted after late completion, successful subsequent restores and integration retry, even with all slots clear. Actual Settings screenshot showed the populated notice clipped/overlapping the version footer at 125%. The disabled-button wording was stale once no operation remained blocked. |
| Pause/resume, retry, close and replacement | Real Settings Pause plus product A/B/A preserved snapped bounds and visibility; resume/retry made no immediate placement changes. Task View still showed a Group afterward. Closing one member removed its rail entry while the survivor kept arranged bounds. Pinning a different owned window gave it zero layouts, with no inherited memory. This is identity/discovery recovery, not replacement group reconstruction. |

Raw evidence is local under `artifacts/snap-next-*.json`, including the
`on-both-visible-return`, `on-both-min-saved-*`, `on-mixed-min-verified-*`,
`different-*`, `shared-*`, `drag-away-after-rail`, `apps-*`, `race-*`,
`paused-group-*`, `resumed-retried-group`, `member-closed` and
`replacement-fresh` observations, with matching `snap-next-status-*` product records.
The first unsynchronized stall attempt did not establish a pending/quarantined
interval and is excluded; the synchronized race overwrote its initial/quarantine
records. Product snapshots and native observations are sequential, not atomic;
one transient minimized-state disagreement was not treated as a stable result.
The verified mixed minimized-start case followed the already-failed visible automatic
case without recreating a fresh snapped arrangement; it verifies minimized-state
handling but is not an independent freshly-snapped-group reproduction. The both-pinned
minimized-start case began after a snapped manual restore and auto-off switch.

The run was interrupted before the remaining unsaved mixed-group cases. No new
computer input was issued afterward. Cleanup removed surviving owned fixture pins,
restored auto-restore on, pause off and automation visibility off, exited the test
rail and stopped owned native/ordinary-app processes only after executable and
creation-time checks. The shared ApplicationFrameHost process was preserved.
Final native inventory had no native fixtures and the original two desktop identities,
with A current. Display size/DPI remained unchanged. The leftover Notepad/Obsidian
closures were user-authorized; they were not reopened. Cleanup records are
`snap-next-cleanup-{status,inventory,display}.json`.

## Remaining verification scope

Future Snap group verification uses **two group members only**, as requested by the
user. A separate owned unsnapped foreground observer is appropriate for measuring
focus and inspecting Task View without activating a member. Retain the historical
three-member evidence above; do not add larger-group tests unless the user changes
this scope.

Prioritize these outstanding two-member scenarios:

1. **Finish the automatic-switch matrix and fix reproduced failures.** Actual product
   switching now fails visible snapped restoration in both-pinned and mixed saved-layout
   cases. Complete the unsaved-destination mixed-group visible/minimized combinations;
   do not call the Cartesian matrix complete. Repeat after fixes with independent
   foreground anchors on both desktops and normal-style rail presence. Preserve the
   already-passing auto-off/minimize and real manual-restore cases as regressions.
2. **Different destination layouts.** Use the same group on A and normal/maximized or
   differently positioned windows on B, then return to A. Check saved memory, actual
   visible bounds/state, pre-snap restore size and the documented geometry fallback
   when Windows no longer retains the native group. Arbitrary group reconstruction
   remains a limitation, not a success criterion silently reduced to rectangle equality.
3. **Real application snapping and edge interaction.** Use disposable ordinary-app
   windows, different applications and application-level pinning in addition to the
   individually pinned native fixture. Shared-edge resizing and drag-away after product
   restoration passed in native fixtures; extend these to real applications. Snap
   Layouts opened on Calculator, but selection failed during tool activation and
   returned it to normal size; that is not a clean selection pass. Edge/corner dragging,
   minimum-size constraints and the user's exact intermittent conflict remain open.
   Diagnose Calculator frame/content duplicate eligibility and the reproduced timeout.
4. **Ordering and failure recovery.** Rapid A/B/A, a desktop switch during a drag,
   clicks while minimize/restore is pending, delayed/hung fixture operations and late
   completion. Verify no focus theft, wrong-desktop memory, stuck buttons or unsafe
   release of an outstanding operation. The synchronized stall/pending-click/late
   completion case reached quarantine and recovered; switching during an actual held
   drag, independent race-focus checks and constrained placements remain unverified.
5. **Shell/group and recovery interactions.** Taskbar/Alt+Tab group restore/minimize,
   one member independently minimized, closing/unpinning/replacing a member, plus
   pause/resume and integration retry with a group and pending actions. Inspect actual
   Group membership and the unpinned member's visibility, rather than only arranged flags.
   Pause/resume, post-completion retry, close/unpin and fresh replacement identity have
   scoped evidence above. Taskbar/Alt+Tab group actions and pause/retry while an action
   is still pending were not run conclusively.
6. **Display coverage.** Two monitors, mixed DPI, different primary/rail monitor,
   monitor disconnect/reconnect, alternate scaling and work areas. Existing local
   single-display results on one Windows revision do not certify these configurations.

The original normal-bounds-only false success now has a regression and a verified
matching-arrangement preservation path. Explicit transitions and fallback completion
still need implementation. Conditional Settings expansion and timeout clearing are
implemented; long-message scrolling, other DPI values, repaint quality and focus during
relayout need broader checks.

## Implementation plan

Steps 1 and 2 and the matching-arrangement part of steps 3–4 now have an initial
implementation and scoped evidence above. Remaining work includes explicit transitions,
broader minimized-intent verification, constrained-placement completion and the wider verification matrix.

1. **Make Settings notices conditional.** Keep the compact no-error layout with zero
   reserved notice space. When a notice appears, measure its wrapped text with the
   active font/DPI and available width, show a separate notice control, move version
   and Done below it, and resize the outer window using the correct nonclient/DPI
   metrics. Collapse and shrink when the notice clears. Clamp to the work area and
   provide scrollable/selectable details if a long message cannot fit. Preserve focus
   and checkbox states during relayout. Use system-appropriate error colors and a
   useful message, rather than asserting that any verification timeout means a hung app.
2. **Add native state and bounds to observation.** Resolve `IsWindowArranged` safely;
   record normal restore bounds separately from visible screen-coordinate bounds,
   explicit normal/maximized/arranged state and display context. Invisible minimized
   parking rectangles must never become saved visible geometry. Migrate stored layouts
   compatibly and treat legacy records as lacking snap evidence. Subscribe to/coalesce
   location changes off the UI hot path and learn only stable user changes on the
   current desktop, outside drags and pending/late own operations.
3. **Select restore behavior explicitly.** If the same live window retains the saved
   arrangement and destination/topology match, reveal a minimized window with
   `SW_SHOWNOACTIVATE` without rewriting its normal rectangle; leave an already visible
   matching arrangement alone. Preserve the pre-snap drag-away size as well as the
   native arrangement. For a genuinely different destination, deliberately leave
   arranged state and apply normal/maximized geometry. The verified candidate is
   `SW_SHOWNOACTIVATE` on the visible arranged window, observe arranged false, then
   apply placement. A minimized arranged window must first be revealed and observed;
   do not issue unobserved show calls back-to-back or assume one command works the
   same from every starting state. Do not use activating `SW_RESTORE` blindly for
   automatic work. If native snapping cannot be recreated,
   label restored geometry as a fallback and retain the documented group limitation.
4. **Verify the visible result and improve failure handling.** Check visibility,
   native state, actual visible bounds and restore bounds as appropriate to the chosen
   path; normal bounds alone cannot certify snapped placement. Add bounded diagnostics
   for requested/observed bounds, state, dispatch phase and timing without titles.
   Distinguish unsupported/constrained placement from unresponsive targets. Do not
   solve timeouts by clearing a slot on elapsed time or visible state: asynchronous
   native work may still execute later. Any recovery design must establish completion
   evidence and preserve identity, generation, focus and one-outstanding-action guards.
5. **Gate support on actual app verification.** Use isolated owned fixture inventory
   and storage for automatic desktop-switch tests. Cover saved same/different/unsaved
   destinations, auto-restore on/off, native group with pinned and unpinned members,
   normal/maximized/snapped transitions, drag-away size, shared-edge resize, real rail
   clicks, rapid A/B/A, a stalled fixture and late results. Check foreground independently.
   Verify a genuine group via shell group thumbnails and group actions before/after
   minimize and desktop switching. Include real ordinary applications in disposable
   state, then mixed DPI/monitor topology separately. Run `./scripts/check-dev.ps1`.
6. **Document the supported boundary.** Advertise native arrangement preservation only
   for scenarios actually passed by the rebuilt app. Keep arbitrary group recreation,
   changed topology and untested group/member combinations explicit. Update the root
   README, Architecture, Status and verification evidence together.

## API references

- [IsWindowArranged](https://learn.microsoft.com/en-us/windows/win32/winmsg/winuser/nf-winuser-iswindowarranged): arranged is distinct from normal/maximized/minimized; minimizing can retain native restoration state.
- [ShowWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow): distinguish `SW_SHOWNA`, `SW_SHOWNOACTIVATE`, activating restore and normal-state requests.
- [WINDOWPLACEMENT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement): normal bounds and workspace coordinate rules.
- [Support Snap layouts](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu): shell Snap UI and application minimum-size constraints, not an arbitrary group-restoration API.
