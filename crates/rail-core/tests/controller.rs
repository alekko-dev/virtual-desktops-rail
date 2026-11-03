use rail_core::*;
use std::collections::BTreeMap;

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
    let record = model.saved.windows.get_mut(&key).unwrap();
    record.layouts.insert("A".into(), layout(100));
    record.layouts.insert("B".into(), layout(700));
    model.set_auto_restore(true);
    (model, window, key)
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
        sequence: model.windows[&window.identity.key()].sequence,
        window,
        desktop: model.current.clone().unwrap(),
        generation: model.generation,
        event_tick,
        tick,
        transition_serial: model.transition_serial,
    }
}

#[test]
fn explicit_minimize_is_remembered_per_desktop_and_restore_clears_only_that_desktop() {
    let (mut model, mut window, key) = saved_destinations();
    model.click(&key, true);
    let minimize = model.take_requests(110).remove(0);
    assert!(model.saved.windows[&key].minimized_desktops.is_empty());
    model.completed(&key, minimize.sequence, 120, true, false);
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved.windows[&key].layouts["A"], layout(100));
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let restore_b = model.take_requests(201).remove(0);
    assert_eq!(restore_b.action, Action::RestoreAutomatic(layout(700)));
    model.completed(&key, restore_b.sequence, 220, true, false);
    window.minimized = false;
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    let minimize_a = model.take_requests(301).remove(0);
    assert_eq!(minimize_a.action, Action::Minimize { automatic: true });
    model.completed(&key, minimize_a.sequence, 320, true, false);
    model
        .saved
        .windows
        .get_mut(&key)
        .unwrap()
        .minimized_desktops
        .insert("B".into());
    model.click(&key, false);
    let restore_a = model.take_requests(330).remove(0);
    assert_eq!(restore_a.action, Action::Restore(layout(100)));
    model.completed(&key, restore_a.sequence, 350, true, false);
    assert_eq!(
        model.saved.windows[&key].minimized_desktops,
        ["B".into()].into()
    );
}

