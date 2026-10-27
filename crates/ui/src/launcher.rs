//! Explicit native entry. Inventory is data, never a create/resume instruction.
use crate::{
    app::{self, NativeInput, drain_native_inbox, next_native_for_surface},
    native_visibility::Visibility,
};
use hermes_tern_core::{
    error::{Error, Result},
    text::display_text,
};
use hermes_tern_gateway::{
    catalog::{self, Catalog, Scope},
    config::{AttachOptions, Limits, SessionOptions},
    inspect,
    protocol::Inbound,
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};
use tern_sdk::{
    Input, Options, Session, SurfaceOptions,
    keys::Key,
    nodes,
    term::Tty,
    ui::{self, View},
    wire::{Event, Op, RevealAt},
};

const PAGE_SIZE: usize = 12;
const STYLE: &str = ".ht-launch-button{padding:4px 10px;margin:4px 8px 4px 0;border-radius:3px;background:var(--l2)} .ht-launch-controls{display:flex;gap:8px;align-items:center;flex-wrap:wrap} .ht-launch-list{max-height:50vh;overflow:auto} .ht-launch-row{margin:6px 0;padding:4px 0;border-bottom:1px solid var(--l2)} .ht-launch-selected{outline:1px solid var(--accent);outline-offset:2px}";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    New,
    Resume(String),
    Profiles,
    Close,
}
#[derive(Debug, Clone)]
pub enum LauncherAction {
    New,
    Profiles,
    Resume {
        scope: Scope,
        index: usize,
        session_id: String,
    },
    Scope(Scope),
    Page {
        scope: Scope,
        from: usize,
        to: usize,
    },
    Close,
}
pub struct Launcher {
    catalog: Catalog,
    profile: Option<String>,
    cwd: PathBuf,
    scope: Scope,
    page: usize,
    selected: usize,
    revision: u64,
}
impl Launcher {
    pub fn new(catalog: Catalog, cwd: PathBuf) -> Self {
        Self {
            catalog,
            profile: None,
            cwd,
            scope: Scope::Live,
            page: 0,
            selected: 0,
            revision: 1,
        }
    }
    pub fn page(&self) -> usize {
        self.page
    }
    fn with_profile(mut self, profile: &str) -> Self {
        self.profile = Some(profile.to_owned());
        self
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn selected_node(&self) -> String {
        if self.selected == 0 {
            "main.controls.new".into()
        } else {
            format!(
                "main.list-{}-{}.entry-{}",
                self.scope.label(),
                self.page,
                self.page * PAGE_SIZE + self.selected - 1
            )
        }
    }
    fn page_count(&self) -> usize {
        self.catalog
            .entries(self.scope)
            .len()
            .div_ceil(PAGE_SIZE)
            .max(1)
    }
    fn page_len(&self) -> usize {
        self.catalog
            .entries(self.scope)
            .len()
            .saturating_sub(self.page * PAGE_SIZE)
            .min(PAGE_SIZE)
    }
    fn changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
    pub fn apply(&mut self, action: &LauncherAction) -> Option<Selection> {
        match action {
            LauncherAction::Close => return Some(Selection::Close),
            LauncherAction::New => return Some(Selection::New),
            LauncherAction::Profiles => return Some(Selection::Profiles),
            LauncherAction::Resume {
                scope,
                index,
                session_id,
            } if *scope == self.scope && *index / PAGE_SIZE == self.page => {
                if self
                    .catalog
                    .entries(*scope)
                    .get(*index)
                    .is_some_and(|e| &e.session_id == session_id)
                {
                    return Some(Selection::Resume(session_id.clone()));
                }
            }
            LauncherAction::Scope(scope) if *scope != self.scope => {
                self.scope = *scope;
                self.page = 0;
                self.selected = 0;
                self.changed();
            }
            LauncherAction::Page { scope, from, to }
                if *scope == self.scope
                    && *from == self.page
                    && *to < self.page_count()
                    && *to != self.page =>
            {
                self.page = *to;
                self.selected = 0;
                self.changed();
            }
            _ => {}
        }
        None
    }
    pub fn key(&mut self, key: &Key, drawn: Option<u64>) -> Option<Selection> {
        if key.is("escape") || key.is("ctrl+d") || key.is("ctrl+c") {
            return Some(Selection::Close);
        }
        if key.is("enter") && drawn == Some(self.revision) {
            return if self.selected == 0 {
                Some(Selection::New)
            } else {
                self.catalog
                    .entries(self.scope)
                    .get(self.page * PAGE_SIZE + self.selected - 1)
                    .map(|e| Selection::Resume(e.session_id.clone()))
            };
        }
        if key.is("tab") || key.is("shift+tab") {
            self.apply(&LauncherAction::Scope(if self.scope == Scope::Live {
                Scope::Stored
            } else {
                Scope::Live
            }));
        } else if key.is("right") || key.is("left") {
            let to = if key.is("right") {
                (self.page + 1).min(self.page_count() - 1)
            } else {
                self.page.saturating_sub(1)
            };
            self.apply(&LauncherAction::Page {
                scope: self.scope,
                from: self.page,
                to,
            });
        } else if key.is("up") || key.is("down") {
            let next = if key.is("down") {
                (self.selected + 1).min(self.page_len())
            } else {
                self.selected.saturating_sub(1)
            };
            if next != self.selected {
                self.selected = next;
                self.changed();
            }
        }
        None
    }
    pub fn render(&self) -> View<LauncherAction> {
        let rows = self.catalog.entries(self.scope);
        let mut main = ui::col()
            .child(ui::text("Hermes / Tern").key("heading"))
            .child(
                ui::text(format!(
                    "New conversation workspace: {}",
                    display_text(&self.cwd.to_string_lossy())
                ))
                .key("cwd"),
            );
        let mut controls = ui::html::div().class("ht-launch-controls").key("controls");
        if let Some(profile) = &self.profile {
            main = main.child(
                ui::text(format!("Qualified profile: {}", display_text(profile))).key("profile"),
            );
        }
        controls = controls.child(
            ui::html::button("New conversation")
                .key("new")
                .class(if self.selected == 0 {
                    "ht-launch-button ht-launch-selected"
                } else {
                    "ht-launch-button"
                })
                .on_click(LauncherAction::New),
        );
        for scope in [Scope::Live, Scope::Stored] {
            controls = controls.child(
                ui::html::button(format!("{} sessions", scope.label()))
                    .key(scope.label())
                    .class("ht-launch-button")
                    .on_click(LauncherAction::Scope(scope)),
            );
        }
        controls = controls.child(
            ui::html::button("Profiles")
                .key("profiles")
                .class("ht-launch-button ht-profiles-entry")
                .on_click(LauncherAction::Profiles),
        );
        controls = controls.child(
            ui::html::button("Close")
                .key("close")
                .class("ht-launch-button")
                .on_click(LauncherAction::Close),
        );
        main = main.child(controls).child(
            ui::text(format!(
                "{} · {} listed · page {}/{}{}",
                self.scope.label(),
                rows.len(),
                self.page + 1,
                self.page_count(),
                if self.catalog.at_limit(self.scope) {
                    " · listing cap reached; more may exist"
                } else {
                    ""
                }
            ))
            .key("count"),
        );
        let mut list = ui::html::div()
            .key(format!("list-{}-{}", self.scope.label(), self.page))
            .class("ht-launch-list");
        for (index, row) in rows
            .iter()
            .enumerate()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
        {
            let selected = self.selected == index % PAGE_SIZE + 1;
            list = list.child(
                ui::html::div()
                    .key(format!("entry-{index}"))
                    .class(if selected {
                        "ht-launch-row ht-launch-selected"
                    } else {
                        "ht-launch-row"
                    })
                    .child(
                        ui::text(if row.title.is_empty() {
                            "Untitled conversation"
                        } else {
                            &row.title
                        })
                        .key("title"),
                    )
                    .child(
                        ui::text(format!(
                            "{} · {} · {}",
                            row.session_id,
                            row.model.as_deref().unwrap_or("model unavailable"),
                            if row.running {
                                "running"
                            } else {
                                "not running at listing"
                            }
                        ))
                        .key("identity"),
                    )
                    .child(
                        ui::html::button("Resume")
                            .key("resume")
                            .class("ht-launch-button")
                            .on_click(LauncherAction::Resume {
                                scope: self.scope,
                                index,
                                session_id: row.session_id.clone(),
                            }),
                    ),
            );
        }
        if rows.is_empty() {
            list = list.child(ui::text("No sessions in this limited inventory."));
        }
        main = main.child(list);
        let mut pages = ui::html::div().class("ht-launch-controls").key("pages");
        for (show, to, label) in [
            (self.page > 0, self.page.saturating_sub(1), "Previous"),
            (self.page + 1 < self.page_count(), self.page + 1, "Next"),
        ] {
            if show {
                pages = pages.child(
                    ui::html::button(label)
                        .key(label)
                        .class("ht-launch-button")
                        .on_click(LauncherAction::Page {
                            scope: self.scope,
                            from: self.page,
                            to,
                        }),
                );
            }
        }
        View::new().main(main).dock(nodes![pages, ui::text("Up/Down selects · Enter opens · Tab switches live/stored · Left/Right pages · Escape closes").key("keys"),ui::text("At most 200 per scope; fixed listing, not complete history. Listing and Close do not create/resume a session or send a prompt.").key("notice")])
    }
}
fn native_error(_: tern_sdk::Error) -> Error {
    Error::Transport("native launcher rendering failed")
}
/// Explicit native-only picker; no fallback creates a plain session.
pub fn open(attach: AttachOptions, cwd: PathBuf) -> Result<()> {
    if !io::stdin().is_terminal()
        || !io::stdout().is_terminal()
        || std::env::var_os("TERN_TSP").is_some_and(|v| v == "0")
        || ["TMUX", "STY", "ZELLIJ"]
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
    {
        return Err(Error::Unsupported(
            "open requires native Tern on a direct TTY; use chat --plain explicitly",
        ));
    }
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(Error::Config("cwd must be an existing absolute directory"));
    }
    let limits = Limits::default();
    let native = native_session()?;
    let client = inspect::connect(&attach, &limits)?;
    run_picker(attach, cwd, limits, client, native)
}
fn run_picker(
    mut attach: AttachOptions,
    cwd: PathBuf,
    limits: Limits,
    mut client: hermes_tern_gateway::transport::RpcClient,
    native: (Session<LauncherAction>, Visibility),
) -> Result<()> {
    let mut native = Some(native);
    loop {
        let native = match native.take() {
            Some(native) => native,
            None => native_session()?,
        };
        let selection = pick_sessions(&mut client, &cwd, &limits, native)?;
        match selection {
            Selection::Profiles => {
                let (catalog, notice) = match hermes_tern_gateway::profiles::fetch(&mut client, &limits) {
                    Ok(c) => (Some(c), None),
                    Err(Error::Refused(_) | Error::Unsupported(_)) => (None, Some("Profile inventory unavailable; no profile or session changed. Back or Close remains available.".into())),
                    Err(error) => return Err(error),
                };
                match crate::profile_picker::show(&mut client, &limits, catalog, notice)? {
                    crate::profile_picker::Choice::Back => {}
                    crate::profile_picker::Choice::Close => {
                        client.close();
                        return Ok(());
                    }
                    crate::profile_picker::Choice::Home(home) => {
                        let expected = home
                            .to_str()
                            .ok_or(Error::Config("profile home must be UTF-8"))?;
                        if expected != client.profile_id {
                            client.close();
                            attach.home = Some(home.clone());
                            client = inspect::connect_selected(&attach, &limits, &home)?;
                        }
                    }
                }
            }
            Selection::Close => {
                client.close();
                return Ok(());
            }
            Selection::New | Selection::Resume(_) => {
                let resume = if let Selection::Resume(id) = selection {
                    Some(id)
                } else {
                    None
                };
                return app::chat_existing(
                    client,
                    limits,
                    SessionOptions {
                        cwd,
                        resume,
                        model: None,
                        provider: None,
                    },
                );
            }
        }
    }
}
fn native_session() -> Result<(Session<LauncherAction>, Visibility)> {
    let seen = Visibility::default();
    let session = Session::<LauncherAction>::with_terminal(
        seen.wrap(Tty::open()?),
        Options::new().app("hermes-tern").record(None),
    )
    .map_err(native_error)?
    .ok_or(Error::Unsupported(
        "open requires native Tern surfaces; use chat --plain explicitly",
    ))?;
    Ok((session, seen))
}
fn pick_sessions(
    client: &mut hermes_tern_gateway::transport::RpcClient,
    cwd: &std::path::Path,
    limits: &Limits,
    (mut session, seen): (Session<LauncherAction>, Visibility),
) -> Result<Selection> {
    let catalog = catalog::fetch(client, limits)?;
    let mut view = Launcher::new(catalog, cwd.to_owned()).with_profile(&client.profile_id);
    let sf = session
        .open(
            SurfaceOptions::inline()
                .id("hermes-tern-launcher")
                .title("Hermes sessions"),
        )
        .map_err(native_error)?;
    session
        .stylesheet(sf, "hermes-tern-launcher", Some(STYLE))
        .map_err(native_error)?;
    seen.bind(session.surface_id(sf));
    seen.rendering(view.revision());
    session.render(sf, view.render()).map_err(native_error)?;
    let selection = loop {
        // No session is attached here. Drain bounded global broadcasts and fail
        // on transport loss rather than leaving an apparently usable picker.
        for _ in 0..limits.max_events {
            match client.try_recv()? {
                None => break,
                Some(Inbound::Event(_)) => {}
                Some(Inbound::ServerRequest { id, .. }) => client.decline_server(&id)?,
                Some(Inbound::Disconnected) => return Err(Error::Disconnected),
                Some(Inbound::Response { .. }) => {
                    return Err(Error::Protocol("unexpected launcher response"));
                }
            }
        }
        let revision = view.revision();
        let result = match next_native_for_surface(&mut session, sf)? {
            NativeInput::Closed => Some(Selection::Close),
            NativeInput::Wake => None,
            NativeInput::Input(input) => match *input {
                Input::Key(key) => view.key(&key, seen.key_visibility(&key)?.positioned),
                Input::Msg(action, _) => view.apply(&action),
                Input::Event(Event::Resize(_) | Event::Theme(_) | Event::Motion(_)) => {
                    view.revision = view.revision.saturating_add(1);
                    None
                }
                _ => None,
            },
        };
        if let Some(selection) = result {
            drain_native_inbox(&mut session, sf)?;
            break selection;
        }
        if revision != view.revision() {
            seen.rendering(view.revision());
            session.render(sf, view.render()).map_err(native_error)?;
        }
        if seen.needs_positioning(view.revision()) {
            seen.positioning(view.revision())?;
            session
                .send(
                    sf,
                    vec![
                        Op::Focus { id: None },
                        Op::Reveal {
                            id: view.selected_node(),
                            at: RevealAt::Nearest,
                        },
                    ],
                )
                .map_err(native_error)?;
        }
    };
    if session.is_open(sf) {
        session.close_surface(sf, false).map_err(native_error)?;
    }
    session.close().map_err(native_error)?;
    Ok(selection)
}
