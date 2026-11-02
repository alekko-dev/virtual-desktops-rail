use rail_core::*;
use std::collections::BTreeMap;

fn layout(left: i32) -> Layout {
    Layout {
        normal: [left, 80, left + 600, 500],
        maximized: false,
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
fn external_restore_and_utility_restart_preserve_saved_layouts() {
    let (mut model, mut window, key) = baseline();
    model.moving(window.hwnd, false, 110, Some(layout(250)));
    let saved = model.saved.clone();
    window.layout = layout(900);
    let mut restarted = Model::new(saved);
    restarted.snapshot(snapshot("A", 200, vec![window.clone()]));
    window.minimized = true;
    restarted.snapshot(snapshot("A", 210, vec![window.clone()]));
    window.minimized = false;
    restarted.snapshot(snapshot("A", 220, vec![window]));
    assert_eq!(restarted.saved.windows[&key].layouts["A"], layout(250));
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
fn verified_maximized_restore_updates_fallback_without_creating_a_desktop_layout() {
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
    assert!(model.saved.windows[&key].layouts.is_empty());
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
    let saved = Saved::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(!saved.experimental_integration);
    let mut enabled = saved;
    enabled.experimental_integration = true;
    assert!(
        Saved::decode(&serde_json::to_vec(&enabled).unwrap())
            .unwrap()
            .experimental_integration
    );
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