#[test]
fn external_minimize_without_geometry_survives_round_trip_and_does_not_learn_parking_bounds() {
    let (mut model, mut window, key) = baseline();
    model.set_auto_restore(true);
    let fallback = model.saved.windows[&key].last_usable.clone();
    window.minimized = true;
    window.layout.normal = [-32000, -32000, -31400, -31580];
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    assert!(model.saved.windows[&key].layouts.is_empty());
    assert_eq!(model.saved.windows[&key].last_usable, fallback);
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
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    window.layout = layout(450);
    model.snapshot(snapshot("A", 270, vec![window.clone()]));
    model.snapshot(snapshot("A", 369, vec![window.clone()]));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    model.snapshot(snapshot("A", 370, vec![window]));
    assert!(!model.saved.windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved.windows[&key].layouts["A"], layout(450));
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
        model.dirty = false;
        for _ in 0..3 {
            model.snapshot(snapshot("A", 130, vec![duplicate.clone()]));
        }
        assert!(
            model.windows[&key].external_restore.is_some(),
            "{duplicate_kind}"
        );
        assert_eq!(
            model.windows[&key].external_restore.as_ref().unwrap().since,
            Some(130)
        );
        assert!(!model.windows[&key].window.minimized);
        assert_eq!(model.windows[&key].window.layout, layout(100));
        assert!(!model.dirty);
        model.snapshot(snapshot("A", 229, vec![window.clone()]));
        assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
        model.snapshot(snapshot("A", 230, vec![window.clone()]));
        assert!(!model.saved.windows[&key].minimized_desktops.contains("A"));
        assert_eq!(model.saved.windows[&key].layouts["A"], layout(100));
        model.snapshot(snapshot("B", 300, vec![window.clone()]));
        let restore_b = model.take_requests(301).remove(0);
        model.completed(&key, restore_b.sequence, 320, true, false);
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
        assert!(model.windows[&key].external_restore.is_some());
        match invalidation {
            "pending" => {
                model.click(&key, false);
                let raise = model.take_requests(130).remove(0);
                model.snapshot(snapshot("A", 130, vec![window.clone()]));
                assert!(model.windows[&key].external_restore.is_none());
                model.completed(&key, raise.sequence, 140, false, true);
            }
            "transition" => {
                model.pause(true);
                let mut frame = snapshot("A", 130, vec![window.clone()]);
                frame.transition_serial = 1;
                model.snapshot(frame);
                assert_eq!(model.transition_serial, 1);
                assert!(model.windows[&key].external_restore.is_none());
            }
            _ => {
                model.unavailable();
                model.snapshot(snapshot("A", 130, vec![window.clone()]));
                assert!(model.windows[&key].external_restore.is_none());
            }
        }
        let serial = model.transition_serial;
        for tick in [150, 250] {
            let mut frame = snapshot("A", tick, vec![window.clone()]);
            frame.transition_serial = serial;
            model.snapshot(frame);
        }
        assert!(
            model.saved.windows[&key].minimized_desktops.contains("A"),
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
                .saved
                .windows
                .get_mut(&key)
                .unwrap()
                .minimized_desktops
                .insert("B".into());
            model.snapshot(snapshot("B", 200, vec![window.clone()]));
            "B"
        } else {
            model.click(&key, true);
            "A"
        };
        let minimize = model.take_requests(210).remove(0);
        assert_eq!(minimize.action, Action::Minimize { automatic });
        model.completed(&key, minimize.sequence, 220, true, false);
        assert!(model.windows[&key].suppress_next_state);
        window.minimized = false;
        model.snapshot(snapshot(desktop, 219, vec![window.clone()]));
        assert!(model.windows[&key].suppress_next_state);
        model.snapshot(snapshot(desktop, 230, vec![window.clone()]));
        model.snapshot(snapshot(desktop, 230, vec![window.clone()]));
        assert!(
            model.saved.windows[&key]
                .minimized_desktops
                .contains(desktop)
        );
        model.snapshot(snapshot(desktop, 329, vec![window.clone()]));
        assert!(
            model.saved.windows[&key]
                .minimized_desktops
                .contains(desktop)
        );
        model.snapshot(snapshot(desktop, 330, vec![window.clone()]));
        assert!(
            !model.saved.windows[&key]
                .minimized_desktops
                .contains(desktop),
            "automatic: {automatic}"
        );
        let other = if desktop == "A" { "B" } else { "A" };
        model.snapshot(snapshot(other, 400, vec![window.clone()]));
        let transition = model.take_requests(401).remove(0);
        model.completed(&key, transition.sequence, 420, true, false);
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
            .saved
            .windows
            .get_mut(&key)
            .unwrap()
            .minimized_desktops
            .insert("A".into());
        model.click(&key, true);
        let minimize = model.take_requests(110).remove(0);
        match outcome {
            "late" => model.completed(&key, minimize.sequence, 120, false, false),
            "superseded" => model.click(&key, false),
            _ => {}
        }
        model.completed(
            &key,
            minimize.sequence,
            130,
            outcome != "cancelled",
            outcome == "cancelled",
        );
        model.snapshot(snapshot("A", 140, vec![window.clone()]));
        model.snapshot(snapshot("A", 240, vec![window]));
        assert!(
            model.saved.windows[&key].minimized_desktops.contains("A"),
            "{outcome}"
        );
    }
}

#[test]
fn post_minimize_restore_evidence_cannot_cross_recovery_switch_or_pending_work() {
    for invalidation in ["recovery", "switch", "pending"] {
        let (mut model, window, key) = saved_destinations();
        model.click(&key, true);
        let minimize = model.take_requests(110).remove(0);
        model.completed(&key, minimize.sequence, 120, true, false);
        match invalidation {
            "recovery" => model.unavailable(),
            "switch" => {
                model.pause(true);
                model.snapshot(snapshot("B", 125, vec![window.clone()]));
            }
            _ => {
                model.click(&key, false);
                let restore = model.take_requests(125).remove(0);
                model.snapshot(snapshot("A", 130, vec![window.clone()]));
                model.completed(&key, restore.sequence, 135, false, true);
            }
        }
        model.snapshot(snapshot("A", 140, vec![window.clone()]));
        model.snapshot(snapshot("A", 240, vec![window]));
        assert!(
            model.saved.windows[&key].minimized_desktops.contains("A"),
            "{invalidation}"
        );
    }
}

