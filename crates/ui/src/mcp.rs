//! Explicit profile-wide MCP consent. Local dismissal never cancels admitted work.
use crate::{composer::Composer, view::UiAction};
use hermes_tern_core::text::display_text;
use hermes_tern_gateway::{
    mcp::{McpReload, Phase, SessionOutcome},
    session::SessionState,
};
use tern_sdk::{
    keys::Key,
    nodes,
    ui::{self, View},
};

#[derive(Default)]
pub struct McpPanel {
    open: bool,
    control_id: u64,
    exhausted: bool,
}
impl McpPanel {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn control_id(&self) -> u64 {
        self.control_id
    }
    pub fn accepts(&self, id: u64) -> bool {
        self.open && !self.exhausted && self.control_id == id
    }
    pub fn set_open(&mut self, open: bool) {
        if self.open != open {
            if let Some(next) = self.control_id.checked_add(1) {
                self.control_id = next;
            } else {
                self.exhausted = true;
            }
        }
        self.open = open && !self.exhausted;
    }
    pub fn key(&mut self, key: &Key) -> bool {
        if !self.open || key.is("ctrl+d") {
            return false;
        }
        if key.is("escape") || key.is("ctrl+c") {
            self.set_open(false);
        }
        true // Inspect-only: Enter, paste and typed text never become consent or a prompt.
    }
    pub fn render(
        &self,
        s: &SessionState,
        m: &McpReload,
        draft: &Composer,
        notice: Option<&str>,
        ready: bool,
    ) -> View<UiAction> {
        let mut body = ui::html::div().key("mcp").class("ht-runtime-panel ht-mcp-body")
            .child(ui::text("MCP reload · profile-wide operation").key("heading"))
            .child(ui::text(format!("Profile: {}", display_text(&s.profile))).key("profile"))
            .child(ui::text(format!("Owner: {} · Session: {}", display_text(&s.instance), display_text(&s.session_id))).key("owner"))
            .child(ui::text("Reload affects every chat; reattach can refresh policy, adoption and hooks.").key("consent"))
            .child(ui::text("Confirm affects every chat of this profile: reconcile MCP servers and cached agents. Server processes may start/stop or connect remotely. Then reattach this chat on the same connection; policy, adoption and hooks may refresh. Closing this panel does not cancel an admitted operation.").key("scope"))
            .child(ui::text("This is not an atomic desired-config/MCP-revision match, a server health check or a promise that concurrent profile changes cannot intervene. No automatic retry.").key("limits"))
            .child(ui::text(format!("Admitted reloads: {}/8 on this connection", m.attempts())).key("attempts"))
            .child(ui::text(m.notice).key("status"));
        if m.phase() == Phase::Reconciled {
            body = body.child(ui::text("MCP reattach readback complete").key("complete"));
        }
        if let Some(report) = m.report() {
            let session = match report.session {
                SessionOutcome::Applied => "applied (owner-reported)",
                SessionOutcome::Unchanged => "unchanged (chat policy only; not no profile effects)",
                SessionOutcome::Busy => "busy (profile may have changed; no automatic reload)",
            };
            body = body
                .child(ui::text(format!("Reload report · chat policy: {session}")).key("reported"));
            if !report.unavailable.is_empty() {
                body = body.child(
                    ui::text(format!(
                        "Reported unavailable: {}",
                        report.unavailable.join(", ")
                    ))
                    .key("unavailable"),
                );
            }
            if let Some(servers) = &report.session_servers {
                body = body.child(
                    ui::text(format!(
                        "Reported session servers: {}",
                        if servers.is_empty() {
                            "none".into()
                        } else {
                            servers.join(", ")
                        }
                    ))
                    .key("servers"),
                );
            }
        }
        body = body.child(ui::text("Owner reports are not proof of tool health. Reattach reads current owner state, not an exact reload receipt.").key("report-scope"));
        let mut controls = ui::html::div().key("controls").class("ht-controls");
        if ready && !self.exhausted && s.can_submit() {
            if m.phase() == Phase::Review {
                controls = controls.child(
                    ui::html::button("Confirm profile reload and reattach")
                        .key(format!("confirm-{}-{}", self.control_id, m.id()))
                        .class("ht-button ht-mcp-confirm")
                        .on_click(UiAction::ConfirmMcp {
                            panel: self.control_id,
                            control: m.id().into(),
                        }),
                );
            } else if !m.blocked() && m.attempts() < 8 {
                controls = controls.child(
                    ui::html::button("Review another reload")
                        .key(format!("review-{}-{}", self.control_id, m.id()))
                        .class("ht-button ht-mcp-review")
                        .on_click(UiAction::ReviewMcp {
                            panel: self.control_id,
                        }),
                );
            }
        }
        controls = controls
            .child(
                ui::html::button("Return to conversation")
                    .key("return")
                    .class("ht-button ht-mcp-return")
                    .on_click(UiAction::McpPanel(false)),
            )
            .child(
                ui::html::button("Detach")
                    .key("exit")
                    .class("ht-button")
                    .on_click(UiAction::Exit),
            );
        View::new().main(ui::col().child(body)).dock(nodes![
            ui::editor().key("ed").text(draft.text()).cursor(draft.utf16_cursor()).mode("inspect").sendable(false),
            controls,
            ui::text(notice.unwrap_or("MCP review: explicit pointer Confirm only · Escape returns to unchanged draft · Ctrl+D detaches.")).key("notice"),
        ])
    }
}
