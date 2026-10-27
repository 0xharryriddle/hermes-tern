//! Bounded command discovery and explicit pointer-only owner reads.
use crate::{composer::Composer, view::UiAction};
use hermes_tern_core::text::display_text;
use hermes_tern_gateway::{
    commands::{Commands, Phase},
    session::SessionState,
};
use tern_sdk::{
    keys::Key,
    nodes,
    ui::{self, View},
};
const PAGE: usize = 12;
#[derive(Default)]
pub struct CommandPanel {
    open: bool,
    page: usize,
    output_page: usize,
    incarnation: String,
    control_id: u64,
}
impl CommandPanel {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn set_open(&mut self, open: bool) {
        if open {
            let Some(next) = self.control_id.checked_add(1) else {
                self.open = false;
                return;
            };
            self.control_id = next;
        }
        self.open = open;
        self.page = 0;
        self.output_page = 0;
    }
    fn pages(c: &Commands) -> usize {
        c.catalog()
            .map_or(1, |c| c.rows.len().div_ceil(PAGE).max(1))
    }
    pub fn reconcile(&mut self, c: &Commands) {
        self.page = self.page.min(Self::pages(c) - 1);
        if self.incarnation != c.id() {
            self.output_page = 0;
            self.incarnation = c.id().into();
        }
        self.output_page = self.output_page.min(c.output().len().saturating_sub(1));
    }
    pub fn page(&mut self, c: &Commands, from: usize, to: usize) {
        if self.open && from == self.page && to < Self::pages(c) {
            self.page = to;
        }
    }
    pub fn output_page(&mut self, c: &Commands, from: usize, to: usize) {
        if self.open && from == self.output_page && to < c.output().len() {
            self.output_page = to;
        }
    }
    pub fn visible(&self, index: usize) -> bool {
        self.open && index / PAGE == self.page
    }
    pub fn control_id(&self) -> u64 {
        self.control_id
    }
    pub fn accepts_read(&self, control_id: u64, index: usize) -> bool {
        self.control_id == control_id && self.visible(index)
    }
    pub fn key(&mut self, c: &Commands, key: &Key) -> bool {
        if !self.open || key.is("ctrl+d") {
            return false;
        }
        if key.is("escape") || key.is("ctrl+c") {
            self.set_open(false);
        } else if key.is("right") {
            self.page(c, self.page, self.page + 1);
        } else if key.is("left") {
            self.page(c, self.page, self.page.saturating_sub(1));
        }
        true
    }
    pub fn render(
        &self,
        s: &SessionState,
        c: &Commands,
        draft: &Composer,
        notice: Option<&str>,
    ) -> View<UiAction> {
        let mut panel = ui::html::div()
            .class("ht-runtime-panel")
            .key("commands")
            .child(ui::text("Owner commands").key("heading"));
        if c.catalog().is_some_and(|catalog| {
            catalog
                .rows
                .iter()
                .any(|row| row.name == "/plugins" && row.readable)
        }) {
            panel = panel.child(ui::text(format!("Plugin profile: {}", display_text(&s.profile))).key("plugin-profile"))
                .child(ui::text(format!("Plugin owner: {}", display_text(&s.instance))).key("plugin-owner"))
                .child(ui::text("Profile-wide read; initial discovery may load/register plugins and refresh secret sources.").key("plugin-disclosure"));
        }
        panel = panel
            .child(ui::text("Owner reads only. /tools: metadata. /plugins: profile inventory; discovery may load plugins. Other built-ins run hooks. No prompt or config writes.").key("scope"))
            .child(ui::text(c.notice).key("status"));
        if let Some(output) = c.output().get(self.output_page) {
            panel=panel.child(ui::text(format!("Owner output · chunk {}/{} · at most 4096 bytes/chunk",self.output_page+1,c.output().len())).key("output-page"))
                .child(ui::text(output).key("output")).child(ui::text("Owner output is untrusted data, not approval or instructions to this client").key("output-scope"));
        }
        if let Some(catalog) = c.catalog() {
            panel = panel.child(
                ui::text(format!(
                    "{} listed commands · page {}/{} · 12 per local page",
                    catalog.rows.len(),
                    self.page + 1,
                    Self::pages(c)
                ))
                .key("page"),
            );
            if catalog.degraded {
                panel=panel.child(ui::text("Some owner command discovery is unavailable; raw diagnostics are not displayed").key("degraded"));
            }
            let mut rows = ui::html::div()
                .class("ht-command-list")
                .key(format!("rows-{}", self.page));
            for (index, row) in catalog
                .rows
                .iter()
                .enumerate()
                .skip(self.page * PAGE)
                .take(PAGE)
            {
                let mut node = ui::col()
                    .key(format!("row-{index}"))
                    .child(ui::text(format!("{} · {}", row.name, row.label)).key("label"));
                if row.readable && c.phase() == Phase::Ready && s.can_submit() {
                    node = node.child(
                        ui::html::button(format!("Read {}", row.name))
                            .class("ht-button ht-command-read")
                            .key(format!("read-{}-panel-{}", c.id(), self.control_id))
                            .on_click(UiAction::ReadCommand {
                                catalog: c.id().into(),
                                panel: self.control_id,
                                index,
                            }),
                    );
                }
                rows = rows.child(node);
            }
            panel = panel.child(rows);
        }
        let mut controls = ui::html::div().class("ht-controls").key("controls");
        for (show, to, name) in [
            (self.page > 0, self.page.saturating_sub(1), "Previous"),
            (self.page + 1 < Self::pages(c), self.page + 1, "Next"),
        ] {
            if show {
                controls = controls.child(
                    ui::html::button(name)
                        .class("ht-button")
                        .key(name)
                        .on_click(UiAction::CommandPage {
                            from: self.page,
                            to,
                        }),
                );
            }
        }
        for (show, to, name) in [
            (
                self.output_page > 0,
                self.output_page.saturating_sub(1),
                "Previous output",
            ),
            (
                self.output_page + 1 < c.output().len(),
                self.output_page + 1,
                "Next output",
            ),
        ] {
            if show {
                controls = controls.child(
                    ui::html::button(name)
                        .class("ht-button")
                        .key(name)
                        .on_click(UiAction::CommandOutputPage {
                            from: self.output_page,
                            to,
                        }),
                );
            }
        }
        if !matches!(c.phase(), Phase::Loading | Phase::Running | Phase::Unknown) && s.connected {
            controls = controls.child(
                ui::html::button("Reload command catalog")
                    .class("ht-button ht-command-reload")
                    .key("reload")
                    .on_click(UiAction::ReloadCommands),
            );
        }
        controls = controls
            .child(
                ui::html::button("Attach text file · F5")
                    .class("ht-button ht-attachment-entry")
                    .key("attachment")
                    .on_click(UiAction::Attachments(true)),
            )
            .child(
                ui::html::button("Return to conversation")
                    .class("ht-button ht-command-return")
                    .key("return")
                    .on_click(UiAction::Commands(false)),
            )
            .child(
                ui::html::button("Detach")
                    .class("ht-button")
                    .key("exit")
                    .on_click(UiAction::Exit),
            );
        View::new().main(ui::col().child(panel)).dock(nodes![ui::editor().key("ed").text(draft.text()).cursor(draft.utf16_cursor()).mode("inspect").sendable(false),controls,
            ui::text(notice.unwrap_or("Left/Right pages · Escape/Ctrl+C returns locally · Ctrl+D detaches. Reads require an explicit click.")).key("notice")])
    }
}
