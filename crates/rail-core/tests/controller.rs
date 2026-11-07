use rail_core::*;
use std::collections::{BTreeMap, BTreeSet};

fn layout(left: i32) -> Layout {
    Layout {
        normal: [left, 80, left + 600, 500],
        maximized: false,
        visible: None,
        arranged: None,
        dpi: 96,
        monitor: "main".into(),
        monitor_rect: [0, 0, 1920, 1080],
        work_area: [25, 0, 1920, 1040],
    }
}
fn window(token: &str) -> Window {
    Window {
        identity: Identity {
            token: token.into(),
            process_created: 100,
            executable: "fixture.exe".into(),
            session_id: 2,
        },
        hwnd: 123,
        pid: 45,
        minimized: false,
        responsive: true,
        layout: layout(100),
        popup: None,
    }
}
fn snapshot(desktop: &str, tick: u64, windows: Vec<Window>) -> Snapshot {
    Snapshot {
        desktops: vec![
            Desktop {
                id: "A".into(),
                name: "A".into(),
            },
            Desktop {
                id: "B".into(),
                name: "B".into(),
            },
        ],
        current: desktop.into(),
        windows,
        tick,
        transition_serial: 0,
    }
}
fn baseline() -> (Model, Window, String) {
    let mut model = Model::new(Saved::default());
    let window = window("one");
    let key = window.identity.key();
    model.snapshot(snapshot("A", 100, vec![window.clone()]));
    (model, window, key)
}

fn saved_destinations() -> (Model, Window, String) {
    let (mut model, window, key) = baseline();
    let record = model.record_mut(&key).unwrap();
    record.layouts.insert("A".into(), layout(100));
    record.layouts.insert("B".into(), layout(700));
    model.set_auto_restore(true);
    (model, window, key)
}

fn click(model: &mut Model, key: &str, was_foreground: bool) {
    let minimized = model
        .runtime(key)
        .is_some_and(|runtime| runtime.window().minimized);
    model.click(
        key,
        Click {
            minimized,
            was_foreground,
        },
    );
}

fn dispatched(model: &mut Model, request: &Request) {
    model.accepted(
        &request.key,
        request.sequence,
        Acceptance::as_requested(&request.action),
    );
}

fn completed(
    model: &mut Model,
    key: &str,
    sequence: u64,
    tick: u64,
    verified: bool,
    cancelled: bool,
) {
    let outcome = match (verified, cancelled) {
        (_, true) => Outcome::Rejected,
        (true, false) => Outcome::Verified,
        (false, false) => Outcome::TimedOut,
    };
    model.settle(key, sequence, tick, outcome);
}

fn minimize_observation(
    model: &Model,
    window: &Window,
    event_tick: u64,
    tick: u64,
) -> MinimizeObservation {
    let mut window = window.clone();
    window.minimized = true;
    MinimizeObservation {
        sequence: model.runtime(&window.identity.key()).unwrap().sequence(),
        window,
        desktop: model.current().unwrap().into(),
        generation: model.generation(),
        event_tick,
        tick,
        transition_serial: model.transition_serial(),
    }
}

#[test]
fn explicit_minimize_is_remembered_per_desktop_and_restore_clears_only_that_desktop() {
    let (mut model, mut window, key) = saved_destinations();
    click(&mut model, &key, true);
    let minimize = model.take_requests(110).remove(0);
    assert!(model.saved().windows[&key].minimized_desktops.is_empty());
    completed(&mut model, &key, minimize.sequence, 120, true, false);
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(100));
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let restore_b = model.take_requests(201).remove(0);
    assert_eq!(restore_b.action, Action::RestoreAutomatic(layout(700)));
    completed(&mut model, &key, restore_b.sequence, 220, true, false);
    window.minimized = false;
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    let minimize_a = model.take_requests(301).remove(0);
    assert_eq!(minimize_a.action, Action::Minimize { automatic: true });
    completed(&mut model, &key, minimize_a.sequence, 320, true, false);
    model
        .record_mut(&key)
        .unwrap()
        .minimized_desktops
        .insert("B".into());
    click(&mut model, &key, false);
    let restore_a = model.take_requests(330).remove(0);
    assert_eq!(restore_a.action, Action::Restore(layout(100)));
    completed(&mut model, &key, restore_a.sequence, 350, true, false);
    assert_eq!(
        model.saved().windows[&key].minimized_desktops,
        ["B".into()].into()
    );
}

#[test]
fn external_minimize_without_geometry_survives_round_trip_and_does_not_learn_parking_bounds() {
    let (mut model, mut window, key) = baseline();
    model.set_auto_restore(true);
    let fallback = model.saved().windows[&key].last_usable.clone();
    window.minimized = true;
    window.layout.normal = [-32000, -32000, -31400, -31580];
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    assert!(model.saved().windows[&key].layouts.is_empty());
    assert_eq!(model.saved().windows[&key].last_usable, fallback);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    assert!(model.take_requests(201).is_empty());
    model.snapshot(snapshot("A", 300, vec![window]));
    assert!(model.take_requests(301).is_empty());
}

