// SPDX-License-Identifier: GPL-3.0-or-later
//! Trusted focus -> worker -> service -> synthetic device. No compositor/HID I/O.
use super::*;
use std::collections::VecDeque;

struct Script {
    steps: VecDeque<DesktopSnapshot>,
    calls: Rc<RefCell<Vec<bool>>>,
}
impl DesktopSource for Script {
    fn describe(&mut self, include_apps: bool, deadline: Instant) -> DesktopSnapshot {
        assert!(Instant::now() < deadline);
        self.calls.borrow_mut().push(include_apps);
        self.steps.pop_front().expect("Unexpected desktop query")
    }
}
fn observed(class: Option<&str>) -> DesktopSnapshot {
    DesktopSnapshot::from_values(&json!({"class":class}), None).unwrap()
}
fn source(steps: Vec<DesktopSnapshot>) -> (Script, Rc<RefCell<Vec<bool>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    (
        Script {
            steps: steps.into(),
            calls: calls.clone(),
        },
        calls,
    )
}
fn fixture(armed: bool) -> (Sandbox, ProfileService<Backend>, Rc<RefCell<State>>) {
    let dir = Sandbox::new();
    let store = dir.store();
    let document=Document::from_value(json!({"version":6,"profiles":[
        {"device":ID,"name":"Work","settings":{"dpi":1800}},
        {"device":ID,"name":"Game","settings":{"dpi":2400}}
    ],"active":if armed {json!({ID:"Work"})}else{json!({})},"rules":[{"device":ID,"application":"GameApp","profile":"Game"}],"keep":"metadata"})).unwrap();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let state = Rc::new(RefCell::new(State {
        dpi: 1850,
        ..State::default()
    }));
    let service =
        ProfileService::new(store, Backend(state.clone()), Duration::from_secs(1)).unwrap();
    (dir, service, state)
}
fn input(requests: Vec<Value>) -> Vec<u8> {
    requests.into_iter().flat_map(frame).collect()
}

