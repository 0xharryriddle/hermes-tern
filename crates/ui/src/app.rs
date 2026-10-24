use crate::{
    composer::Composer,
    native_visibility::Visibility,
    transcript::Transcript,
    view::{self, UiAction},
};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
    text::display_text,
};
use hermes_tern_gateway::{
    config::{AttachOptions, Limits, SessionOptions},
    driver::Conversation,
    inspect,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, IsTerminal, Read, Write},
    sync::Arc,
    time::{Duration, Instant},
};
use tern_sdk::{
    Input, Options, Session, Surface, SurfaceOptions,
    keys::Key,
    term::Tty,
    wire::{Event, Op, RevealAt, feature},
};

pub enum NativeInput<M = UiAction> {
    Wake,
    Closed,
    Input(Box<Input<M>>),
}
pub fn next_native<M>(session: &mut Session<M>) -> Result<NativeInput<M>> {
    match session.next(Some(Duration::from_millis(25))) {
        Ok(None) => Ok(NativeInput::Wake),
        Ok(Some(input)) => Ok(NativeInput::Input(Box::new(input))),
        Err(tern_sdk::Error::InputClosed) => Ok(NativeInput::Closed),
        Err(_) => Err(Error::Transport("native terminal input failed")),
    }
}
/// Fence closed-surface actions without losing rejections already decoded by
/// the SDK. This adapter, not the renderer, owns native lifecycle policy.
pub fn next_native_for_surface<M>(session: &mut Session<M>, sf: Surface) -> Result<NativeInput<M>> {
    let input = next_native(session)?;
    if let NativeInput::Input(input) = &input {
        reject_terminal_error(input, session.surface_id(sf))?;
    }
    if session.is_open(sf) {
        return Ok(input);
    }
    // Pinned SDK next() drains its inbox BEFORE checking its waker and BEFORE
    // reading. Signal once: the first None is an exact inbox-end barrier, not
    // a timed poll that might ingest an unbounded stream after surface loss.
    // Production Tty supplies this waker. Refuse an unbounded custom transport.
    session
        .waker()
        .ok_or(Error::Transport(
            "native terminal shutdown requires a waker",
        ))?
        .wake();
    for _ in 0..8192 {
        match session.next(Some(Duration::ZERO)) {
            Ok(Some(input)) => reject_terminal_error(&input, session.surface_id(sf))?,
            Ok(None) | Err(tern_sdk::Error::InputClosed) => return Ok(NativeInput::Closed),
            Err(_) => return Err(Error::Transport("native terminal input failed")),
        }
    }
    // An oversized decoded queue is not evidence of a clean shutdown.
    Err(Error::Transport(
        "native terminal shutdown queue limit exceeded",
    ))
}
fn reject_terminal_error<M>(input: &Input<M>, surface_id: &str) -> Result<()> {
    if let Input::Event(Event::Error(error)) | Input::Msg(_, Event::Error(error)) = input
        && error.sf.as_deref().is_none_or(|id| id == surface_id)
    {
        // Never copy untrusted terminal diagnostics into stderr.
        return Err(Error::Transport("native terminal rejected output"));
    }
    Ok(())
}
pub fn edit_key(composer: &mut Composer, key: &Key) -> Result<Option<UiAction>> {
    if key.is("ctrl+d") || key.is("escape") {
        return Ok(Some(UiAction::Exit));
    }
    if key.is("ctrl+c") {
        return Ok(Some(UiAction::Interrupt));
    }
    match key.name.as_str() {
        "enter" if !key.shift && !key.alt => return Ok(Some(UiAction::SubmitDraft)),
        "enter" => composer.insert("\n")?,
        "backspace" => composer.backspace(),
        "delete" => composer.delete(),
        "left" => composer.left(),
        "right" => composer.right(),
        "home" => composer.home(),
        "end" => composer.end(),
        "paste" => composer.insert(key.text.as_deref().unwrap_or_default())?,
        _ => {
            if let Some(text) = key.typed() {
                composer.insert(text)?;
            }
        }
    }
    Ok(None)
}
fn native_error(_: tern_sdk::Error) -> Error {
    Error::Transport("native terminal rendering failed")
}

