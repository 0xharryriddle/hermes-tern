use hermes_tern_gateway::{
    commands::{Catalog, Commands, Phase},
    session::SessionState,
};
use serde_json::json;
fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})).unwrap()
}
fn catalog() -> serde_json::Value {
    json!({"pairs":[["/memory","Inspect memory\u{1b}"],["/skills","Skills"],["/yolo","Danger"],["/custom","PRIVATE shell command"]],
        "canon":{"/memory":"/memory","/skills":"/skills","/yolo":"/yolo","/custom":"/custom"},
        "commands":{"/memory":{},"/skills":{},"/yolo":{}},"skills":{},"plugin_commands":{"custom":"PRIVATE command"},
        "warning":"PRIVATE diagnostic","api_key":"PRIVATE secret"})
}
#[test]
fn catalog_allowlists_fields_and_exact_builtin_empty_argument_reads_only() {
    let c = Catalog::parse(catalog()).unwrap();
    assert_eq!(c.rows.len(), 4);
    assert_eq!(c.rows[0].name, "/memory");
    assert!(c.rows[0].readable && c.rows[1].readable && !c.rows[2].readable && !c.rows[3].readable);
    assert!(c.degraded);
    assert!(!format!("{c:?}").contains("PRIVATE") && !c.rows[0].label.contains('\u{1b}'));
}
#[test]
fn malformed_duplicate_nonexact_and_oversized_catalogs_refuse() {
    for bad in [
        json!({}),
        json!({"pairs":[]}),
        json!({"pairs":[[" /memory","desc"]]}),
        json!({"pairs":[["/memory","a"],["/memory","b"]]}),
        json!({"pairs":[["/memory\n","bad"]]}),
        json!({"pairs":[["/memory",5]]}),
        json!({"pairs":[["/memory","x".repeat(1025)]]}),
        json!({"pairs":(0..4097).map(|i|json!([format!("/c{i}"),"desc"])).collect::<Vec<_>>()}),
    ] {
        assert!(Catalog::parse(bad).is_err());
    }
    let mut v = catalog();
    v["canon"]["/memory"] = json!("/yolo");
    assert!(!Catalog::parse(v).unwrap().rows[0].readable);
}
#[test]
fn namespaced_and_unicode_skills_with_long_descriptions_do_not_disable_builtin_reads() {
    let value = json!({
        "pairs":[["/memory","Memory"],["/plugin:review","x".repeat(1038)],["/小说拆条","Skill"]],
        "canon":{"/memory":"/memory"},"commands":{"/memory":{}},
        "skills":{"/plugin:review":{},"/小说拆条":{}},"plugin_commands":{}
    });
    let catalog = Catalog::parse(value.clone())
        .expect("canonical skill metadata must not disable owner reads");
    assert_eq!(catalog.rows.len(), 3);
    assert!(catalog.rows[0].readable);
    for row in &catalog.rows[1..] {
        assert!(!row.readable);
        assert_eq!(row.label, "Not supported in this panel");
    }
    let s = state();
    let mut commands = Commands::default();
    let id = commands.loading(&s).unwrap();
    commands.loaded(&s, &id, value).unwrap();
    assert!(commands.read(&s, &id, 1).is_err());
    let request = commands.read(&s, &id, 0).unwrap();
    assert_eq!(
        request.params,
        json!({"session_id":"s","name":"memory","arg":""})
    );
}

#[test]
fn untrusted_catalog_names_and_unsupported_descriptions_remain_bounded() {
    for name in [
        "/plugin:review\n",
        "/tool\u{202e}read",
        "/has space",
        "/../status",
        "/:review",
    ] {
        let mut value = catalog();
        value["pairs"][3][0] = json!(name);
        assert!(Catalog::parse(value).is_err(), "accepted invalid identity");
    }
    let mut value = catalog();
    value["pairs"][3][1] = json!("x".repeat(65537));
    assert!(Catalog::parse(value).is_err());
}

