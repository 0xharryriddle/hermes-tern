use hermes_tern_gateway::profiles::Catalog;
use serde_json::json;

#[test]
fn profile_inventory_keeps_exact_home_and_name_not_private_configuration() {
    let c = Catalog::parse(
        json!({"profiles":[
        {"name":"default","path":"/private/hermes","display_name":"Main","api_key":"PRIVATE-KEY"},
        {"name":"coder","path":"/private/hermes/profiles/coder","display_name":"Code\u{1b}"}],
        "install_id":"PRIVATE-INSTALL"}),
        "/private/hermes",
    )
    .unwrap();
    assert_eq!(c.rows.len(), 2);
    assert_eq!(
        c.rows[1].home.to_str(),
        Some("/private/hermes/profiles/coder")
    );
    assert_eq!(c.rows[1].name, "coder");
    assert!(!format!("{c:?}").contains("PRIVATE-"));
    assert!(!c.rows[1].label.contains('\u{1b}'));
}
#[test]
fn malformed_missing_current_duplicate_and_oversized_profiles_refuse_not_empty_success() {
    for value in [
        json!({}),
        json!({"profiles":[]}),
        json!({"profiles":[{"name":"Default","path":"/private/hermes"}]}),
        json!({"profiles":[{"name":"default","path":"relative"}]}),
        json!({"profiles":[{"name":"default","path":"/private/../hermes"}]}),
        json!({"profiles":[{"name":"default","path":"/private/hermes"},{"name":"default","path":"/other"}]}),
        json!({"profiles":[{"name":"default","path":"/private/hermes"},{"name":"other","path":"/private/hermes"}]}),
        json!({"profiles":[{"name":"default","path":"/private/hermes\n"}]}),
        json!({"profiles":[{"name":"other","path":"/other"}]}),
        json!({"profiles":(0..65).map(|n|json!({"name":format!("p{n}"),"path":format!("/p{n}")})).collect::<Vec<_>>()}),
    ] {
        assert!(Catalog::parse(value, "/private/hermes").is_err());
    }
}
