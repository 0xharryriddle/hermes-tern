use hermes_tern_gateway::tool_catalog::render;
use serde_json::{Value, json};

fn response(sections: Value, total: Value) -> Value {
    json!({"sections": sections, "total": total})
}

#[test]
fn projects_only_allowed_fields_in_upstream_order() {
    let result = render(json!({
        "sections": [
            {"name":"built-ins", "tools":[
                {"name":"files.read", "description":"Read a file", "api_key":"SECRET-ROW"},
                {"name":"plugin:run-1", "description":"Run task"}
            ], "config":"SECRET-GROUP"},
            {"name":"extras", "tools":[{"name":"_other", "description":"Other"}]}
        ], "total":3, "schema":"SECRET-ROOT", "error":"SECRET-ERROR"
    }))
    .unwrap();
    assert_eq!(
        result,
        "Owner tool catalog · 3 reported tools\nCatalog entries are data, not execution permission or health checks.\nGroup: built-ins\n  Tool: files.read\n    Description: Read a file\n  Tool: plugin:run-1\n    Description: Run task\nGroup: extras\n  Tool: _other\n    Description: Other\n"
    );
    assert!(!result.contains("SECRET-"));
}

#[test]
fn empty_catalog_is_a_valid_report_not_a_failure() {
    for sections in [json!([]), json!([{"name":"empty", "tools":[]}])] {
        let result = render(response(sections, json!(0))).unwrap();
        assert!(result.starts_with("Owner tool catalog · 0 reported tools\n"));
        assert!(result.contains("No tools reported by this owner"));
        assert!(
            result.contains("Catalog entries are data, not execution permission or health checks.")
        );
    }
}

#[test]
fn sanitizes_untrusted_description_but_preserves_unicode_and_line_breaks() {
    let result = render(response(json!([{"name":"group", "tools":[{
        "name":"owner.command", "description":"中文\u{1b}]0;bad\u{7}\u{202e} yes\nsecond\tline\r"
    }]}]), json!(1))).unwrap();
    assert!(result.contains("    Description: 中文]0;bad yes\n    Description: second\tline\n"));
    assert!(!result.contains('\u{1b}'));
    assert!(!result.contains('\u{7}'));
    assert!(!result.contains('\u{202e}'));
    assert!(!result.contains('\r'));
}

#[test]
fn missing_or_wrong_typed_structure_refuses_instead_of_reporting_zero() {
    for bad in [
        json!(null),
        json!({}),
        json!({"total":0}),
        json!({"sections":null,"total":0}),
        json!({"sections":{},"total":0}),
        json!({"sections":[null],"total":0}),
        json!({"sections":[{"name":"x"}],"total":0}),
        json!({"sections":[{"name":"x","tools":null}],"total":0}),
        json!({"sections":[{"name":"x","tools":[{}]}],"total":1}),
        json!({"sections":[{"name":"x","tools":[{"name":"a"}]}],"total":1}),
        json!({"sections":[{"name":"x","tools":[{"name":"a","description":null}]}],"total":1}),
    ] {
        assert!(render(bad.clone()).is_err(), "accepted {bad}");
    }
}

#[test]
fn total_must_be_an_exact_nonnegative_integer_equal_to_rows() {
    for total in [
        json!(null),
        json!(true),
        json!(1.0),
        json!("1"),
        json!(-1),
        json!(2),
    ] {
        let bad = response(
            json!([{"name":"a", "tools":[{"name":"b", "description":"c"}]}]),
            total,
        );
        assert!(render(bad.clone()).is_err(), "accepted {bad}");
    }
    assert!(render(json!({"sections":[], "total":1})).is_err());
    assert!(render(json!({"sections":[]})).is_err());
}

