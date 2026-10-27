//! Explicit existing-profile selection, not profile creation or runtime ownership.
use crate::{
    app::{NativeInput, drain_native_inbox, next_native_for_surface},
    native_visibility::Visibility,
};
use hermes_tern_core::{
    error::{Error, Result},
    text::display_text,
};
use hermes_tern_gateway::{
    config::Limits, profiles::Catalog, protocol::Inbound, transport::RpcClient,
};
use std::path::PathBuf;
use tern_sdk::{
    Input, Options, Session, SurfaceOptions,
    keys::Key,
    term::Tty,
    ui::{self, View},
    wire::{Event, Op, RevealAt},
};

const PAGE: usize = 12;
const STYLE: &str = ".ht-profile-list{max-height:48vh;overflow:auto}.ht-profile-controls{display:flex;gap:8px;flex-wrap:wrap}.ht-profile-button{padding:4px 10px;margin:4px 8px 4px 0;border-radius:3px;background:var(--l2)}.ht-profile-selected{outline:1px solid var(--accent);outline-offset:2px}";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Home(PathBuf),
    Back,
    Close,
}
#[derive(Debug, Clone)]
pub enum Action {
    Select { index: usize, home: PathBuf },
    Page { from: usize, to: usize },
    Back,
    Close,
}
pub struct Picker {
    catalog: Option<Catalog>,
    current: String,
    notice: Option<String>,
    page: usize,
    selected: usize,
    revision: u64,
}
impl Picker {
    pub fn new(catalog: Option<Catalog>, current: String, notice: Option<String>) -> Self {
        Self {
            catalog,
            current,
            notice,
            page: 0,
            selected: 0,
            revision: 1,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn pages(&self) -> usize {
        self.catalog
            .as_ref()
            .map_or(1, |c| c.rows.len().div_ceil(PAGE).max(1))
    }
    fn len(&self) -> usize {
        self.catalog.as_ref().map_or(0, |c| {
            c.rows.len().saturating_sub(self.page * PAGE).min(PAGE)
        })
    }
    fn node(&self) -> String {
        format!(
            "main.profiles-{}.entry-{}",
            self.page,
            self.page * PAGE + self.selected
        )
    }
    pub fn apply(&mut self, action: &Action) -> Option<Choice> {
        match action {
            Action::Close => return Some(Choice::Close),
            Action::Back => return Some(Choice::Back),
            Action::Select { index, home } if index / PAGE == self.page => {
                if self
                    .catalog
                    .as_ref()
                    .and_then(|c| c.rows.get(*index))
                    .is_some_and(|r| &r.home == home)
                {
                    return Some(Choice::Home(home.clone()));
                }
            }
            Action::Page { from, to }
                if *from == self.page && *to < self.pages() && *to != self.page =>
            {
                self.page = *to;
                self.selected = 0;
                self.revision = self.revision.saturating_add(1);
            }
            _ => {}
        }
        None
    }
    pub fn key(&mut self, key: &Key, positioned: Option<u64>) -> Option<Choice> {
        if key.is("ctrl+d") {
            return Some(Choice::Close);
        }
        if key.is("escape") || key.is("ctrl+c") {
            return Some(Choice::Back);
        }
        if key.is("enter")
            && positioned == Some(self.revision)
            && let Some(row) = self
                .catalog
                .as_ref()
                .and_then(|c| c.rows.get(self.page * PAGE + self.selected))
        {
            return Some(Choice::Home(row.home.clone()));
        }
        if key.is("right") || key.is("left") {
            let to = if key.is("right") {
                (self.page + 1).min(self.pages() - 1)
            } else {
                self.page.saturating_sub(1)
            };
            self.apply(&Action::Page {
                from: self.page,
                to,
            });
        } else if key.is("down") || key.is("up") {
            let to = if key.is("down") {
                (self.selected + 1).min(self.len().saturating_sub(1))
            } else {
                self.selected.saturating_sub(1)
            };
            if to != self.selected {
                self.selected = to;
                self.revision = self.revision.saturating_add(1);
            }
        }
        None
    }
    pub fn render(&self) -> View<Action> {
        let mut main=ui::col().child(ui::text("Hermes profiles · existing owners only").key("heading"))
            .child(ui::text(format!("Current profile: {}",display_text(&self.current))).key("current"))
            .child(ui::text("Selecting a profile discovers its existing owner; it never creates a profile, copies credentials or starts a gateway. New/Resume is a separate next action.").key("scope"));
        if let Some(c) = &self.catalog {
            main = main.child(
                ui::text(format!(
                    "{} profiles · page {}/{} · {} per page",
                    c.rows.len(),
                    self.page + 1,
                    self.pages(),
                    PAGE
                ))
                .key("count"),
            );
            let mut list = ui::html::div()
                .class("ht-profile-list")
                .key(format!("profiles-{}", self.page));
            for (index, row) in c.rows.iter().enumerate().skip(self.page * PAGE).take(PAGE) {
                list = list.child(
                    ui::html::div()
                        .key(format!("entry-{index}"))
                        .class(if index % PAGE == self.selected {
                            "ht-profile-selected"
                        } else {
                            "ht-profile-row"
                        })
                        .child(ui::text(format!("{} · {}", row.label, row.name)).key("label"))
                        .child(ui::text(display_text(&row.home.to_string_lossy())).key("home"))
                        .child(
                            ui::html::button("Use existing profile")
                                .class("ht-profile-button ht-profile-select")
                                .key("select")
                                .on_click(Action::Select {
                                    index,
                                    home: row.home.clone(),
                                }),
                        ),
                );
            }
            main = main.child(list);
        }
        if let Some(notice) = &self.notice {
            main = main.child(ui::text(notice).key("notice"));
        }
        let mut controls = ui::html::div().class("ht-profile-controls").key("controls");
        for (show, to, label) in [
            (self.page > 0, self.page.saturating_sub(1), "Previous"),
            (self.page + 1 < self.pages(), self.page + 1, "Next"),
        ] {
            if show {
                controls = controls.child(
                    ui::html::button(label)
                        .class("ht-profile-button")
                        .key(label)
                        .on_click(Action::Page {
                            from: self.page,
                            to,
                        }),
                );
            }
        }
        controls = controls
            .child(
                ui::html::button("Back to sessions")
                    .class("ht-profile-button ht-profile-back")
                    .key("back")
                    .on_click(Action::Back),
            )
            .child(
                ui::html::button("Close")
                    .class("ht-profile-button ht-profile-close")
                    .key("close")
                    .on_click(Action::Close),
            );
        View::new().main(main).dock(ui::col().child(controls))
    }
}
fn native_error(_: tern_sdk::Error) -> Error {
    Error::Transport("native profile picker failed")
}
pub fn show(
    client: &mut RpcClient,
    limits: &Limits,
    catalog: Option<Catalog>,
    notice: Option<String>,
) -> Result<Choice> {
    let seen = Visibility::default();
    let mut session = Session::<Action>::with_terminal(
        seen.wrap(Tty::open()?),
        Options::new().app("hermes-tern").record(None),
    )
    .map_err(native_error)?
    .ok_or(Error::Unsupported("profiles require native Tern"))?;
    let mut picker = Picker::new(catalog, client.profile_id.clone(), notice);
    let sf = session
        .open(
            SurfaceOptions::inline()
                .id("hermes-tern-profiles")
                .title("Hermes profiles"),
        )
        .map_err(native_error)?;
    session
        .stylesheet(sf, "hermes-tern-profiles", Some(STYLE))
        .map_err(native_error)?;
    seen.bind(session.surface_id(sf));
    seen.rendering(picker.revision());
    session.render(sf, picker.render()).map_err(native_error)?;
    let choice = loop {
        for _ in 0..limits.max_events {
            match client.try_recv()? {
                None => break,
                Some(Inbound::Event(_)) => {}
                Some(Inbound::ServerRequest { id, .. }) => client.decline_server(&id)?,
                Some(Inbound::Disconnected) => return Err(Error::Disconnected),
                Some(Inbound::Response { .. }) => {
                    return Err(Error::Protocol("unexpected profile response"));
                }
            }
        }
        let before = picker.revision();
        let selected = match next_native_for_surface(&mut session, sf)? {
            NativeInput::Closed => Some(Choice::Close),
            NativeInput::Wake => None,
            NativeInput::Input(input) => match *input {
                Input::Key(key) => picker.key(&key, seen.key_visibility(&key)?.positioned),
                Input::Msg(action, _) => picker.apply(&action),
                Input::Event(Event::Resize(_) | Event::Theme(_) | Event::Motion(_)) => {
                    picker.revision = picker.revision.saturating_add(1);
                    None
                }
                _ => None,
            },
        };
        if let Some(choice) = selected {
            drain_native_inbox(&mut session, sf)?;
            break choice;
        }
        if picker.revision() != before {
            seen.rendering(picker.revision());
            session.render(sf, picker.render()).map_err(native_error)?;
        }
        if picker.len() > 0 && seen.needs_positioning(picker.revision()) {
            seen.positioning(picker.revision())?;
            session
                .send(
                    sf,
                    vec![
                        Op::Focus { id: None },
                        Op::Reveal {
                            id: picker.node(),
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
    Ok(choice)
}
