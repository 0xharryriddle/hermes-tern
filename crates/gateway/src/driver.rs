//! The one command path shared by native and plain clients.
use crate::{
    config::{Limits, SessionOptions},
    inspect,
    protocol::Inbound,
    session::{Action, SessionState},
    transport::RpcClient,
};
use hermes_tern_core::error::{Error, Result};
use serde_json::json;
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
}
pub struct Conversation {
    pub state: SessionState,
    pub client: RpcClient,
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
        };
        for inbound in buffered {
            for action in result.state.apply(inbound)? {
                result.dispatch(action)?;
            }
        }
        Ok(result)
    }
    pub fn submit(&mut self, text: &str) -> Result<()> {
        let action = self.state.submit(text, &uuid::Uuid::new_v4().to_string())?;
        self.dispatch(action)
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
                            if matches!(kind, Pending::Submit) {
                                self.state.submission_refused();
                            }
                            return Err(Error::Refused(e));
                        }
                    };
                    match kind {
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
                        Pending::Snapshot => self.state.reconcile(value)?,
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
                    for action in self.state.apply(other)? {
                        self.dispatch(action)?;
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
