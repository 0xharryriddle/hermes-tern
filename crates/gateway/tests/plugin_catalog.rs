use hermes_tern_gateway::plugin_catalog::render;
use serde_json::{Value, json};

fn empty() -> Value {
    json!({"plugins":[],"toolsets":[],"portable_mcp_servers":[]})
}
fn plugin(key: &str) -> Value {
    json!({"name":"p","key":key,"version":"1","enabled":true,
        "tools":1,"hooks":0,"commands":2,"error":""})
}
fn toolset(key: &str) -> Value {
    json!({"key":key,"label":"Label","description":"Description"})
}
fn one() -> Value {
    let mut value = empty();
    value["plugins"] = json!([plugin("p")]);
    value
}
fn refuses(value: Value) {
    let error = render(value).expect_err("malformed catalog must refuse");
    assert!(!format!("{error:?} {error}").contains("PRIVATE-CANARY"));
}

#[test]
fn canonical_projection_preserves_order_and_redacts_private_fields() {
    let mut value = one();
    value["plugins"][0]["path"] = json!("PRIVATE-CANARY/path");
    let mut second = plugin("other");
    second["enabled"] = json!(false);
    second["error"] = json!("PRIVATE-CANARY\n\u{1b}secret");
    value["plugins"].as_array_mut().unwrap().push(second);
    value["toolsets"] = json!([toolset("z"), toolset("a")]);
    value["portable_mcp_servers"] = json!(["server-z", "server-a"]);
    value["config"] = json!({"api_key":"PRIVATE-CANARY"});
    assert_eq!(
        render(value).unwrap(),
        concat!(
            "Owner plugins · 2 plugins · 2 toolsets · 2 portable MCP\n",
            "Report only, not health or execution permission.\n",
            "Plugin: p [p] v1 enabled; tools=1 hooks=0 commands=2; load error=no\n",
            "Plugin: p [other] v1 disabled; tools=1 hooks=0 commands=2; load error=reported\n",
            "Toolset: z · Label\n  Description: Description\n",
            "Toolset: a · Label\n  Description: Description\n",
            "Portable MCP: server-z\nPortable MCP: server-a\n"
        )
    );
    assert!(render(one()).unwrap().len() <= 200);
}

#[test]
fn empty_arrays_are_reported_zero_not_unavailable() {
    assert_eq!(
        render(empty()).unwrap(),
        concat!(
            "Owner plugins · 0 plugins · 0 toolsets · 0 portable MCP\n",
            "Report only, not health or execution permission.\n",
            "No plugins reported by this owner\n"
        )
    );
    for value in [
        Value::Null,
        json!({}),
        json!([]),
        json!({"error":"PRIVATE-CANARY"}),
    ] {
        refuses(value);
    }
}

#[test]
fn all_three_canonical_arrays_are_required_and_typed() {
    for field in ["plugins", "toolsets", "portable_mcp_servers"] {
        let mut value = empty();
        value.as_object_mut().unwrap().remove(field);
        refuses(value);
        for bad in [
            Value::Null,
            json!({}),
            json!("PRIVATE-CANARY"),
            json!(true),
            json!([null]),
        ] {
            let mut value = empty();
            value[field] = bad;
            refuses(value);
        }
    }
}

#[test]
fn every_plugin_field_is_required_and_exactly_typed() {
    for field in [
        "name", "key", "version", "enabled", "tools", "hooks", "commands", "error",
    ] {
        let mut value = one();
        value["plugins"][0].as_object_mut().unwrap().remove(field);
        refuses(value);
        let mut value = one();
        value["plugins"][0][field] = Value::Null;
        refuses(value);
        let mut value = one();
        value["plugins"][0][field] = json!({"PRIVATE-CANARY":true});
        refuses(value);
    }
    for bad in [json!("true"), json!(1), json!(0), json!([])] {
        let mut value = one();
        value["plugins"][0]["enabled"] = bad;
        refuses(value);
    }
}