#[test]
fn external_restore_clears_minimized_intent_only_after_stable_reachable_geometry() {
    let (mut model, mut window, key) = saved_destinations();
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    window.minimized = false;
    window.layout.normal = [-32000, -32000, -31400, -31580];
    model.snapshot(snapshot("A", 140, vec![window.clone()]));
    model.snapshot(snapshot("A", 250, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    window.layout = layout(450);
    model.snapshot(snapshot("A", 270, vec![window.clone()]));
    model.snapshot(snapshot("A", 369, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    model.snapshot(snapshot("A", 370, vec![window]));
    assert!(!model.saved().windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(450));
}

#[test]
fn equal_tick_samples_preserve_restore_evidence_without_learning_stale_state() {
    for duplicate_kind in ["identical", "minimized", "geometry"] {
        let (mut model, mut window, key) = saved_destinations();
        window.minimized = true;
        model.snapshot(snapshot("A", 120, vec![window.clone()]));
        window.minimized = false;
        model.snapshot(snapshot("A", 130, vec![window.clone()]));
        let mut duplicate = window.clone();
        match duplicate_kind {
            "minimized" => duplicate.minimized = true,
            "geometry" => duplicate.layout = layout(900),
            _ => {}
        }
        model.mark_clean();
        for _ in 0..3 {
            model.snapshot(snapshot("A", 130, vec![duplicate.clone()]));
        }
        assert!(
            model.runtime(&key).unwrap().external_restore().is_some(),
            "{duplicate_kind}"
        );
        assert_eq!(
            model
                .runtime(&key)
                .unwrap()
                .external_restore()
                .unwrap()
                .since,
            Some(130)
        );
        assert!(!model.runtime(&key).unwrap().window().minimized);
        assert_eq!(model.runtime(&key).unwrap().window().layout, layout(100));
        assert!(!model.is_dirty());
        model.snapshot(snapshot("A", 229, vec![window.clone()]));
        assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
        model.snapshot(snapshot("A", 230, vec![window.clone()]));
        assert!(!model.saved().windows[&key].minimized_desktops.contains("A"));
        assert_eq!(model.saved().windows[&key].layouts["A"], layout(100));
        model.snapshot(snapshot("B", 300, vec![window.clone()]));
        let restore_b = model.take_requests(301).remove(0);
        completed(&mut model, &key, restore_b.sequence, 320, true, false);
        model.snapshot(snapshot("A", 400, vec![window]));
        assert_eq!(
            model.take_requests(401)[0].action,
            Action::RestoreAutomatic(layout(100))
        );
    }
}

#[test]
fn equal_tick_samples_still_invalidate_restore_evidence_when_context_changes() {
    for invalidation in ["pending", "transition", "recovery"] {
        let (mut model, mut window, key) = saved_destinations();
        window.minimized = true;
        model.snapshot(snapshot("A", 120, vec![window.clone()]));
        window.minimized = false;
        model.snapshot(snapshot("A", 130, vec![window.clone()]));
        assert!(model.runtime(&key).unwrap().external_restore().is_some());
        match invalidation {
            "pending" => {
                click(&mut model, &key, false);
                let raise = model.take_requests(130).remove(0);
                model.snapshot(snapshot("A", 130, vec![window.clone()]));
                assert!(model.runtime(&key).unwrap().external_restore().is_none());
                completed(&mut model, &key, raise.sequence, 140, false, true);
            }
            "transition" => {
                model.pause(true);
                let mut frame = snapshot("A", 130, vec![window.clone()]);
                frame.transition_serial = 1;
                model.snapshot(frame);
                assert_eq!(model.transition_serial(), 1);
                assert!(model.runtime(&key).unwrap().external_restore().is_none());
            }
            _ => {
                model.unavailable();
                model.snapshot(snapshot("A", 130, vec![window.clone()]));
                assert!(model.runtime(&key).unwrap().external_restore().is_none());
            }
        }
        let serial = model.transition_serial();
        for tick in [150, 250] {
            let mut frame = snapshot("A", tick, vec![window.clone()]);
            frame.transition_serial = serial;
            model.snapshot(frame);
        }
        assert!(
            model.saved().windows[&key].minimized_desktops.contains("A"),
            "{invalidation}"
        );
    }
}

#[test]
fn first_visible_sample_after_verified_minimize_preserves_external_restore_intent() {
    for automatic in [false, true] {
        let (mut model, mut window, key) = saved_destinations();
        let desktop = if automatic {
            model
                .record_mut(&key)
                .unwrap()
                .minimized_desktops
                .insert("B".into());
            model.snapshot(snapshot("B", 200, vec![window.clone()]));
            "B"
        } else {
            click(&mut model, &key, true);
            "A"
        };
        let minimize = model.take_requests(210).remove(0);
        assert_eq!(minimize.action, Action::Minimize { automatic });
        completed(&mut model, &key, minimize.sequence, 220, true, false);
        assert!(model.runtime(&key).unwrap().suppress_next_state());
        window.minimized = false;
        model.snapshot(snapshot(desktop, 219, vec![window.clone()]));
        assert!(model.runtime(&key).unwrap().suppress_next_state());
        model.snapshot(snapshot(desktop, 230, vec![window.clone()]));
        model.snapshot(snapshot(desktop, 230, vec![window.clone()]));
        assert!(
            model.saved().windows[&key]
                .minimized_desktops
                .contains(desktop)
        );
        model.snapshot(snapshot(desktop, 329, vec![window.clone()]));
        assert!(
            model.saved().windows[&key]
                .minimized_desktops
                .contains(desktop)
        );
        model.snapshot(snapshot(desktop, 330, vec![window.clone()]));
        assert!(
            !model.saved().windows[&key]
                .minimized_desktops
                .contains(desktop),
            "automatic: {automatic}"
        );
        let other = if desktop == "A" { "B" } else { "A" };
        model.snapshot(snapshot(other, 400, vec![window.clone()]));
        let transition = model.take_requests(401).remove(0);
        completed(&mut model, &key, transition.sequence, 420, true, false);
        model.snapshot(snapshot(desktop, 500, vec![window.clone()]));
        assert_eq!(
            model.take_requests(501)[0].action,
            Action::RestoreAutomatic(window.layout)
        );
    }
}

#[test]
fn late_cancelled_or_superseded_minimize_does_not_authorize_suppressed_restore_learning() {
    for outcome in ["late", "cancelled", "superseded"] {
        let (mut model, window, key) = saved_destinations();
        model
            .record_mut(&key)
            .unwrap()
            .minimized_desktops
            .insert("A".into());
        click(&mut model, &key, true);
        let minimize = model.take_requests(110).remove(0);
        match outcome {
            "late" => completed(&mut model, &key, minimize.sequence, 120, false, false),
            "superseded" => click(&mut model, &key, false),
            _ => {}
        }
        completed(
            &mut model,
            &key,
            minimize.sequence,
            130,
            outcome != "cancelled",
            outcome == "cancelled",
        );
        model.snapshot(snapshot("A", 140, vec![window.clone()]));
        model.snapshot(snapshot("A", 240, vec![window]));
        assert!(
            model.saved().windows[&key].minimized_desktops.contains("A"),
            "{outcome}"
        );
    }
}

#[test]
fn post_minimize_restore_evidence_cannot_cross_recovery_switch_or_pending_work() {
    for invalidation in ["recovery", "switch", "pending"] {
        let (mut model, window, key) = saved_destinations();
        click(&mut model, &key, true);
        let minimize = model.take_requests(110).remove(0);
        completed(&mut model, &key, minimize.sequence, 120, true, false);
        match invalidation {
            "recovery" => model.unavailable(),
            "switch" => {
                model.pause(true);
                model.snapshot(snapshot("B", 125, vec![window.clone()]));
            }
            _ => {
                click(&mut model, &key, false);
                let restore = model.take_requests(125).remove(0);
                model.snapshot(snapshot("A", 130, vec![window.clone()]));
                completed(&mut model, &key, restore.sequence, 135, false, true);
            }
        }
        model.snapshot(snapshot("A", 140, vec![window.clone()]));
        model.snapshot(snapshot("A", 240, vec![window]));
        assert!(
            model.saved().windows[&key].minimized_desktops.contains("A"),
            "{invalidation}"
        );
    }
}

#[test]
fn first_post_minimize_restore_sample_during_drag_waits_for_post_drag_stability() {
    let (mut model, mut window, key) = saved_destinations();
    click(&mut model, &key, true);
    let minimize = model.take_requests(110).remove(0);
    completed(&mut model, &key, minimize.sequence, 120, true, false);
    model.moving(window.hwnd, true, 125, None);
    window.minimized = false;
    window.layout = layout(450);
    model.snapshot(snapshot("A", 130, vec![window.clone()]));
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    model.moving(window.hwnd, false, 310, None);
    model.snapshot(snapshot("A", 320, vec![window.clone()]));
    model.snapshot(snapshot("A", 419, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    model.snapshot(snapshot("A", 420, vec![window]));
    assert!(!model.saved().windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(450));
}

#[test]
fn external_restore_followed_by_drag_clears_intent_after_post_drag_stability() {
    for observed_before_drag in [false, true] {
        let (mut model, mut window, key) = saved_destinations();
        window.minimized = true;
        model.snapshot(snapshot("A", 120, vec![window.clone()]));
        window.minimized = false;
        if observed_before_drag {
            model.snapshot(snapshot("A", 130, vec![window.clone()]));
        }
        model.moving(window.hwnd, true, 140, None);
        window.layout = layout(400);
        model.snapshot(snapshot("A", 160, vec![window.clone()]));
        model.snapshot(snapshot("A", 300, vec![window.clone()]));
        model.snapshot(snapshot("A", 300, vec![window.clone()]));
        assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
        model.moving(window.hwnd, false, 310, None);
        model.moving(window.hwnd, false, 310, Some(window.layout.clone()));
        model.snapshot(snapshot("A", 320, vec![window.clone()]));
        model.snapshot(snapshot("A", 320, vec![window.clone()]));
        model.snapshot(snapshot("A", 419, vec![window.clone()]));
        assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
        model.snapshot(snapshot("A", 420, vec![window.clone()]));
        assert!(
            !model.saved().windows[&key].minimized_desktops.contains("A"),
            "restore observed before drag: {observed_before_drag}"
        );
        assert_eq!(model.saved().windows[&key].layouts["A"], layout(400));
        assert_eq!(model.saved().windows[&key].last_usable, layout(400));
        model.snapshot(snapshot("B", 500, vec![window.clone()]));
        let restore_b = model.take_requests(501).remove(0);
        completed(&mut model, &key, restore_b.sequence, 520, true, false);
        model.snapshot(snapshot("A", 600, vec![window]));
        assert_eq!(
            model.take_requests(601)[0].action,
            Action::RestoreAutomatic(layout(400))
        );
    }
}

#[test]
fn restore_drag_evidence_does_not_survive_untrusted_context_or_reminimization() {
    for invalidation in ["switch", "recovery", "pending", "minimize"] {
        let (mut model, mut window, key) = saved_destinations();
        model.pause(true);
        window.minimized = true;
        model.snapshot(snapshot("A", 120, vec![window.clone()]));
        model.moving(window.hwnd, true, 140, None);
        window.minimized = false;
        model.snapshot(snapshot("A", 160, vec![window.clone()]));
        model.moving(window.hwnd, false, 170, None);
        let current = match invalidation {
            "switch" => {
                model.snapshot(snapshot("B", 180, vec![window.clone()]));
                "B"
            }
            "recovery" => {
                model.unavailable();
                model.snapshot(snapshot("A", 180, vec![window.clone()]));
                "A"
            }
            "pending" => {
                click(&mut model, &key, false);
                let raise = model.take_requests(180).remove(0);
                model.snapshot(snapshot("A", 190, vec![window.clone()]));
                completed(&mut model, &key, raise.sequence, 200, false, true);
                "A"
            }
            _ => {
                window.minimized = true;
                model.snapshot(snapshot("A", 180, vec![window.clone()]));
                "A"
            }
        };
        model.snapshot(snapshot(current, 300, vec![window.clone()]));
        model.snapshot(snapshot(current, 500, vec![window]));
        assert!(
            model.saved().windows[&key].minimized_desktops.contains("A"),
            "{invalidation}"
        );
        assert!(
            model.runtime(&key).unwrap().external_restore().is_none(),
            "{invalidation}"
        );
    }
}

#[test]
fn post_drag_restore_rejects_unreachable_bounds_and_late_capture_cannot_backdate_stability() {
    let (mut model, mut window, key) = saved_destinations();
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    model.moving(window.hwnd, true, 140, None);
    window.minimized = false;
    window.layout.normal = [-32000, -32000, -31400, -31580];
    model.snapshot(snapshot("A", 160, vec![window.clone()]));
    model.moving(window.hwnd, false, 170, None);
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    window.layout = layout(500);
    model.snapshot(snapshot("A", 320, vec![window.clone()]));
    // The asynchronous drag-end capture can arrive after this newer frame.
    model.moving(window.hwnd, false, 170, Some(window.layout.clone()));
    model.snapshot(snapshot("A", 419, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    model.snapshot(snapshot("A", 420, vec![window]));
    assert!(!model.saved().windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(500));
}

#[test]
fn automatic_actions_and_their_native_events_do_not_change_user_intent() {
    let (mut model, mut window, key) = saved_destinations();
    model.set_auto_restore(false);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    let event = minimize_observation(&model, &window, 210, 211);
    assert_eq!(
        model.observe_minimized(event.clone()),
        Err(MinimizeRejection::OperationOutstanding)
    );
    window.minimized = true;
    model.snapshot(snapshot("B", 215, vec![window.clone()]));
    completed(&mut model, &key, request.sequence, 220, true, false);
    assert_eq!(
        model.observe_minimized(event),
        Err(MinimizeRejection::AlreadyMinimized)
    );
    model.snapshot(snapshot("B", 230, vec![window.clone()]));
    model.snapshot(snapshot("B", 240, vec![window]));
    assert!(model.saved().windows[&key].minimized_desktops.is_empty());
}

#[test]
fn stale_cancelled_failed_and_late_explicit_completions_do_not_commit_intent() {
    for outcome in ["cancel", "timeout", "supersede", "switch"] {
        let (mut model, window, key) = saved_destinations();
        click(&mut model, &key, true);
        let request = model.take_requests(110).remove(0);
        match outcome {
            "cancel" => completed(&mut model, &key, request.sequence, 120, false, true),
            "timeout" => {
                completed(&mut model, &key, request.sequence, 120, false, false);
                completed(&mut model, &key, request.sequence, 130, true, false);
            }
            "supersede" => {
                click(&mut model, &key, false);
                completed(&mut model, &key, request.sequence, 130, true, false);
            }
            _ => {
                model.snapshot(snapshot("B", 120, vec![window]));
                completed(&mut model, &key, request.sequence, 130, true, false);
            }
        }
        assert!(
            model.saved().windows[&key].minimized_desktops.is_empty(),
            "{outcome}"
        );
    }
}

#[test]
fn native_minimize_before_immediate_switch_preserves_outgoing_intent() {
    let (mut model, window, key) = saved_destinations();
    assert!(
        model
            .observe_minimized(minimize_observation(&model, &window, 110, 115))
            .is_ok()
    );
    // An already-queued older frame must not undo the newer native state.
    model.snapshot(snapshot("A", 112, vec![window.clone()]));
    assert!(model.runtime(&key).unwrap().window().minimized);
    model.snapshot(snapshot("B", 113, vec![window.clone()]));
    assert_eq!(model.current(), Some("A"));
    model.snapshot(snapshot("A", 115, vec![window.clone()]));
    assert!(model.runtime(&key).unwrap().window().minimized);
    model.snapshot(snapshot("B", 120, vec![window.clone()]));
    let request = model.take_requests(121).remove(0);
    assert_eq!(request.action, Action::RestoreAutomatic(layout(700)));
    completed(&mut model, &key, request.sequence, 140, true, false);
    model.snapshot(snapshot("A", 150, vec![window]));
    assert_eq!(
        model.take_requests(151)[0].action,
        Action::Minimize { automatic: true }
    );
    assert_eq!(
        model.saved().windows[&key].minimized_desktops,
        ["A".into()].into()
    );
}

#[test]
fn native_minimize_rejects_duplicates_wrong_context_and_recycled_identity() {
    for (invalid, expected) in [
        ("duplicate", MinimizeRejection::AlreadyMinimized),
        ("generation", MinimizeRejection::StaleContext),
        ("desktop", MinimizeRejection::StaleContext),
        ("serial", MinimizeRejection::StaleContext),
        ("sequence", MinimizeRejection::StaleSequence),
        ("identity", MinimizeRejection::UnknownWindow),
        ("hwnd", MinimizeRejection::IdentityChanged),
        ("drag", MinimizeRejection::StaleSequence),
        ("unavailable", MinimizeRejection::StaleContext),
    ] {
        let (mut model, window, key) = baseline();
        let mut event = minimize_observation(&model, &window, 110, 120);
        match invalid {
            "duplicate" => {
                assert!(model.observe_minimized(event.clone()).is_ok());
            }
            "generation" => event.generation += 1,
            "desktop" => event.desktop = "B".into(),
            "serial" => event.transition_serial += 1,
            "sequence" => event.sequence += 1,
            "identity" => event.window.identity.process_created += 1,
            "hwnd" => event.window.hwnd += 1,
            "drag" => model.moving(window.hwnd, true, 105, None),
            _ => model.unavailable(),
        }
        model.mark_clean();
        assert_eq!(model.observe_minimized(event), Err(expected), "{invalid}");
        assert!(!model.is_dirty(), "{invalid}");
        if invalid != "duplicate" {
            assert!(model.saved().windows[&key].minimized_desktops.is_empty());
        }
    }
}

#[test]
fn late_restore_reconciles_to_minimized_saved_destination_without_clearing_intent() {
    let (mut model, mut window, key) = saved_destinations();
    model
        .record_mut(&key)
        .unwrap()
        .minimized_desktops
        .insert("A".into());
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let restore = model.take_requests(201).remove(0);
    dispatched(&mut model, &restore);
    model.snapshot(snapshot("A", 210, vec![window]));
    assert!(model.take_requests(211).is_empty());
    completed(&mut model, &key, restore.sequence, 220, false, false);
    completed(&mut model, &key, restore.sequence, 300, true, false);
    assert_eq!(
        model.take_requests(301)[0].action,
        Action::Minimize { automatic: true }
    );
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
}

#[test]
fn minimized_intent_persists_but_baselines_do_not_infer_it_and_deleted_desktops_are_pruned() {
    let (mut model, mut window, key) = saved_destinations();
    model.pause(true);
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    let saved = Saved::decode(&serde_json::to_vec(model.saved()).unwrap()).unwrap();
    let mut restarted = Model::new(saved);
    window.minimized = false;
    restarted.snapshot(snapshot("A", 200, vec![window.clone()]));
    restarted.pause(false);
    restarted.set_auto_restore(false);
    restarted.set_auto_restore(true);
    restarted.unavailable();
    restarted.snapshot(snapshot("A", 220, vec![window.clone()]));
    assert!(restarted.take_requests(221).is_empty());
    assert!(
        restarted.saved().windows[&key]
            .minimized_desktops
            .contains("A")
    );
    let mut frame = snapshot("B", 300, vec![window]);
    frame.desktops.retain(|d| d.id == "B");
    restarted.snapshot(frame);
    assert!(
        restarted.saved().windows[&key]
            .minimized_desktops
            .is_empty()
    );
    assert!(!restarted.saved().windows[&key].layouts.contains_key("A"));
}

#[test]
fn minimized_intent_legacy_default_and_union_limit_are_validated() {
    let (model, _, key) = baseline();
    let mut json = serde_json::to_value(model.saved()).unwrap();
    json["windows"][&key]
        .as_object_mut()
        .unwrap()
        .remove("minimized_desktops");
    let mut saved = Saved::decode(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(saved.windows[&key].minimized_desktops.is_empty());
    let record = saved.windows.get_mut(&key).unwrap();
    record.layouts = (0..64).map(|i| (i.to_string(), layout(100))).collect();
    record.minimized_desktops = ["0".into()].into();
    assert!(Saved::decode(&serde_json::to_vec(&saved).unwrap()).is_ok());
    saved
        .windows
        .get_mut(&key)
        .unwrap()
        .minimized_desktops
        .insert("65".into());
    assert!(Saved::decode(&serde_json::to_vec(&saved).unwrap()).is_err());
}

#[test]
fn unsuccessful_manual_restore_retains_the_minimized_preference() {
    for cancelled in [false, true] {
        let (mut model, mut window, key) = saved_destinations();
        window.minimized = true;
        model.snapshot(snapshot("A", 120, vec![window.clone()]));
        click(&mut model, &key, false);
        let restore = model.take_requests(121).remove(0);
        completed(&mut model, &key, restore.sequence, 140, false, cancelled);
        if !cancelled {
            completed(&mut model, &key, restore.sequence, 300, true, false);
            window.minimized = false;
            model.snapshot(snapshot("A", 310, vec![window.clone()]));
            model.snapshot(snapshot("A", 420, vec![window]));
        }
        assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
    }
}

#[test]
fn native_manual_minimize_immediately_after_verified_restore_is_not_suppressed() {
    let (mut model, mut window, key) = saved_destinations();
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    click(&mut model, &key, false);
    let restore = model.take_requests(121).remove(0);
    completed(&mut model, &key, restore.sequence, 140, true, false);
    assert!(model.runtime(&key).unwrap().suppress_next_state());
    assert_eq!(
        model.observe_minimized(minimize_observation(&model, &window, 130, 141)),
        Err(MinimizeRejection::OwnAction)
    );
    assert!(
        model
            .observe_minimized(minimize_observation(&model, &window, 150, 151))
            .is_ok()
    );
    assert!(model.saved().windows[&key].minimized_desktops.contains("A"));
}

#[test]
fn baseline_pin_discovery_recovery_and_resume_do_not_minimize() {
    let (mut model, window, _) = baseline();
    assert!(model.take_requests(101).is_empty());
    model.pause(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    assert!(model.take_requests(201).is_empty());
    model.pause(false);
    assert!(model.take_requests(202).is_empty());
    model.unavailable();
    model.snapshot(snapshot("A", 300, vec![window]));
    assert!(model.take_requests(301).is_empty());
}

#[test]
fn switches_only_minimize_visible_windows_and_never_restore_automatically() {
    let (mut model, mut window, key) = baseline();
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let requests = model.take_requests(201);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].action, Action::Minimize { automatic: true });
    completed(&mut model, &key, requests[0].sequence, 210, true, false);
    window.minimized = true;
    model.snapshot(snapshot("A", 300, vec![window]));
    assert!(model.take_requests(301).is_empty());
}

#[test]
fn pause_cancels_queued_automatic_work_but_keeps_manual_actions() {
    let (mut model, window, key) = baseline();
    model.snapshot(snapshot("B", 200, vec![window]));
    model.pause(true);
    assert!(model.take_requests(201).is_empty());
    click(&mut model, &key, true);
    assert_eq!(
        model.take_requests(202)[0].action,
        Action::Minimize { automatic: false }
    );
}

#[test]
fn latest_click_waits_for_outstanding_operation_and_supersedes_auto_intent() {
    let (mut model, mut window, key) = baseline();
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let old = model.take_requests(201).remove(0);
    dispatched(&mut model, &old);
    window.minimized = true;
    model.snapshot(snapshot("B", 210, vec![window]));
    click(&mut model, &key, false);
    assert!(model.take_requests(211).is_empty());
    completed(&mut model, &key, old.sequence, 220, true, false);
    assert!(matches!(
        model.take_requests(221)[0].action,
        Action::Restore(_)
    ));
}

#[test]
fn old_restore_completion_cannot_write_into_the_incoming_desktop() {
    let (mut model, mut window, key) = baseline();
    model.moving(window.hwnd, false, 110, Some(layout(250)));
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    click(&mut model, &key, false);
    let request = model.take_requests(121).remove(0);
    dispatched(&mut model, &request);
    window.minimized = false;
    window.layout = layout(800);
    model.snapshot(snapshot("B", 130, vec![window]));
    completed(&mut model, &key, request.sequence, 140, true, false);
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(250));
    assert!(!model.saved().windows[&key].layouts.contains_key("B"));
    assert_eq!(
        model.take_requests(141)[0].action,
        Action::Minimize { automatic: true }
    );
}

#[test]
fn utility_restart_preserves_layout_but_external_restore_updates_it() {
    let (mut model, mut window, key) = baseline();
    model.moving(window.hwnd, false, 110, Some(layout(250)));
    let saved = model.saved().clone();
    window.layout = layout(900);
    let mut restarted = Model::new(saved);
    restarted.snapshot(snapshot("A", 200, vec![window.clone()]));
    assert_eq!(restarted.saved().windows[&key].layouts["A"], layout(250));
    window.minimized = true;
    restarted.snapshot(snapshot("A", 210, vec![window.clone()]));
    window.minimized = false;
    restarted.snapshot(snapshot("A", 220, vec![window.clone()]));
    restarted.snapshot(snapshot("A", 330, vec![window]));
    assert_eq!(restarted.saved().windows[&key].layouts["A"], layout(900));
}

#[test]
fn recycled_hwnd_and_ambiguous_restart_never_receive_old_layouts_or_results() {
    let (mut model, old, old_key) = baseline();
    model.moving(old.hwnd, false, 110, Some(layout(250)));
    click(&mut model, &old_key, true);
    let request = model.take_requests(120).remove(0);
    let mut replacement = window("new-token");
    replacement.identity.process_created = 200;
    let new_key = replacement.identity.key();
    model.snapshot(snapshot("A", 130, vec![replacement]));
    completed(&mut model, &old_key, request.sequence, 140, true, false);
    assert!(model.runtime(&new_key).unwrap().pending().is_none());
    assert!(model.saved().windows[&new_key].layouts.is_empty());
    assert!(model.saved().windows.contains_key(&old_key));
    assert!(
        model.saved().windows[&old_key]
            .minimized_desktops
            .is_empty()
    );
}

#[test]
fn duplicate_tokens_restore_independent_records_on_utility_restart() {
    let (mut model, mut first, first_key) = baseline();
    let mut second = window("two");
    second.hwnd = 124;
    let second_key = second.identity.key();
    model.snapshot(snapshot("A", 110, vec![first.clone(), second.clone()]));
    model.moving(first.hwnd, false, 120, Some(layout(200)));
    model.moving(second.hwnd, false, 121, Some(layout(800)));
    let mut restarted = Model::new(model.saved().clone());
    first.minimized = true;
    second.minimized = true;
    restarted.snapshot(snapshot("A", 200, vec![second, first]));
    click(&mut restarted, &first_key, false);
    click(&mut restarted, &second_key, false);
    let requests = restarted.take_requests(210);
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .any(|r| r.key == first_key && r.action == Action::Restore(layout(200)))
    );
    assert!(
        requests
            .iter()
            .any(|r| r.key == second_key && r.action == Action::Restore(layout(800)))
    );
}

#[test]
fn timeout_blocks_backlogs_and_late_verification_does_not_commit_geometry() {
    let (mut model, _, key) = baseline();
    click(&mut model, &key, true);
    let request = model.take_requests(120).remove(0);
    completed(&mut model, &key, request.sequence, 200, false, false);
    assert!(model.has_quarantined());
    click(&mut model, &key, false);
    assert!(model.take_requests(201).is_empty());
    completed(&mut model, &key, request.sequence, 300, true, false);
    assert!(!model.runtime(&key).unwrap().blocked());
    assert!(model.saved().windows[&key].layouts.is_empty());
}

#[test]
fn drag_cancels_queued_auto_work_and_stale_drag_events_are_ignored() {
    let (mut model, window, key) = baseline();
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    model.moving(window.hwnd, true, 201, None);
    assert!(model.take_requests(202).is_empty());
    model.moving(window.hwnd, false, 150, Some(layout(900)));
    assert!(model.saved().windows[&key].layouts.is_empty());
    model.moving(window.hwnd, false, 210, Some(layout(300)));
    assert_eq!(model.saved().windows[&key].layouts["B"], layout(300));
}

#[test]
fn coalesced_a_b_a_transition_still_minimizes() {
    let (mut model, window, _) = baseline();
    let mut incoming = snapshot("A", 200, vec![window]);
    incoming.transition_serial = 2;
    model.snapshot(incoming);
    assert_eq!(
        model.take_requests(201)[0].action,
        Action::Minimize { automatic: true }
    );
}

#[test]
fn minimized_window_restores_even_if_windows_still_reports_it_as_foreground() {
    let (mut model, window, key) = baseline();
    model.snapshot(snapshot("A", 200, vec![window]));
    // Snapshot saw the window visible, but the click-time `IsIconic` query finds it
    // minimized while Windows still reports it as foreground.
    model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: true,
        },
    );
    assert!(matches!(
        model.take_requests(201)[0].action,
        Action::Restore(_)
    ));
}

#[test]
fn verified_manual_maximized_restore_establishes_a_desktop_layout() {
    let (mut model, mut window, key) = baseline();
    let mut saved = layout(250);
    saved.maximized = true;
    model.record_mut(&key).unwrap().last_usable = saved.clone();
    window.minimized = true;
    model.snapshot(snapshot("A", 200, vec![window.clone()]));
    click(&mut model, &key, false);
    let request = model.take_requests(201).remove(0);
    completed(&mut model, &key, request.sequence, 220, true, false);
    window.minimized = false;
    window.layout = saved.clone();
    model.snapshot(snapshot("A", 230, vec![window]));
    assert_eq!(model.saved().windows[&key].layouts["A"], saved);
    assert_eq!(model.saved().windows[&key].last_usable, saved);
}

#[test]
fn monitor_clamping_handles_removed_monitor_and_reserved_work_offsets() {
    let saved = Layout {
        normal: [3000, -100, 4200, 1000],
        monitor_rect: [2560, 0, 5120, 1440],
        work_area: [2590, 0, 5120, 1400],
        ..layout(100)
    };
    let adjusted = saved.adjusted("main", [0, 0, 1280, 720], [31, 0, 1280, 680], 120);
    let screen_left = adjusted.normal[0] + 31;
    assert!(screen_left >= 31 && adjusted.normal[2] + 31 <= 1280);
    assert!(adjusted.normal[1] >= 0 && adjusted.normal[3] <= 680);
}

#[test]
fn persisted_state_has_no_hwnd_and_rejects_unsafe_geometry() {
    let (mut model, _, key) = baseline();
    let encoded = serde_json::to_vec(model.saved()).unwrap();
    assert!(!String::from_utf8(encoded.clone()).unwrap().contains("hwnd"));
    assert!(Saved::decode(&encoded).is_ok());
    model.record_mut(&key).unwrap().layouts = BTreeMap::from([(
        "A".into(),
        Layout {
            dpi: 0,
            ..layout(100)
        },
    )]);
    assert!(Saved::decode(&serde_json::to_vec(model.saved()).unwrap()).is_err());
}

#[test]
fn legacy_configuration_does_not_opt_into_unvalidated_revisions() {
    let (model, _, _) = baseline();
    let mut value = serde_json::to_value(model.saved()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("experimental_integration");
    value.as_object_mut().unwrap().remove("auto_restore");
    let saved = Saved::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(!saved.experimental_integration);
    assert!(!saved.auto_restore);
    let mut enabled = saved;
    enabled.experimental_integration = true;
    assert!(
        Saved::decode(&serde_json::to_vec(&enabled).unwrap())
            .unwrap()
            .experimental_integration
    );
}

#[test]
fn auto_restore_uses_only_destination_layouts_for_each_window() {
    let (mut model, first, first_key) = baseline();
    let mut second = window("two");
    second.hwnd = 124;
    let second_key = second.identity.key();
    model.snapshot(snapshot("A", 110, vec![first.clone(), second.clone()]));
    model
        .record_mut(&first_key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    model.set_auto_restore(true);
    assert!(model.take_requests(120).is_empty());
    model.snapshot(snapshot("B", 200, vec![first, second]));
    let requests = model.take_requests(201);
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .any(|r| r.key == first_key && r.action == Action::RestoreAutomatic(layout(700)))
    );
    assert!(
        requests
            .iter()
            .any(|r| r.key == second_key && r.action == Action::Minimize { automatic: true })
    );
}

#[test]
fn snap_policy_defaults_migrates_and_persists_explicit_opt_out() {
    let mut value = serde_json::to_value(Saved::default()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("minimize_snapped_elsewhere");
    let saved = Saved::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(saved.minimize_snapped_elsewhere);
    let mut model = Model::new(saved);
    let revision = model.saved().preferences_revision;
    model.set_minimize_snapped_elsewhere(false);
    assert!(model.is_dirty());
    assert_eq!(model.saved().preferences_revision, revision + 1);
    let saved = Saved::decode(&serde_json::to_vec(model.saved()).unwrap()).unwrap();
    assert!(!saved.minimize_snapped_elsewhere);
    assert!(!Model::new(saved).saved().minimize_snapped_elsewhere);
}

fn pre_minimize_snap_sample() -> (Model, Window, String, Request, Window) {
    let (mut model, mut window, key) = saved_destinations();
    window.layout.visible = Some([125, 80, 725, 500]);
    window.layout.arranged = Some(false);
    model.snapshot(snapshot("A", 110, vec![window.clone()]));
    model.click(
        &key,
        Click {
            minimized: false,
            was_foreground: true,
        },
    );
    let request = model.take_requests(120).pop().unwrap();
    let mut sampled = window.clone();
    sampled.layout.arranged = Some(true);
    sampled.layout.visible = Some([25, 0, 960, 1040]);
    model.accepted(
        &key,
        request.sequence,
        Acceptance {
            effect: Effect::Minimize,
            target: Some(sampled.layout.clone()),
        },
    );
    (model, window, key, request, sampled)
}

#[test]
fn native_snap_evidence_survives_supersession_timeout_and_delayed_delivery_without_memory_commit() {
    for late in [false, true] {
        for sample_tick in [120, 130] {
            let (mut model, mut window, key, request, sampled) = pre_minimize_snap_sample();
            window.minimized = true;
            window.layout.visible = None;
            model.snapshot(snapshot("B", 130, vec![window]));
            let saved = serde_json::to_value(model.saved()).unwrap();
            model.mark_clean();
            if late {
                model.settle(&key, request.sequence, 140, Outcome::TimedOut);
            }
            // A later minimized snapshot (including the same tick) is not unsnap evidence.
            model.observed_arrangement(&request, &sampled.layout, sample_tick);
            assert!(!model.is_dirty());
            let settled = model
                .settle(&key, request.sequence, 150, Outcome::Verified)
                .unwrap();
            assert!(!settled.current_verified);
            assert!(!settled.committed);
            let next = model.take_requests(160).pop().unwrap();
            assert!(matches!(next.action, Action::RestoreAutomatic(_)));
            assert_eq!(next.retained_arrangement.as_deref(), Some(&sampled.layout));
            assert_eq!(serde_json::to_value(model.saved()).unwrap(), saved);
        }
    }
}

#[test]
fn old_native_snap_evidence_cannot_override_newer_visible_state_or_display_boundaries() {
    for mode in 0..6 {
        let (mut model, mut window, key, request, sampled) = pre_minimize_snap_sample();
        window.minimized = true;
        window.layout.visible = None;
        model.snapshot(snapshot("B", 130, vec![window.clone()]));
        let expected = match mode {
            0 | 1 => {
                // Newer, or equal-time, visible unsnap.
                window.minimized = false;
                window.layout.visible = Some([125, 80, 725, 500]);
                model.snapshot(snapshot("B", 140, vec![window.clone()]));
                None
            }
            2 => {
                // A newer visible Snap wins over the delayed old bounds.
                window = sampled.clone();
                window.layout.visible = Some([25, 0, 1100, 1040]);
                model.snapshot(snapshot("B", 140, vec![window.clone()]));
                Some(window.layout.clone())
            }
            _ => {
                // Context changes invalidate even when topology returns later.
                match mode {
                    3 => window.layout.dpi = 120,
                    4 => window.layout.work_area[0] += 25,
                    _ => window.layout.monitor = "other".into(),
                }
                model.snapshot(snapshot("B", 140, vec![window.clone()]));
                window.layout = sampled.layout.clone();
                window.layout.visible = None;
                window.layout.arranged = Some(false);
                model.snapshot(snapshot("B", 150, vec![window.clone()]));
                None
            }
        };
        model.observed_arrangement(&request, &sampled.layout, if mode == 1 { 140 } else { 120 });
        model.settle(&key, request.sequence, 160, Outcome::Verified);
        let next = model.take_requests(170).pop().unwrap();
        assert_eq!(
            next.retained_arrangement.as_deref(),
            expected.as_ref(),
            "mode {mode}"
        );
    }
}

#[test]
fn current_minimize_completion_does_not_redate_old_snap_evidence_over_a_newer_unsnap() {
    let (mut model, mut window, key, request, sampled) = pre_minimize_snap_sample();
    model.observed_arrangement(&request, &sampled.layout, 120);
    model.snapshot(snapshot("A", 130, vec![window.clone()]));
    let settled = model
        .settle(&key, request.sequence, 140, Outcome::Verified)
        .unwrap();
    assert!(settled.current_verified);
    window.minimized = true;
    window.layout.visible = None;
    model.snapshot(snapshot("B", 150, vec![window]));
    assert!(
        model
            .take_requests(160)
            .pop()
            .unwrap()
            .retained_arrangement
            .is_none()
    );
}

#[test]
fn delayed_native_snap_evidence_cannot_override_a_newer_drag_capture() {
    for arranged in [false, true] {
        let (mut model, mut window, key, request, sampled) = pre_minimize_snap_sample();
        window.minimized = true;
        window.layout.visible = None;
        model.snapshot(snapshot("B", 130, vec![window]));
        model.settle(&key, request.sequence, 140, Outcome::Verified);
        let mut captured = sampled.layout.clone();
        captured.arranged = Some(arranged);
        captured.visible = Some([25, 0, 1100, 1040]);
        model.captured(&key, model.generation(), 180, captured.clone());
        model.observed_arrangement(&request, &sampled.layout, 120);
        model.click(
            &key,
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let next = model.take_requests(190).pop().unwrap();
        assert_eq!(
            next.retained_arrangement.as_deref(),
            arranged.then_some(&captured)
        );
    }
}

#[test]
fn equal_tick_native_snap_requires_unchanged_request_baseline_and_ignores_duplicate_samples() {
    for observed_before_request in [false, true] {
        let (mut model, mut window, key) = saved_destinations();
        window.layout.visible = Some([125, 80, 725, 500]);
        window.layout.arranged = Some(false);
        model.snapshot(snapshot("A", 110, vec![window.clone()]));
        if observed_before_request {
            model.snapshot(snapshot("A", 120, vec![window.clone()]));
        }
        model.click(
            &key,
            Click {
                minimized: false,
                was_foreground: true,
            },
        );
        let request = model.take_requests(120).pop().unwrap();
        if !observed_before_request {
            model.snapshot(snapshot("A", 120, vec![window.clone()]));
        }
        let mut sampled = window.layout.clone();
        sampled.arranged = Some(true);
        sampled.visible = Some([25, 0, 960, 1040]);
        model.observed_arrangement(&request, &sampled, 120);
        // A repeated report with contradictory bounds/state at the same tick
        // cannot replace the first accepted evidence, even for a current ticket.
        model.observed_arrangement(&request, &window.layout, 120);
        model.accepted(
            &key,
            request.sequence,
            Acceptance {
                effect: Effect::Minimize,
                target: Some(sampled.clone()),
            },
        );
        model.settle(&key, request.sequence, 140, Outcome::Verified);
        window.minimized = true;
        window.layout.visible = None;
        model.snapshot(snapshot("B", 150, vec![window]));
        let next = model.take_requests(160).pop().unwrap();
        assert_eq!(
            next.retained_arrangement.as_deref(),
            observed_before_request.then_some(&sampled)
        );
    }
}

#[test]
fn native_snap_samples_cannot_follow_identity_replacement_or_removal() {
    for mode in 0..4 {
        let (mut model, mut window, key, request, sampled) = pre_minimize_snap_sample();
        window.minimized = true;
        window.layout.visible = None;
        match mode {
            0 => window.hwnd += 1,
            1 => window.pid += 1,
            _ => window.identity.token = "replacement".into(),
        }
        if mode == 3 {
            // Definitively removed before a new identity appears.
            model.snapshot(snapshot("B", 130, vec![]));
            model.settle(&key, request.sequence, 140, Outcome::IdentityLost);
            model.snapshot(snapshot("B", 150, vec![window.clone()]));
        } else {
            model.snapshot(snapshot("B", 130, vec![window.clone()]));
        }
        model.observed_arrangement(&request, &sampled.layout, 160);
        if mode != 3 {
            model.settle(&key, request.sequence, 170, Outcome::IdentityLost);
        }
        model.click(
            &window.identity.key(),
            Click {
                minimized: true,
                was_foreground: false,
            },
        );
        let next = model
            .take_requests(180)
            .into_iter()
            .find(|next| next.key == window.identity.key())
            .unwrap();
        assert!(next.retained_arrangement.is_none(), "mode {mode}");
    }
}

#[test]
fn snap_policy_changes_cancel_automatic_work_without_moving_windows_or_manual_intent() {
    let (mut model, window, key) = saved_destinations();
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(200).pop().unwrap();
    model.set_minimize_snapped_elsewhere(false);
    assert_ne!(model.runtime(&key).unwrap().sequence(), request.sequence);
    assert!(!model.runtime(&key).unwrap().window().minimized);
    model.accepted(
        &key,
        request.sequence,
        Acceptance {
            effect: Effect::Minimize,
            target: Some(window.layout.clone()),
        },
    );
    let settled = model
        .settle(&key, request.sequence, 210, Outcome::Verified)
        .unwrap();
    assert!(!settled.current_verified);
    assert!(!settled.committed);
    assert!(model.take_requests(220).is_empty());
    model.click(
        &key,
        Click {
            minimized: true,
            was_foreground: false,
        },
    );
    let manual = model.take_requests(230).pop().unwrap();
    model.set_minimize_snapped_elsewhere(true);
    assert_eq!(model.runtime(&key).unwrap().sequence(), manual.sequence);
    assert!(model.runtime(&key).unwrap().pending().is_some());
    model.pause(true);
    model.snapshot(snapshot("A", 240, vec![window]));
    assert!(model.runtime(&key).unwrap().desired().is_none());
}

#[test]
fn manual_rail_restore_on_new_desktop_enables_later_automatic_restore() {
    let (mut model, mut window, key) = baseline();
    model.set_auto_restore(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let minimize = model.take_requests(201).remove(0);
    completed(&mut model, &key, minimize.sequence, 220, true, false);
    window.minimized = true;
    model.snapshot(snapshot("B", 230, vec![window.clone()]));
    click(&mut model, &key, false);
    let restore = model.take_requests(240).remove(0);
    completed(&mut model, &key, restore.sequence, 260, true, false);
    assert_eq!(model.saved().windows[&key].layouts["B"], window.layout);
    window.minimized = false;
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    let minimize = model.take_requests(301).remove(0);
    completed(&mut model, &key, minimize.sequence, 320, true, false);
    window.minimized = true;
    model.snapshot(snapshot("B", 400, vec![window]));
    assert!(matches!(
        model.take_requests(401)[0].action,
        Action::RestoreAutomatic(_)
    ));
}

#[test]
fn external_taskbar_restore_establishes_layout_without_move_or_resize() {
    let (mut model, mut window, key) = baseline();
    model.set_auto_restore(true);
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    assert!(model.take_requests(201).is_empty());
    window.minimized = false;
    window.layout = layout(600);
    model.snapshot(snapshot("B", 220, vec![window.clone()]));
    model.snapshot(snapshot("B", 330, vec![window.clone()]));
    assert_eq!(model.saved().windows[&key].layouts["B"], layout(600));
    model.snapshot(snapshot("A", 400, vec![window.clone()]));
    let minimize = model.take_requests(401).remove(0);
    completed(&mut model, &key, minimize.sequence, 420, true, false);
    window.minimized = true;
    model.snapshot(snapshot("B", 500, vec![window]));
    assert_eq!(
        model.take_requests(501)[0].action,
        Action::RestoreAutomatic(layout(600))
    );
}

#[test]
fn external_restore_waits_for_stable_reachable_geometry() {
    let (mut model, mut window, key) = baseline();
    window.minimized = true;
    model.snapshot(snapshot("A", 200, vec![window.clone()]));
    window.minimized = false;
    window.layout.normal = [-32000, -32000, -31800, -31900];
    model.snapshot(snapshot("A", 220, vec![window.clone()]));
    model.snapshot(snapshot("A", 400, vec![window.clone()]));
    assert!(model.saved().windows[&key].layouts.is_empty());
    window.layout = layout(600);
    model.snapshot(snapshot("A", 500, vec![window.clone()]));
    assert!(model.saved().windows[&key].layouts.is_empty());
    model.snapshot(snapshot("A", 610, vec![window]));
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(600));
}

#[test]
fn automatic_restore_never_rewrites_saved_layout_or_fallback() {
    let (mut model, mut window, key) = baseline();
    model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    let last_usable = model.saved().windows[&key].last_usable.clone();
    model.set_auto_restore(true);
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    model.accepted(
        &key,
        request.sequence,
        Acceptance {
            effect: Effect::Restore,
            target: Some(layout(500)),
        },
    );
    completed(&mut model, &key, request.sequence, 220, true, false);
    // A snapshot captured before verification cannot consume own-action suppression.
    model.snapshot(snapshot("B", 210, vec![window.clone()]));
    window.minimized = false;
    window.layout = layout(500);
    model.snapshot(snapshot("B", 230, vec![window.clone()]));
    model.snapshot(snapshot("B", 240, vec![window]));
    assert_eq!(model.saved().windows[&key].layouts["B"], layout(700));
    assert_eq!(model.saved().windows[&key].last_usable, last_usable);
}

#[test]
fn late_automatic_restore_is_minimized_on_a_new_destination() {
    let (mut model, mut window, key) = baseline();
    model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    model.set_auto_restore(true);
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    dispatched(&mut model, &request);
    model.snapshot(snapshot("A", 210, vec![window]));
    assert!(model.take_requests(211).is_empty());
    completed(&mut model, &key, request.sequence, 230, true, false);
    assert_eq!(
        model.take_requests(231)[0].action,
        Action::Minimize { automatic: true }
    );
    assert!(!model.saved().windows[&key].layouts.contains_key("A"));
}

#[test]
fn pending_minimize_finishes_before_latest_desktop_restore() {
    let (mut model, window, key) = baseline();
    model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), layout(250));
    model.set_auto_restore(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let old = model.take_requests(201).remove(0);
    dispatched(&mut model, &old);
    model.snapshot(snapshot("A", 210, vec![window]));
    assert!(model.take_requests(211).is_empty());
    completed(&mut model, &key, old.sequence, 230, true, false);
    assert_eq!(
        model.take_requests(231)[0].action,
        Action::RestoreAutomatic(layout(250))
    );
}

#[test]
fn auto_restore_setting_pause_and_recovery_do_not_reveal_windows() {
    let (mut model, mut window, key) = baseline();
    model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    model.set_auto_restore(true);
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    model.pause(true);
    assert!(model.take_requests(201).is_empty());
    model.pause(false);
    assert!(model.take_requests(202).is_empty());
    model.unavailable();
    model.snapshot(snapshot("B", 220, vec![window.clone()]));
    assert!(model.take_requests(221).is_empty());
    let mut restarted = Model::new(model.saved().clone());
    restarted.snapshot(snapshot("B", 300, vec![window]));
    assert!(restarted.saved().auto_restore);
    assert!(restarted.take_requests(301).is_empty());
}

#[test]
fn disabling_auto_restore_cancels_pending_auto_intent_without_moving_windows() {
    let (mut model, window, key) = baseline();
    model
        .record_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    model.set_auto_restore(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    model.set_auto_restore(false);
    assert_ne!(model.runtime(&key).unwrap().sequence(), request.sequence);
    completed(&mut model, &key, request.sequence, 220, false, true);
    assert!(model.take_requests(221).is_empty());
    model.snapshot(snapshot("A", 300, vec![window]));
    assert_eq!(
        model.take_requests(301)[0].action,
        Action::Minimize { automatic: true }
    );
}

#[test]
fn timed_out_manual_restore_never_establishes_a_layout_from_late_observation() {
    let (mut model, mut window, key) = baseline();
    window.minimized = true;
    model.snapshot(snapshot("A", 200, vec![window.clone()]));
    click(&mut model, &key, false);
    let request = model.take_requests(201).remove(0);
    completed(&mut model, &key, request.sequence, 2200, false, false);
    window.minimized = false;
    model.snapshot(snapshot("A", 2300, vec![window.clone()]));
    completed(&mut model, &key, request.sequence, 2400, true, false);
    model.snapshot(snapshot("A", 2500, vec![window]));
    assert!(model.saved().windows[&key].layouts.is_empty());
}

#[test]
fn stale_move_start_cannot_block_a_recycled_hwnd() {
    let (mut model, old, _) = baseline();
    let mut new = window("replacement");
    new.identity.process_created = 999;
    let key = new.identity.key();
    model.snapshot(snapshot("A", 200, vec![new]));
    model.moving(old.hwnd, true, 150, None);
    assert!(!model.runtime(&key).unwrap().dragging());
}

#[test]
fn legacy_layouts_decode_without_native_evidence_and_invalid_visible_bounds_are_rejected() {
    let (model, _, _) = baseline();
    let mut json = serde_json::to_value(model.saved()).unwrap();
    let records = json["windows"].as_object_mut().unwrap();
    let record = records.values_mut().next().unwrap();
    record["last_usable"]
        .as_object_mut()
        .unwrap()
        .remove("visible");
    record["last_usable"]
        .as_object_mut()
        .unwrap()
        .remove("arranged");
    let decoded = Saved::decode(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(
        decoded.windows.values().next().unwrap().last_usable.visible,
        None
    );
    let record = json["windows"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    record["last_usable"]["visible"] = serde_json::json!([0, 0, -1, 500]);
    assert!(Saved::decode(&serde_json::to_vec(&json).unwrap()).is_err());
}

#[test]
fn snap_bounds_and_divider_changes_are_learned_only_after_stable_observations() {
    let (mut model, mut window, key) = baseline();
    window.layout.arranged = Some(true);
    window.layout.visible = Some([17, 0, 970, 1048]);
    model.snapshot(snapshot("A", 200, vec![window.clone()]));
    assert!(model.saved().windows[&key].layouts.is_empty());
    model.snapshot(snapshot("A", 301, vec![window.clone()]));
    assert_eq!(model.saved().windows[&key].layouts["A"], window.layout);
    window.layout.visible = Some([17, 0, 1100, 1048]);
    model.snapshot(snapshot("A", 400, vec![window.clone()]));
    assert_ne!(model.saved().windows[&key].layouts["A"], window.layout);
    model.snapshot(snapshot("A", 501, vec![window.clone()]));
    assert_eq!(model.saved().windows[&key].layouts["A"], window.layout);
}

#[test]
fn explicit_restore_commits_adjusted_or_raise_target_without_rewriting_request() {
    for (effect, target) in [(Effect::Restore, layout(340)), (Effect::Raise, layout(510))] {
        let (mut model, mut window, key) = saved_destinations();
        window.minimized = true;
        model.snapshot(snapshot("A", 120, vec![window]));
        click(&mut model, &key, false);
        let request = model.take_requests(121).remove(0);
        model.accepted(
            &key,
            request.sequence,
            Acceptance {
                effect,
                target: Some(target.clone()),
            },
        );
        // The original user intent stays intact; acceptance records the execution plan.
        assert_eq!(
            model
                .runtime(&key)
                .unwrap()
                .pending()
                .unwrap()
                .request()
                .action,
            Action::Restore(layout(100))
        );
        let settled = model
            .settle(&key, request.sequence, 140, Outcome::Verified)
            .unwrap();
        assert_eq!(settled.effect, Some(effect));
        assert!(settled.committed);
        assert_eq!(model.saved().windows[&key].layouts["A"], target);
        assert_eq!(model.saved().windows[&key].last_usable, target);
        assert!(!model.saved().windows[&key].minimized_desktops.contains("A"));
    }
}

#[test]
fn captured_geometry_is_matched_by_identity_key_and_generation() {
    let (mut model, window, key) = baseline();
    let capture = model.capture_request(window.hwnd).unwrap();
    assert_eq!(capture.key, key);
    model.captured(&key, capture.generation + 1, 110, layout(300));
    assert!(model.saved().windows[&key].layouts.is_empty());
    model.captured(&key, capture.generation, 110, layout(300));
    assert_eq!(model.saved().windows[&key].layouts["A"], layout(300));
}

// Seeded event-sequence generator and invariant checker (Stage 5).

#[derive(Clone, Debug)]
enum Step {
    Snapshot {
        desktop: &'static str,
        coalesced: bool,
        w1_min: bool,
        w1_left: i32,
        w2_present: bool,
        w2_min: bool,
        w2_generation: u64,
        dt: u64,
    },
    Click {
        which: usize,
        minimized: bool,
        was_foreground: bool,
    },
    NativeMin {
        which: usize,
        dt: u64,
    },
    Drag {
        which: usize,
        started: bool,
        capture_left: Option<i32>,
        dt: u64,
    },
    Pause(bool),
    AutoRestore(bool),
    Unavailable,
    Issue {
        dt: u64,
    },
    Accept {
        slot: usize,
        as_raise: bool,
        adjusted_left: i32,
    },
    Settle {
        slot: usize,
        outcome: Outcome,
        dt: u64,
    },
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0 >> 16
    }
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next() as usize) % items.len()]
    }
    fn boolean(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

fn generate_sequence(seed: u64, len: usize) -> Vec<Step> {
    let mut rng = Rng(seed ^ 0x9e3779b97f4a7c15);
    let desktops = ["A", "B", "C"];
    let lefts = [100, 250, 500, -32000];
    let outcomes = [
        Outcome::Verified,
        Outcome::Rejected,
        Outcome::IdentityLost,
        Outcome::TimedOut,
        Outcome::Busy,
    ];
    let mut w2_gen = 100;
    (0..len)
        .map(|_| match rng.next() % 10 {
            0..=2 => {
                if rng.next().is_multiple_of(8) {
                    w2_gen += 1;
                }
                Step::Snapshot {
                    desktop: rng.pick(&desktops),
                    coalesced: rng.next().is_multiple_of(5),
                    w1_min: rng.boolean(),
                    w1_left: rng.pick(&lefts),
                    w2_present: !rng.next().is_multiple_of(4),
                    w2_min: rng.boolean(),
                    w2_generation: w2_gen,
                    dt: (rng.next() % 130) + 1,
                }
            }
            3 => Step::Click {
                which: (rng.next() % 2) as usize,
                minimized: rng.boolean(),
                was_foreground: rng.boolean(),
            },
            4 => Step::NativeMin {
                which: (rng.next() % 2) as usize,
                dt: (rng.next() % 20) + 1,
            },
            5 => Step::Drag {
                which: (rng.next() % 2) as usize,
                started: rng.boolean(),
                capture_left: if rng.boolean() {
                    Some(rng.pick(&lefts))
                } else {
                    None
                },
                dt: (rng.next() % 30) + 1,
            },
            6 => Step::Pause(rng.boolean()),
            7 => Step::AutoRestore(rng.boolean()),
            8 => {
                if rng.next().is_multiple_of(3) {
                    Step::Unavailable
                } else {
                    Step::Issue {
                        dt: rng.next() % 10,
                    }
                }
            }
            _ => {
                if rng.boolean() {
                    Step::Accept {
                        slot: (rng.next() % 3) as usize,
                        as_raise: rng.boolean(),
                        adjusted_left: rng.pick(&[100, 300, 600]),
                    }
                } else {
                    Step::Settle {
                        slot: (rng.next() % 3) as usize,
                        outcome: rng.pick(&outcomes),
                        dt: (rng.next() % 40) + 1,
                    }
                }
            }
        })
        .collect()
}

fn check_sequence(steps: &[Step]) -> Result<(), String> {
    let mut model = Model::new(Saved::default());
    let mut tick = 100u64;
    let mut serial = 0u64;
    let mut issued: Vec<Request> = Vec::new();
    let mut known_keys: BTreeSet<String> = BTreeSet::new();

    for (index, step) in steps.iter().enumerate() {
        let before_saved = model.saved().clone();
        match *step {
            Step::Snapshot {
                desktop,
                coalesced,
                w1_min,
                w1_left,
                w2_present,
                w2_min,
                w2_generation,
                dt,
            } => {
                tick += dt;
                if coalesced {
                    serial += 1;
                }
                let mut w1 = window("w1");
                w1.hwnd = 101;
                w1.minimized = w1_min;
                w1.layout = layout(w1_left);
                known_keys.insert(w1.identity.key());
                let mut windows = vec![w1];
                if w2_present {
                    let mut w2 = window("w2");
                    w2.hwnd = 102;
                    w2.identity.process_created = w2_generation;
                    w2.minimized = w2_min;
                    known_keys.insert(w2.identity.key());
                    windows.push(w2);
                }
                model.snapshot(Snapshot {
                    desktops: vec![
                        Desktop {
                            id: "A".into(),
                            name: "A".into(),
                        },
                        Desktop {
                            id: "B".into(),
                            name: "B".into(),
                        },
                        Desktop {
                            id: "C".into(),
                            name: "C".into(),
                        },
                    ],
                    current: desktop.into(),
                    windows,
                    tick,
                    transition_serial: serial,
                });
            }
            Step::Click {
                which,
                minimized,
                was_foreground,
            } => {
                let keys: Vec<_> = model.runtimes().map(|(k, _)| k.clone()).collect();
                if let Some(key) = keys.get(which) {
                    model.click(
                        key,
                        Click {
                            minimized,
                            was_foreground,
                        },
                    );
                }
            }
            Step::NativeMin { which, dt } => {
                let hwnd = if which == 0 { 101 } else { 102 };
                if let Some(mut probe) = model.minimize_probe(hwnd, tick) {
                    tick += dt;
                    probe.tick = tick;
                    probe.window.minimized = true;
                    let _ = model.observe_minimized(probe);
                }
            }
            Step::Drag {
                which,
                started,
                capture_left,
                dt,
            } => {
                tick += dt;
                let hwnd = if which == 0 { 101 } else { 102 };
                model.moving(hwnd, started, tick, capture_left.map(layout));
            }
            Step::Pause(paused) => model.pause(paused),
            Step::AutoRestore(enabled) => model.set_auto_restore(enabled),
            Step::Unavailable => model.unavailable(),
            Step::Issue { dt } => {
                tick += dt;
                for request in model.take_requests(tick) {
                    issued.push(request);
                }
            }
            Step::Accept {
                slot,
                as_raise,
                adjusted_left,
            } => {
                if !issued.is_empty() {
                    let req = &issued[slot % issued.len()];
                    let acceptance = if as_raise {
                        Acceptance {
                            effect: Effect::Raise,
                            target: Some(layout(adjusted_left)),
                        }
                    } else {
                        Acceptance {
                            effect: req.action.effect(),
                            target: Some(layout(adjusted_left)),
                        }
                    };
                    model.accepted(&req.key, req.sequence, acceptance);
                }
            }
            Step::Settle { slot, outcome, dt } => {
                tick += dt;
                if !issued.is_empty() {
                    let req = issued[slot % issued.len()].clone();
                    let settled = model.settle(&req.key, req.sequence, tick, outcome);
                    // Invariant: automatic, late, stale-generation or non-verified
                    // settlements never commit memory.
                    if let Some(settled) = settled {
                        if req.action.automatic() || settled.late || outcome != Outcome::Verified {
                            if settled.committed {
                                return Err(format!("step {index}: unauthorized commit"));
                            }
                            for (k, before) in &before_saved.windows {
                                let after = &model.saved().windows[k];
                                if before.layouts != after.layouts
                                    || before.minimized_desktops != after.minimized_desktops
                                    || before.last_usable != after.last_usable
                                {
                                    return Err(format!(
                                        "step {index}: memory mutated by {outcome:?}"
                                    ));
                                }
                            }
                        }
                        // Non-releasing outcomes must retain quarantine.
                        if !outcome.releases() && !model.runtime(&req.key).unwrap().blocked() {
                            return Err(format!("step {index}: slot released on {outcome:?}"));
                        }
                    }
                }
            }
        }

        // Global invariants after every step.
        if model.current().is_none() && !model.take_requests(tick).is_empty() {
            return Err(format!("step {index}: requests issued while unavailable"));
        }
        for (key, runtime) in model.runtimes() {
            if !known_keys.contains(key) {
                return Err(format!("step {index}: unknown identity {key}"));
            }
            if !runtime.present() && runtime.pending().is_none() {
                return Err(format!(
                    "step {index}: absent runtime {key} retained without pending"
                ));
            }
            if runtime.blocked()
                != runtime
                    .pending()
                    .is_some_and(|p| p.state() == OperationState::Quarantined)
            {
                return Err(format!("step {index}: blocked mismatch on {key}"));
            }
            if model.saved().paused && runtime.desired().is_some_and(Action::automatic) {
                return Err(format!(
                    "step {index}: automatic desired retained while paused"
                ));
            }
        }
        for (key, record) in &model.saved().windows {
            if *key != record.identity.key() {
                return Err(format!("step {index}: record key mismatch"));
            }
            for remembered in record.layouts.values() {
                if remembered.normal[0] <= -30000 {
                    return Err(format!("step {index}: parking bounds learned for {key}"));
                }
            }
        }
    }
    Ok(())
}

fn shrink(mut steps: Vec<Step>) -> Vec<Step> {
    let mut i = 0;
    while i < steps.len() {
        let mut candidate = steps.clone();
        candidate.remove(i);
        if check_sequence(&candidate).is_err() {
            steps = candidate;
        } else {
            i += 1;
        }
    }
    steps
}

#[test]
fn drag_end_capture_rejects_unreachable_parking_bounds_and_releases_absent_identity_on_settle() {
    let (mut model, window, key) = baseline();
    let mut parking = layout(-32000);
    parking.normal = [-32000, -32000, -31400, -31580];
    model.moving(window.hwnd, false, 110, Some(parking));
    assert!(model.saved().windows[&key].layouts.is_empty());

    click(&mut model, &key, true);
    let request = model.take_requests(120).remove(0);
    model.snapshot(snapshot("A", 130, vec![]));
    assert!(model.runtime(&key).is_some());
    model.settle(&key, request.sequence, 140, Outcome::IdentityLost);
    assert!(model.runtime(&key).is_none());
}

#[test]
fn generated_event_sequences_preserve_controller_invariants() {
    for seed in 0..200 {
        let steps = generate_sequence(seed, 60);
        if let Err(failure) = check_sequence(&steps) {
            let minimal = shrink(steps);
            panic!("seed {seed} failed ({failure}); minimal trace: {minimal:#?}");
        }
    }
}
