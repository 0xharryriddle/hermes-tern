//! Native Tern projection. Never embeds a shell command or an approval decision.
use crate::{
    composer::Composer,
    transcript::{self, Transcript},
};
use hermes_tern_core::error::{Error, Result};
use hermes_tern_gateway::session::SessionState;
use std::time::Instant;
use tern_sdk::{
    nodes,
    ui::{self, View},
};
pub const STYLE: &str = ".ht-button{padding:4px 10px;margin:4px 8px 4px 0;border-radius:3px;background:var(--l2)} .ht-controls{display:flex;gap:8px;align-items:center} .ht-keyboard-focus{outline:2px dashed var(--accent);outline-offset:2px} .ht-keyboard-ready{outline-style:solid}";
#[derive(Clone, Debug)]
pub enum UiAction {
    Send(String),
    SubmitDraft,
    Focus,
    /// Keyboard Ctrl+C intentionally addresses the currently running turn.
    Interrupt,
    /// Pointer controls carry the generation the user actually saw.
    InterruptTurn {
        generation: u64,
    },
    Exit,
    /// Local retained-text inspection, never an upstream action or approval.
    InspectText {
        id: String,
        control_id: String,
        expand: bool,
    },
    Approve {
        id: String,
        control_id: String,
        generation: u64,
        choice: String,
    },
    Clarify {
        id: String,
        control_id: String,
        generation: u64,
    },
    Skip {
        id: String,
        control_id: String,
        generation: u64,
    },
}
impl UiAction {
    /// A decoded native action belongs to the exact prompt instance rendered.
    /// ID/generation alone cannot distinguish a same-generation replacement.
    pub fn validate_control(&self, state: &SessionState) -> Result<()> {
        if let Self::InspectText { id, control_id, .. } = self {
            return transcript::validate_row(state, id, control_id);
        }
        if let Self::InterruptTurn { generation } = self {
            return if state.connected
                && !state.is_suspended()
                && state.running
                && state.generation == *generation
            {
                Ok(())
            } else {
                Err(Error::Protocol("native turn changed; use current controls"))
            };
        }
        let (id, control_id, generation) = match self {
            Self::Approve {
                id,
                control_id,
                generation,
                ..
            }
            | Self::Clarify {
                id,
                control_id,
                generation,
            }
            | Self::Skip {
                id,
                control_id,
                generation,
            } => (id, control_id, generation),
            _ => return Ok(()),
        };
        if state.connected
            && !state.is_suspended()
            && state.generation == *generation
            && state.prompts.get(id).is_some_and(|p| {
                p.control_id == *control_id
                    && p.generation == *generation
                    && !p.answered
                    && p.expires > Instant::now()
            })
        {
            Ok(())
        } else {
            Err(Error::Protocol(
                "native prompt changed; use current controls",
            ))
        }
    }
}
pub fn prompt_actions(state: &SessionState) -> Vec<UiAction> {
    if !state.connected || state.is_suspended() {
        return vec![];
    }
    state
        .prompts
        .values()
        .filter(|p| !p.answered && p.expires > Instant::now())
        .flat_map(|p| {
            if p.kind == "approval" {
                p.choices
                    .iter()
                    .filter(|choice| {
                        matches!(choice.as_str(), "once" | "deny" | "session" | "always")
                    })
                    .map(|choice| UiAction::Approve {
                        id: p.id.clone(),
                        control_id: p.control_id.clone(),
                        generation: p.generation,
                        choice: choice.clone(),
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![
                    UiAction::Clarify {
                        id: p.id.clone(),
                        control_id: p.control_id.clone(),
                        generation: p.generation,
                    },
                    UiAction::Skip {
                        id: p.id.clone(),
                        control_id: p.control_id.clone(),
                        generation: p.generation,
                    },
                ]
            }
        })
        .collect()
}
pub fn render(state: &SessionState, composer: &Composer, notice: Option<&str>) -> View<UiAction> {
    render_with_transcript(state, composer, notice, &Transcript::default())
}
pub fn render_with_transcript(
    state: &SessionState,
    composer: &Composer,
    notice: Option<&str>,
    transcript: &Transcript,
) -> View<UiAction> {
    let mut main =
        ui::col().child(ui::text(format!("Hermes / {}", state.session_id)).key("heading"));
    main = main.child(
        ui::text(format!(
            "{} · {} · {}",
            state.info["model"]
                .as_str()
                .unwrap_or("runtime-selected model"),
            state.info["provider"]
                .as_str()
                .unwrap_or("runtime-selected provider"),
            state.status
        ))
        .key("state"),
    );
    if state.display_truncated {
        main = main.child(
            ui::text("Display retention limit reached; canonical history remains in Hermes.")
                .key("retention"),
        );
    }
    for m in &state.messages {
        let prefix = transcript::preview(&m.text);
        let foldable = prefix.len() < m.text.len();
        let expanded = foldable && transcript.is_expanded(&m.id, &m.control_id);
        let mut row = ui::col()
            .key(m.id.clone())
            .child(ui::text(m.role.clone()).key("role"));
        if foldable {
            row = row.child(
                ui::html::button(if expanded {
                    "Collapse retained text"
                } else {
                    "Show retained text"
                })
                .class(if transcript.is_selected(&m.id, &m.control_id) {
                    if transcript.keyboard_ready() {
                        "ht-button ht-transcript-toggle ht-keyboard-focus ht-keyboard-ready"
                    } else {
                        "ht-button ht-transcript-toggle ht-keyboard-focus"
                    }
                } else {
                    "ht-button ht-transcript-toggle"
                })
                .attr("aria-expanded", expanded)
                .key(format!(
                    "{}-{}",
                    if expanded { "collapse" } else { "show" },
                    m.control_id
                ))
                .on_click(UiAction::InspectText {
                    id: m.id.clone(),
                    control_id: m.control_id.clone(),
                    expand: !expanded,
                }),
            );
            row = row.child(
                ui::text(if expanded {
                    format!("Showing {} retained bytes; local view only.", m.text.len())
                } else {
                    format!(
                        "Preview: {} of {} retained bytes; expand to inspect.",
                        prefix.len(),
                        m.text.len()
                    )
                })
                .key("fold-state"),
            );
        }
        row = row.child(ui::text(if expanded { m.text.as_str() } else { prefix }).key("body"));
        if m.truncated {
            row = row.child(
                ui::text("Display text truncated at 64 KiB; full history remains in Hermes.")
                    .key("truncated"),
            );
        }
        main = main.child(row);
    }
    for p in state.prompts.values().filter(|p| !p.answered) {
        let mut panel = ui::col()
            // SDK routes update before a credit-blocked frame is visible. An old
            // node must never route to new content, even if upstream reuses IDs.
            .key(format!("prompt-{}", p.control_id))
            .child(
                ui::text(format!(
                    "Human {} · {} · generation {}",
                    p.kind, p.id, p.generation
                ))
                .key("label"),
            )
            .child(ui::text(p.text.clone()).key("body"));
        if let Some(questions) = p.questions.as_array() {
            for (n, q) in questions.iter().enumerate() {
                panel = panel.child(
                    ui::text(format!(
                        "{}: {}\nChoices: {}",
                        q["qid"].as_str().unwrap_or(""),
                        q["question"].as_str().unwrap_or(""),
                        q["choices"]
                    ))
                    .key(format!("q{n}")),
                );
            }
            panel = panel.child(
                ui::text(
                    "Type a JSON object keyed by question ID in the composer, then choose Answer.",
                )
                .key("hint"),
            );
        }
        for (n, a) in prompt_actions(state)
            .into_iter()
            .filter(|a| match a {
                UiAction::Approve { id, .. }
                | UiAction::Clarify { id, .. }
                | UiAction::Skip { id, .. } => id == &p.id,
                _ => false,
            })
            .enumerate()
        {
            let label = match &a {
                UiAction::Approve { choice, .. } => choice.as_str(),
                UiAction::Skip { .. } => "Skip",
                _ => "Answer from composer",
            };
            panel = panel.child(
                ui::html::button(label)
                    .class("ht-button")
                    .key(format!("choice{n}"))
                    .on_click(a),
            );
        }
        main = main.child(panel);
    }
    let mut controls = ui::html::div().class("ht-controls").key("controls");
    if state.can_submit() && !transcript.inspecting() {
        controls = controls.child(
            ui::html::button("Send")
                .class("ht-button")
                .key("send")
                .on_click(UiAction::SubmitDraft),
        );
    }
    if state.running && state.connected && !state.is_suspended() {
        controls = controls.child(
            ui::html::button("Interrupt turn")
                .class("ht-button")
                .key(format!("interrupt-{}", state.generation))
                .on_click(UiAction::InterruptTurn {
                    generation: state.generation,
                }),
        );
    }
    controls = controls.child(
        ui::html::button("Detach")
            .class("ht-button")
            .key("exit")
            .on_click(UiAction::Exit),
    );
    View::new().main(main).dock(nodes![
        ui::editor()
            .key("ed")
            .text(composer.text())
            .cursor(composer.utf16_cursor())
            .placeholder("Enter sends · Tab inspects · Shift+Enter newline · Ctrl+C interrupts · Ctrl+D detaches")
            .mode(if transcript.inspecting() { "inspect" } else { "insert" })
            .sendable(state.can_submit() && !transcript.inspecting())
            .on_send(|text| UiAction::Send(text.to_owned()))
            .on_focus(UiAction::Focus),
        controls,
        ui::text(
            notice.unwrap_or(if transcript.inspecting() {
                "Inspect retained text: Tab/Shift+Tab moves · Enter/Space toggles · Escape returns to draft."
            } else {
                "Approvals require an explicit click; peer/tool text grants no authority."
            })
        )
        .key("notice"),
    ])
}
