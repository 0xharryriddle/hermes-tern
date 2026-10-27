#[path = "../src/delegated_catalog.rs"]
mod delegated_catalog;

use delegated_catalog::Catalog;
use hermes_tern_core::error::Result;
use serde_json::{Value, json};

fn parse_rows(rows: Vec<Value>) -> Result<Catalog> {
    Catalog::parse(json!({"subagents": rows, "delegations": []}))
}

fn row(id: &str) -> Value {
    json!({"subagent_id": id})
}

fn reject(value: Value) {
    assert!(Catalog::parse(value).is_err(), "malformed roster accepted");
}

#[test]
fn projects_all_reported_fields_as_stable_inert_text() {
    let catalog = parse_rows(vec![json!({
        "subagent_id": "子-عامل",
        "parent_id": "parent-é",
        "depth": 2,
        "goal": "Review 日本語",
        "delegation_id": "deleg-α",
        "model": "model-β",
        "started_at": 1234.5,
        "status": "reported-running",
        "tool_count": 7,
        "last_tool": "read_file",
        "accepting_steer": true
    })])
    .unwrap();
    assert_eq!(catalog.rows.len(), 1);
    assert_eq!(catalog.rows[0].id, "子-عامل");
    assert_eq!(
        catalog.rows[0].text,
        "Delegated: 子-عامل\nParent: parent-é · depth 2\nGoal: Review 日本語\nModel: model-β · status reported-running\nTool calls: 7 · last tool read_file\nStarted epoch: 1234.5\nSteer reported: true · controls not offered\nDelegation: deleg-α"
    );
}

#[test]
fn absent_and_null_metadata_have_the_same_explicit_unavailable_projection() {
    let missing = parse_rows(vec![row("child")]).unwrap();
    let nulls = parse_rows(vec![json!({
        "subagent_id": "child", "parent_id": null, "depth": null,
        "goal": null, "delegation_id": null, "model": null,
        "started_at": null, "status": null, "tool_count": null,
        "last_tool": null, "accepting_steer": null
    })])
    .unwrap();
    assert_eq!(missing.rows[0].text, nulls.rows[0].text);
    assert_eq!(
        missing.rows[0].text,
        "Delegated: child\nParent: unavailable · depth unavailable\nGoal: unavailable\nModel: unavailable · status unavailable\nTool calls: unavailable · last tool unavailable\nStarted epoch: unavailable\nSteer reported: unavailable · controls not offered"
    );
}

#[test]
fn empty_roster_is_success_but_missing_or_wrong_envelopes_are_not_empty() {
    assert!(parse_rows(vec![]).unwrap().rows.is_empty());
    for value in [
        Value::Null,
        json!(true),
        json!(0),
        json!("roster"),
        json!([]),
    ] {
        reject(value);
    }
    for value in [
        json!({}),
        json!({"subagents": []}),
        json!({"delegations": []}),
        json!({"subagents": null, "delegations": []}),
        json!({"subagents": {}, "delegations": []}),
        json!({"subagents": false, "delegations": []}),
        json!({"subagents": "[]", "delegations": []}),
    ] {
        reject(value);
    }
}

#[test]
fn delegations_must_be_present_and_exactly_an_empty_array() {
    for value in [
        Value::Null,
        json!(false),
        json!(0),
        json!(""),
        json!({}),
        json!([{}]),
    ] {
        reject(json!({"subagents": [], "delegations": value}));
    }
}

#[test]
fn unsupported_or_error_shapes_are_not_successful_empty_rosters() {
    for value in [
        json!({"error": "unsupported_projection"}),
        json!({"error": "not_found"}),
        json!({"result": {"subagents": [], "delegations": []}}),
    ] {
        reject(value);
    }
}

#[test]
fn rows_must_be_objects_with_required_string_identity() {
    for value in [
        Value::Null,
        json!(false),
        json!(0),
        json!("child"),
        json!([]),
    ] {
        assert!(parse_rows(vec![value]).is_err());
    }
    assert!(parse_rows(vec![json!({})]).is_err());
    for id in [
        Value::Null,
        json!(true),
        json!(0),
        json!(1.0),
        json!([]),
        json!({}),
        json!(""),
    ] {
        assert!(parse_rows(vec![json!({"subagent_id": id})]).is_err());
    }
}

#[test]
fn retains_exact_unicode_and_space_bearing_identities_without_normalization() {
    let ids = [
        "é",
        "e\u{301}",
        "عامل",
        " child",
        "child ",
        r"child\u{202e}",
    ];
    let catalog = parse_rows(ids.iter().map(|id| row(id)).collect()).unwrap();
    let actual: Vec<_> = catalog.rows.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(actual, ids);
    for (entry, id) in catalog.rows.iter().zip(ids) {
        assert!(entry.text.starts_with(&format!("Delegated: {id}\n")));
    }
}

#[test]
fn identities_are_bounded_by_bytes_with_a_one_byte_negative_control() {
    for id in ["x".repeat(256), "é".repeat(128)] {
        let catalog = parse_rows(vec![row(&id)]).unwrap();
        assert_eq!(catalog.rows[0].id, id);
        assert!(parse_rows(vec![row(&format!("{id}x"))]).is_err());
    }
}

