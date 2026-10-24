use hermes_tern_gateway::history::HistorySnapshot;
use hermes_tern_ui::history::{HistoryAction, HistoryView};
use serde_json::json;
use tern_sdk::keys::Key;

fn snapshot() -> HistorySnapshot {
    HistorySnapshot::from_response(json!({"count":100,"messages":(0..70).map(|n|json!({"text":format!("row {n}")})).collect::<Vec<_>>()})).unwrap()
}
#[test]
fn history_navigation_is_snapshot_local_and_old_pointer_actions_are_inert() {
    let data = snapshot();
    let mut view = HistoryView::new(data, 1).unwrap();
    assert_eq!(view.page(), 0);
    let next = HistoryAction::Page { from: 0, to: 1 };
    assert!(!view.apply(&next));
    assert_eq!(view.page(), 1);
    view.apply(&next);
    assert_eq!(view.page(), 1);
    view.apply(&HistoryAction::Page {
        from: 1,
        to: usize::MAX,
    });
    assert_eq!(view.page(), 1);
    let right = Key {
        name: "right".into(),
        text: None,
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
    assert!(!view.key(&right));
    assert_eq!(view.page(), 2);
    view.key(&right);
    assert_eq!(view.page(), 2);
    assert!(view.apply(&HistoryAction::Close));
    assert!(HistoryView::new(snapshot(), 0).is_err());
    assert!(HistoryView::new(snapshot(), 4).is_err());
}
#[test]
fn pinned_sdk_page_keys_navigate_and_modified_chords_are_inert() {
    let mut view = HistoryView::new(snapshot(), 1).unwrap();
    let mut decoder = tern_sdk::keys::Decoder::default();
    for (bytes, expected) in [(b"\x1b[6~".as_slice(), 1), (b"\x1b[5~".as_slice(), 0)] {
        let keys = decoder.feed(bytes);
        assert_eq!(keys.len(), 1);
        assert!(!view.key(&keys[0]));
        assert_eq!(view.page(), expected);
    }
    let mut key = Key::named("page_down");
    key.ctrl = true;
    assert!(!view.key(&key));
    assert_eq!(view.page(), 0);
}

#[test]
fn native_history_has_only_read_only_controls_and_plain_output_matches_page() {
    let view = HistoryView::new(snapshot(), 2).unwrap();
    let v = view.render();
    fn walk(
        node: &tern_sdk::ui::Node<HistoryAction>,
        texts: &mut Vec<String>,
        kinds: &mut Vec<String>,
    ) {
        if let Some(text) = node.props().get("text").and_then(|v| v.as_str()) {
            texts.push(text.to_owned());
        }
        kinds.push(node.kind().to_owned());
        for child in node.child_nodes() {
            walk(child, texts, kinds);
        }
    }
    let mut texts = vec![];
    let mut kinds = vec![];
    for region in v.regions().iter().flatten() {
        walk(region, &mut texts, &mut kinds);
    }
    assert!(!kinds.iter().any(|kind| kind == "editor"));
    let plain = view.plain().unwrap();
    assert!(plain.contains("Page 2/3"));
    assert!(plain.contains("100 source rows"));
    assert!(plain.contains("70 projected rows"));
    assert!(plain.contains("row 32"));
    assert!(!plain.contains("row 31\""));
    assert!(texts.iter().any(|text| text.contains("Page 2/3")));
    assert!(
        !texts
            .iter()
            .any(|text| matches!(text.as_str(), "Send" | "Interrupt turn" | "Approve"))
    );
}

#[test]
fn each_page_has_a_distinct_scroll_container_instead_of_following_chat_tail() {
    let mut view = HistoryView::new(snapshot(), 1).unwrap();
    let first = view.render();
    let scroll = &first.regions()[0].unwrap().child_nodes()[1];
    assert_eq!(scroll.kind(), "el");
    assert_eq!(scroll.props()["class"], "ht-history-scroll");
    let before = scroll.props()["key"].clone();
    view.apply(&HistoryAction::Page { from: 0, to: 1 });
    let next = view.render();
    assert_ne!(
        next.regions()[0].unwrap().child_nodes()[1].props()["key"],
        before
    );
}