#[test]
fn toolsets_and_portable_names_require_complete_typed_rows() {
    for field in ["key", "label", "description"] {
        let mut row = toolset("k");
        row.as_object_mut().unwrap().remove(field);
        let mut value = empty();
        value["toolsets"] = json!([row]);
        refuses(value);
        for bad in [Value::Null, json!(false), json!(7), json!([])] {
            let mut row = toolset("k");
            row[field] = bad;
            let mut value = empty();
            value["toolsets"] = json!([row]);
            refuses(value);
        }
    }
    for bad in [
        json!(false),
        json!(7),
        json!({"name":"PRIVATE-CANARY"}),
        json!([]),
    ] {
        let mut value = empty();
        value["portable_mcp_servers"] = json!([bad]);
        refuses(value);
    }
}

#[test]
fn counts_are_strict_unsigned_integers_through_4096() {
    for field in ["tools", "hooks", "commands"] {
        for count in [
            json!(true),
            json!(false),
            json!("1"),
            json!(1.0),
            json!(-1),
            json!(4097),
            json!(u64::MAX),
        ] {
            let mut value = one();
            value["plugins"][0][field] = count;
            refuses(value);
        }
        for count in [0, 4096] {
            let mut value = one();
            value["plugins"][0][field] = json!(count);
            assert!(render(value).unwrap().contains(&format!("{field}={count}")));
        }
    }
}

#[test]
fn identities_reject_blank_trim_changes_and_all_display_controls() {
    let mut bad = vec![
        "".to_owned(),
        " ".to_owned(),
        " leading".to_owned(),
        "trailing ".to_owned(),
    ];
    bad.extend(
        (0..=0x9f)
            .filter_map(char::from_u32)
            .filter(|c| c.is_control())
            .map(|c| format!("a{c}b")),
    );
    bad.extend(
        [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
            '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{2028}', '\u{2029}',
        ]
        .map(|c| format!("a{c}b")),
    );
    for text in bad {
        for pointer in [
            "/plugins/0/name",
            "/plugins/0/key",
            "/toolsets/0/key",
            "/portable_mcp_servers/0",
        ] {
            let mut value = one();
            value["toolsets"] = json!([toolset("k")]);
            value["portable_mcp_servers"] = json!(["m"]);
            *value.pointer_mut(pointer).unwrap() = json!(text);
            refuses(value);
        }
        if text.chars().any(|c| {
            c.is_control()
                || hermes_tern_core::text::is_bidi_control(c)
                || matches!(c, '\u{2028}' | '\u{2029}')
        }) {
            let mut value = one();
            value["plugins"][0]["version"] = json!(text);
            refuses(value);
        }
    }
    for version in ["", " v1 "] {
        let mut value = one();
        value["plugins"][0]["version"] = json!(version);
        assert!(
            render(value)
                .unwrap()
                .contains(&format!("v{version} enabled"))
        );
    }
}

#[test]
fn utf8_byte_limits_are_inclusive_for_every_string_field() {
    for (pointer, limit) in [
        ("/plugins/0/name", 256),
        ("/plugins/0/key", 256),
        ("/plugins/0/version", 256),
        ("/plugins/0/error", 16384),
        ("/toolsets/0/key", 128),
        ("/toolsets/0/label", 256),
        ("/toolsets/0/description", 1024),
        ("/portable_mcp_servers/0", 256),
    ] {
        let mut value = one();
        value["toolsets"] = json!([toolset("k")]);
        value["portable_mcp_servers"] = json!(["m"]);
        let text = format!("{}{}", "界".repeat(limit / 3), "x".repeat(limit % 3));
        *value.pointer_mut(pointer).unwrap() = json!(text);
        assert!(render(value.clone()).is_ok(), "boundary {pointer}");
        *value.pointer_mut(pointer).unwrap() = json!(format!("{text}x"));
        refuses(value);
    }
}

