use hermes_tern_gateway::session::SessionState;
use hermes_tern_ui::{composer::Composer, runtime::RuntimeDetails, transcript::Transcript, view};
use serde_json::{Value, json};
use tern_sdk::{keys::Key, ui::Node};

fn state(info: Value) -> SessionState {
    SessionState::from_snapshot(
        "verified-profile",
        "verified-instance",
        json!({
            "session_id":"session-exact","subscription_id":"sub","authority_epoch":1,
            "revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,
            "running":false,"messages":[],"pending":[],"prompts":[],"info":info
        }),
    )
    .unwrap()
}
fn text<M>(node: &Node<M>) -> String {
    let mut out = node
        .props()
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    for child in node.child_nodes() {
        out.push('\n');
        out.push_str(&text(child));
    }
    out
}
fn rendered(s: &SessionState, panel: &RuntimeDetails) -> String {
    let v = view::render_with_details(s, &Composer::default(), None, &Transcript::default(), panel);
    v.regions()
        .iter()
        .flatten()
        .map(|node| text(node))
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn runtime_identity_and_inventory_use_only_qualified_allowlisted_fields() {
    let s = state(
        json!({"profile_id":"forged-profile","model":"route-model","provider":"route-provider",
        "cwd":"/workspace","tools":{"core":["read_file","terminal"]},
        "skills":{"rust":["safe-rust"]},"mcp_servers":[{"name":"catalog","status":"connected","error":"PRIVATE-MCP-ERROR"}],
        "system_prompt":"PRIVATE-SYSTEM","api_key":"PRIVATE-KEY","launch_request":{"api_key":"PRIVATE-LAUNCH"},
        "credential_hint":"PRIVATE-HINT","base_url":"PRIVATE-ENDPOINT"}),
    );
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    let all = rendered(&s, &panel);
    for expected in [
        "verified-profile",
        "verified-instance",
        "route-model",
        "route-provider",
        "/workspace",
        "Tools: 2",
        "Skills: 1",
        "MCP servers: 1",
        "read_file",
        "safe-rust",
        "catalog",
    ] {
        assert!(all.contains(expected), "missing {expected}: {all}");
    }
    assert!(!all.contains("forged-profile") && !all.contains("PRIVATE-"));
    assert!(all.contains("Memory") && all.contains("not exposed"));
}
#[test]
fn malformed_or_absent_inventory_is_unavailable_not_a_fabricated_zero() {
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    for info in [
        json!({}),
        json!({"tools":{"core":["ok",true]},"skills":[],"mcp_servers":[{"name":4}]}),
        json!({"tools":null,"skills":null,"mcp_servers":null}),
    ] {
        let out = rendered(&state(info), &panel);
        for name in ["Tools", "Skills", "MCP servers"] {
            assert!(out.contains(&format!("{name}: unavailable")), "{out}");
            assert!(!out.contains(&format!("{name}: 0")));
        }
    }
    let out = rendered(
        &state(json!({"tools":{},"skills":{},"mcp_servers":[]})),
        &panel,
    );
    for name in ["Tools", "Skills", "MCP servers"] {
        assert!(out.contains(&format!("{name}: 0")));
    }
}
#[test]
fn runtime_pages_are_bounded_inert_and_never_treat_labels_as_commands() {
    let names = (0..50)
        .map(|n| json!(format!("name-{n:02}")))
        .collect::<Vec<_>>();
    let s = state(
        json!({"model":"bad\u{1b}\u{202e}\n$(touch nope)","tools":{"group":names},"skills":{},"mcp_servers":[]}),
    );
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    let first = rendered(&s, &panel);
    assert!(first.contains("Tools: 50") && first.contains("name-00") && !first.contains("name-49"));
    assert!(!first.contains('\u{1b}') && !first.contains('\u{202e}'));
    assert!(first.contains("$(touch nope)"));
    panel.page(&s, 0, 1);
    assert_eq!(panel.current_page(), 1);
    let next = rendered(&s, &panel);
    assert!(!next.contains("name-00") && next.contains("name-20"));
    panel.page(&s, 0, 2);
    assert_eq!(panel.current_page(), 1, "stale page control advanced");
    let huge = state(json!({"model":"x".repeat(10000),"tools":{"g":["x".repeat(10000)]}}));
    assert!(rendered(&huge, &panel).len() < 12000);
}
#[test]
fn runtime_local_navigation_closes_without_editing_or_upstream_authority() {
    let s = state(json!({}));
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    assert!(panel.key(&s, &Key::named("enter")));
    assert!(panel.is_open());
    assert!(panel.key(&s, &Key::named("escape")));
    assert!(!panel.is_open());
    assert!(!panel.key(&s, &Key::named("enter")));
}

#[test]
fn runtime_delegated_read_action_is_bound_to_the_current_opening_only() {
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    let first = panel.control_id();
    assert!(panel.accepts_read(first));
    panel.set_open(false);
    assert!(!panel.accepts_read(first));
    panel.set_open(true);
    assert!(!panel.accepts_read(first));
    assert!(panel.accepts_read(panel.control_id()));
}

#[test]
fn delegated_roster_pages_are_local_inert_and_do_not_render_private_or_previous_panel_data() {
    use hermes_tern_gateway::delegated::Delegated;
    let s = state(json!({}));
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    let current = panel.control_id();
    let mut delegated = Delegated::default();
    let token = delegated.loading(&s, current).unwrap();
    delegated
        .loaded(
            &s,
            &token,
            json!({"subagents":[
        {"subagent_id":"first-child","goal":"inert $(shell)","private":"PRIVATE-1"},
        {"subagent_id":"second-child","goal":"second goal","agent":{"secret":"PRIVATE-2"}}
    ],"delegations":[]}),
        )
        .unwrap();
    let mut draft = Composer::default();
    draft.insert("draft 😀").unwrap();
    let render = |panel: &RuntimeDetails| {
        panel
            .render_with_delegated(&s, &draft, None, &delegated)
            .regions()
            .iter()
            .flatten()
            .map(|n| text(n))
            .collect::<String>()
    };
    let first = render(&panel);
    assert!(
        first.contains("2 reported delegated agents") && first.contains("Delegated: first-child")
    );
    assert!(first.contains("draft 😀") && first.contains("not independent peers"));
    assert!(!first.contains("Delegated: second-child") && !first.contains("PRIVATE-"));
    panel.delegated_page(&delegated, &s, current, delegated.id(), 0, 1);
    let second = render(&panel);
    assert!(
        second.contains("Delegated: second-child") && !second.contains("Delegated: first-child")
    );
    panel.delegated_page(&delegated, &s, current, delegated.id(), 0, 0);
    assert!(
        render(&panel).contains("Delegated: second-child"),
        "old page action changed current page"
    );
    panel.delegated_page(&delegated, &s, current, "old-snapshot", 1, 0);
    assert!(
        render(&panel).contains("Delegated: second-child"),
        "decoded old-snapshot page action crossed the inventory fence"
    );
    panel.reset_delegated_page();
    assert!(render(&panel).contains("Delegated: first-child"));
    assert!(render(&panel).contains("Delegated page 1/2"));
    panel.set_open(false);
    panel.set_open(true);
    assert!(!render(&panel).contains("Delegated: first-child"));
    assert!(!render(&panel).contains("Delegated: second-child"));
    assert_eq!(draft.text(), "draft 😀");
}

#[test]
fn task_plan_is_read_only_and_snapshot_page_and_panel_actions_are_distinct() {
    let mut s = state(json!({}));
    s.task_plan.update(&json!({"revision":1,"todos":[
        {"id":"first","content":"First reported task","status":"in_progress"},
        {"id":"second","content":"Second reported task","status":"completed","parent":"first","private":"PRIVATE-TASK"}
    ]}));
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    let runtime_panel = panel.control_id();
    panel.show_plan(&s, runtime_panel, true);
    let plan_panel = panel.control_id();
    assert_ne!(runtime_panel, plan_panel);
    let original = s.task_plan.snapshot_id().to_owned();
    let first = rendered(&s, &panel);
    assert!(first.contains("2 reported tasks") && first.contains("First reported task"));
    assert!(first.contains("in_progress: 1") && first.contains("completed: 1"));
    assert!(!first.contains("Second reported task") && !first.contains("PRIVATE-"));
    panel.plan_page(&s, plan_panel, &original, 0, 1);
    assert!(rendered(&s, &panel).contains("Second reported task"));
    panel.plan_page(&s, plan_panel, "stale-snapshot", 1, 0);
    assert!(rendered(&s, &panel).contains("Second reported task"));
    s.task_plan.update(
        &json!({"revision":2,"todos":[{"id":"new","content":"Fresh task","status":"pending"}]}),
    );
    panel.reconcile(&s);
    panel.plan_page(&s, plan_panel, &original, 0, 1);
    let fresh = rendered(&s, &panel);
    assert!(fresh.contains("Fresh task") && fresh.contains("Task page 1/1"));
    assert!(!fresh.contains("Second reported task"));
    panel.show_plan(&s, runtime_panel, false);
    assert!(rendered(&s, &panel).contains("Task plan · owner-reported snapshot"));
    panel.show_plan(&s, plan_panel, false);
    assert!(!rendered(&s, &panel).contains("Task plan · owner-reported snapshot"));
    assert!(rendered(&s, &panel).contains("Show task plan"));
}

#[test]
fn unavailable_task_plan_is_not_an_empty_success_and_keys_cannot_change_draft_or_runtime() {
    let s = state(json!({}));
    let mut panel = RuntimeDetails::default();
    panel.set_open(true);
    panel.show_plan(&s, panel.control_id(), true);
    let before = rendered(&s, &panel);
    assert!(before.contains("Task plan unavailable") && !before.contains("0 reported tasks"));
    assert!(panel.key(&s, &Key::named("enter")));
    assert!(panel.key(&s, &Key::named("right")));
    assert!(panel.key(
        &s,
        &Key {
            ctrl: true,
            ..Key::named("c")
        }
    ));
    assert!(!panel.is_open());
    assert!(s.can_submit() && s.messages.is_empty() && s.prompts.is_empty());
}
