//! Local, allowlisted runtime inspection. Inventory is not execution permission.
use crate::{composer::Composer, view::UiAction};
use hermes_tern_core::text::display_text;
use hermes_tern_gateway::delegated::{Delegated, Phase};
use hermes_tern_gateway::session::SessionState;
use serde_json::Value;
use tern_sdk::{
    keys::Key,
    nodes,
    ui::{self, View},
};

const PAGE_ROWS: usize = 20;
const MAX_ROWS: usize = 4096;
const LABEL_BYTES: usize = 256;

#[derive(Default)]
pub struct RuntimeDetails {
    open: bool,
    page: usize,
    control_id: u64,
    exhausted: bool,
    delegated_page: usize,
    plan_open: bool,
    plan_page: usize,
    plan_snapshot: String,
}
struct Inventory {
    tools: Option<usize>,
    skills: Option<usize>,
    mcp: Option<usize>,
    rows: Vec<String>,
}
fn label(value: &str) -> String {
    let mut end = value.len().min(LABEL_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut shown = display_text(&value[..end]);
    if end < value.len() {
        shown.push_str("… [label clipped]");
    }
    shown
}
fn field(info: &Value, key: &str) -> String {
    info.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(label)
        .unwrap_or_else(|| "unavailable".into())
}
fn grouped(value: &Value, family: &str) -> Option<(usize, Vec<String>)> {
    let groups = value.as_object()?;
    if groups.len() > MAX_ROWS {
        return None;
    }
    let mut count = 0usize;
    let mut rows = Vec::new();
    for (group, names) in groups {
        let names = names.as_array()?;
        count = count.checked_add(names.len())?;
        if count > MAX_ROWS {
            return None;
        }
        for name in names {
            let name = name.as_str().filter(|s| !s.is_empty())?;
            rows.push(format!("{family} · {} · {}", label(group), label(name)));
        }
    }
    rows.sort();
    Some((count, rows))
}
fn inventory(state: &SessionState) -> Inventory {
    let mut rows = Vec::new();
    let tools = grouped(&state.info["tools"], "Tool").map(|(count, names)| {
        rows.extend(names);
        count
    });
    let skills = grouped(&state.info["skills"], "Skill").map(|(count, names)| {
        rows.extend(names);
        count
    });
    let mcp = (|| {
        let servers = state.info["mcp_servers"].as_array()?;
        if servers.len() > MAX_ROWS {
            return None;
        }
        let mut names = Vec::new();
        for server in servers {
            let name = server.get("name")?.as_str().filter(|s| !s.is_empty())?;
            let status = match server.get("status") {
                Some(Value::String(s)) => label(s),
                None | Some(Value::Null) => "status unavailable".into(),
                _ => return None,
            };
            names.push(format!("MCP · {} · {status}", label(name)));
        }
        names.sort();
        rows.extend(names);
        Some(servers.len())
    })();
    Inventory {
        tools,
        skills,
        mcp,
        rows,
    }
}
fn count(value: Option<usize>) -> String {
    value
        .map(|n| n.to_string())
        .unwrap_or_else(|| "unavailable".into())
}
impl RuntimeDetails {
    pub fn is_open(&self) -> bool {
        self.open
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
        self.page = 0;
        self.delegated_page = 0;
        self.plan_open = false;
        self.plan_page = 0;
        self.plan_snapshot.clear();
    }
    pub fn control_id(&self) -> u64 {
        self.control_id
    }
    pub fn accepts_read(&self, panel: u64) -> bool {
        self.open && !self.exhausted && panel == self.control_id
    }
    pub fn reset_delegated_page(&mut self) {
        self.delegated_page = 0;
    }
    pub fn delegated_page(
        &mut self,
        d: &Delegated,
        s: &SessionState,
        panel: u64,
        snapshot: &str,
        from: usize,
        to: usize,
    ) {
        let pages = d.catalog(s, panel).map_or(1, |c| c.rows.len().max(1));
        if self.accepts_read(panel)
            && d.id() == snapshot
            && self.delegated_page == from
            && to < pages
        {
            self.delegated_page = to;
        }
    }
    pub fn current_page(&self) -> usize {
        self.page
    }
    pub fn show_plan(&mut self, state: &SessionState, panel: u64, open: bool) {
        if self.accepts_read(panel) && self.plan_open != open {
            if let Some(next) = self.control_id.checked_add(1) {
                self.control_id = next;
                self.plan_open = open;
                self.plan_page = 0;
                self.plan_snapshot = state.task_plan.snapshot_id().into();
            } else {
                self.exhausted = true;
                self.open = false;
            }
        }
    }
    pub fn plan_page(
        &mut self,
        state: &SessionState,
        panel: u64,
        snapshot: &str,
        from: usize,
        to: usize,
    ) {
        let count = state.task_plan.catalog().map_or(0, |p| p.rows.len());
        if self.accepts_read(panel)
            && self.plan_open
            && state.task_plan.snapshot_id() == snapshot
            && self.plan_snapshot == snapshot
            && self.plan_page == from
            && to < count
        {
            self.plan_page = to;
        }
    }
    pub fn reconcile(&mut self, state: &SessionState) {
        let pages = inventory(state).rows.len().div_ceil(PAGE_ROWS).max(1);
        self.page = self.page.min(pages - 1);
        if self.plan_snapshot != state.task_plan.snapshot_id() {
            self.plan_page = 0;
            self.plan_snapshot = state.task_plan.snapshot_id().into();
        }
    }
    pub fn page(&mut self, state: &SessionState, from: usize, to: usize) {
        let pages = inventory(state).rows.len().div_ceil(PAGE_ROWS).max(1);
        if self.open && self.page == from && to < pages {
            self.page = to;
        }
    }
    pub fn key(&mut self, state: &SessionState, key: &Key) -> bool {
        if !self.open || key.is("ctrl+d") {
            return false;
        }
        if key.is("escape") || key.is("ctrl+c") {
            self.set_open(false);
        } else if self.plan_open && (key.is("right") || key.is("left")) {
            let to = if key.is("right") {
                self.plan_page.saturating_add(1)
            } else {
                self.plan_page.saturating_sub(1)
            };
            self.plan_page(
                state,
                self.control_id,
                state.task_plan.snapshot_id(),
                self.plan_page,
                to,
            );
        } else if key.is("right") {
            self.page(state, self.page, self.page.saturating_add(1));
        } else if key.is("left") {
            self.page(state, self.page, self.page.saturating_sub(1));
        }
        true
    }
    pub fn render(
        &self,
        state: &SessionState,
        composer: &Composer,
        notice: Option<&str>,
    ) -> View<UiAction> {
        self.render_with_delegated(state, composer, notice, &Delegated::default())
    }
    fn render_plan(
        &self,
        state: &SessionState,
        composer: &Composer,
        notice: Option<&str>,
    ) -> View<UiAction> {
        let mut body = ui::col()
            .key("plan")
            .child(ui::text("Task plan · owner-reported snapshot").key("heading"));
        let mut controls = ui::html::div().key("controls").class("ht-controls").child(
            ui::html::button("Show runtime details")
                .class("ht-button ht-plan-back")
                .key(format!("plan-back-{}", self.control_id))
                .on_click(UiAction::TaskPlan {
                    panel: self.control_id,
                    open: false,
                }),
        );
        if let Some(plan) = state.task_plan.catalog() {
            body = body.child(
                ui::text(format!(
                    "{} reported tasks · revision {}",
                    plan.rows.len(),
                    plan.revision
                ))
                .key("count"),
            );
            let totals = ["pending", "in_progress", "completed", "cancelled"]
                .map(|status| {
                    format!(
                        "{status}: {}",
                        plan.rows.iter().filter(|r| r.status == status).count()
                    )
                })
                .join(" · ");
            body = body.child(ui::text(totals).key("totals"));
            let page = self.plan_page.min(plan.rows.len().saturating_sub(1));
            body = body.child(
                ui::text(format!(
                    "Task page {}/{} · one reported task per local page",
                    page + 1,
                    plan.rows.len().max(1)
                ))
                .key("page"),
            );
            if let Some(row) = plan.rows.get(page) {
                let mut entry = ui::col().key("row");
                for (i, line) in row.text.lines().enumerate() {
                    entry = entry.child(ui::text(line).key(format!("line-{i}")));
                }
                body = body.child(entry);
            }
            for (show, to, name) in [
                (page > 0, page.saturating_sub(1), "Previous task"),
                (
                    page + 1 < plan.rows.len(),
                    page.saturating_add(1),
                    "Next task",
                ),
            ] {
                if show {
                    controls = controls.child(
                        ui::html::button(name)
                            .class("ht-button")
                            .key(format!(
                                "{name}-{}-{}-{page}",
                                self.control_id,
                                state.task_plan.snapshot_id()
                            ))
                            .on_click(UiAction::TaskPlanPage {
                                panel: self.control_id,
                                snapshot: state.task_plan.snapshot_id().into(),
                                from: page,
                                to,
                            }),
                    );
                }
            }
        } else {
            body = body.child(
                ui::text("Task plan unavailable; no complete current owner report")
                    .key("unavailable"),
            );
        }
        body = body.child(ui::text("Reported task status is data, not proof of success or live work. No task edits, approvals or automatic prompts.").key("scope"));
        controls = controls
            .child(
                ui::html::button("Return to conversation")
                    .key("return")
                    .class("ht-button ht-runtime-return")
                    .on_click(UiAction::RuntimeDetails(false)),
            )
            .child(
                ui::html::button("Detach")
                    .key("exit")
                    .class("ht-button")
                    .on_click(UiAction::Exit),
            );
        View::new().main(ui::col().child(ui::html::div().class("ht-runtime-panel ht-plan-body").key("runtime").child(body))).dock(nodes![
            ui::editor().key("ed").text(composer.text()).cursor(composer.utf16_cursor()).mode("inspect").sendable(false),
            controls,
            ui::text(notice.unwrap_or("Task plan: Left/Right pages locally · Escape returns to unchanged draft · Ctrl+D detaches.")).key("notice"),
        ])
    }
    pub fn render_with_delegated(
        &self,
        state: &SessionState,
        composer: &Composer,
        notice: Option<&str>,
        delegated: &Delegated,
    ) -> View<UiAction> {
        if self.plan_open {
            return self.render_plan(state, composer, notice);
        }
        let data = inventory(state);
        let pages = data.rows.len().div_ceil(PAGE_ROWS).max(1);
        let page = self.page.min(pages - 1);
        let mut main = ui::html::div()
            .class("ht-runtime-panel")
            .key("runtime")
            .child(ui::text("Runtime details · local read-only view").key("heading"));
        let mut children = ui::col().key("delegated").child(
            ui::text(if delegated.notice.is_empty() {
                "Read delegated agents explicitly; no automatic refresh"
            } else {
                delegated.notice
            })
            .key("status"),
        );
        let catalog = delegated.catalog(state, self.control_id);
        if let Some(catalog) = catalog {
            children = children.child(
                ui::text(format!(
                    "{} reported delegated agents · snapshot, not independent peers",
                    catalog.rows.len()
                ))
                .key("count"),
            );
            children = children.child(
                ui::text(format!(
                    "Delegated page {}/{} · one child per local page",
                    self.delegated_page
                        .min(catalog.rows.len().saturating_sub(1))
                        + 1,
                    catalog.rows.len().max(1)
                ))
                .key("page"),
            );
            if let Some(row) = catalog.rows.get(
                self.delegated_page
                    .min(catalog.rows.len().saturating_sub(1)),
            ) {
                let mut entry = ui::col().key("row");
                for (index, line) in row.text.lines().enumerate() {
                    entry = entry.child(ui::text(line).key(format!("field-{index}")));
                }
                children = children.child(entry);
            }
        }
        children = children.child(ui::text("Owner-reported data only; snapshot may be stale. No Steer, Stop, transcript or peer controls.").key("scope"));
        main = main.child(children);
        for (key, text) in [
            ("profile", format!("Profile: {}", label(&state.profile))),
            ("instance", format!("Owner: {}", label(&state.instance))),
            ("session", format!("Session: {}", label(&state.session_id))),
            (
                "workspace",
                format!("Workspace: {}", field(&state.info, "cwd")),
            ),
            ("model", format!("Model: {}", field(&state.info, "model"))),
            (
                "provider",
                format!("Provider: {}", field(&state.info, "provider")),
            ),
            (
                "connection",
                format!(
                    "Connection: {} · generation {} · {}",
                    if state.connected {
                        "connected"
                    } else {
                        "disconnected; last reported inventory"
                    },
                    state.generation,
                    label(&state.status)
                ),
            ),
            (
                "counts",
                format!(
                    "Tools: {} · Skills: {} · MCP servers: {}",
                    count(data.tools),
                    count(data.skills),
                    count(data.mcp)
                ),
            ),
        ] {
            main = main.child(ui::text(text).key(key));
        }
        main = main.child(ui::text("Inventory reported by Hermes; not proof of tool health or permission. Missing/malformed sections are unavailable, not zero. Raw configuration and system prompt are not displayed.").key("scope"))
            .child(ui::text("Memory, plugins, commands and model switching: not exposed as controls in this panel; Hermes remains their owner.").key("unsupported"))
            .child(ui::text(format!("Inventory page {}/{} · {} rows per local page · labels at most {} bytes before escaping",page+1,pages,PAGE_ROWS,LABEL_BYTES)).key("page"));
        let mut list = ui::html::div()
            .key(format!("inventory-{page}"))
            .class("ht-runtime-list");
        for (n, text) in data
            .rows
            .iter()
            .enumerate()
            .skip(page * PAGE_ROWS)
            .take(PAGE_ROWS)
        {
            list = list.child(ui::text(text).key(format!("row-{n}")));
        }
        main = main.child(list);
        let mut controls = ui::html::div().key("controls").class("ht-controls");
        if state.connected
            && !state.is_suspended()
            && delegated.phase() != Phase::Loading
            && !self.exhausted
        {
            controls = controls.child(
                ui::html::button("Read delegated agents")
                    .class("ht-button ht-delegated-read")
                    .key(format!("delegated-read-{}", self.control_id))
                    .on_click(UiAction::ReadDelegated {
                        panel: self.control_id,
                    }),
            );
        }
        if let Some(catalog) = catalog {
            let current = self
                .delegated_page
                .min(catalog.rows.len().saturating_sub(1));
            for (show, to, name) in [
                (current > 0, current.saturating_sub(1), "Previous child"),
                (
                    current + 1 < catalog.rows.len(),
                    current.saturating_add(1),
                    "Next child",
                ),
            ] {
                if show {
                    controls = controls.child(
                        ui::html::button(name)
                            .class("ht-button")
                            .key(format!("{name}-{}-{}", self.control_id, delegated.id()))
                            .on_click(UiAction::DelegatedPage {
                                panel: self.control_id,
                                snapshot: delegated.id().into(),
                                from: current,
                                to,
                            }),
                    );
                }
            }
        }
        for (show, to, name) in [
            (page > 0, page.saturating_sub(1), "Previous"),
            (page + 1 < pages, page + 1, "Next"),
        ] {
            if show {
                controls = controls.child(
                    ui::html::button(name)
                        .key(name)
                        .class("ht-button")
                        .on_click(UiAction::RuntimePage { from: page, to }),
                );
            }
        }
        controls = controls.child(
            ui::html::button("Review MCP reload")
                .class("ht-button ht-mcp-entry")
                .key(format!("mcp-entry-{}", self.control_id))
                .on_click(UiAction::OpenMcp {
                    panel: self.control_id,
                }),
        );
        controls = controls.child(
            ui::html::button("Show task plan")
                .class("ht-button ht-plan-entry")
                .key(format!("plan-toggle-{}", self.control_id))
                .on_click(UiAction::TaskPlan {
                    panel: self.control_id,
                    open: true,
                }),
        );
        controls = controls
            .child(
                ui::html::button("Return to conversation")
                    .key("return")
                    .class("ht-button ht-runtime-return")
                    .on_click(UiAction::RuntimeDetails(false)),
            )
            .child(
                ui::html::button("Detach")
                    .key("exit")
                    .class("ht-button")
                    .on_click(UiAction::Exit),
            );
        View::new().main(ui::col().child(main)).dock(nodes![
            ui::editor().key("ed").text(composer.text()).cursor(composer.utf16_cursor()).mode("inspect").sendable(false),
            controls,
            ui::text(notice.unwrap_or("Runtime details: Left/Right pages · Escape returns to unchanged draft · Ctrl+D detaches.")).key("notice"),
        ])
    }
}
