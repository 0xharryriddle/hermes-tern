//! Explicit session model picker. Navigation is local; Apply requires a pointer action.
use crate::{composer::Composer, view::UiAction};
use hermes_tern_core::text::display_text;
use hermes_tern_gateway::{
    models::{ModelControl, Phase},
    session::SessionState,
};
use tern_sdk::{
    keys::Key,
    nodes,
    ui::{self, View},
};
const PAGE: usize = 12;
#[derive(Default)]
pub struct ModelPicker {
    open: bool,
    page: usize,
}
fn label(text: &str) -> String {
    display_text(text)
}
impl ModelPicker {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        self.page = 0;
    }
    pub fn current_page(&self) -> usize {
        self.page
    }
    fn pages(models: &ModelControl) -> usize {
        models
            .catalog()
            .map(|c| c.rows.len().div_ceil(PAGE).max(1))
            .unwrap_or(1)
    }
    pub fn reconcile(&mut self, models: &ModelControl) {
        self.page = self.page.min(Self::pages(models) - 1);
    }
    pub fn page(&mut self, models: &ModelControl, from: usize, to: usize) {
        if self.open && from == self.page && to < Self::pages(models) {
            self.page = to;
        }
    }
    pub fn visible(&self, index: usize) -> bool {
        self.open && index / PAGE == self.page
    }
    pub fn key(&mut self, models: &ModelControl, key: &Key) -> bool {
        if !self.open || key.is("ctrl+d") {
            return false;
        }
        if key.is("escape") || key.is("ctrl+c") {
            self.set_open(false);
        } else if key.is("right") {
            self.page(models, self.page, self.page + 1);
        } else if key.is("left") {
            self.page(models, self.page, self.page.saturating_sub(1));
        }
        true
    }
    pub fn render(
        &self,
        s: &SessionState,
        models: &ModelControl,
        composer: &Composer,
        notice: Option<&str>,
    ) -> View<UiAction> {
        let mut content=ui::html::div().key("model-panel").class("ht-runtime-panel")
            .child(ui::text("Session model selection").key("heading"))
            .child(ui::text(format!("Active route: {} · {}",label(s.info["model"].as_str().unwrap_or("unavailable")),label(s.info["provider"].as_str().unwrap_or("unavailable")))).key("active"))
            .child(ui::text("Selection changes only this session; no prompt or profile config write. Inventory may warm provider catalogs; availability is not a successful inference check. Apply/Confirm requires an explicit click.").key("scope"))
            .child(ui::text(&models.notice).key("status"));
        if let Some(warning) = &models.warning {
            content = content.child(ui::text(warning).key("warning"));
        }
        if let Some((provider, model)) = models.selected() {
            content = content.child(
                ui::text(format!("Selected: {} · {}", label(model), label(provider)))
                    .key("selected"),
            );
        }
        if let Some((provider, model)) = models.resolved_confirmation() {
            content = content.child(
                ui::text(format!(
                    "Hermes resolved guarded target: {} · {}",
                    label(model),
                    label(provider)
                ))
                .key("resolved"),
            );
        }
        if let Some(catalog) = models.catalog() {
            content = content.child(
                ui::text(format!(
                    "{} listed models · page {}/{} · {} per local page",
                    catalog.rows.len(),
                    self.page + 1,
                    Self::pages(models),
                    PAGE
                ))
                .key("count"),
            );
            for (index, row) in catalog
                .rows
                .iter()
                .enumerate()
                .skip(self.page * PAGE)
                .take(PAGE)
            {
                let mut entry = ui::col().key(format!("entry-{index}")).child(
                    ui::text(format!(
                        "{} · {} · {}",
                        label(&row.model),
                        row.provider_name,
                        row.availability
                    ))
                    .key("label"),
                );
                if row.selectable
                    && s.can_submit()
                    && matches!(
                        models.phase(),
                        Phase::Ready
                            | Phase::Selected
                            | Phase::Confirmation
                            | Phase::Committed
                            | Phase::Failed
                    )
                {
                    entry = entry.child(
                        ui::html::button(format!("Select {}", label(&row.model)))
                            .class("ht-button ht-model-select")
                            .key(format!("select-{}", models.catalog_id()))
                            .on_click(UiAction::SelectModel {
                                catalog: models.catalog_id().to_owned(),
                                index,
                            }),
                    );
                }
                content = content.child(entry);
            }
        }
        let mut controls = ui::html::div().class("ht-controls").key("controls");
        if let Some(control) = models.control_id()
            && s.can_submit()
        {
            let confirm = models.phase() == Phase::Confirmation;
            if confirm || models.phase() == Phase::Selected {
                controls = controls.child(
                    ui::html::button(if confirm {
                        "Confirm guarded change"
                    } else {
                        "Apply to this session"
                    })
                    .class("ht-button ht-model-apply")
                    .key(format!("apply-{control}"))
                    .on_click(UiAction::ApplyModel {
                        control: control.into(),
                        confirm,
                    }),
                );
            }
        }
        for (shown, to, name) in [
            (self.page > 0, self.page.saturating_sub(1), "Previous"),
            (self.page + 1 < Self::pages(models), self.page + 1, "Next"),
        ] {
            if shown {
                controls = controls.child(
                    ui::html::button(name)
                        .class("ht-button")
                        .key(name)
                        .on_click(UiAction::ModelPage {
                            from: self.page,
                            to,
                        }),
                );
            }
        }
        if !matches!(
            models.phase(),
            Phase::Loading | Phase::Sending | Phase::Reconciling | Phase::Unknown | Phase::Deferred
        ) && s.connected
        {
            controls = controls.child(
                ui::html::button("Reload model inventory")
                    .class("ht-button")
                    .key("reload")
                    .on_click(UiAction::ReloadModels),
            );
        }
        controls = controls
            .child(
                ui::html::button("Return to conversation")
                    .class("ht-button")
                    .key("return")
                    .on_click(UiAction::Models(false)),
            )
            .child(
                ui::html::button("Detach")
                    .class("ht-button")
                    .key("exit")
                    .on_click(UiAction::Exit),
            );
        View::new().main(ui::col().child(content)).dock(nodes![
            ui::editor().key("ed").text(composer.text()).cursor(composer.utf16_cursor()).mode("inspect").sendable(false),controls,
            ui::text(notice.unwrap_or("Left/Right pages · Escape/Ctrl+C returns locally · Ctrl+D detaches. No keyboard activation of Apply/Confirm.")).key("notice"),
        ])
    }
}
