#[path = "../src/task_plan.rs"]
mod task_plan;

use hermes_tern_core::error::Result;
use serde_json::{Value, json};
use task_plan::{Catalog, Projection, Row};

fn row(id: &str) -> Value {
    json!({"id": id, "content": "Reported work", "status": "pending"})
}

fn plan(revision: u64, rows: Vec<Value>) -> Value {
    json!({"revision": revision, "todos": rows})
}

fn parse(rows: Vec<Value>) -> Result<Option<Catalog>> {
    Catalog::parse(&plan(1, rows))
}

fn catalog(rows: Vec<Value>) -> Catalog {
    parse(rows).unwrap().unwrap()
}

fn controls() -> impl Iterator<Item = char> {
    (0..=31).chain(127..=159).filter_map(char::from_u32).chain([
        '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
        '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ])
}

fn snapshot(projection: &Projection) -> String {
    projection.snapshot_id().to_owned()
}

fn assert_unavailable(projection: &Projection) {
    assert!(projection.catalog().is_none());
    assert!(projection.notice().contains("unavailable"));
}

#[test]
fn projects_exact_public_fields_in_order_as_complete_human_lines() {
    let catalog = catalog(vec![
        json!({"id": "子-عامل", "status": "in_progress", "content": "Review 日本語", "parent": "root"}),
        row("root"),
    ]);
    assert_eq!(catalog.revision, 1);
    assert_eq!(
        catalog.rows[0],
        Row {
            id: "子-عامل".to_owned(),
            parent: Some("root".to_owned()),
            status: "in_progress".to_owned(),
            text: "Task: 子-عامل\nStatus: in_progress\nParent: root\nReview 日本語".to_owned(),
        }
    );
    assert_eq!(
        catalog.rows[1].text,
        "Task: root\nStatus: pending\nParent: not reported\nReported work"
    );
    assert_eq!(catalog.clone(), catalog);
}

#[test]
fn null_and_unused_empty_revision_zero_are_absent_not_reported_clears() {
    assert_eq!(Catalog::parse(&Value::Null).unwrap(), None);
    assert_eq!(Catalog::parse(&plan(0, vec![])).unwrap(), None);
    assert_eq!(
        Catalog::parse(&json!({"revision": 0, "todos": [], "private": "ignored"})).unwrap(),
        None
    );
    assert!(Catalog::parse(&json!({"revision": 0})).is_err());
    assert!(Catalog::parse(&json!({"todos": []})).is_err());
}

#[test]
fn positive_empty_revision_is_a_reported_clear_and_nonempty_zero_is_valid() {
    for revision in [1, u64::MAX] {
        let parsed = Catalog::parse(&plan(revision, vec![])).unwrap().unwrap();
        assert_eq!(parsed.revision, revision);
        assert!(parsed.rows.is_empty());
    }
    assert_eq!(
        Catalog::parse(&plan(0, vec![row("zero")]))
            .unwrap()
            .unwrap()
            .revision,
        0
    );
}

#[test]
fn malformed_envelopes_are_never_successful_empty_plans() {
    for value in [
        json!({}),
        json!(false),
        json!(0),
        json!("plan"),
        json!([]),
        json!({"error": "unsupported"}),
        json!({"todo_state": {"revision": 1, "todos": []}}),
        json!({"revision": 1}),
        json!({"todos": []}),
    ] {
        assert!(Catalog::parse(&value).is_err(), "accepted {value:?}");
    }
    for value in [Value::Null, json!(false), json!(0), json!("[]"), json!({})] {
        assert!(Catalog::parse(&json!({"revision": 1, "todos": value})).is_err());
    }
}

#[test]
fn revision_is_strict_unsigned_json_integer_through_u64_max() {
    for revision in [0, 1, u64::MAX] {
        assert_eq!(
            Catalog::parse(&plan(revision, vec![row("a")]))
                .unwrap()
                .unwrap()
                .revision,
            revision
        );
    }
    let above: Value = serde_json::from_str("18446744073709551616").unwrap();
    let negative_zero: Value = serde_json::from_str("-0").unwrap();
    for revision in [
        Value::Null,
        json!(true),
        json!(false),
        json!(-1),
        json!(0.0),
        json!(1.0),
        json!(1.5),
        json!("1"),
        json!([]),
        json!({}),
        above,
        negative_zero,
    ] {
        assert!(Catalog::parse(&json!({"revision": revision, "todos": []})).is_err());
    }
}

#[test]
fn rows_require_objects_and_every_known_required_field() {
    for value in [
        Value::Null,
        json!(true),
        json!(0),
        json!("a"),
        json!([]),
        json!({}),
    ] {
        assert!(parse(vec![value]).is_err());
    }
    for field in ["id", "status", "content"] {
        let mut value = row("a");
        value.as_object_mut().unwrap().remove(field);
        assert!(parse(vec![value]).is_err(), "missing {field} accepted");
    }
}

#[test]
fn known_fields_never_coerce_wrong_json_types() {
    for field in ["id", "status", "content", "parent"] {
        for bad in [json!(false), json!(0), json!(1.0), json!([]), json!({})] {
            let mut value = row("a");
            value[field] = bad;
            assert!(
                parse(vec![value]).is_err(),
                "wrong type for {field} accepted"
            );
        }
        if field != "parent" {
            let mut value = row("a");
            value[field] = Value::Null;
            assert!(parse(vec![value]).is_err());
        }
    }
}

#[test]
fn identities_are_exact_unicode_without_space_or_normalization_repair() {
    let ids = [
        "é",
        "e\u{301}",
        "عامل",
        " child",
        "child ",
        r"child\u{202e}",
    ];
    let parsed = catalog(ids.iter().map(|id| row(id)).collect());
    assert_eq!(
        parsed
            .rows
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn identities_must_be_nonblank() {
    for id in ["", " ", "\t", "\n", "\u{2003}", " \u{3000} "] {
        assert!(parse(vec![row(id)]).is_err(), "blank identity accepted");
    }
}

#[test]
fn identities_fit_exactly_256_utf8_bytes_but_not_one_more() {
    for id in ["a".repeat(256), "é".repeat(128), "🦀".repeat(64)] {
        assert_eq!(catalog(vec![row(&id)]).rows[0].id, id);
        assert!(parse(vec![row(&format!("{id}x"))]).is_err());
    }
}

#[test]
fn identities_and_parents_refuse_every_c0_c1_and_bidi_control() {
    for character in controls() {
        let unsafe_id = format!("a{character}");
        assert!(parse(vec![row(&unsafe_id)]).is_err());
        let mut child = row("child");
        child["parent"] = json!(unsafe_id);
        assert!(parse(vec![row("a"), child]).is_err());
    }
    let mut child = row("child");
    child["parent"] = json!("a");
    assert_eq!(catalog(vec![row("a"), child]).rows.len(), 2);
}

#[test]
fn duplicate_identity_refuses_the_entire_plan() {
    assert!(parse(vec![row("a"), row("b"), row("a")]).is_err());
    assert_eq!(catalog(vec![row("a"), row("b")]).rows.len(), 2);
}

#[test]
fn exactly_256_unique_rows_fit_but_a_257th_is_not_truncated() {
    let mut rows: Vec<_> = (0..256).map(|n| row(&format!("task-{n}"))).collect();
    let parsed = catalog(rows.clone());
    assert_eq!(parsed.rows.len(), 256);
    assert_eq!(parsed.rows[255].id, "task-255");
    rows.push(row("task-256"));
    assert!(parse(rows).is_err());
}

#[test]
fn four_exact_statuses_are_reports_and_unknown_statuses_are_rejected() {
    for status in ["pending", "in_progress", "completed", "cancelled"] {
        let mut value = row("a");
        value["status"] = json!(status);
        assert_eq!(catalog(vec![value]).rows[0].status, status);
    }
    for status in [
        "",
        "done",
        "canceled",
        "Pending",
        "completed ",
        " in_progress",
        "pending\n",
    ] {
        let mut value = row("a");
        value["status"] = json!(status);
        assert!(parse(vec![value]).is_err());
    }
}

#[test]
fn content_requires_nonblank_text_without_trimming_valid_content() {
    for content in ["", " ", "\n\t", "\u{3000}"] {
        assert!(
            parse(vec![
                json!({"id": "a", "status": "pending", "content": content})
            ])
            .is_err()
        );
    }
    let parsed = catalog(vec![
        json!({"id": "a", "status": "pending", "content": "  work  "}),
    ]);
    assert!(parsed.rows[0].text.ends_with("\n  work  "));
}

#[test]
fn content_accepts_4000_unicode_characters_and_16000_utf8_bytes_without_truncation() {
    for content in ["a".repeat(4000), "é".repeat(4000), "🦀".repeat(4000)] {
        let mut value = row("a");
        value["content"] = json!(content);
        let parsed = catalog(vec![value.clone()]);
        assert!(parsed.rows[0].text.ends_with(&content));
        value["content"] = json!(format!("{content}x"));
        assert!(parse(vec![value]).is_err());
    }
}

#[test]
fn metadata_controls_and_bidi_are_visibly_escaped_without_losing_unicode() {
    for character in controls() {
        let content = format!("عé{character}tail");
        let parsed = catalog(vec![
            json!({"id": "a", "status": "pending", "content": content}),
        ]);
        let escaped: String = character.escape_default().collect();
        assert!(parsed.rows[0].text.ends_with(&format!("عé{escaped}tail")));
        assert_eq!(parsed.rows[0].text.matches('\n').count(), 3);
        assert!(
            !parsed.rows[0].text.chars().any(
                |c| (c.is_control() && c != '\n') || hermes_tern_core::text::is_bidi_control(c)
            )
        );
    }
}

#[test]
fn private_fields_are_omitted_and_consume_no_display_budget_or_error_output() {
    let canary = "PRIVATE_CONFIG_ROUTE_TRANSCRIPT_PROCESS";
    let private = canary.repeat(8192);
    let plain = catalog(vec![row("a")]);
    let value = json!({"revision": 1, "todos": [{"id": "a", "status": "pending", "content": "Reported work",
        "private": private, "config": {"route": canary}, "process": [canary], "transcript": canary}], "private": private});
    let parsed = Catalog::parse(&value).unwrap().unwrap();
    assert_eq!(parsed, plain);
    assert!(!format!("{parsed:?}").contains(canary));
    let error = parse(vec![
        json!({"id": canary, "content": false, "status": "pending"}),
    ])
    .unwrap_err();
    assert!(!format!("{error:?}: {error}").contains(canary));
}

#[test]
fn missing_and_null_parent_are_equivalent_explicit_not_reported_labels() {
    let mut value = row("a");
    value["parent"] = Value::Null;
    assert_eq!(catalog(vec![value]), catalog(vec![row("a")]));
    assert_eq!(catalog(vec![row("a")]).rows[0].parent, None);
}

#[test]
fn parent_has_the_same_exact_256_byte_identity_bound() {
    for parent in ["a".repeat(256), "é".repeat(128), "🦀".repeat(64)] {
        let mut child = row("child");
        child["parent"] = json!(parent);
        let parsed = catalog(vec![row(&parent), child.clone()]);
        assert_eq!(parsed.rows[1].parent.as_deref(), Some(parent.as_str()));
        child["parent"] = json!(format!("{parent}x"));
        assert!(parse(vec![row(&parent), child]).is_err());
    }
    for parent in ["", " ", "\u{3000}"] {
        let mut child = row("child");
        child["parent"] = json!(parent);
        assert!(parse(vec![child]).is_err());
    }
}

#[test]
fn parent_references_must_exist_with_exact_identity_not_trimmed_or_normalized() {
    for (parent, target) in [("a", " a"), ("é", "e\u{301}"), ("missing", "root")] {
        let mut child = row("child");
        child["parent"] = json!(parent);
        assert!(parse(vec![row(target), child.clone()]).is_err());
        child["parent"] = json!(target);
        assert_eq!(catalog(vec![row(target), child]).rows.len(), 2);
    }
}

#[test]
fn forward_parent_references_preserve_reported_order_without_tree_reconstruction() {
    let parsed = catalog(vec![
        json!({"id": "leaf", "status": "pending", "content": "leaf", "parent": "middle"}),
        row("root"),
        json!({"id": "middle", "status": "pending", "content": "middle", "parent": "root"}),
    ]);
    assert_eq!(
        parsed
            .rows
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["leaf", "root", "middle"]
    );
}

#[test]
fn self_and_disconnected_multirow_cycles_refuse_the_whole_plan() {
    for rows in [
        vec![json!({"id": "a", "status": "pending", "content": "work", "parent": "a"})],
        vec![
            row("root"),
            json!({"id": "a", "status": "pending", "content": "work", "parent": "b"}),
            json!({"id": "b", "status": "pending", "content": "work", "parent": "c"}),
            json!({"id": "c", "status": "pending", "content": "work", "parent": "a"}),
        ],
    ] {
        assert!(parse(rows).is_err());
    }
}

#[test]
fn maximal_acyclic_chain_fits_but_closing_it_into_a_cycle_refuses() {
    let mut rows: Vec<_> = (0..256)
        .map(|n| {
            let mut value = row(&format!("task-{n}"));
            if n != 0 {
                value["parent"] = json!(format!("task-{}", n - 1));
            }
            value
        })
        .collect();
    assert_eq!(catalog(rows.clone()).rows.len(), 256);
    rows[0]["parent"] = json!("task-255");
    assert!(parse(rows).is_err());
}

#[test]
fn complete_escaped_text_including_metadata_fits_exactly_65536_bytes_not_one_more() {
    let mut rows: Vec<_> = (0..64)
        .map(|n| {
            let mut value =
                json!({"id": format!("task-{n}-β"), "status": "in_progress", "content": "x"});
            if n != 0 {
                value["parent"] = json!("task-0-β");
            }
            value
        })
        .collect();
    let baseline: usize = catalog(rows.clone())
        .rows
        .iter()
        .map(|entry| entry.text.len())
        .sum();
    let mut remaining = 65_536usize.checked_sub(baseline).unwrap();
    for entry in &mut rows {
        let extra = remaining.min(3999);
        entry["content"] = json!(format!(
            "x{}{}",
            "é".repeat(extra / 2),
            "x".repeat(extra % 2)
        ));
        remaining -= extra;
    }
    assert_eq!(remaining, 0, "fixture cannot reach boundary");
    let parsed = catalog(rows.clone());
    assert_eq!(parsed.rows.len(), 64);
    assert_eq!(
        parsed
            .rows
            .iter()
            .map(|entry| entry.text.len())
            .sum::<usize>(),
        65_536
    );
    let entry = rows
        .iter_mut()
        .find(|entry| entry["content"].as_str().unwrap().chars().count() < 4000)
        .unwrap();
    entry["content"] = json!(format!("{}x", entry["content"].as_str().unwrap()));
    assert!(parse(rows).is_err());
}

#[test]
fn escape_expansion_counts_against_render_budget_even_when_raw_content_fits() {
    let rows: Vec<_> = (0..4).map(|n| json!({"id": format!("task-{n}"), "status": "pending", "content": "\u{1}".repeat(4000)})).collect();
    assert!(
        rows.iter()
            .map(|entry| entry["content"].as_str().unwrap().len())
            .sum::<usize>()
            < 65_536
    );
    assert!(parse(rows.clone()).is_err());
    assert_eq!(catalog(rows.into_iter().take(1).collect()).rows.len(), 1);
}

#[test]
fn malformed_final_row_never_returns_a_valid_prefix() {
    assert!(
        parse(vec![
            row("valid"),
            json!({"id": "invalid", "status": "pending"})
        ])
        .is_err()
    );
}

#[test]
fn default_projection_is_unavailable_and_unchanged_absence_never_mints_a_token() {
    let mut projection = Projection::default();
    assert_unavailable(&projection);
    let before = snapshot(&projection);
    projection.update(&Value::Null);
    projection.update(&plan(0, vec![]));
    projection.invalidate();
    assert_eq!(snapshot(&projection), before);
    assert_eq!(projection.clone().snapshot_id(), before);
}

#[test]
fn increasing_revisions_replace_with_new_v4_tokens_and_lower_valid_or_malformed_are_ignored() {
    let mut projection = Projection::default();
    projection.update(&plan(4, vec![row("a")]));
    let first = snapshot(&projection);
    assert_eq!(uuid::Uuid::parse_str(&first).unwrap().get_version_num(), 4);
    projection.update(&plan(3, vec![row("older")]));
    projection.update(&json!({"revision": 2, "todos": false}));
    assert_eq!(snapshot(&projection), first);
    assert_eq!(projection.catalog().unwrap().rows[0].id, "a");
    projection.update(&plan(5, vec![row("b")]));
    assert_ne!(snapshot(&projection), first);
    assert_eq!(projection.catalog().unwrap().revision, 5);
    assert_eq!(projection.catalog().unwrap().rows[0].id, "b");
}

#[test]
fn equal_known_projection_is_idempotent_ignoring_private_fields_and_null_parent() {
    let mut projection = Projection::default();
    projection.update(&plan(4, vec![row("a")]));
    let before = snapshot(&projection);
    let mut value = plan(4, vec![row("a")]);
    value["private"] = json!("private".repeat(20_000));
    value["todos"][0]["parent"] = Value::Null;
    value["todos"][0]["transcript"] = json!({"secret": "never retained"});
    projection.update(&value);
    assert_eq!(snapshot(&projection), before);
    assert_eq!(
        projection.catalog().unwrap(),
        &catalog(vec![row("a")]).with_revision(4)
    );
}

trait RevisionFixture {
    fn with_revision(self, revision: u64) -> Self;
}

impl RevisionFixture for Catalog {
    fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }
}

#[test]
fn equal_revision_changes_to_any_known_field_or_row_order_invalidate() {
    let original = plan(
        4,
        vec![
            row("a"),
            json!({"id": "b", "content": "Reported work", "status": "pending", "parent": "a"}),
        ],
    );
    for field in ["id", "status", "content", "parent", "order"] {
        let mut changed = original.clone();
        match field {
            "id" => {
                changed["todos"][0]["id"] = json!("c");
                changed["todos"][1]["parent"] = json!("c");
            }
            "status" => changed["todos"][0]["status"] = json!("completed"),
            "content" => changed["todos"][0]["content"] = json!("Other work"),
            "parent" => changed["todos"][1]["parent"] = Value::Null,
            "order" => changed["todos"].as_array_mut().unwrap().reverse(),
            _ => unreachable!(),
        }
        assert!(Catalog::parse(&changed).is_ok());
        let mut projection = Projection::default();
        projection.update(&original);
        let before = snapshot(&projection);
        projection.update(&changed);
        assert_unavailable(&projection);
        assert_ne!(snapshot(&projection), before, "no invalidation for {field}");
    }
}

#[test]
fn equal_conflict_latches_unavailable_until_strictly_higher_revision() {
    let mut projection = Projection::default();
    let original = plan(5, vec![row("a")]);
    projection.update(&original);
    projection.update(&plan(5, vec![row("conflict")]));
    assert_unavailable(&projection);
    let unavailable = snapshot(&projection);
    for value in [
        original,
        plan(5, vec![row("conflict")]),
        plan(4, vec![row("older")]),
        json!({"revision": 3, "todos": false}),
    ] {
        projection.update(&value);
        assert_unavailable(&projection);
        assert_eq!(snapshot(&projection), unavailable);
    }
    projection.update(&plan(6, vec![row("newer")]));
    assert_eq!(projection.catalog().unwrap().rows[0].id, "newer");
    assert_ne!(snapshot(&projection), unavailable);
}

#[test]
fn malformed_typed_revision_advances_watermark_and_never_keeps_old_rows() {
    for revision in [5, 6, u64::MAX] {
        let mut projection = Projection::default();
        projection.update(&plan(5, vec![row("a")]));
        let before = snapshot(&projection);
        projection.update(&json!({"revision": revision, "todos": false}));
        assert_unavailable(&projection);
        assert_ne!(snapshot(&projection), before);
        let unavailable = snapshot(&projection);
        projection.update(&plan(revision, vec![row("same")]));
        projection.update(&plan(4, vec![row("lower")]));
        assert_unavailable(&projection);
        assert_eq!(snapshot(&projection), unavailable);
        if let Some(next) = revision.checked_add(1) {
            projection.update(&plan(next, vec![row("recovered")]));
            assert_eq!(projection.catalog().unwrap().rows[0].id, "recovered");
        }
    }
}

#[test]
fn malformed_untyped_revision_clears_rows_without_erasing_the_watermark() {
    for value in [
        json!({}),
        json!(false),
        json!({"revision": null}),
        json!({"revision": 5.0, "todos": []}),
        json!({"revision": "5", "todos": []}),
    ] {
        let mut projection = Projection::default();
        projection.update(&plan(5, vec![row("a")]));
        projection.update(&value);
        assert_unavailable(&projection);
        let unavailable = snapshot(&projection);
        projection.update(&plan(4, vec![row("older")]));
        projection.update(&plan(5, vec![row("same")]));
        assert_unavailable(&projection);
        assert_eq!(snapshot(&projection), unavailable);
        projection.update(&plan(6, vec![row("newer")]));
        assert_eq!(projection.catalog().unwrap().revision, 6);
    }
}

#[test]
fn null_or_unused_zero_make_current_plan_unavailable_but_preserve_watermark() {
    for absence in [Value::Null, plan(0, vec![])] {
        let mut projection = Projection::default();
        projection.update(&plan(5, vec![row("a")]));
        projection.update(&absence);
        assert_unavailable(&projection);
        let unavailable = snapshot(&projection);
        projection.update(&plan(4, vec![row("older")]));
        projection.update(&plan(5, vec![row("same")]));
        assert_unavailable(&projection);
        assert_eq!(snapshot(&projection), unavailable);
        projection.update(&plan(6, vec![row("newer")]));
        assert_eq!(projection.catalog().unwrap().revision, 6);
    }
}

#[test]
fn explicit_empty_clear_is_available_and_blocks_older_nonempty_snapshots() {
    let mut projection = Projection::default();
    let absent_notice = projection.notice();
    projection.update(&plan(4, vec![row("a")]));
    projection.update(&plan(5, vec![]));
    assert!(projection.catalog().unwrap().rows.is_empty());
    assert_ne!(projection.notice(), absent_notice);
    assert!(projection.notice().contains("empty"));
    let cleared = snapshot(&projection);
    projection.update(&plan(4, vec![row("older")]));
    projection.update(&plan(5, vec![]));
    assert_eq!(snapshot(&projection), cleared);
    assert!(projection.catalog().unwrap().rows.is_empty());
    projection.update(&Value::Null);
    assert_unavailable(&projection);
    assert_ne!(snapshot(&projection), cleared);
}

#[test]
fn repeated_invalid_state_does_not_rotate_tokens_but_new_watermarks_do() {
    let mut projection = Projection::default();
    projection.update(&json!({"revision": 7, "todos": false}));
    let invalid = snapshot(&projection);
    assert_eq!(
        uuid::Uuid::parse_str(&invalid).unwrap().get_version_num(),
        4
    );
    for value in [
        json!({"revision": 7, "todos": false}),
        json!({}),
        Value::Null,
        plan(0, vec![]),
    ] {
        projection.update(&value);
        assert_eq!(snapshot(&projection), invalid);
    }
    projection.update(&json!({"revision": 8, "todos": false}));
    assert_unavailable(&projection);
    assert_ne!(snapshot(&projection), invalid);
}

#[test]
fn invalidation_clears_rows_and_watermark_allowing_any_new_scope_revision() {
    for bad in [false, true] {
        let mut projection = Projection::default();
        projection.update(&plan(u64::MAX, vec![row("a")]));
        if bad {
            projection.update(&plan(u64::MAX, vec![row("conflict")]));
        }
        let before = snapshot(&projection);
        projection.invalidate();
        assert_unavailable(&projection);
        assert_ne!(snapshot(&projection), before);
        let invalidated = snapshot(&projection);
        projection.invalidate();
        assert_eq!(snapshot(&projection), invalidated);
        projection.update(&plan(0, vec![row("new-scope")]));
        assert_eq!(projection.catalog().unwrap().revision, 0);
        assert_eq!(projection.catalog().unwrap().rows[0].id, "new-scope");
    }
}

#[test]
fn unreported_zero_does_not_establish_a_watermark_that_conflicts_with_nonempty_zero() {
    let mut projection = Projection::default();
    projection.update(&plan(0, vec![]));
    projection.update(&plan(0, vec![row("first-reported")]));
    assert_eq!(projection.catalog().unwrap().rows[0].id, "first-reported");
}

#[test]
fn literal_escape_text_cannot_alias_a_control_for_equal_revision_comparison() {
    for character in controls() {
        let escaped: String = character.escape_default().collect();
        let mut original = plan(7, vec![row("a")]);
        original["todos"][0]["content"] = json!(format!("work{character}tail"));
        let mut literal = original.clone();
        literal["todos"][0]["content"] = json!(format!("work{escaped}tail"));
        assert_ne!(
            Catalog::parse(&original).unwrap(),
            Catalog::parse(&literal).unwrap()
        );
        let mut projection = Projection::default();
        projection.update(&original);
        projection.update(&literal);
        assert_unavailable(&projection);
    }
}

#[test]
fn initial_untyped_malformed_input_preserves_the_absence_of_a_watermark() {
    let mut projection = Projection::default();
    let before = snapshot(&projection);
    projection.update(&json!({"revision": "0", "todos": false}));
    assert_unavailable(&projection);
    assert_eq!(snapshot(&projection), before);
    projection.update(&plan(0, vec![row("zero")]));
    assert_eq!(projection.catalog().unwrap().revision, 0);
}

#[test]
fn malformed_typed_zero_establishes_a_watermark_unlike_unreported_empty_zero() {
    let mut projection = Projection::default();
    projection.update(&json!({"revision": 0, "todos": false}));
    assert_unavailable(&projection);
    let invalid = snapshot(&projection);
    projection.update(&plan(0, vec![]));
    projection.update(&plan(0, vec![row("same")]));
    assert_unavailable(&projection);
    assert_eq!(snapshot(&projection), invalid);
    projection.update(&plan(1, vec![row("newer")]));
    assert_eq!(projection.catalog().unwrap().rows[0].id, "newer");
}
