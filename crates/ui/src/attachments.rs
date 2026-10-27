//! Local file-path input, separate review/upload/add consent and no prompt submission.
use crate::{composer::Composer, view::UiAction};
use hermes_tern_core::{
    error::{Error, Result},
    text::is_bidi_control,
};
use hermes_tern_gateway::{
    attachments::{Attachments, Phase},
    session::SessionState,
};
use tern_sdk::{
    keys::Key,
    nodes,
    ui::{self, View},
};

#[derive(Default)]
pub struct AttachmentPanel {
    open: bool,
    path: Composer,
    control_id: u64,
}
impl AttachmentPanel {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn path(&self) -> &str {
        self.path.text()
    }
    pub fn control_id(&self) -> u64 {
        self.control_id
    }
    pub fn accepts(&self, id: u64) -> bool {
        self.open && self.control_id == id
    }
    fn rotate(&mut self) -> Result<()> {
        self.control_id = self.control_id.checked_add(1).ok_or_else(|| {
            self.open = false;
            Error::Protocol("attachment panel identity exhausted")
        })?;
        Ok(())
    }
    pub fn set_open(&mut self, open: bool) {
        if open && self.rotate().is_err() {
            return;
        }
        self.open = open;
    }
    pub fn key(&mut self, key: &Key) -> Result<bool> {
        if !self.open || key.is("ctrl+d") {
            return Ok(false);
        }
        if key.is("escape") || key.is("ctrl+c") {
            self.set_open(false);
            return Ok(true);
        }
        let before = self.path.text().to_owned();
        match key.name.as_str() {
            "enter" => {} // No review, stage, add or send on keyboard Enter.
            "backspace" => self.path.backspace(),
            "delete" => self.path.delete(),
            "left" => self.path.left(),
            "right" => self.path.right(),
            "home" => self.path.home(),
            "end" => self.path.end(),
            _ => {
                let text = if key.name == "paste" {
                    key.text.as_deref()
                } else {
                    key.typed()
                };
                if let Some(text) = text {
                    if self.path.text().len().saturating_add(text.len()) > 4096
                        || text.chars().any(|c| {
                            c.is_control()
                                || is_bidi_control(c)
                                || matches!(c, '\u{2028}' | '\u{2029}')
                        })
                    {
                        return Err(Error::Config(
                            "file path must be exact control-free text within 4096 bytes",
                        ));
                    }
                    self.path.insert(text)?;
                }
            }
        }
        if before != self.path.text() {
            self.rotate()?;
        }
        Ok(true)
    }
    pub fn render(
        &self,
        s: &SessionState,
        a: &Attachments,
        draft: &Composer,
        notice: Option<&str>,
        ready: bool,
    ) -> View<UiAction> {
        let mut panel = ui::html::div().class("ht-runtime-panel ht-attachment-panel").key("attachment")
            .child(ui::text("Attach a text file").key("heading"))
            .child(ui::text("Review one local UTF-8 text file, at most 256 KiB. Stage uploads reviewed bytes to this owner; Add reference is separate. Nothing sends automatically. Owner staging is retained; local dismissal does not delete it.").key("scope"))
            .child(ui::text(format!("Staging attempts: {}/8 for this connection",a.attempts())).key("count"))
            .child(ui::text(a.notice).key("status"));
        if let Some(file) = a.prepared() {
            panel = panel
                .child(
                    ui::text(format!("Reviewed: {} · {} bytes", file.name(), file.len()))
                        .key("review"),
                )
                .child(ui::text(file.path()).key("review-path"));
        }
        if let Some(receipt) = a.receipt() {
            panel = panel.child(
                ui::text(format!(
                    "Staged: {} · {}",
                    receipt.name(),
                    receipt.ref_path()
                ))
                .key("receipt"),
            );
        }
        panel = panel
            .child(ui::text("Conversation draft (not sent)").key("draft-label"))
            .child(ui::text(draft.text()).key("draft"));
        let mut controls = ui::html::div().class("ht-controls").key("controls");
        if ready && !a.blocked() && s.can_submit() {
            controls = controls.child(
                ui::html::button("Review local file")
                    .class("ht-button ht-attachment-review")
                    .key(format!("review-{}", self.control_id))
                    .on_click(UiAction::ReviewAttachment {
                        panel: self.control_id,
                    }),
            );
            if a.phase() == Phase::Prepared && a.attempts() < 8 {
                controls = controls.child(
                    ui::html::button("Stage reviewed bytes")
                        .class("ht-button ht-attachment-stage")
                        .key(format!("stage-{}-{}", self.control_id, a.id()))
                        .on_click(UiAction::StageAttachment {
                            panel: self.control_id,
                            prepared: a.id().into(),
                        }),
                );
            }
            if a.phase() == Phase::Staged {
                controls = controls.child(
                    ui::html::button("Add reference to draft")
                        .class("ht-button ht-attachment-add")
                        .key(format!("add-{}-{}", self.control_id, a.id()))
                        .on_click(UiAction::AddAttachment {
                            panel: self.control_id,
                            receipt: a.id().into(),
                        }),
                );
            }
        }
        controls = controls
            .child(
                ui::html::button("Return to conversation")
                    .class("ht-button ht-attachment-return")
                    .key("return")
                    .on_click(UiAction::Attachments(false)),
            )
            .child(
                ui::html::button("Detach")
                    .class("ht-button")
                    .key("exit")
                    .on_click(UiAction::Exit),
            );
        View::new().main(ui::col().child(panel)).dock(nodes![
            ui::editor().key("path").text(self.path.text()).cursor(self.path.utf16_cursor())
                .placeholder("Exact absolute local text-file path").mode("inspect").sendable(false),
            controls,
            ui::text(notice.unwrap_or("Enter does nothing · Review/Stage/Add require separate clicks · Escape/Ctrl+C returns · Ctrl+D detaches.")).key("notice")])
    }
}