#[test]
fn command_read_is_catalog_and_owner_bound_and_output_never_becomes_a_prompt() {
    let mut s = state();
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, catalog()).unwrap();
    assert!(c.read(&s, "stale", 0).is_err());
    assert!(c.read(&s, &id, 2).is_err());
    let request = c.read(&s, &id, 0).unwrap();
    assert_eq!(request.method, "command.dispatch");
    assert_eq!(
        request.params,
        json!({"session_id":"s","name":"memory","arg":""})
    );
    assert_eq!(c.phase(), Phase::Running);
    assert!(c.read(&s, &id, 0).is_err());
    c.result(
        &s,
        json!({"type":"exec","output":"owner text\u{1b}[31m\u{202e}"}),
    )
    .unwrap();
    assert_eq!(c.phase(), Phase::Ready);
    assert!(c.output().join("").contains("owner text"));
    assert!(!c.output().join("").contains('\u{1b}'));
    s.revision += 1;
    assert!(c.read(&s, &id, 0).is_err());
}
#[test]
fn command_directives_malformed_and_late_owner_results_fail_closed() {
    for result in [
        json!({"type":"send","message":"must not submit"}),
        json!({"type":"skill","message":"must not submit"}),
        json!({"type":"exec","output":5}),
        json!({"type":"exec","output":"x".repeat(65537)}),
        json!({}),
    ] {
        let s = state();
        let mut c = Commands::default();
        let id = c.loading(&s).unwrap();
        c.loaded(&s, &id, catalog()).unwrap();
        c.read(&s, &id, 0).unwrap();
        assert!(c.result(&s, result).is_err());
        assert_eq!(c.phase(), Phase::Unknown);
        assert!(c.output().is_empty());
    }
    let mut s = state();
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, catalog()).unwrap();
    c.read(&s, &id, 0).unwrap();
    s.generation += 1;
    assert!(
        c.result(&s, json!({"type":"exec","output":"stale"}))
            .is_err()
    );
    assert!(c.output().is_empty());
}

#[test]
fn tools_builtin_is_readable_but_alias_and_plugin_or_skill_collisions_are_not() {
    let base = json!({"pairs":[["/tools","Tools"]],"canon":{"/tools":"/tools"},
        "commands":{"/tools":{}},"skills":{},"plugin_commands":{}});
    assert!(Catalog::parse(base.clone()).unwrap().rows[0].readable);
    for (key, value) in [
        ("canon", json!({"/tools":"/yolo"})),
        ("commands", json!({})),
        ("skills", json!({"/tools":{"prompt":"PRIVATE"}})),
        ("plugin_commands", json!({"tools":"PRIVATE"})),
    ] {
        let mut v = base.clone();
        v[key] = value;
        let c = Catalog::parse(v).unwrap();
        assert!(!c.rows[0].readable);
        assert!(!format!("{c:?}").contains("PRIVATE"));
    }
}

#[test]
fn tools_read_is_exact_rpc_and_uses_the_bound_response_variant() {
    let s = state();
    let value = json!({"pairs":[["/tools","Tools"]],"canon":{"/tools":"/tools"},
        "commands":{"/tools":{}},"skills":{},"plugin_commands":{}});
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, value.clone()).unwrap();
    let request = c.read(&s, &id, 0).unwrap();
    assert_eq!(request.method, "tools.show");
    assert_eq!(request.params, json!({"session_id":"s"}));
    c.result(&s, json!({"sections":[],"total":0})).unwrap();
    assert!(
        c.output()
            .join("")
            .contains("No tools reported by this owner")
    );
    assert_eq!(c.phase(), Phase::Ready);
    assert!(c.read(&s, &id, 0).is_err());
    let current = c.id().to_owned();
    c.read(&s, &current, 0).unwrap();
    assert!(
        c.result(&s, json!({"type":"exec","output":"PRIVATE wrong RPC"}))
            .is_err()
    );
    assert_eq!(c.phase(), Phase::Unknown);
    assert!(c.output().is_empty());

    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, catalog()).unwrap();
    c.read(&s, &id, 0).unwrap();
    assert!(c.result(&s, json!({"sections":[],"total":0})).is_err());
    assert_eq!(c.phase(), Phase::Unknown);
    assert!(c.output().is_empty());
}

#[test]
fn plugins_exact_builtin_reads_profile_inventory_not_command_or_management() {
    let s = state();
    let base = json!({"pairs":[["/plugins","Loaded plugins"]],"canon":{"/plugins":"/plugins"},
        "commands":{"/plugins":{}},"skills":{},"plugin_commands":{}});
    assert!(Catalog::parse(base.clone()).unwrap().rows[0].readable);
    for (field, value) in [
        ("canon", json!({"/plugins":"/yolo"})),
        ("commands", json!({})),
        ("skills", json!({"/plugins":{}})),
        ("plugin_commands", json!({"plugins":"private alias"})),
    ] {
        let mut bad = base.clone();
        bad[field] = value;
        assert!(!Catalog::parse(bad).unwrap().rows[0].readable);
    }
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, base).unwrap();
    let request = c.read(&s, &id, 0).unwrap();
    assert_eq!(request.method, "plugins.list");
    assert_eq!(
        request.params,
        json!({}),
        "canonical handler refuses session_id/profile params"
    );
    c.result(
        &s,
        json!({"plugins":[],"toolsets":[],"portable_mcp_servers":[]}),
    )
    .unwrap();
    assert!(
        c.output()
            .join("")
            .contains("No plugins reported by this owner")
    );
    assert!(
        c.read(&s, &id, 0).is_err(),
        "old callback must not repeat discovery"
    );
    let id = c.id().to_owned();
    c.read(&s, &id, 0).unwrap();
    assert!(
        c.result(&s, json!({"type":"exec","output":"wrong variant"}))
            .is_err()
    );
    assert_eq!(c.phase(), Phase::Unknown);
}
