//! The one command path shared by native and plain clients.
use crate::{
    attachments::Attachments,
    commands::Commands,
    config::{Limits, SessionOptions},
    delegated::Delegated,
    inspect,
    models::{ModelControl, Phase},
    protocol::Inbound,
    session::{Action, SessionState},
    transport::RpcClient,
};
use hermes_tern_core::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone)]
enum Pending {
    Submit,
    Receipt(String),
    Snapshot,
    Control,
    Detach,
    ModelOptions(String),
    ModelChange,
    ModelInfo(u64),
    ModelSnapshot,
    McpReload,
    McpSnapshot,
    CommandCatalog(String),
    CommandRead,
    Attachment,
    Delegated(String),
}
pub struct Conversation {
    pub state: SessionState,
    pub client: RpcClient,
    pub models: ModelControl,
    pub commands: Commands,
    pub attachments: Attachments,
    pub delegated: Delegated,
    pub mcp: crate::mcp::McpReload,
    limits: Limits,
    requests: HashMap<u64, Pending>,
    next_receipt: Instant,
    detached: bool,
}
impl Conversation {
    pub fn open(mut client: RpcClient, limits: Limits, options: SessionOptions) -> Result<Self> {
        if !options.cwd.is_absolute() || !options.cwd.is_dir() {
            return Err(Error::Config("cwd must be an existing absolute directory"));
        }
        let describe = inspect::qualify(&mut client, &limits)?;
        // Handlers below are installed before the first possible session request.
        inspect::call(
            &mut client,
            "client.capabilities",
            json!({"server_requests":true}),
            limits.request_timeout,
        )?;
        let (method, params) = if let Some(session) = options.resume {
            if session.is_empty() || session.len() > 256 {
                return Err(Error::Config("invalid resume identity"));
            }
            (
                "session.resume",
                json!({"session_id":session,"display_projection":true}),
            )
        } else {
            let sources = describe["session_create"]["sources"]
                .as_array()
                .ok_or(Error::Unsupported("session_create.sources"))?;
            if !sources.iter().any(|s| s == "tui") {
                return Err(Error::Unsupported("tui session creation"));
            }
            let mut params = json!({"source":"tui","request_id":uuid::Uuid::new_v4().to_string(),"cwd":options.cwd,"yolo":false});
            if let Some(model) = options.model {
                params["model"] = json!(model);
            }
            if let Some(provider) = options.provider {
                params["provider"] = json!(provider);
            }
            let supported = describe["session_create"]["parameters"]
                .as_array()
                .ok_or(Error::Unsupported("session creation field report"))?;
            for field in params
                .as_object()
                .ok_or(Error::Protocol("create parameters"))?
                .keys()
            {
                if !supported.iter().any(|s| s == field) {
                    return Err(Error::Unsupported("required session creation field"));
                }
            }
            ("session.create", params)
        };
        let request = client.request(method, params)?;
        let deadline = Instant::now() + limits.request_timeout;
        let mut buffered = Vec::new();
        let mut size = 0;
        let snapshot = loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(Error::Timeout("session attachment"))?;
            let inbound = client.recv(remaining)?;
            match inbound {
                Inbound::Response { id, result } if id == request => {
                    break result.map_err(Error::Refused)?;
                }
                Inbound::Response { .. } | Inbound::Disconnected => {
                    return Err(Error::Protocol("unexpected attach response"));
                }
                other => {
                    size += match &other {
                        Inbound::Event(e) => e.payload.to_string().len(),
                        Inbound::ServerRequest { params, .. } => params.to_string().len(),
                        _ => 0,
                    };
                    if buffered.len() >= limits.max_events || size > 8 * 1024 * 1024 {
                        return Err(Error::Protocol("attach event buffer exceeded"));
                    }
                    buffered.push(other);
                }
            }
        };
        let state = SessionState::from_snapshot(&client.profile_id, &client.instance_id, snapshot)?;
        // Resuming must not silently inherit an existing bypass session.
        if state.info["yolo"] == true {
            return Err(Error::Unsupported(
                "resuming a yolo session is not allowed by this client",
            ));
        }
        let mut result = Self {
            state,
            client,
            limits,
            requests: HashMap::new(),
            next_receipt: Instant::now(),
            detached: false,
            models: ModelControl::default(),
            commands: Commands::default(),
            attachments: Attachments::default(),
            delegated: Delegated::default(),
            mcp: crate::mcp::McpReload::default(),
        };
        for inbound in buffered {
            for action in result.state.apply(inbound)? {
                result.dispatch(action)?;
            }
        }
        Ok(result)
    }
    pub fn submit(&mut self, text: &str) -> Result<()> {
        if !self.can_submit() {
            return Err(Error::Protocol(
                "session not ready or model change not reconciled; do not submit yet",
            ));
        }
        let action = self.state.submit(text, &uuid::Uuid::new_v4().to_string())?;
        self.dispatch(action)
    }
    pub fn can_submit(&self) -> bool {
        self.state.can_submit()
            && !matches!(
                self.models.phase(),
                Phase::Sending | Phase::Reconciling | Phase::Unknown
            )
            && !matches!(
                self.commands.phase(),
                crate::commands::Phase::Running | crate::commands::Phase::Unknown
            )
            && !self.attachments.blocked()
            && !self.mcp.blocked()
    }
    pub fn review_mcp(&mut self) -> Result<()> {
        if !self.can_submit() || self.models.phase() == Phase::Deferred {
            return Err(Error::Protocol(
                "session controls unsettled; MCP review unavailable",
            ));
        }
        self.mcp.review(&self.state)
    }
    pub fn confirm_mcp(&mut self, control: &str) -> Result<()> {
        if !self.can_submit() || self.models.phase() == Phase::Deferred {
            return Err(Error::Protocol("session controls unsettled; no MCP reload"));
        }
        let params = self.mcp.confirm(&self.state, control)?;
        if let Err(error) = self.optional_request("reload.mcp", params, Pending::McpReload) {
            self.mcp.not_enqueued();
            return Err(error);
        }
        Ok(())
    }
    fn optional_request(&mut self, method: &str, params: Value, kind: Pending) -> Result<()> {
        if self.requests.len() >= self.limits.max_pending_requests {
            return Err(Error::Transport("command limit exceeded"));
        }
        let id = self.client.request(method, params)?;
        self.requests.insert(id, kind);
        Ok(())
    }
    pub fn load_commands(&mut self) -> Result<()> {
        let catalog = self.commands.loading(&self.state)?;
        if let Err(error) = self.optional_request(
            "commands.catalog",
            json!({"session_id":self.state.session_id}),
            Pending::CommandCatalog(catalog),
        ) {
            self.commands.failed("Command catalog request not enqueued; reload explicitly after pending requests settle");
            return Err(error);
        }
        Ok(())
    }
    pub fn prepare_attachment(&mut self, path: &str) -> Result<()> {
        if !self.can_submit() || self.models.phase() == Phase::Deferred {
            return Err(Error::Protocol(
                "attachment review unavailable until session controls settle",
            ));
        }
        self.attachments.prepare(&self.state, path)
    }
    pub fn load_delegated(&mut self, panel: u64) -> Result<()> {
        let token = self.delegated.loading(&self.state, panel)?;
        if let Err(error) = self.optional_request(
            "subagent.list",
            json!({"session_id":self.state.session_id}),
            Pending::Delegated(token),
        ) {
            self.delegated
                .failed("Delegated agents unavailable; read not enqueued, reload explicitly");
            return Err(error);
        }
        Ok(())
    }
    pub fn stage_attachment(&mut self, id: &str) -> Result<()> {
        if !self.can_submit() || self.models.phase() == Phase::Deferred {
            return Err(Error::Protocol(
                "attachment upload unavailable until session controls settle",
            ));
        }
        let params = self.attachments.stage(&self.state, id)?;
        if let Err(error) = self.optional_request("file.attach", params, Pending::Attachment) {
            self.attachments.not_enqueued();
            return Err(error);
        }
        Ok(())
    }
    pub fn read_command(&mut self, catalog: &str, index: usize) -> Result<()> {
        if !self.can_submit() || self.models.phase() == Phase::Deferred {
            return Err(Error::Protocol(
                "owner read unavailable while session controls are pending",
            ));
        }
        let request = self.commands.read(&self.state, catalog, index)?;
        if let Err(error) =
            self.optional_request(request.method, request.params, Pending::CommandRead)
        {
            self.commands.failed(
                "Owner read was not enqueued; reload explicitly after pending requests settle",
            );
            return Err(error);
        }
        Ok(())
    }
    pub fn load_models(&mut self) -> Result<()> {
        if self.mcp.blocked()
            || !self.state.connected
            || self.state.is_suspended()
            || matches!(
                self.models.phase(),
                Phase::Loading
                    | Phase::Sending
                    | Phase::Deferred
                    | Phase::Reconciling
                    | Phase::Unknown
            )
        {
            return Err(Error::Protocol(
                "model controls unavailable until reconciliation",
            ));
        }
        let id = self.models.loading();
        if let Err(error) = self.optional_request("model.options", json!({"session_id":self.state.session_id,"explicit_only":true,"include_unconfigured":false,"refresh":false}), Pending::ModelOptions(id)) {
            self.models.failed("Model inventory request was not enqueued; retry explicitly after pending requests settle");
            return Err(error);
        }
        Ok(())
    }
    pub fn select_model(&mut self, catalog: &str, index: usize) -> Result<()> {
        self.models.select(&self.state, catalog, index)
    }
    pub fn apply_model(&mut self, control: &str, confirm: bool) -> Result<()> {
        if self.mcp.blocked() {
            return Err(Error::Protocol(
                "MCP reload pending or unknown; no model mutation",
            ));
        }
        if self.attachments.blocked() {
            return Err(Error::Protocol(
                "attachment staging pending or unknown; no model mutation",
            ));
        }
        if matches!(
            self.commands.phase(),
            crate::commands::Phase::Running | crate::commands::Phase::Unknown
        ) {
            return Err(Error::Protocol(
                "owner command pending or unknown; no model mutation",
            ));
        }
        let params = self.models.apply(&self.state, control, confirm)?;
        if let Err(e) = self.optional_request("session.mutate", params, Pending::ModelChange) {
            self.models.disconnected();
            return Err(e);
        }
        Ok(())
    }
    pub fn dispatch(&mut self, action: Action) -> Result<()> {
        let sid = &self.state.session_id;
        let (method, params, kind) = match action {
            Action::Submit { text, input_id } => (
                "prompt.submit",
                json!({"session_id":sid,"text":text,"input_id":input_id,"clarify_batch":true}),
                Pending::Submit,
            ),
            Action::Snapshot => {
                if self
                    .requests
                    .values()
                    .any(|p| matches!(p, Pending::Snapshot))
                {
                    return Ok(());
                }
                (
                    "session.resume",
                    json!({"session_id":sid,"display_projection":true}),
                    Pending::Snapshot,
                )
            }
            Action::Receipt { id } => {
                if self
                    .requests
                    .values()
                    .any(|p| matches!(p,Pending::Receipt(existing) if existing==&id))
                {
                    return Ok(());
                }
                (
                    "prompt.receipt",
                    json!({"session_id":sid,"admission_id":id,"include_result":true}),
                    Pending::Receipt(id),
                )
            }
            Action::Interrupt { generation } => (
                "session.interrupt",
                json!({"session_id":sid,"execution_generation":generation}),
                Pending::Control,
            ),
            Action::Approval {
                id,
                generation,
                choice,
            } => (
                "approval.respond",
                json!({"session_id":sid,"execution_generation":generation,"prompt_id":id,"choice":choice}),
                Pending::Control,
            ),
            Action::Clarify {
                id,
                generation,
                answer,
            } => (
                "clarify.respond",
                json!({"session_id":sid,"execution_generation":generation,"prompt_id":id,"answer":answer}),
                Pending::Control,
            ),
            Action::ServerReply { id, result } => return self.client.respond_server(&id, result),
            Action::Decline { id } => return self.client.decline_server(&id),
            Action::Detach => (
                "session.detach",
                json!({"session_id":sid,"subscription_id":self.state.subscription_id}),
                Pending::Detach,
            ),
        };
        if self.requests.len() >= self.limits.max_pending_requests {
            self.state.mark_disconnected();
            return Err(Error::Transport("command limit exceeded"));
        }
        match self.client.request(method, params) {
            Ok(id) => {
                self.requests.insert(id, kind);
                Ok(())
            }
            Err(e) => {
                self.state.mark_disconnected();
                Err(e)
            }
        }
    }
    pub fn poll(&mut self) -> Result<bool> {
        let mut changed = false;
        for _ in 0..self.limits.max_events {
            let inbound = match self.client.try_recv() {
                Ok(Some(v)) => v,
                Ok(None) => break,
                Err(e) => {
                    self.state.mark_disconnected();
                    self.models.disconnected();
                    self.commands.disconnected();
                    self.attachments.disconnected();
                    self.delegated.disconnected();
                    self.mcp.disconnected();
                    return Err(e);
                }
            };
            changed = true;
            match inbound {
                Inbound::Response { id, result } => {
                    let kind = self
                        .requests
                        .remove(&id)
                        .ok_or(Error::Protocol("uncorrelated application response"))?;
                    let value = match result {
                        Ok(v) => v,
                        Err(e) => {
                            if matches!(kind, Pending::McpReload | Pending::McpSnapshot) {
                                self.mcp.unknown();
                                continue;
                            }
                            if matches!(kind, Pending::Delegated(_)) {
                                self.delegated.failed("Delegated agents unavailable; owner refused the read, not an empty roster");
                                continue;
                            }
                            if matches!(kind, Pending::Attachment) {
                                self.attachments.unknown();
                                continue;
                            }
                            if matches!(kind, Pending::CommandCatalog(_)) {
                                self.commands.failed(
                                    "Command catalog unavailable; no command was dispatched",
                                );
                                continue;
                            }
                            if matches!(kind, Pending::CommandRead) {
                                self.commands.unknown("Owner read refused or outcome unavailable; no blind command retry");
                                continue;
                            }
                            if matches!(kind, Pending::ModelInfo(_) | Pending::ModelSnapshot) {
                                self.models.unknown(&format!("Model readback unavailable: {e}; outcome unknown, reconnect/reconcile before retry"));
                                continue;
                            }
                            if matches!(kind, Pending::ModelChange) {
                                self.models.unknown(&format!("Model operation unavailable: {e}; outcome unknown, reconnect/reconcile before retry"));
                                continue;
                            }
                            if matches!(kind, Pending::ModelOptions(_)) {
                                self.models.failed(&format!(
                                    "Model controls unavailable: {e}; owner route unchanged"
                                ));
                                continue;
                            }
                            if matches!(kind, Pending::Submit) {
                                self.state.submission_refused();
                            }
                            return Err(Error::Refused(e));
                        }
                    };
                    match kind {
                        Pending::McpReload => {
                            if self.mcp.result(&self.state, value).is_err()
                                || self.optional_request(
                                    "session.resume",
                                    json!({"session_id":self.state.session_id,"display_projection":true}),
                                    Pending::McpSnapshot,
                                ).is_err()
                            {
                                self.mcp.unknown();
                            }
                        }
                        Pending::McpSnapshot => {
                            if self.mcp.validate_readback(&self.state, &value).is_err()
                                || self.state.reconcile(value.clone()).is_err()
                                || self.mcp.validate_readback(&self.state, &value).is_err()
                            {
                                self.mcp.unknown();
                            } else {
                                self.mcp.reconciled();
                            }
                        }
                        Pending::Delegated(token) => {
                            let _ = self.delegated.loaded(&self.state, &token, value);
                        }
                        Pending::Attachment => {
                            let _ = self.attachments.result(&self.state, value);
                        }
                        Pending::CommandCatalog(catalog) => {
                            let _ = self.commands.loaded(&self.state, &catalog, value);
                        }
                        Pending::CommandRead => {
                            let _ = self.commands.result(&self.state, value);
                        }
                        Pending::ModelOptions(catalog) => {
                            let _ = self.models.loaded(&catalog, value);
                        }
                        Pending::ModelChange => {
                            match self.models.result(&self.state, value) {
                                Ok(true) => self.optional_request("session.info", json!({"session_id":self.state.session_id}), Pending::ModelInfo(self.state.last_sequence))?,
                                Ok(false) => {},
                                Err(_) => self.models.unknown("Malformed model response; outcome unknown. Reconnect/reconcile; never blindly retry"),
                            }
                        }
                        Pending::ModelInfo(sequence) => {
                            if !value.is_object() || value.get("profile_id").is_some_and(|v| v != &self.state.profile) {
                                self.models.unknown("Invalid effective route response; reconnect/reconcile before another change");
                            } else {
                                if sequence == self.state.last_sequence { self.state.info = value; }
                                // session.info has no revision/generation fields on the pinned owner.
                                // A complete read snapshot, not the selected target, supplies those fences.
                                self.optional_request("session.resume", json!({"session_id":self.state.session_id,"display_projection":true}), Pending::ModelSnapshot)?;
                            }
                        }
                        Pending::Submit => {
                            if value["voice_stopped"] == true {
                                self.state.submission_refused();
                                self.state.status = "voice stopped; prompt not admitted".into();
                            } else {
                                self.state.accept_receipt(value)?;
                            }
                        }
                        Pending::Receipt(expected) => {
                            if value["admission_id"] != expected {
                                return Err(Error::Protocol("receipt response identity mismatch"));
                            }
                            self.state.accept_receipt(value)?;
                        }
                        Pending::Snapshot => {
                            self.state.reconcile(value)?;
                        }
                        Pending::ModelSnapshot => {
                            self.models.validate_readback(&value)?;
                            self.state.reconcile(value)?;
                            self.models.reconciled();
                        }
                        Pending::Control => {} // An interrupt acknowledgment is not a terminal outcome.
                        Pending::Detach => {
                            if value["detached"] != true {
                                return Err(Error::Protocol("detach not confirmed"));
                            }
                            self.detached = true;
                            return Ok(true);
                        }
                    }
                }
                other => {
                    let disconnected = matches!(other, Inbound::Disconnected);
                    let before = self.state.last_sequence;
                    let update = match &other {
                        Inbound::Event(e) if e.kind == "session.updated" => Some(e.clone()),
                        _ => None,
                    };
                    for action in self.state.apply(other)? {
                        self.dispatch(action)?;
                    }
                    if let Some(e) = update
                        && e.seq
                            .is_some_and(|seq| seq > before && seq == self.state.last_sequence)
                        && !self.state.is_suspended()
                        && self.models.updated(&self.state, &e.payload)
                    {
                        self.optional_request(
                            "session.resume",
                            json!({"session_id":self.state.session_id,"display_projection":true}),
                            Pending::ModelSnapshot,
                        )?;
                    }
                    if disconnected {
                        self.models.disconnected();
                        self.commands.disconnected();
                        self.attachments.disconnected();
                        self.delegated.disconnected();
                        self.mcp.disconnected();
                    }
                }
            }
        }
        if !self.state.is_suspended() && Instant::now() >= self.next_receipt {
            let ids: Vec<_> = self.state.admissions.keys().cloned().collect();
            for id in ids {
                self.dispatch(Action::Receipt { id })?;
            }
            self.next_receipt = Instant::now() + Duration::from_millis(250);
        }
        self.delegated.reconcile(&self.state);
        Ok(changed)
    }
    pub fn detach(&mut self) -> Result<()> {
        self.dispatch(Action::Detach)?;
        let deadline = Instant::now() + self.limits.request_timeout;
        while !self.detached {
            self.poll()?;
            if Instant::now() >= deadline {
                return Err(Error::Timeout("detach"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.client.close();
        Ok(())
    }
}