#[test]
fn raw_load_errors_are_discarded_after_validation() {
    for error in ["", "PRIVATE-CANARY/home/config\n\u{1b}\u{202e}", " "] {
        let mut value = one();
        value["plugins"][0]["error"] = json!(error);
        let result = render(value).unwrap();
        assert!(result.ends_with(if error.is_empty() {
            "load error=no\n"
        } else {
            "load error=reported\n"
        }));
        assert!(!result.contains("PRIVATE-CANARY"));
    }
    let mut value = one();
    value["plugins"][0]["error"] = json!("PRIVATE-CANARY".repeat(2000));
    refuses(value);
}

#[test]
fn duplicate_keys_refuse_but_plugin_display_names_may_match() {
    let mut value = one();
    value["plugins"] = json!([plugin("a"), plugin("b")]);
    assert!(render(value.clone()).is_ok());
    value["plugins"][1]["key"] = json!("a");
    value["plugins"][1]["name"] = json!("different");
    refuses(value);
    let mut value = empty();
    value["toolsets"] = json!([toolset("k"), toolset("k")]);
    refuses(value);
    let mut value = empty();
    value["portable_mcp_servers"] = json!(["m", "m"]);
    refuses(value);
}

#[test]
fn each_array_accepts_128_rows_and_rejects_129_without_truncation() {
    for field in ["plugins", "toolsets", "portable_mcp_servers"] {
        let rows: Vec<_> = (0..129)
            .map(|i| match field {
                "plugins" => plugin(&format!("p{i}")),
                "toolsets" => toolset(&format!("t{i}")),
                _ => json!(format!("m{i}")),
            })
            .collect();
        let mut value = empty();
        value[field] = json!(&rows[..128]);
        assert!(render(value).is_ok(), "{field} at 128");
        let mut value = empty();
        value[field] = json!(rows);
        refuses(value);
    }
}

#[test]
fn free_text_is_sanitized_without_forging_unattributed_lines() {
    let mut value = one();
    value["plugins"][0]["name"] = json!("日本語");
    value["toolsets"] = json!([{"key":"k","label":"中文\u{1b}\u{7}\u{202e}\nPlugin: forged","description":"שלום\r\u{200f}\nnext\tline"}]);
    value["portable_mcp_servers"] = json!(["שרת"]);
    let result = render(value).unwrap();
    assert!(result.contains(
        "Toolset: k · 中文\\nPlugin: forged\n  Description: שלום\n  Description: next\tline\n"
    ));
    assert!(result.contains("Portable MCP: שרת\n"));
    for c in ['\u{1b}', '\u{7}', '\u{202e}', '\u{200f}', '\r'] {
        assert!(!result.contains(c));
    }
    assert!(!result.contains("\nPlugin: forged"));
}

#[test]
fn complete_output_includes_all_punctuation_at_the_inclusive_64k_boundary() {
    let mut value = empty();
    value["toolsets"] = json!(
        (0..64)
            .map(|i| json!({"key":format!("t{i}"),"label":"","description":""}))
            .collect::<Vec<_>>()
    );
    let mut remaining = 65_536 - render(value.clone()).unwrap().len();
    let mut last = 0;
    for i in 0..64 {
        let count = remaining.min(1024);
        value["toolsets"][i]["description"] = json!("x".repeat(count));
        remaining -= count;
        if count > 0 {
            last = i;
        }
    }
    assert_eq!(remaining, 0);
    let result = render(value.clone()).unwrap();
    assert_eq!(result.len(), 65_536);
    assert!(result.contains("Toolset: t63 · \n"));
    let text = value["toolsets"][last]["description"].as_str().unwrap();
    value["toolsets"][last]["description"] = json!(format!("{text}x"));
    refuses(value);
}

#[test]
fn multiline_projection_expansion_is_bounded_and_never_truncated() {
    let mut value = empty();
    value["toolsets"] = json!(
        (0..8)
            .map(|i| json!({"key":format!("t{i}"),"label":"","description":"\n".repeat(1024)}))
            .collect::<Vec<_>>()
    );
    refuses(value);
}