#[test]
fn observed_focus_selects_the_rule_and_absent_or_unmatched_focus_returns_to_base() {
    let (dir, service, state) = fixture(true);
    let before = fs::read(dir.0.join("linux-scrollmaster/profiles.json")).unwrap();
    let (source, calls) = source(vec![
        observed(Some("GameApp")),
        observed(Some("gameapp")),
        observed(None),
        observed(Some("Other")),
    ]);
    let requests = input(vec![json!({"op":"snapshot"}); 4]);
    let mut output = Vec::new();
    serve_with_desktop(service, source, requests.as_slice(), &mut output).unwrap();
    let replies = replies(&output);
    assert_eq!(
        replies
            .iter()
            .map(|r| r["data"]["reconciliation"][0]["profile"].clone())
            .collect::<Vec<_>>(),
        vec![json!("Game"), json!("Work"), json!("Work"), json!("Work")]
    );
    assert_eq!(
        replies[0]["data"]["desktop"]["active_application"],
        "GameApp"
    );
    assert_eq!(state.borrow().dpi, 1800);
    assert_eq!(*calls.borrow(), vec![false; 4]);
    assert_eq!(
        fs::read(dir.0.join("linux-scrollmaster/profiles.json")).unwrap(),
        before
    );
}
#[test]
fn desktop_failure_clears_the_old_application_and_uses_base_without_losing_rules() {
    let (_dir, service, _state) = fixture(true);
    let (source, _) = source(vec![
        observed(Some("GameApp")),
        DesktopSnapshot::unavailable("synthetic disconnect"),
        observed(Some("GameApp")),
    ]);
    let bytes = input(vec![json!({"op":"snapshot"}); 3]);
    let mut output = Vec::new();
    serve_with_desktop(service, source, bytes.as_slice(), &mut output).unwrap();
    let replies = replies(&output);
    assert_eq!(replies[1]["data"]["desktop"]["available"], false);
    assert_eq!(
        replies[1]["data"]["desktop"]["active_application"],
        Value::Null
    );
    assert_eq!(replies[1]["data"]["reconciliation"][0]["profile"], "Work");
    assert_eq!(replies[2]["data"]["reconciliation"][0]["profile"], "Game");
    assert_eq!(replies[1]["data"]["rules"][0]["application"], "GameApp");
    assert_eq!(replies[1]["data"]["active"][ID], "Work");
}
#[test]
fn focus_rules_never_apply_without_an_explicitly_armed_base() {
    let (_dir, service, state) = fixture(false);
    let (source, _) = source(vec![observed(Some("GameApp"))]);
    let mut output = Vec::new();
    serve_with_desktop(
        service,
        source,
        frame(json!({"op":"snapshot"})).as_slice(),
        &mut output,
    )
    .unwrap();
    assert_eq!(state.borrow().calls, vec!["snapshot"]);
    assert_eq!(replies(&output)[0]["data"]["reconciliation"], json!([]));
    assert_eq!(state.borrow().dpi, 1850);
}
#[test]
fn a_manual_edit_disarms_focus_reconciliation_before_next_snapshot() {
    let (dir, service, state) = fixture(true);
    let (source, calls) = source(vec![observed(Some("GameApp")), observed(Some("GameApp"))]);
    let bytes = input(vec![
        json!({"op":"snapshot"}),
        json!({"op":"set","device":ID,"changes":{"dpi":1200}}),
        json!({"op":"snapshot"}),
    ]);
    let mut output = Vec::new();
    serve_with_desktop(service, source, bytes.as_slice(), &mut output).unwrap();
    let replies = replies(&output);
    assert_eq!(
        replies[0]["data"]["devices"][0]["settings"]["dpi"]["value"],
        2400
    );
    assert_eq!(
        replies[2]["data"]["devices"][0]["settings"]["dpi"]["value"],
        1200
    );
    assert_eq!(replies[2]["data"]["reconciliation"], json!([]));
    assert_eq!(state.borrow().dpi, 1200);
    assert_eq!(*calls.borrow(), vec![false, false]);
    assert!(dir.store().load().unwrap().document().as_value()["active"]
        .get(ID)
        .is_none());
    assert_eq!(replies[2]["data"]["rules"][0]["profile"], "Game");
}
#[test]
fn applications_is_read_only_even_with_armed_profiles_and_only_exposes_class_ids() {
    let (dir, service, state) = fixture(true);
    let original = fs::read(dir.0.join("linux-scrollmaster/profiles.json")).unwrap();
    let desktop = DesktopSnapshot::from_values(
        &json!({"class":"GameApp","title":"private"}),
        Some(&json!([{"class":"GameApp","pid":123}])),
    )
    .unwrap();
    let (source, calls) = source(vec![desktop]);
    let mut output = Vec::new();
    serve_with_desktop(
        service,
        source,
        frame(json!({"op":"applications","device":ID})).as_slice(),
        &mut output,
    )
    .unwrap();
    let reply = &replies(&output)[0];
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["desktop"]["applications"], json!(["GameApp"]));
    assert_eq!(reply["data"]["reconciliation"], json!([]));
    assert_eq!(reply["data"]["active"][ID], "Work");
    assert!(state.borrow().calls.is_empty());
    assert_eq!(*calls.borrow(), vec![true]);
    assert_eq!(
        fs::read(dir.0.join("linux-scrollmaster/profiles.json")).unwrap(),
        original
    );
    assert!(!output.windows(7).any(|w| w == b"private"));
}
#[test]
fn missing_desktop_keeps_application_listing_and_profile_management_available_offline() {
    let (_dir, service, state) = fixture(true);
    let (source, calls) = source(vec![DesktopSnapshot::unavailable("No session")]);
    let mut output = Vec::new();
    let bytes = input(vec![
        json!({"op":"applications"}),
        json!({"op":"rename","device":ID,"name":"Work","new_name":"Office"}),
        json!({"op":"shutdown"}),
    ]);
    serve_with_desktop(service, source, bytes.as_slice(), &mut output).unwrap();
    let replies = replies(&output);
    assert!(replies.iter().all(|r| r["ok"] == true));
    assert_eq!(replies[0]["data"]["desktop"]["available"], false);
    assert_eq!(replies[1]["data"]["active"][ID], "Office");
    assert!(state.borrow().calls.is_empty());
    assert_eq!(*calls.borrow(), vec![true]);
}
#[test]
fn ipc_cannot_supply_focus_or_commands_and_local_operations_do_not_query_desktop() {
    let (_dir, service, state) = fixture(false);
    let (source, calls) = source(vec![]);
    let mut output = Vec::new();
    let bytes = input(vec![
        json!({"op":"snapshot","application":"GameApp"}),
        json!({"op":"applications","command":"j/clients"}),
        json!({"op":"applications","socket":"/tmp/forbidden"}),
        json!({"op":"export","device":ID,"name":"Work"}),
        json!({"op":"shutdown"}),
    ]);
    serve_with_desktop(service, source, bytes.as_slice(), &mut output).unwrap();
    let replies = replies(&output);
    assert!(replies[..3].iter().all(|r| r["stage"] == "validation"));
    assert!(replies[3]["ok"] == true);
    assert!(calls.borrow().is_empty());
    assert!(state.borrow().calls.is_empty());
}
#[test]
fn invalid_store_prevents_catalogue_query_and_is_not_repaired() {
    let (dir, service, state) = fixture(true);
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    fs::write(&path, b"broken").unwrap();
    let (source, calls) = source(vec![]);
    let mut output = Vec::new();
    serve_with_desktop(
        service,
        source,
        frame(json!({"op":"applications"})).as_slice(),
        &mut output,
    )
    .unwrap();
    assert_eq!(replies(&output)[0]["stage"], "persistence");
    assert!(calls.borrow().is_empty());
    assert!(state.borrow().calls.is_empty());
    assert_eq!(fs::read(path).unwrap(), b"broken");
}
#[test]
fn late_desktop_callback_cannot_apply_a_rule_or_publish_stale_focus() {
    struct Late;
    impl DesktopSource for Late {
        fn describe(&mut self, _: bool, deadline: Instant) -> DesktopSnapshot {
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
            observed(Some("GameApp"))
        }
    }
    let (_dir, service, state) = fixture(true);
    let mut output = Vec::new();
    serve_with_desktop(
        service,
        Late,
        frame(json!({"op":"snapshot"})).as_slice(),
        &mut output,
    )
    .unwrap();
    let reply = &replies(&output)[0];
    assert_eq!(reply["data"]["desktop"]["available"], false);
    assert_eq!(reply["data"]["desktop"]["active_application"], Value::Null);
    assert_eq!(reply["data"]["reconciliation"][0]["profile"], "Work");
    assert_eq!(state.borrow().dpi, 1800);
}
