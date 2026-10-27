use hermes_tern_gateway::profiles::Catalog;
use hermes_tern_ui::profile_picker::{Action, Choice, Picker};
use serde_json::json;
use tern_sdk::keys::Key;

fn picker() -> Picker {
    let rows=(0..14).map(|n|json!({"name":format!("p{n}"),"path":format!("/profiles/p{n}"),"display_name":"Same label"})).collect::<Vec<_>>();
    Picker::new(
        Some(Catalog::parse(json!({"profiles":rows}), "/profiles/p0").unwrap()),
        "/profiles/p0".into(),
        None,
    )
}
#[test]
fn explicit_profile_selection_is_exact_visible_and_keyboard_positioning_ack_fenced() {
    let mut p = picker();
    assert_eq!(p.key(&Key::named("enter"), None), None);
    assert_eq!(
        p.key(&Key::named("enter"), Some(p.revision())),
        Some(Choice::Home("/profiles/p0".into()))
    );
    assert_eq!(
        p.apply(&Action::Select {
            index: 0,
            home: "/foreign".into()
        }),
        None
    );
    p.apply(&Action::Page { from: 0, to: 1 });
    assert_eq!(
        p.apply(&Action::Select {
            index: 0,
            home: "/profiles/p0".into()
        }),
        None
    );
    assert_eq!(
        p.apply(&Action::Select {
            index: 12,
            home: "/profiles/p12".into()
        }),
        Some(Choice::Home("/profiles/p12".into()))
    );
    assert_eq!(p.key(&Key::named("escape"), None), Some(Choice::Back));
    assert_eq!(
        p.key(
            &Key {
                ctrl: true,
                ..Key::named("d")
            },
            None
        ),
        Some(Choice::Close)
    );
}
#[test]
fn unavailable_profiles_are_named_not_empty_success_and_only_back_close_work() {
    let mut p = Picker::new(
        None,
        "/profiles/p0".into(),
        Some("Profile inventory unavailable".into()),
    );
    assert_eq!(p.key(&Key::named("enter"), Some(p.revision())), None);
    assert_eq!(
        p.apply(&Action::Select {
            index: 0,
            home: "/profiles/p0".into()
        }),
        None
    );
    fn text<M>(n: &tern_sdk::ui::Node<M>) -> String {
        let mut s = n
            .props()
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        for child in n.child_nodes() {
            s.push_str(&text(child));
        }
        s
    }
    let all = p
        .render()
        .regions()
        .iter()
        .flatten()
        .map(|node| text(node))
        .collect::<String>();
    assert!(all.contains("unavailable") && all.contains("/profiles/p0"));
    assert!(all.contains("Back") && all.contains("Close"));
}
