use hermes_tern_gateway::catalog::{Catalog, Scope};
use hermes_tern_ui::launcher::{Launcher, LauncherAction, Selection};
use serde_json::json;
use tern_sdk::keys::Key;

fn launcher() -> Launcher {
    let live = (0..13).map(|n| json!({"session_id":format!("opaque-{n}"),"title":"duplicate title","running":false})).collect::<Vec<_>>();
    let c = Catalog::from_responses(json!({"scope":"live","sessions":live}), json!({"scope":"stored","sessions":[{"session_id":"saved-exact","title":"duplicate title","running":false}]})).unwrap();
    Launcher::new(c, "/workspace with spaces".into())
}
#[test]
fn new_and_resume_require_explicit_selection_and_exact_identity() {
    let mut v = launcher();
    assert_eq!(v.apply(&LauncherAction::New), Some(Selection::New));
    assert_eq!(
        v.apply(&LauncherAction::Resume {
            scope: Scope::Live,
            index: 0,
            session_id: "opaque-0".into()
        }),
        Some(Selection::Resume("opaque-0".into()))
    );
    assert_eq!(
        v.apply(&LauncherAction::Resume {
            scope: Scope::Live,
            index: 0,
            session_id: "duplicate title".into()
        }),
        None
    );
    assert_eq!(v.apply(&LauncherAction::Close), Some(Selection::Close));
}
#[test]
fn paged_or_foreign_pointer_actions_cannot_resume_a_hidden_row() {
    let mut v = launcher();
    let old = LauncherAction::Resume {
        scope: Scope::Live,
        index: 0,
        session_id: "opaque-0".into(),
    };
    v.apply(&LauncherAction::Page {
        scope: Scope::Live,
        from: 0,
        to: 1,
    });
    assert_eq!(v.page(), 1);
    assert_eq!(v.apply(&old), None);
    v.apply(&LauncherAction::Scope(Scope::Stored));
    assert_eq!(v.page(), 0);
    assert_eq!(v.apply(&old), None);
    assert_eq!(
        v.apply(&LauncherAction::Resume {
            scope: Scope::Stored,
            index: 0,
            session_id: "saved-exact".into()
        }),
        Some(Selection::Resume("saved-exact".into()))
    );
}
#[test]
fn keyboard_navigation_never_activates_before_current_view_is_drawn() {
    let mut v = launcher();
    assert_eq!(v.key(&Key::named("enter"), None), None);
    v.key(&Key::named("down"), None);
    assert_eq!(v.key(&Key::named("enter"), Some(v.revision() - 1)), None);
    assert_eq!(
        v.key(&Key::named("enter"), Some(v.revision())),
        Some(Selection::Resume("opaque-0".into()))
    );
    assert_eq!(v.key(&Key::named("escape"), None), Some(Selection::Close));
}
#[test]
fn launcher_is_inert_text_and_exposes_new_cwd_and_inventory_limits() {
    let v = launcher();
    let view = v.render();
    fn texts<M>(node: &tern_sdk::ui::Node<M>) -> String {
        let mut text = node
            .props()
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        for child in node.child_nodes() {
            text.push_str(&texts(child));
        }
        text
    }
    let text = view
        .regions()
        .iter()
        .flatten()
        .map(|node| texts(node))
        .collect::<String>();
    assert!(text.contains("Hermes / Tern"));
    assert!(text.contains("/workspace with spaces"));
    assert!(text.contains("200"));
    assert!(text.contains("13 listed"));
    assert!(
        !text.contains("opaque-12"),
        "hidden second-page session rendered"
    );
    assert!(view.regions()[1].is_some());
}
