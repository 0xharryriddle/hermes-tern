//! Read-only immutable history viewer. The owner socket is closed before presentation.
use crate::app::{NativeInput, next_native_for_surface};
use crate::native_visibility::Visibility;
use hermes_tern_core::error::{Error, Result};
use hermes_tern_gateway::{
    config::{AttachOptions, Limits},
    history::{self, HistorySnapshot},
    inspect,
};
use std::{
    fmt::Write as _,
    io::{self, IsTerminal},
};
use tern_sdk::{
    Input, Options, Session, SurfaceOptions,
    keys::Key,
    nodes,
    term::Tty,
    ui::{self, View},
    wire::ScrollBy,
};

const NOTICE: &str = "Immutable display projection, not raw transcript. Hermes may omit hidden rows/tool bodies. No live controls. Another invocation fetches a new snapshot.";
const STYLE: &str = ".ht-history-button{padding:4px 10px;margin:4px 8px 4px 0;border-radius:3px;background:var(--l2)} .ht-history-controls{display:flex;gap:8px;align-items:center} .ht-history-scroll{max-height:45vh;overflow:auto} .ht-history-body{white-space:pre-wrap;overflow-wrap:anywhere;font-family:monospace}";
#[derive(Clone, Debug)]
pub enum HistoryAction {
    Page { from: usize, to: usize },
    Close,
}
pub struct HistoryView {
    snapshot: HistorySnapshot,
    page: usize,
}
impl HistoryView {
    pub fn new(snapshot: HistorySnapshot, one_based_page: usize) -> Result<Self> {
        let page = one_based_page
            .checked_sub(1)
            .ok_or(Error::Config("history page must be positive"))?;
        snapshot.page(page)?;
        Ok(Self { snapshot, page })
    }
    pub fn page(&self) -> usize {
        self.page
    }
    /// True only for local Close. Old pointer actions cannot advance another page.
    pub fn apply(&mut self, action: &HistoryAction) -> bool {
        match action {
            HistoryAction::Close => return true,
            HistoryAction::Page { from, to }
                if *from == self.page && *to < self.snapshot.page_count() =>
            {
                self.page = *to
            }
            _ => {}
        }
        false
    }
    pub fn key(&mut self, key: &Key) -> bool {
        if key.is("escape") || key.is("ctrl+d") || key.is("ctrl+c") {
            return true;
        }
        if key.is("right") || key.is("page_down") {
            self.page = (self.page + 1).min(self.snapshot.page_count() - 1);
        }
        if key.is("left") || key.is("page_up") {
            self.page = self.page.saturating_sub(1);
        }
        if key.is("home") {
            self.page = 0;
        }
        if key.is("end") {
            self.page = self.snapshot.page_count() - 1;
        }
        false
    }
    fn heading(&self) -> String {
        format!(
            "Page {}/{} · {} source rows · {} projected rows",
            self.page + 1,
            self.snapshot.page_count(),
            self.snapshot.source_count(),
            self.snapshot.row_count()
        )
    }
    pub fn plain(&self) -> Result<String> {
        let mut text = format!("History projection\n{}\n{NOTICE}\n", self.heading());
        for f in self.snapshot.page(self.page)? {
            writeln!(
                text,
                "\nProjection row {} · bytes {}..{} of {}\n{}",
                f.row + 1,
                f.start,
                f.end,
                f.total_bytes,
                f.text
            )
            .expect("writing to String");
        }
        Ok(text)
    }
    pub fn render(&self) -> View<HistoryAction> {
        let mut main = ui::col().child(ui::text("History projection").key("heading"));
        let mut scroll = ui::html::div()
            .class("ht-history-scroll")
            .key(format!("page-{}", self.page));
        for f in self.snapshot.page(self.page).expect("validated page") {
            scroll = scroll.child(
                ui::col()
                    .key(format!("row-{}-{}", f.row, f.start))
                    .child(
                        ui::text(format!(
                            "Projection row {} · bytes {}..{} of {}",
                            f.row + 1,
                            f.start,
                            f.end,
                            f.total_bytes
                        ))
                        .key("label"),
                    )
                    .child(
                        ui::html::div()
                            .class("ht-history-body")
                            .key("body")
                            .child(ui::text(f.text)),
                    ),
            );
        }
        main = main.child(scroll);
        let mut controls = ui::html::div().class("ht-history-controls").key("controls");
        for (show, target, name) in [
            (self.page > 0, self.page.saturating_sub(1), "Previous"),
            (
                self.page + 1 < self.snapshot.page_count(),
                self.page + 1,
                "Next",
            ),
        ] {
            if show {
                controls = controls.child(
                    ui::html::button(name)
                        .class("ht-history-button")
                        .key(format!("page-{}-{target}", self.page))
                        .on_click(HistoryAction::Page {
                            from: self.page,
                            to: target,
                        }),
                );
            }
        }
        controls = controls.child(
            ui::html::button("Close")
                .class("ht-history-button")
                .key("close")
                .on_click(HistoryAction::Close),
        );
        View::new().main(main).dock(nodes![
            ui::text(self.heading()).key("page"),
            controls,
            ui::text(NOTICE).key("notice"),
            ui::text("Left/Right: pages · Up/Down: scroll (direct TTY) · Home/End · Escape closes")
                .key("keys")
        ])
    }
}
fn native_error(_: tern_sdk::Error) -> Error {
    Error::Transport("history terminal rendering failed")
}

