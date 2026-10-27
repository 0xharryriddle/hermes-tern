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
    transport::RpcClient,
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
    drain_native_inbox(session, sf)?;
    Ok(NativeInput::Closed)
}
/// Fence an explicit surface transition against already-decoded rejections.
/// Discard queued UI actions; never read another terminal chunk in this adapter.
pub fn drain_native_inbox<M>(session: &mut Session<M>, sf: Surface) -> Result<()> {
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
            Ok(None) | Err(tern_sdk::Error::InputClosed) => return Ok(()),
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

fn connect_native() -> Result<(Option<Session<UiAction>>, Option<Visibility>)> {
    let options = Options::new()
        .app("hermes-tern")
        .feature(feature::SEND)
        .record(None);
    let direct = io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var_os("TERN_TSP").is_none_or(|v| v != "0")
        && ["TMUX", "STY", "ZELLIJ"]
            .iter()
            .all(|k| std::env::var_os(k).is_none_or(|v| v.is_empty()));
    if direct {
        let seen = Visibility::default();
        let session =
            Session::with_terminal(seen.wrap(Tty::open()?), options).map_err(native_error)?;
        Ok((session, Some(seen)))
    } else {
        Ok((Session::connect(options).map_err(native_error)?, None))
    }
}

pub(crate) fn chat_existing(
    client: RpcClient,
    limits: Limits,
    options: SessionOptions,
) -> Result<()> {
    let (session, visibility) = connect_native()?;
    let session = session.ok_or(Error::Unsupported(
        "native Tern unavailable after launcher; no session opened",
    ))?;
    let mut conversation = Conversation::open(client, limits, options)?;
    eprintln!("Hermes session: {}", conversation.state.session_id);
    let result = run_native(&mut conversation, session, visibility);
    let detached = if conversation.state.connected {
        conversation.detach()
    } else {
        Ok(())
    };
    result.and(detached)
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
        connect_native()?
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
    let mut details = crate::runtime::RuntimeDetails::default();
    let mut models = crate::models::ModelPicker::default();
    let mut commands = crate::commands::CommandPanel::default();
    let mut attachments = crate::attachments::AttachmentPanel::default();
    let mut mcp = crate::mcp::McpPanel::default();
    let mut notice = None;
    if let Some(seen) = &visibility {
        seen.bind(session.surface_id(sf));
        seen.rendering(transcript.navigation_revision());
    }

    session
        .render(
            sf,
            view::render_with_details(&conversation.state, &composer, None, &transcript, &details),
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
            details.reconcile(&conversation.state);
            models.reconcile(&conversation.models);
            commands.reconcile(&conversation.commands);
            transcript.layout_changed()?;
        }
        let input = next_native_for_surface(&mut session, sf)?;
        let action = match input {
            NativeInput::Wake => None,
            NativeInput::Closed => break,
            NativeInput::Input(input) => match *input {
                Input::Key(key) => {
                    dirty = true;
                    let visible = visibility
                        .as_ref()
                        .map(|seen| seen.key_visibility(&key))
                        .transpose()?;
                    if key.is("f5") {
                        Some(UiAction::Attachments(!attachments.is_open()))
                    } else if attachments.is_open() {
                        let before = attachments.path().to_owned();
                        let handled = attachments.key(&key);
                        if before != attachments.path() {
                            conversation.attachments.invalidate_prepared();
                        }
                        match handled {
                            Ok(true) => {
                                transcript.layout_changed()?;
                                None
                            }
                            Ok(false) => edit_key(&mut composer, &key)?,
                            Err(error) => {
                                notice = Some(error.to_string());
                                None
                            }
                        }
                    } else if mcp.is_open() {
                        if mcp.key(&key) {
                            if !mcp.is_open() {
                                conversation.mcp.cancel_review();
                            }
                            transcript.layout_changed()?;
                            None
                        } else {
                            edit_key(&mut composer, &key)?
                        }
                    } else if key.is("f2") {
                        Some(UiAction::RuntimeDetails(!details.is_open()))
                    } else if key.is("f3") {
                        Some(UiAction::Models(!models.is_open()))
                    } else if key.is("f4") {
                        Some(UiAction::Commands(!commands.is_open()))
                    } else if commands.is_open() {
                        if commands.key(&conversation.commands, &key) {
                            transcript.layout_changed()?;
                            None
                        } else {
                            edit_key(&mut composer, &key)?
                        }
                    } else if models.is_open() {
                        if models.key(&conversation.models, &key) {
                            transcript.layout_changed()?;
                            None
                        } else {
                            edit_key(&mut composer, &key)?
                        }
                    } else if details.is_open() {
                        if details.key(&conversation.state, &key) {
                            transcript.layout_changed()?;
                            None
                        } else {
                            edit_key(&mut composer, &key)?
                        }
                    } else {
                        let local = if let Some(visible) = visible {
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
                UiAction::Attachments(open) => {
                    mcp.set_open(false);
                    conversation.mcp.cancel_review();
                    attachments.set_open(open);
                    details.set_open(false);
                    models.set_open(false);
                    commands.set_open(false);
                    if open
                        && conversation.attachments.phase()
                            == hermes_tern_gateway::attachments::Phase::Prepared
                    {
                        conversation.attachments.invalidate_prepared();
                    }
                    transcript.layout_changed()?;
                    Ok(())
                }
                UiAction::ReviewAttachment { panel } if attachments.accepts(panel) => {
                    conversation.prepare_attachment(attachments.path())
                }
                UiAction::StageAttachment { panel, prepared } if attachments.accepts(panel) => {
                    conversation.stage_attachment(&prepared)
                }
                UiAction::AddAttachment { panel, receipt } if attachments.accepts(panel) => {
                    if !conversation.can_submit() {
                        Err(Error::Protocol(
                            "session controls pending; no attachment insertion",
                        ))
                    } else {
                        conversation
                            .attachments
                            .reference(&conversation.state, &receipt)
                            .map(str::to_owned)
                            .and_then(|reference| {
                                // Build the whole draft before consuming insertion authority.
                                let mut next = composer.clone();
                                next.end();
                                next.insert(&format!("\n{reference}\n"))?;
                                conversation
                                    .attachments
                                    .added(&conversation.state, &receipt)?;
                                composer = next;
                                Ok(())
                            })
                    }
                }
                UiAction::ReviewAttachment { .. }
                | UiAction::StageAttachment { .. }
                | UiAction::AddAttachment { .. } => Err(Error::Protocol(
                    "attachment controls changed; use the current visible panel",
                )),
                UiAction::Commands(open) => {
                    mcp.set_open(false);
                    conversation.mcp.cancel_review();
                    commands.set_open(open);
                    attachments.set_open(false);
                    details.set_open(false);
                    models.set_open(false);
                    transcript.layout_changed()?;
                    if open
                        && conversation.commands.phase()
                            == hermes_tern_gateway::commands::Phase::Empty
                    {
                        conversation.load_commands()
                    } else {
                        Ok(())
                    }
                }
                UiAction::ReloadCommands if commands.is_open() => conversation.load_commands(),
                UiAction::ReadCommand {
                    catalog,
                    panel,
                    index,
                } if commands.accepts_read(panel, index) => {
                    conversation.read_command(&catalog, index)
                }
                UiAction::CommandPage { from, to } if commands.is_open() => {
                    commands.page(&conversation.commands, from, to);
                    Ok(())
                }
                UiAction::CommandOutputPage { from, to } if commands.is_open() => {
                    commands.output_page(&conversation.commands, from, to);
                    Ok(())
                }
                UiAction::ReloadCommands
                | UiAction::ReadCommand { .. }
                | UiAction::CommandPage { .. }
                | UiAction::CommandOutputPage { .. } => Err(Error::Protocol(
                    "command controls changed; use the current visible panel",
                )),
                UiAction::Models(open) => {
                    mcp.set_open(false);
                    conversation.mcp.cancel_review();
                    models.set_open(open);
                    attachments.set_open(false);
                    details.set_open(false);
                    commands.set_open(false);
                    transcript.layout_changed()?;
                    if open
                        && conversation.models.phase() == hermes_tern_gateway::models::Phase::Empty
                    {
                        conversation.load_models()
                    } else {
                        Ok(())
                    }
                }
                UiAction::ReloadModels if models.is_open() => conversation.load_models(),
                UiAction::SelectModel { catalog, index } if models.visible(index) => {
                    conversation.select_model(&catalog, index)
                }
                UiAction::ApplyModel { control, confirm } if models.is_open() => {
                    conversation.apply_model(&control, confirm)
                }
                UiAction::ModelPage { from, to } if models.is_open() => {
                    models.page(&conversation.models, from, to);
                    Ok(())
                }
                UiAction::ReloadModels
                | UiAction::SelectModel { .. }
                | UiAction::ApplyModel { .. }
                | UiAction::ModelPage { .. } => Err(Error::Protocol(
                    "model controls changed; use the current visible panel",
                )),
                UiAction::OpenMcp { panel } if details.accepts_read(panel) => {
                    details.set_open(false);
                    attachments.set_open(false);
                    models.set_open(false);
                    commands.set_open(false);
                    mcp.set_open(true);
                    transcript.layout_changed()?;
                    if conversation.mcp.blocked() {
                        Ok(())
                    } else {
                        conversation.review_mcp()
                    }
                }
                UiAction::McpPanel(false) => {
                    mcp.set_open(false);
                    conversation.mcp.cancel_review();
                    transcript.layout_changed()?;
                    Ok(())
                }
                UiAction::ReviewMcp { panel } if mcp.accepts(panel) => conversation.review_mcp(),
                UiAction::ConfirmMcp { panel, control } if mcp.accepts(panel) => {
                    conversation.confirm_mcp(&control)
                }
                UiAction::OpenMcp { .. }
                | UiAction::McpPanel(true)
                | UiAction::ReviewMcp { .. }
                | UiAction::ConfirmMcp { .. } => Err(Error::Protocol(
                    "MCP panel changed; use current visible consent",
                )),
                UiAction::RuntimeDetails(open) => {
                    mcp.set_open(false);
                    conversation.mcp.cancel_review();
                    details.set_open(open);
                    attachments.set_open(false);
                    models.set_open(false);
                    commands.set_open(false);
                    transcript.layout_changed()?;
                    Ok(())
                }
                UiAction::RuntimePage { from, to } => {
                    details.page(&conversation.state, from, to);
                    transcript.layout_changed()?;
                    Ok(())
                }
                UiAction::TaskPlan { panel, open } if details.accepts_read(panel) => {
                    details.show_plan(&conversation.state, panel, open);
                    transcript.layout_changed()?;
                    Ok(())
                }
                UiAction::TaskPlanPage {
                    panel,
                    snapshot,
                    from,
                    to,
                } if details.accepts_read(panel) => {
                    details.plan_page(&conversation.state, panel, &snapshot, from, to);
                    transcript.layout_changed()?;
                    Ok(())
                }
                UiAction::TaskPlan { .. } | UiAction::TaskPlanPage { .. } => Err(Error::Protocol(
                    "task plan panel changed; use current controls",
                )),
                UiAction::ReadDelegated { panel } if details.accepts_read(panel) => {
                    let result = conversation.load_delegated(panel);
                    if result.is_ok() {
                        details.reset_delegated_page();
                    }
                    result
                }
                UiAction::DelegatedPage {
                    panel,
                    snapshot,
                    from,
                    to,
                } if details.accepts_read(panel) => {
                    details.delegated_page(
                        &conversation.delegated,
                        &conversation.state,
                        panel,
                        &snapshot,
                        from,
                        to,
                    );
                    Ok(())
                }
                UiAction::ReadDelegated { .. } | UiAction::DelegatedPage { .. } => Err(
                    Error::Protocol("runtime panel changed; use the current visible read control"),
                ),
                UiAction::Send(_)
                | UiAction::SubmitDraft
                | UiAction::Approve { .. }
                | UiAction::Clarify { .. }
                | UiAction::Skip { .. }
                | UiAction::InterruptTurn { .. }
                | UiAction::InspectText { .. }
                | UiAction::Focus
                    if details.is_open()
                        || models.is_open()
                        || commands.is_open()
                        || attachments.is_open()
                        || mcp.is_open() =>
                {
                    Err(Error::Protocol(
                        "return to conversation before using its controls",
                    ))
                }
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
        conversation.delegated.panel_changed(details.control_id());
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
                    if mcp.is_open() {
                        mcp.render(
                            &conversation.state,
                            &conversation.mcp,
                            &composer,
                            notice.as_deref(),
                            conversation.can_submit()
                                && conversation.models.phase()
                                    != hermes_tern_gateway::models::Phase::Deferred,
                        )
                    } else if attachments.is_open() {
                        attachments.render(
                            &conversation.state,
                            &conversation.attachments,
                            &composer,
                            notice.as_deref(),
                            conversation.can_submit(),
                        )
                    } else if commands.is_open() {
                        commands.render(
                            &conversation.state,
                            &conversation.commands,
                            &composer,
                            notice.as_deref(),
                        )
                    } else if models.is_open() {
                        models.render(
                            &conversation.state,
                            &conversation.models,
                            &composer,
                            notice.as_deref(),
                        )
                    } else if details.is_open() {
                        details.render_with_delegated(
                            &conversation.state,
                            &composer,
                            notice.as_deref(),
                            &conversation.delegated,
                        )
                    } else {
                        view::render_with_submission(
                            &conversation.state,
                            &composer,
                            if !conversation.can_submit() && conversation.state.can_submit() {
                                Some(if conversation.mcp.blocked() {
                                    conversation.mcp.notice
                                } else if conversation.attachments.blocked() {
                                    conversation.attachments.notice
                                } else if matches!(
                                    conversation.commands.phase(),
                                    hermes_tern_gateway::commands::Phase::Running
                                        | hermes_tern_gateway::commands::Phase::Unknown
                                ) {
                                    conversation.commands.notice
                                } else {
                                    conversation.models.notice.as_str()
                                })
                            } else {
                                notice.as_deref()
                            },
                            &transcript,
                            conversation.can_submit(),
                        )
                    },
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
                id: if transcript.inspecting()
                    || details.is_open()
                    || models.is_open()
                    || commands.is_open()
                    || attachments.is_open()
                    || mcp.is_open()
                {
                    None
                } else {
                    Some("dock.ed".into())
                },
            }];
            if !details.is_open()
                && !models.is_open()
                && !commands.is_open()
                && !attachments.is_open()
                && !mcp.is_open()
                && let Some(id) = transcript.reveal_target(&conversation.state)
            {
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