#[test]
fn first_post_minimize_restore_sample_during_drag_waits_for_post_drag_stability() {
    let (mut model, mut window, key) = saved_destinations();
    model.click(&key, true);
    let minimize = model.take_requests(110).remove(0);
    model.completed(&key, minimize.sequence, 120, true, false);
    model.moving(window.hwnd, true, 125, None);
    window.minimized = false;
    window.layout = layout(450);
    model.snapshot(snapshot("A", 130, vec![window.clone()]));
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    model.moving(window.hwnd, false, 310, None);
    model.snapshot(snapshot("A", 320, vec![window.clone()]));
    model.snapshot(snapshot("A", 419, vec![window.clone()]));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    model.snapshot(snapshot("A", 420, vec![window]));
    assert!(!model.saved.windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved.windows[&key].layouts["A"], layout(450));
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
        assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
        model.moving(window.hwnd, false, 310, None);
        model.moving(window.hwnd, false, 310, Some(window.layout.clone()));
        model.snapshot(snapshot("A", 320, vec![window.clone()]));
        model.snapshot(snapshot("A", 320, vec![window.clone()]));
        model.snapshot(snapshot("A", 419, vec![window.clone()]));
        assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
        model.snapshot(snapshot("A", 420, vec![window.clone()]));
        assert!(
            !model.saved.windows[&key].minimized_desktops.contains("A"),
            "restore observed before drag: {observed_before_drag}"
        );
        assert_eq!(model.saved.windows[&key].layouts["A"], layout(400));
        assert_eq!(model.saved.windows[&key].last_usable, layout(400));
        model.snapshot(snapshot("B", 500, vec![window.clone()]));
        let restore_b = model.take_requests(501).remove(0);
        model.completed(&key, restore_b.sequence, 520, true, false);
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
                model.click(&key, false);
                let raise = model.take_requests(180).remove(0);
                model.snapshot(snapshot("A", 190, vec![window.clone()]));
                model.completed(&key, raise.sequence, 200, false, true);
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
            model.saved.windows[&key].minimized_desktops.contains("A"),
            "{invalidation}"
        );
        assert!(
            model.windows[&key].external_restore.is_none(),
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
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    window.layout = layout(500);
    model.snapshot(snapshot("A", 320, vec![window.clone()]));
    // The asynchronous drag-end capture can arrive after this newer frame.
    model.moving(window.hwnd, false, 170, Some(window.layout.clone()));
    model.snapshot(snapshot("A", 419, vec![window.clone()]));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    model.snapshot(snapshot("A", 420, vec![window]));
    assert!(!model.saved.windows[&key].minimized_desktops.contains("A"));
    assert_eq!(model.saved.windows[&key].layouts["A"], layout(500));
}

#[test]
fn automatic_actions_and_their_native_events_do_not_change_user_intent() {
    let (mut model, mut window, key) = saved_destinations();
    model.set_auto_restore(false);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    let event = minimize_observation(&model, &window, 210, 211);
    assert!(!model.observe_minimized(event.clone()));
    window.minimized = true;
    model.snapshot(snapshot("B", 215, vec![window.clone()]));
    model.completed(&key, request.sequence, 220, true, false);
    assert!(!model.observe_minimized(event));
    model.snapshot(snapshot("B", 230, vec![window.clone()]));
    model.snapshot(snapshot("B", 240, vec![window]));
    assert!(model.saved.windows[&key].minimized_desktops.is_empty());
}

#[test]
fn stale_cancelled_failed_and_late_explicit_completions_do_not_commit_intent() {
    for outcome in ["cancel", "timeout", "supersede", "switch"] {
        let (mut model, window, key) = saved_destinations();
        model.click(&key, true);
        let request = model.take_requests(110).remove(0);
        match outcome {
            "cancel" => model.completed(&key, request.sequence, 120, false, true),
            "timeout" => {
                model.completed(&key, request.sequence, 120, false, false);
                model.completed(&key, request.sequence, 130, true, false);
            }
            "supersede" => {
                model.click(&key, false);
                model.completed(&key, request.sequence, 130, true, false);
            }
            _ => {
                model.snapshot(snapshot("B", 120, vec![window]));
                model.completed(&key, request.sequence, 130, true, false);
            }
        }
        assert!(
            model.saved.windows[&key].minimized_desktops.is_empty(),
            "{outcome}"
        );
    }
}