#[test]
fn duplicate_groups_and_tool_identities_are_protocol_errors() {
    assert!(
        render(response(
            json!([
                {"name":"g", "tools":[]}, {"name":"g", "tools":[]}
            ]),
            json!(0)
        ))
        .is_err()
    );
    for groups in [
        json!([{"name":"g", "tools":[{"name":"dup", "description":"a"}, {"name":"dup", "description":"b"}]}]),
        json!([{"name":"one", "tools":[{"name":"dup", "description":"a"}]},
               {"name":"two", "tools":[{"name":"dup", "description":"b"}]}]),
    ] {
        assert!(render(response(groups, json!(2))).is_err());
    }
}

#[test]
fn group_and_tool_names_must_be_exact_safe_nonblank_identities() {
    for name in [
        "",
        " ",
        " leading",
        "trailing ",
        "a\nb",
        "a\tb",
        "\u{202e}x",
        "a\u{200f}b",
    ] {
        assert!(
            render(response(json!([{"name":name,"tools":[]}]), json!(0))).is_err(),
            "group {name:?}"
        );
        assert!(
            render(response(
                json!([{"name":"g","tools":[{"name":name,"description":"ok"}]}]),
                json!(1)
            ))
            .is_err(),
            "tool {name:?}"
        );
    }
    assert!(render(response(json!([{"name":"日本語", "tools":[{"name":"owner.v2:read_thing-1", "description":"ok"}]}]), json!(1))).is_ok());
}

#[test]
fn enforces_utf8_byte_limits_on_each_field() {
    let name_128 = format!("{}ab", "界".repeat(42));
    let tool_256 = format!("{}a", "界".repeat(85));
    let desc_1024 = format!("{}a", "界".repeat(341));
    assert!(
        render(response(
            json!([{"name":name_128,"tools":[{"name":tool_256,"description":desc_1024}]}]),
            json!(1)
        ))
        .is_ok()
    );
    let name_129 = format!("{}abc", "界".repeat(42));
    assert!(render(response(json!([{"name":name_129,"tools":[]}]), json!(0))).is_err());
    let tool_257 = format!("{}ab", "界".repeat(85));
    assert!(
        render(response(
            json!([{"name":"g","tools":[{"name":tool_257,"description":"ok"}]}]),
            json!(1)
        ))
        .is_err()
    );
    let desc_1025 = format!("{}ab", "界".repeat(341));
    assert!(
        render(response(
            json!([{"name":"g","tools":[{"name":"x","description":desc_1025}]}]),
            json!(1)
        ))
        .is_err()
    );
}

#[test]
fn limits_section_count_and_total_tools_without_truncation() {
    let groups = (0..128)
        .map(|n| json!({"name":format!("g{n}"), "tools":[]}))
        .collect::<Vec<_>>();
    assert!(render(response(json!(groups), json!(0))).is_ok());
    let groups = (0..129)
        .map(|n| json!({"name":format!("g{n}"), "tools":[]}))
        .collect::<Vec<_>>();
    assert!(render(response(json!(groups), json!(0))).is_err());
    let tools = (0..4096)
        .map(|n| json!({"name":format!("t{n}"), "description":""}))
        .collect::<Vec<_>>();
    // The count is within the row cap, but a complete rendering exceeds 64 KiB.
    assert!(render(response(json!([{"name":"g","tools":tools}]), json!(4096))).is_err());
    let tools = (0..4097)
        .map(|n| json!({"name":format!("t{n}"), "description":""}))
        .collect::<Vec<_>>();
    assert!(render(response(json!([{"name":"g","tools":tools}]), json!(4097))).is_err());
}

#[test]
fn complete_formatted_output_is_bounded_at_64_kib_without_silent_truncation() {
    let tools = (0..50)
        .map(|n| json!({"name":format!("t{n}"), "description":"x".repeat(1024)}))
        .collect::<Vec<_>>();
    let result = render(response(json!([{"name":"g", "tools":tools}]), json!(50))).unwrap();
    assert!(result.len() <= 65_536);
    assert!(result.contains("  Tool: t49\n"));
    let tools = (0..100)
        .map(|n| json!({"name":format!("t{n}"), "description":"x".repeat(1024)}))
        .collect::<Vec<_>>();
    assert!(render(response(json!([{"name":"g", "tools":tools}]), json!(100))).is_err());
}