pub fn chat(
    attach: AttachOptions,
    options: SessionOptions,
    plain: bool,
    query: Option<String>,
    timeout: Duration,
) -> Result<()> {
    if timeout.is_zero() || timeout > Duration::from_secs(3600) {
        return Err(Error::Config("timeout must be 1..3600 seconds"));
    }
    if !io::stdin().is_terminal() && query.is_none() {
        return Err(Error::Config("noninteractive chat requires --query"));
    }
    if let Some(query) = query.as_deref()
        && (query.trim().is_empty() || query.len() > 65536)
    {
        return Err(Error::Config("query must be nonempty and at most 64 KiB"));
    }
    let (native, visibility) = if !plain && query.is_none() {
        let options = Options::new()
            .app("hermes-tern")
            .feature(feature::SEND)
            .record(None);
        // Preserve SDK mux detection/title ownership. The additional visibility
        // observer is qualified only for the direct TTY path, not mux transports.
        let direct = io::stdin().is_terminal()
            && io::stdout().is_terminal()
            && std::env::var_os("TERN_TSP").is_none_or(|v| v != "0")
            && ["TMUX", "STY", "ZELLIJ"]
                .iter()
                .all(|k| std::env::var_os(k).is_none_or(|v| v.is_empty()));
        if direct {
            let seen = Visibility::default();
            let session = Session::<UiAction>::with_terminal(seen.wrap(Tty::open()?), options)
                .map_err(native_error)?;
            (session, Some(seen))
        } else {
            (
                Session::<UiAction>::connect(options).map_err(native_error)?,
                None,
            )
        }
    } else {
        (None, None)
    };
    let limits = Limits::default();
    let mut conversation =
        Conversation::open(inspect::connect(&attach, &limits)?, limits, options)?;
    eprintln!("Hermes session: {}", conversation.state.session_id);
    let result = match native {
        Some(session) => run_native(&mut conversation, session, visibility),
        None => run_plain(&mut conversation, query, timeout),
    };
    let detached = if conversation.state.connected {
        conversation.detach()
    } else {
        Ok(())
    };
    result.and(detached)
}
fn run_native(
    conversation: &mut Conversation,
    mut session: Session<UiAction>,
    visibility: Option<Visibility>,
) -> Result<()> {
    let sf = session
        .open(SurfaceOptions::inline().title("Hermes"))
        .map_err(native_error)?;
    session
        .stylesheet(sf, "hermes-tern", Some(view::STYLE))
        .map_err(native_error)?;
    if let Some(waker) = session.waker() {
        conversation
            .client
            .set_waker(Arc::new(move || waker.wake()));
    }
    let mut composer = Composer::default();
    let mut transcript = Transcript::default();
    let mut notice = None;
    if let Some(seen) = &visibility {
        seen.bind(session.surface_id(sf));
        seen.rendering(transcript.navigation_revision());
    }

    session
        .render(
            sf,
            view::render_with_transcript(&conversation.state, &composer, None, &transcript),
        )
        .map_err(native_error)?;
    session.focus(sf, Some("dock.ed")).map_err(native_error)?;
    loop {
        let mut dirty = false;
        if conversation.state.connected {
            match conversation.poll() {
                Ok(changed) => dirty = changed,
                Err(e) => {
                    notice = Some(e.to_string());
                    conversation.state.mark_disconnected();
                    dirty = true;
                }
            }
        }
        if dirty {
            transcript.layout_changed()?;
        }
        let input = next_native_for_surface(&mut session, sf)?;
        let action = match input {
            NativeInput::Wake => None,
            NativeInput::Closed => break,
            NativeInput::Input(input) => match *input {
                Input::Key(key) => {
                    dirty = true;
                    let local = if let Some(seen) = &visibility {
                        let visible = seen.key_visibility(&key)?;
                        transcript.keyboard(&conversation.state, &key, visible.positioned)
                    } else if key.is("tab") || key.is("shift+tab") {
                        Err(Error::Unsupported(
                            "keyboard inspection requires direct TTY visibility",
                        ))
                    } else {
                        Ok(false)
                    };
                    match local.and_then(|consumed| {
                        if consumed {
                            Ok(None)
                        } else {
                            edit_key(&mut composer, &key)
                        }
                    }) {
                        Ok(a) => a,
                        Err(e) => {
                            notice = Some(e.to_string());
                            None
                        }
                    }
                }
                Input::Msg(action, _) => match action.validate_control(&conversation.state) {
                    Ok(()) => Some(action),
                    Err(e) => {
                        notice = Some(e.to_string());
                        dirty = true;
                        None
                    }
                },
                Input::Event(event) => {
                    if matches!(event, Event::Resize(_) | Event::Theme(_) | Event::Motion(_)) {
                        transcript.layout_changed()?;
                    }
                    dirty = true;
                    None
                }
                Input::Clipboard(_) => None,
            },
        };
        if let Some(action) = action {
            dirty = true;
            let result = match action {
                UiAction::Exit => break,
                UiAction::InspectText {
                    id,
                    control_id,
                    expand,
                } => transcript.select(&conversation.state, &id, &control_id, expand),
                UiAction::Focus => {
                    transcript.composer()?;
                    if visibility.is_none() {
                        session.focus(sf, Some("dock.ed")).map_err(native_error)?;
                    }
                    Ok(())
                }
                UiAction::Send(_) | UiAction::SubmitDraft if transcript.inspecting() => {
                    Err(Error::Protocol("return to composer before sending"))
                }
                UiAction::Send(text) => conversation.submit(&text),
                UiAction::SubmitDraft => {
                    let r = conversation.submit(composer.text());
                    if r.is_ok() {
                        composer.take();
                    }
                    r
                }
                UiAction::Interrupt | UiAction::InterruptTurn { .. } => conversation
                    .state
                    .interrupt()
                    .and_then(|a| conversation.dispatch(a)),
                UiAction::Approve {
                    id,
                    generation,
                    choice,
                    ..
                } => conversation
                    .state
                    .answer_approval(&id, generation, &choice)
                    .and_then(|a| conversation.dispatch(a)),
                UiAction::Skip { id, generation, .. } => conversation
                    .state
                    .skip_prompt(&id, generation)
                    .and_then(|a| conversation.dispatch(a)),
                UiAction::Clarify { id, generation, .. } => {
                    let batch = conversation
                        .state
                        .prompts
                        .get(&id)
                        .is_some_and(|p| p.server);
                    let action = if batch {
                        strict_json(composer.text().as_bytes()).and_then(|answers| {
                            conversation.state.answer_batch(&id, generation, answers)
                        })
                    } else {
                        conversation
                            .state
                            .answer_clarify(&id, generation, composer.text())
                    };
                    let r = action.and_then(|a| conversation.dispatch(a));
                    if r.is_ok() {
                        composer.take();
                    }
                    r
                }
            };
            notice = result.err().map(|e| e.to_string());
        }
        if let Some(seen) = &visibility {
            dirty |= transcript.positioned(seen.positioned());
        }
        if dirty {
            transcript.reconcile(&conversation.state);
            if let Some(seen) = &visibility {
                seen.rendering(transcript.navigation_revision());
            }
            session
                .render(
                    sf,
                    view::render_with_transcript(
                        &conversation.state,
                        &composer,
                        notice.as_deref(),
                        &transcript,
                    ),
                )
                .map_err(native_error)?;
        }
        // Never enqueue raw focus/reveal on every repaint while credits are
        // blocked. At most one two-op positioning frame is outstanding, and
        // its targets already belong to an acknowledged view incarnation.
        if let Some(seen) = &visibility
            && seen.needs_positioning(transcript.navigation_revision())
        {
            let mut ops = vec![Op::Focus {
                id: if transcript.inspecting() {
                    None
                } else {
                    Some("dock.ed".into())
                },
            }];
            if let Some(id) = transcript.reveal_target(&conversation.state) {
                ops.push(Op::Reveal {
                    id,
                    at: RevealAt::Nearest,
                });
            }
            seen.positioning(transcript.navigation_revision())?;
            session.send(sf, ops).map_err(native_error)?;
        }
    }
    session.close().map_err(native_error)
}