#[test]
fn native_minimize_before_immediate_switch_preserves_outgoing_intent() {
    let (mut model, window, key) = saved_destinations();
    assert!(model.observe_minimized(minimize_observation(&model, &window, 110, 115)));
    // An already-queued older frame must not undo the newer native state.
    model.snapshot(snapshot("A", 112, vec![window.clone()]));
    assert!(model.windows[&key].window.minimized);
    model.snapshot(snapshot("B", 113, vec![window.clone()]));
    assert_eq!(model.current.as_deref(), Some("A"));
    model.snapshot(snapshot("A", 115, vec![window.clone()]));
    assert!(model.windows[&key].window.minimized);
    model.snapshot(snapshot("B", 120, vec![window.clone()]));
    let request = model.take_requests(121).remove(0);
    assert_eq!(request.action, Action::RestoreAutomatic(layout(700)));
    model.completed(&key, request.sequence, 140, true, false);
    model.snapshot(snapshot("A", 150, vec![window]));
    assert_eq!(
        model.take_requests(151)[0].action,
        Action::Minimize { automatic: true }
    );
    assert_eq!(
        model.saved.windows[&key].minimized_desktops,
        ["A".into()].into()
    );
}

#[test]
fn native_minimize_rejects_duplicates_wrong_context_and_recycled_identity() {
    for invalid in [
        "duplicate",
        "generation",
        "desktop",
        "serial",
        "sequence",
        "identity",
        "hwnd",
        "drag",
        "unavailable",
    ] {
        let (mut model, window, key) = baseline();
        let mut event = minimize_observation(&model, &window, 110, 120);
        match invalid {
            "duplicate" => {
                assert!(model.observe_minimized(event.clone()));
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
        model.dirty = false;
        assert!(!model.observe_minimized(event), "{invalid}");
        assert!(!model.dirty, "{invalid}");
        if invalid != "duplicate" {
            assert!(model.saved.windows[&key].minimized_desktops.is_empty());
        }
    }
}

#[test]
fn late_restore_reconciles_to_minimized_saved_destination_without_clearing_intent() {
    let (mut model, mut window, key) = saved_destinations();
    model
        .saved
        .windows
        .get_mut(&key)
        .unwrap()
        .minimized_desktops
        .insert("A".into());
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let restore = model.take_requests(201).remove(0);
    model.dispatched(&key, restore.sequence);
    model.snapshot(snapshot("A", 210, vec![window]));
    assert!(model.take_requests(211).is_empty());
    model.completed(&key, restore.sequence, 220, false, false);
    model.completed(&key, restore.sequence, 300, true, false);
    assert_eq!(
        model.take_requests(301)[0].action,
        Action::Minimize { automatic: true }
    );
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
}

#[test]
fn minimized_intent_persists_but_baselines_do_not_infer_it_and_deleted_desktops_are_pruned() {
    let (mut model, mut window, key) = saved_destinations();
    model.pause(true);
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    let saved = Saved::decode(&serde_json::to_vec(&model.saved).unwrap()).unwrap();
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
        restarted.saved.windows[&key]
            .minimized_desktops
            .contains("A")
    );
    let mut frame = snapshot("B", 300, vec![window]);
    frame.desktops.retain(|d| d.id == "B");
    restarted.snapshot(frame);
    assert!(restarted.saved.windows[&key].minimized_desktops.is_empty());
    assert!(!restarted.saved.windows[&key].layouts.contains_key("A"));
}

#[test]
fn minimized_intent_legacy_default_and_union_limit_are_validated() {
    let (model, _, key) = baseline();
    let mut json = serde_json::to_value(&model.saved).unwrap();
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
        model.click(&key, false);
        let restore = model.take_requests(121).remove(0);
        model.completed(&key, restore.sequence, 140, false, cancelled);
        if !cancelled {
            model.completed(&key, restore.sequence, 300, true, false);
            window.minimized = false;
            model.snapshot(snapshot("A", 310, vec![window.clone()]));
            model.snapshot(snapshot("A", 420, vec![window]));
        }
        assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
    }
}

