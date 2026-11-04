# Snap arrangement lost across desktop layouts

## Finding

The rail replaces Obsidian's snapped geometry when it restores an ordinary layout
saved for another desktop. Returning to the original desktop then fails because
the current Snap restoration path can preserve only a matching live arrangement;
it cannot reconstruct the saved arrangement after that placement change.

This is a confirmed gap in the app's destination-transition design. The saved Snap
layout is retained. The failure is applying it again, rather than losing its record.

## Expected behavior and reported workflow

The user has five virtual desktops. On desktop 4, Obsidian and Terminal form a
native Windows Snap group, aligned left and right. Obsidian is shown on all
desktops; Terminal is local to desktop 4. After working on other desktops, returning
to desktop 4 should restore Obsidian correctly aligned within the original group.

The user reported that Terminal stayed aligned while Obsidian returned at its
ordinary size. The taskbar still presented the group, but selecting it did not
repair the alignment. After manually re-snapping Obsidian, desktop actions could
also stop affecting it. The latter symptom has a separately identified timeout
cause described below.

## Evidence

The decisive archive is `VirtualDesktopRail-diagnostic-1793801598.zip`, analyzed on
2026-11-04. It reports Windows 26300.9711, a 3840×2160 display at 144 DPI, five
desktops and auto-restore enabled. Build provenance is dirty source
`1d5ddf8ba88a73ee1990f8f8dda05e6564cc47ab`; the archive does not identify an exact
executable hash. Desktop labels below are archive aliases, not persistent IDs.

| Step | Recorded evidence | Meaning |
| --- | --- | --- |
| Leave `desktop-004` for `desktop-003` | Action sequence 10, `window_action_native`, stage `dispatch`: native arranged state true, visible bounds `[29,0,1948,2097]`, normal restore bounds `[257,420,2726,1831]` | Obsidian still had snapped geometry before the app's restore dispatch. |
| Restore the other desktop's layout | Same sequence, stage `normal_placement`: target arranged false, visible bounds `[185,147,2654,1558]`, normal bounds `[147,147,2616,1558]` | The app deliberately applied the saved ordinary placement. |
| Complete that restore | Same sequence, stage `verified`: visible and normal bounds match the ordinary target; native arranged state remains true | The ordinary placement completed. The native flag alone did not establish preserved Snap geometry. |
| Visit an unsaved desktop, then return to `desktop-003` | Sequences 11 and 12 verify minimize and ordinary restore | The window remained managed; the earlier timeout did not recur in these actions. |
| Return to `desktop-004` | Sequence 13, stage `decline_snap`: target is the saved snapped layout, but native state is ordinary with arranged false | The matching-arrangement preservation check rejects restoration. |
| Collect the report | Integration ready; Obsidian responsive and present; no pending action, blocked state or active drag; monitor, work area and DPI unchanged | A held drag, quarantine or display-context mismatch does not explain this refusal. |

The action worker observations establish that the app changed the geometry. They do
not establish when Windows removed or retained native group membership: the archive
has no native Snap Group membership observation. The taskbar behavior comes from
the user's account.

## Separate timeout defect

`VirtualDesktopRail-diagnostic-1793800980.zip` identified a false ordinary-restore
timeout. Obsidian reached the requested ordinary visible and normal bounds, but
Windows retained its arranged flag. The old verifier required that flag to clear,
kept the action outstanding and blocked subsequent automatic actions.

The verifier now accepts that stale flag only for an ordinary nonmaximized target
whose normal and visible bounds match, with visible bounds also matching the normal
rectangle converted to screen coordinates. Saved Snap verification remains strict.
The regression first failed on the old verifier and passed after the fix. Sequence
10 in the decisive archive demonstrates successful verification of that case.

This fix resolves the false timeout; it does not restore the original Snap group.
The first supplied archive,
`VirtualDesktopRail_diagnostic_1793701509_Outlook_popedup_on_desktop.zip`, describes
a different four-desktop Obsidian/Outlook session and cannot establish this Snap
failure path.

## Implementation boundary and required work

The relevant code is [`actions.rs`](../apps/rail/src/actions.rs):
`dispatch` applies ordinary layouts with `SetWindowPlacement`, while
`preserves_arrangement` accepts only compatible live Snap evidence. An incompatible
saved Snap target is declined before placement with the reported message:

> The saved Snap arrangement cannot be preserved in the current window/display state. Automatic Snap group reconstruction is unavailable.

Correct restoration remains unimplemented. A solution must restore native alignment
and group behavior after an intervening ordinary layout. Placing a window at the
saved rectangle alone does not prove that requirement. Do not remove the refusal,
weaken Snap verification, or clear outstanding operations just to suppress the
notice. Preserve identity, focus, action-ordering and fresh-baseline invariants;
keep blocking native work off the UI thread.

Acceptance requires an owned two-member native group, one shared member and one
desktop-local member, an independently saved ordinary destination, and a round
trip through both. Verify actual group membership, member alignment, shared-divider
behavior, foreground preservation, saved memory and absence of pending/quarantined
work through real shell interaction and native observations. Include repeated and
rapid transitions. This complete workflow has not passed live verification.

Current policy belongs in [Architecture](Architecture.md#native-snap-preservation),
remaining work in [Status and roadmap](Status-and-Roadmap.md#planned-work), and
validation evidence and procedures in [Testing](Testing.md#native-snap-procedure).