fn run_plain(c: &mut Conversation, query: Option<String>, timeout: Duration) -> Result<()> {
    let noninteractive = query.is_some();
    if noninteractive {
        eprintln!(
            "Noninteractive policy: deny every approval, skip clarification, decline sensitive input. Timeout detaches; runtime may still be settling."
        );
    } else {
        eprintln!(
            "Plain chat: /quit detaches; /cancel interrupts; /approve ID GENERATION CHOICE and /answer ID GENERATION TEXT answer exact human requests. Batch answers use a JSON object."
        );
    }
    if let Some(text) = query {
        c.submit(&text)?;
    }
    let deadline = Instant::now() + timeout;
    let mut shown = BTreeMap::<String, String>::new();
    let mut prompts = BTreeMap::<String, u64>::new();
    let mut pending = VecDeque::<u8>::new();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    loop {
        c.poll()?;
        for row in &c.state.messages {
            let text = display_text(&row.text);
            match shown.get(&row.id) {
                Some(before) if before == &text => {}
                Some(before) if row.role == "assistant" && text.starts_with(before) => {
                    print!("{}", &text[before.len()..]);
                }
                _ => {
                    println!("\n{}: {text}", display_text(&row.role));
                }
            }
            shown.insert(row.id.clone(), text);
        }
        shown.retain(|id, _| c.state.messages.iter().any(|m| &m.id == id));
        io::stdout().flush()?;
        for p in c
            .state
            .prompts
            .values()
            .filter(|p| !p.answered)
            .cloned()
            .collect::<Vec<_>>()
        {
            if p.expires <= Instant::now() {
                continue;
            }
            if noninteractive {
                let action = c.state.skip_prompt(&p.id, p.generation)?;
                c.dispatch(action)?;
            } else if prompts.insert(p.id.clone(), p.generation) != Some(p.generation) {
                println!(
                    "\nHuman {} {} generation {}:\n{}\nChoices: {:?}",
                    p.kind, p.id, p.generation, p.text, p.choices
                );
                if p.server && p.kind == "clarify" {
                    println!("{}", p.questions);
                }
            }
        }
        prompts.retain(|id, _| c.state.prompts.contains_key(id));
        if noninteractive {
            if let Some(receipt) = &c.state.terminal {
                eprintln!(
                    "Admission {}: terminal / {}",
                    receipt["admission_id"], receipt["outcome"]
                );
                return if receipt["outcome"] == "completed" {
                    Ok(())
                } else {
                    Err(Error::Protocol("turn did not complete successfully"))
                };
            }
            if c.state.outcome_unknown() {
                return Err(Error::Disconnected);
            }
            if Instant::now() >= deadline {
                if let Ok(action) = c.state.interrupt() {
                    let _ = c.dispatch(action);
                }
                return Err(Error::Timeout("query"));
            }
            std::thread::sleep(Duration::from_millis(10));
        } else {
            let mut fds = [rustix::event::PollFd::new(
                &stdin,
                rustix::event::PollFlags::IN,
            )];
            rustix::event::poll(&mut fds, 25).map_err(|_| Error::Transport("stdin poll failed"))?;
            if !fds[0].revents().is_empty() {
                let mut buf = [0; 4096];
                let n = input.read(&mut buf)?;
                if n == 0 {
                    return Ok(());
                }
                if pending.len() + n > 65536 {
                    return Err(Error::Config("input line exceeds 64 KiB"));
                }
                pending.extend(&buf[..n]);
            }
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let bytes: Vec<_> = pending.drain(..=end).collect();
                let line =
                    String::from_utf8(bytes).map_err(|_| Error::Config("input must be UTF-8"))?;
                let line = line.trim_end_matches(['\n', '\r']);
                if line == "/quit" {
                    return Ok(());
                }
                let result = plain_action(c, line);
                if let Err(e) = result {
                    eprintln!("hermes-tern: {e}");
                }
            }
        }
    }
}
fn plain_action(c: &mut Conversation, line: &str) -> Result<()> {
    if line == "/cancel" {
        let a = c.state.interrupt()?;
        return c.dispatch(a);
    }
    for (prefix, approval) in [("/approve ", true), ("/answer ", false)] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let mut parts = rest.splitn(3, ' ');
            let id = parts.next().ok_or(Error::Config("missing prompt id"))?;
            let generation = parts
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or(Error::Config("missing generation"))?;
            let answer = parts.next().ok_or(Error::Config("missing answer"))?;
            let action = if approval {
                c.state.answer_approval(id, generation, answer)?
            } else if c.state.prompts.get(id).is_some_and(|p| p.server) {
                c.state
                    .answer_batch(id, generation, strict_json(answer.as_bytes())?)?
            } else {
                c.state.answer_clarify(id, generation, answer)?
            };
            return c.dispatch(action);
        }
    }
    c.submit(line)
}

pub fn sessions(options: &AttachOptions, stored: bool) -> Result<Value> {
    let limits = Limits::default();
    let mut c = inspect::connect(options, &limits)?;
    inspect::qualify(&mut c, &limits)?;
    let result = inspect::call(
        &mut c,
        "session.list",
        serde_json::json!({"scope":if stored {"stored"} else {"live"},"limit":200}),
        limits.request_timeout,
    )?;
    c.close();
    Ok(result)
}