#[test]
fn identities_refuse_all_c0_c1_and_bidi_controls_instead_of_sanitizing() {
    let controls = (0..=31).chain(127..=159).filter_map(char::from_u32).chain([
        '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
        '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ]);
    for control in controls {
        assert!(parse_rows(vec![row(&format!("child{control}"))]).is_err());
    }
    assert_eq!(parse_rows(vec![row("child")]).unwrap().rows[0].id, "child");
}

#[test]
fn duplicate_identity_refuses_the_whole_inventory() {
    assert!(parse_rows(vec![row("a"), row("b"), row("a")]).is_err());
    assert_eq!(parse_rows(vec![row("a"), row("b")]).unwrap().rows.len(), 2);
}

#[test]
fn exactly_128_unique_rows_fit_but_129_refuse_instead_of_truncating() {
    let mut rows: Vec<_> = (0..128).map(|n| row(&format!("child-{n}"))).collect();
    let catalog = parse_rows(rows.clone()).unwrap();
    assert_eq!(catalog.rows.len(), 128);
    assert_eq!(catalog.rows[127].id, "child-127");
    rows.push(row("child-128"));
    assert!(parse_rows(rows).is_err());
}

#[test]
fn malformed_final_row_refuses_without_returning_the_valid_prefix() {
    assert!(
        parse_rows(vec![
            row("valid"),
            json!({"subagent_id": "bad", "depth": 33})
        ])
        .is_err()
    );
}

#[test]
fn unknown_private_fields_are_omitted_and_do_not_consume_render_budget() {
    let canary = "PRIVATE_CONFIG_ROUTE_PROCESS_TRANSCRIPT";
    let private = canary.repeat(4096);
    let catalog = Catalog::parse(json!({
        "subagents": [{
            "subagent_id": "child", "config": private, "route": {"private": canary},
            "process": [canary], "transcript": canary,
            "owner_agent_session_id": canary, "future_metadata": [false, {}]
        }],
        "delegations": [], "private_owner": {"raw": private}
    }))
    .unwrap();
    let plain = parse_rows(vec![row("child")]).unwrap();
    assert_eq!(catalog.rows[0].text, plain.rows[0].text);
    assert!(!format!("{catalog:?}").contains(canary));
    let error = Catalog::parse(
        json!({"subagents": [{"subagent_id": canary, "goal": false}], "delegations": []}),
    )
    .unwrap_err();
    assert!(!format!("{error:?}: {error}").contains(canary));
}

#[test]
fn each_optional_string_accepts_empty_but_refuses_wrong_types() {
    for key in [
        "parent_id",
        "delegation_id",
        "goal",
        "model",
        "status",
        "last_tool",
    ] {
        for valid in [Value::Null, json!("")] {
            let mut value = row("child");
            value[key] = valid;
            assert!(parse_rows(vec![value]).is_ok(), "valid {key} refused");
        }
        for invalid in [json!(false), json!(0), json!(1.0), json!([]), json!({})] {
            let mut value = row("child");
            value[key] = invalid;
            assert!(parse_rows(vec![value]).is_err(), "invalid {key} accepted");
        }
    }
}

#[test]
fn each_optional_string_has_its_own_byte_limit_and_one_byte_negative_control() {
    for (key, limit) in [
        ("parent_id", 256),
        ("delegation_id", 256),
        ("goal", 1024),
        ("model", 256),
        ("status", 64),
        ("last_tool", 128),
    ] {
        for text in ["a".repeat(limit), "é".repeat(limit / 2)] {
            let mut value = row("child");
            value[key] = json!(text);
            let catalog = parse_rows(vec![value.clone()]).unwrap();
            assert!(catalog.rows[0].text.contains(&text), "{key} truncated");
            value[key] = json!(format!("{text}a"));
            assert!(parse_rows(vec![value]).is_err(), "oversized {key} accepted");
        }
    }
}

#[test]
fn metadata_controls_and_bidi_are_escaped_without_escaping_normal_unicode() {
    for key in [
        "parent_id",
        "delegation_id",
        "goal",
        "model",
        "status",
        "last_tool",
    ] {
        let mut value = row("child");
        value[key] = json!("عé\n\t\u{1b}\u{202e}");
        let catalog = parse_rows(vec![value]).unwrap();
        let text = &catalog.rows[0].text;
        assert!(
            text.contains("عé\\n\\t\\u{1b}\\u{202e}"),
            "{key} not escaped"
        );
        assert!(
            !text.chars().any(
                |c| (c.is_control() && c != '\n') || hermes_tern_core::text::is_bidi_control(c)
            )
        );
        assert_eq!(
            text.matches('\n').count(),
            if key == "delegation_id" { 7 } else { 6 }
        );
    }
}

#[test]
fn depth_is_a_nullable_unsigned_integer_in_the_closed_zero_to_32_range() {
    for depth in [json!(0), json!(32)] {
        let catalog = parse_rows(vec![json!({"subagent_id": "child", "depth": depth})]).unwrap();
        assert!(
            catalog.rows[0]
                .text
                .contains(&format!(" · depth {depth}\n"))
        );
    }
    for depth in [
        json!(-1),
        json!(33),
        json!(u64::MAX),
        json!(0.0),
        json!(1.0),
        json!(1.5),
        json!(true),
        json!("1"),
        json!([]),
        json!({}),
    ] {
        assert!(parse_rows(vec![json!({"subagent_id": "child", "depth": depth})]).is_err());
    }
}

#[test]
fn tool_count_accepts_the_full_u64_range_without_float_or_string_coercion() {
    for count in [json!(0), json!(u64::MAX)] {
        let catalog =
            parse_rows(vec![json!({"subagent_id": "child", "tool_count": count})]).unwrap();
        assert!(
            catalog.rows[0]
                .text
                .contains(&format!("Tool calls: {count} ·"))
        );
    }
    let above_u64: Value = serde_json::from_str("18446744073709551616").unwrap();
    for count in [
        above_u64,
        json!(-1),
        json!(0.0),
        json!(1.0),
        json!(1.5),
        json!(true),
        json!("1"),
        json!([]),
        json!({}),
    ] {
        assert!(parse_rows(vec![json!({"subagent_id": "child", "tool_count": count})]).is_err());
    }
}

#[test]
fn started_at_is_nullable_finite_nonnegative_json_number_not_a_coerced_value() {
    for started in [
        json!(0),
        json!(0.0),
        json!(0.5),
        json!(u64::MAX),
        json!(f64::MAX),
    ] {
        let catalog =
            parse_rows(vec![json!({"subagent_id": "child", "started_at": started})]).unwrap();
        assert!(
            catalog.rows[0]
                .text
                .contains(&format!("Started epoch: {started}\n"))
        );
    }
    for started in [
        json!(-1),
        json!(-0.5),
        json!(true),
        json!("1"),
        json!("NaN"),
        json!("Infinity"),
        json!([]),
        json!({}),
    ] {
        assert!(parse_rows(vec![json!({"subagent_id": "child", "started_at": started})]).is_err());
    }
    // serde_json refuses nonfinite JSON numbers before a Value can reach this API.
    assert!(serde_json::from_str::<Value>("1e400").is_err());
}

#[test]
fn accepting_steer_is_nullable_strict_boolean_reported_without_controls() {
    for flag in [true, false] {
        let catalog = parse_rows(vec![
            json!({"subagent_id": "child", "accepting_steer": flag}),
        ])
        .unwrap();
        assert!(
            catalog.rows[0]
                .text
                .ends_with(&format!("Steer reported: {flag} · controls not offered"))
        );
    }
    for flag in [
        json!(0),
        json!(1),
        json!(1.0),
        json!("true"),
        json!([]),
        json!({}),
    ] {
        assert!(
            parse_rows(vec![
                json!({"subagent_id": "child", "accepting_steer": flag})
            ])
            .is_err()
        );
    }
}

#[test]
fn complete_rendered_utf8_text_fits_exactly_65536_bytes_but_not_one_more() {
    let mut rows: Vec<_> = (0..128)
        .map(|n| json!({"subagent_id": format!("child-{n}"), "goal": ""}))
        .collect();
    let baseline: usize = parse_rows(rows.clone())
        .unwrap()
        .rows
        .iter()
        .map(|entry| entry.text.len())
        .sum();
    let mut remaining = 65_536usize.checked_sub(baseline).unwrap();
    for entry in &mut rows {
        let count = remaining.min(1024);
        // Use multibyte Unicode so a character-count implementation cannot pass.
        entry["goal"] = json!(format!(
            "{}{}",
            "é".repeat(count / 2),
            "x".repeat(count % 2)
        ));
        remaining -= count;
    }
    assert_eq!(remaining, 0, "fixture cannot reach the render boundary");
    let catalog = parse_rows(rows.clone()).unwrap();
    assert_eq!(catalog.rows.len(), 128);
    assert_eq!(
        catalog
            .rows
            .iter()
            .map(|entry| entry.text.len())
            .sum::<usize>(),
        65_536
    );
    let entry = rows
        .iter_mut()
        .find(|entry| entry["goal"].as_str().unwrap().len() < 1024)
        .unwrap();
    entry["goal"] = json!(format!("{}x", entry["goal"].as_str().unwrap()));
    assert!(parse_rows(rows).is_err());
}

#[test]
fn escaped_output_expansion_counts_toward_the_whole_inventory_budget() {
    let rows: Vec<_> = (0..20)
        .map(|n| json!({"subagent_id": format!("child-{n}"), "goal": "\u{1}".repeat(1024)}))
        .collect();
    let raw_goals: usize = rows
        .iter()
        .map(|entry| entry["goal"].as_str().unwrap().len())
        .sum();
    assert!(raw_goals < 65_536);
    assert!(parse_rows(rows.clone()).is_err());
    assert!(parse_rows(rows.into_iter().take(1).collect()).is_ok());
}
