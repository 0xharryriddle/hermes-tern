//! Labelled UI fixture: exercises Tern rendering, not Hermes inference.
use hermes_tern_gateway::{protocol::parse_frame, session::SessionState};
use hermes_tern_ui::{
    composer::Composer,
    view::{UiAction, render},
};
use serde_json::json;
use std::time::{Duration, Instant};
use tern_sdk::{Options, Session, SurfaceOptions, wire::feature};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = SessionState::from_snapshot(
        "fixture-profile",
        "fixture-instance",
        json!({"session_id":"FIXTURE — not a live conversation","subscription_id":"fixture-sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"fixture-epoch","last_sequence":0,"running":true,"messages":[{"row_id":1,"role":"user","content":"Review the parser without modifying files."},{"row_id":2,"role":"assistant","content":"The input is validated before it reaches a command channel. Peer messages remain attributed data."}],"pending":[],"prompts":[],"info":{"model":"fixture-model","provider":"no inference"}}),
    )?;
    state.apply(parse_frame(&json!({"jsonrpc":"2.0","method":"event","params":{"type":"approval.request","session_id":state.session_id,"replay_epoch":"fixture-epoch","seq":1,"execution_generation":2,"payload":{"kind":"approval","prompt_id":"fixture-approval","execution_generation":2,"choices":["once","deny"],"command":"example tool operation","description":"Rendering fixture only; no command executes"}}}).to_string())?)?;
    let mut composer = Composer::default();
    composer.insert("Unicode draft: tiếng Việt 😀")?;
    let Some(mut session) = Session::<UiAction>::connect(
        Options::new()
            .app("hermes-tern-fixture")
            .feature(feature::SEND)
            .record(None),
    )?
    else {
        return Err("native TSP required".into());
    };
    let sf = session.open(SurfaceOptions::inline().title("Hermes rendering fixture"))?;
    session.stylesheet(sf, "hermes-tern", Some(hermes_tern_ui::view::STYLE))?;
    session.render(
        sf,
        render(
            &state,
            &composer,
            Some("FIXTURE ONLY — approval buttons cannot execute any action"),
        ),
    )?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        match session.next(Some(Duration::from_millis(50))) {
            Ok(_) => {}
            Err(tern_sdk::Error::InputClosed) => break,
            Err(e) => return Err(e.into()),
        }
    }
    session.close()?;
    Ok(())
}