pub fn show(attach: &AttachOptions, session_id: &str, plain: bool, page: usize) -> Result<()> {
    history::validate_session(session_id)?;
    if page == 0 {
        return Err(Error::Config("history page must be positive"));
    }
    let limits = Limits::default();
    let mut client = inspect::connect(attach, &limits)?;
    let snapshot = history::fetch(&mut client, &limits, session_id);
    client.close();
    let mut view = HistoryView::new(snapshot?, page)?;
    let (native, visibility) = if !plain && io::stdin().is_terminal() && io::stdout().is_terminal()
    {
        let options = Options::new().app("hermes-tern-history").record(None);
        let direct = std::env::var_os("TERN_TSP").is_none_or(|v| v != "0")
            && ["TMUX", "STY", "ZELLIJ"]
                .iter()
                .all(|k| std::env::var_os(k).is_none_or(|v| v.is_empty()));
        if direct {
            let seen = Visibility::default();
            let session = Session::<HistoryAction>::with_terminal(seen.wrap(Tty::open()?), options)
                .map_err(native_error)?;
            (session, Some(seen))
        } else {
            (
                Session::<HistoryAction>::connect(options).map_err(native_error)?,
                None,
            )
        }
    } else {
        (None, None)
    };
    let Some(mut session) = native else {
        print!("{}", view.plain()?);
        return Ok(());
    };
    let sf = session
        .open(SurfaceOptions::inline().title("Hermes history"))
        .map_err(native_error)?;
    session
        .stylesheet(sf, "hermes-tern-history", Some(STYLE))
        .map_err(native_error)?;
    if let Some(seen) = &visibility {
        seen.bind(session.surface_id(sf));
        seen.rendering(view.page() as u64);
    }
    session.render(sf, view.render()).map_err(native_error)?;
    loop {
        let before = view.page();
        let close = match next_native_for_surface(&mut session, sf)? {
            NativeInput::Closed => true,
            NativeInput::Wake => false,
            NativeInput::Input(input) => match *input {
                Input::Key(key) => {
                    if let Some(seen) = &visibility {
                        let drawn = seen.key_frame(&key)?;
                        // A raw scroll is never queued behind a not-yet-drawn page
                        // or another scroll. Read-only input while blocked is inert.
                        if (key.is("up") || key.is("down"))
                            && seen.settled()
                            && drawn == Some(view.page() as u64)
                        {
                            session
                                .scroll(
                                    sf,
                                    &format!("main.page-{}", view.page()),
                                    if key.is("up") {
                                        ScrollBy::LineUp
                                    } else {
                                        ScrollBy::LineDown
                                    },
                                )
                                .map_err(native_error)?;
                        }
                    }
                    view.key(&key)
                }
                Input::Msg(action, _) => view.apply(&action),
                _ => false,
            },
        };
        if close {
            break;
        }
        if before != view.page() {
            if let Some(seen) = &visibility {
                seen.rendering(view.page() as u64);
            }
            session.render(sf, view.render()).map_err(native_error)?;
        }
    }
    session.close().map_err(native_error)
}