#[test]
fn native_manual_minimize_immediately_after_verified_restore_is_not_suppressed() {
    let (mut model, mut window, key) = saved_destinations();
    window.minimized = true;
    model.snapshot(snapshot("A", 120, vec![window.clone()]));
    model.click(&key, false);
    let restore = model.take_requests(121).remove(0);
    model.completed(&key, restore.sequence, 140, true, false);
    assert!(model.windows[&key].suppress_next_state);
    assert!(!model.observe_minimized(minimize_observation(&model, &window, 130, 141)));
    assert!(model.observe_minimized(minimize_observation(&model, &window, 150, 151)));
    assert!(model.saved.windows[&key].minimized_desktops.contains("A"));
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
    model.completed(&key, requests[0].sequence, 210, true, false);
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
    model.click(&key, true);
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
    model.dispatched(&key, old.sequence);
    window.minimized = true;
    model.snapshot(snapshot("B", 210, vec![window]));
    model.click(&key, false);
    assert!(model.take_requests(211).is_empty());
    model.completed(&key, old.sequence, 220, true, false);
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
    model.click(&key, false);
    let request = model.take_requests(121).remove(0);
    model.dispatched(&key, request.sequence);
    window.minimized = false;
    window.layout = layout(800);
    model.snapshot(snapshot("B", 130, vec![window]));
    model.completed(&key, request.sequence, 140, true, false);
    assert_eq!(model.saved.windows[&key].layouts["A"], layout(250));
    assert!(!model.saved.windows[&key].layouts.contains_key("B"));
    assert_eq!(
        model.take_requests(141)[0].action,
        Action::Minimize { automatic: true }
    );
}

#[test]
fn utility_restart_preserves_layout_but_external_restore_updates_it() {
    let (mut model, mut window, key) = baseline();
    model.moving(window.hwnd, false, 110, Some(layout(250)));
    let saved = model.saved.clone();
    window.layout = layout(900);
    let mut restarted = Model::new(saved);
    restarted.snapshot(snapshot("A", 200, vec![window.clone()]));
    assert_eq!(restarted.saved.windows[&key].layouts["A"], layout(250));
    window.minimized = true;
    restarted.snapshot(snapshot("A", 210, vec![window.clone()]));
    window.minimized = false;
    restarted.snapshot(snapshot("A", 220, vec![window.clone()]));
    restarted.snapshot(snapshot("A", 330, vec![window]));
    assert_eq!(restarted.saved.windows[&key].layouts["A"], layout(900));
}

#[test]
fn recycled_hwnd_and_ambiguous_restart_never_receive_old_layouts_or_results() {
    let (mut model, old, old_key) = baseline();
    model.moving(old.hwnd, false, 110, Some(layout(250)));
    model.click(&old_key, true);
    let request = model.take_requests(120).remove(0);
    let mut replacement = window("new-token");
    replacement.identity.process_created = 200;
    let new_key = replacement.identity.key();
    model.snapshot(snapshot("A", 130, vec![replacement]));
    model.completed(&old_key, request.sequence, 140, true, false);
    assert!(model.windows[&new_key].pending.is_none());
    assert!(model.saved.windows[&new_key].layouts.is_empty());
    assert!(model.saved.windows.contains_key(&old_key));
    assert!(model.saved.windows[&old_key].minimized_desktops.is_empty());
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
    let mut restarted = Model::new(model.saved);
    first.minimized = true;
    second.minimized = true;
    restarted.snapshot(snapshot("A", 200, vec![second, first]));
    restarted.click(&first_key, false);
    restarted.click(&second_key, false);
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
    model.click(&key, true);
    let request = model.take_requests(120).remove(0);
    model.completed(&key, request.sequence, 200, false, false);
    model.click(&key, false);
    assert!(model.take_requests(201).is_empty());
    model.completed(&key, request.sequence, 300, true, false);
    assert!(!model.windows[&key].blocked);
    assert!(model.saved.windows[&key].layouts.is_empty());
}

#[test]
fn drag_cancels_queued_auto_work_and_stale_drag_events_are_ignored() {
    let (mut model, window, key) = baseline();
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    model.moving(window.hwnd, true, 201, None);
    assert!(model.take_requests(202).is_empty());
    model.moving(window.hwnd, false, 150, Some(layout(900)));
    assert!(model.saved.windows[&key].layouts.is_empty());
    model.moving(window.hwnd, false, 210, Some(layout(300)));
    assert_eq!(model.saved.windows[&key].layouts["B"], layout(300));
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
    let (mut model, mut window, key) = baseline();
    window.minimized = true;
    model.snapshot(snapshot("A", 200, vec![window]));
    model.click(&key, true);
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
    model.saved.windows.get_mut(&key).unwrap().last_usable = saved.clone();
    window.minimized = true;
    model.snapshot(snapshot("A", 200, vec![window.clone()]));
    model.click(&key, false);
    let request = model.take_requests(201).remove(0);
    model.completed(&key, request.sequence, 220, true, false);
    window.minimized = false;
    window.layout = saved.clone();
    model.snapshot(snapshot("A", 230, vec![window]));
    assert_eq!(model.saved.windows[&key].layouts["A"], saved);
    assert_eq!(model.saved.windows[&key].last_usable, saved);
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
    let encoded = serde_json::to_vec(&model.saved).unwrap();
    assert!(!String::from_utf8(encoded.clone()).unwrap().contains("hwnd"));
    assert!(Saved::decode(&encoded).is_ok());
    model.saved.windows.get_mut(&key).unwrap().layouts = BTreeMap::from([(
        "A".into(),
        Layout {
            dpi: 0,
            ..layout(100)
        },
    )]);
    assert!(Saved::decode(&serde_json::to_vec(&model.saved).unwrap()).is_err());
}

#[test]
fn legacy_configuration_does_not_opt_into_unvalidated_revisions() {
    let (model, _, _) = baseline();
    let mut value = serde_json::to_value(&model.saved).unwrap();
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
        .saved
        .windows
        .get_mut(&first_key)
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
fn manual_rail_restore_on_new_desktop_enables_later_automatic_restore() {
    let (mut model, mut window, key) = baseline();
    model.set_auto_restore(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let minimize = model.take_requests(201).remove(0);
    model.completed(&key, minimize.sequence, 220, true, false);
    window.minimized = true;
    model.snapshot(snapshot("B", 230, vec![window.clone()]));
    model.click(&key, false);
    let restore = model.take_requests(240).remove(0);
    model.completed(&key, restore.sequence, 260, true, false);
    assert_eq!(model.saved.windows[&key].layouts["B"], window.layout);
    window.minimized = false;
    model.snapshot(snapshot("A", 300, vec![window.clone()]));
    let minimize = model.take_requests(301).remove(0);
    model.completed(&key, minimize.sequence, 320, true, false);
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
    assert_eq!(model.saved.windows[&key].layouts["B"], layout(600));
    model.snapshot(snapshot("A", 400, vec![window.clone()]));
    let minimize = model.take_requests(401).remove(0);
    model.completed(&key, minimize.sequence, 420, true, false);
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
    assert!(model.saved.windows[&key].layouts.is_empty());
    window.layout = layout(600);
    model.snapshot(snapshot("A", 500, vec![window.clone()]));
    assert!(model.saved.windows[&key].layouts.is_empty());
    model.snapshot(snapshot("A", 610, vec![window]));
    assert_eq!(model.saved.windows[&key].layouts["A"], layout(600));
}

#[test]
fn automatic_restore_never_rewrites_saved_layout_or_fallback() {
    let (mut model, mut window, key) = baseline();
    model
        .saved
        .windows
        .get_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    let last_usable = model.saved.windows[&key].last_usable.clone();
    model.set_auto_restore(true);
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    model
        .windows
        .get_mut(&key)
        .unwrap()
        .pending
        .as_mut()
        .unwrap()
        .request
        .action = Action::RestoreAutomatic(layout(500));
    model.completed(&key, request.sequence, 220, true, false);
    // A snapshot captured before verification cannot consume own-action suppression.
    model.snapshot(snapshot("B", 210, vec![window.clone()]));
    window.minimized = false;
    window.layout = layout(500);
    model.snapshot(snapshot("B", 230, vec![window.clone()]));
    model.snapshot(snapshot("B", 240, vec![window]));
    assert_eq!(model.saved.windows[&key].layouts["B"], layout(700));
    assert_eq!(model.saved.windows[&key].last_usable, last_usable);
}

#[test]
fn late_automatic_restore_is_minimized_on_a_new_destination() {
    let (mut model, mut window, key) = baseline();
    model
        .saved
        .windows
        .get_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    model.set_auto_restore(true);
    window.minimized = true;
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    model.dispatched(&key, request.sequence);
    model.snapshot(snapshot("A", 210, vec![window]));
    assert!(model.take_requests(211).is_empty());
    model.completed(&key, request.sequence, 230, true, false);
    assert_eq!(
        model.take_requests(231)[0].action,
        Action::Minimize { automatic: true }
    );
    assert!(!model.saved.windows[&key].layouts.contains_key("A"));
}

#[test]
fn pending_minimize_finishes_before_latest_desktop_restore() {
    let (mut model, window, key) = baseline();
    model
        .saved
        .windows
        .get_mut(&key)
        .unwrap()
        .layouts
        .insert("A".into(), layout(250));
    model.set_auto_restore(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let old = model.take_requests(201).remove(0);
    model.dispatched(&key, old.sequence);
    model.snapshot(snapshot("A", 210, vec![window]));
    assert!(model.take_requests(211).is_empty());
    model.completed(&key, old.sequence, 230, true, false);
    assert_eq!(
        model.take_requests(231)[0].action,
        Action::RestoreAutomatic(layout(250))
    );
}

#[test]
fn auto_restore_setting_pause_and_recovery_do_not_reveal_windows() {
    let (mut model, mut window, key) = baseline();
    model
        .saved
        .windows
        .get_mut(&key)
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
    let mut restarted = Model::new(model.saved.clone());
    restarted.snapshot(snapshot("B", 300, vec![window]));
    assert!(restarted.saved.auto_restore);
    assert!(restarted.take_requests(301).is_empty());
}

#[test]
fn disabling_auto_restore_cancels_pending_auto_intent_without_moving_windows() {
    let (mut model, window, key) = baseline();
    model
        .saved
        .windows
        .get_mut(&key)
        .unwrap()
        .layouts
        .insert("B".into(), layout(700));
    model.set_auto_restore(true);
    model.snapshot(snapshot("B", 200, vec![window.clone()]));
    let request = model.take_requests(201).remove(0);
    model.set_auto_restore(false);
    assert_ne!(model.windows[&key].sequence, request.sequence);
    model.completed(&key, request.sequence, 220, false, true);
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
    model.click(&key, false);
    let request = model.take_requests(201).remove(0);
    model.completed(&key, request.sequence, 2200, false, false);
    window.minimized = false;
    model.snapshot(snapshot("A", 2300, vec![window.clone()]));
    model.completed(&key, request.sequence, 2400, true, false);
    model.snapshot(snapshot("A", 2500, vec![window]));
    assert!(model.saved.windows[&key].layouts.is_empty());
}

#[test]
fn stale_move_start_cannot_block_a_recycled_hwnd() {
    let (mut model, old, _) = baseline();
    let mut new = window("replacement");
    new.identity.process_created = 999;
    let key = new.identity.key();
    model.snapshot(snapshot("A", 200, vec![new]));
    model.moving(old.hwnd, true, 150, None);
    assert!(!model.windows[&key].dragging);
}

#[test]
fn legacy_layouts_decode_without_native_evidence_and_invalid_visible_bounds_are_rejected() {
    let (model, _, _) = baseline();
    let mut json = serde_json::to_value(&model.saved).unwrap();
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
    assert!(model.saved.windows[&key].layouts.is_empty());
    model.snapshot(snapshot("A", 301, vec![window.clone()]));
    assert_eq!(model.saved.windows[&key].layouts["A"], window.layout);
    window.layout.visible = Some([17, 0, 1100, 1048]);
    model.snapshot(snapshot("A", 400, vec![window.clone()]));
    assert_ne!(model.saved.windows[&key].layouts["A"], window.layout);
    model.snapshot(snapshot("A", 501, vec![window.clone()]));
    assert_eq!(model.saved.windows[&key].layouts["A"], window.layout);
}

#[test]
fn centered_rail_preference_defaults_false_persists_and_does_not_issue_window_actions() {
    let (mut model, window, _) = baseline();
    assert!(!model.saved.centered_rail);
    model.dirty = false;
    let initial_revision = model.saved.preferences_revision;
    model.set_centered_rail(false);
    assert!(!model.dirty);
    assert_eq!(model.saved.preferences_revision, initial_revision);
    model.set_centered_rail(true);
    assert!(model.saved.centered_rail);
    assert!(model.dirty);
    assert_eq!(model.saved.preferences_revision, initial_revision + 1);
    assert!(model.take_requests(150).is_empty());

    let mut json = serde_json::to_value(&model.saved).unwrap();
    let decoded = Saved::decode(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(decoded.centered_rail);

    json.as_object_mut().unwrap().remove("centered_rail");
    let legacy = Saved::decode(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(!legacy.centered_rail);

    let mut restarted = Model::new(decoded);
    restarted.snapshot(snapshot("A", 200, vec![window]));
    assert!(restarted.saved.centered_rail);
    assert!(restarted.take_requests(201).is_empty());
}
