//! Session projection. Only upstream receipts decide admission outcomes.
use crate::protocol::{Event, Inbound};
use hermes_tern_core::{
    error::{Error, Result},
    text::display_text,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

const ROWS: usize = 256;
const TEXT_BYTES: usize = 65536;
const DISPLAY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum Action {
    Submit {
        text: String,
        input_id: String,
    },
    Snapshot,
    Receipt {
        id: String,
    },
    Interrupt {
        generation: u64,
    },
    Approval {
        id: String,
        generation: u64,
        choice: String,
    },
    Clarify {
        id: String,
        generation: u64,
        answer: String,
    },
    ServerReply {
        id: String,
        result: Value,
    },
    Decline {
        id: String,
    },
    Detach,
}
#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    /// Local display incarnation; not an upstream identity or dispatch authority.
    pub control_id: String,
    pub role: String,
    pub text: String,
    /// The retained display prefix omits content; not a canonical-history flag.
    pub truncated: bool,
}
impl Message {
    fn replace_text(&mut self, text: &str) {
        let (text, truncated) = bounded_text(text, TEXT_BYTES);
        if self.text != text || self.truncated != truncated {
            self.control_id = uuid::Uuid::new_v4().to_string();
            self.text = text;
            self.truncated = truncated;
        }
    }
}
#[derive(Debug, Clone)]
pub struct Prompt {
    pub id: String,
    /// Local viewer correlation only; replaced whenever a prompt is hydrated.
    /// Never sent to Hermes as authority or persisted as an upstream identity.
    pub control_id: String,
    pub generation: u64,
    pub kind: String,
    pub text: String,
    pub choices: Vec<String>,
    pub questions: Value,
    pub server: bool,
    pub answered: bool,
    pub expires: Instant,
}
#[derive(Debug)]
pub struct SessionState {
    pub profile: String,
    pub instance: String,
    pub session_id: String,
    pub subscription_id: String,
    pub authority_epoch: u64,
    pub generation: u64,
    pub revision: u64,
    pub replay_epoch: String,
    pub last_sequence: u64,
    pub running: bool,
    pub connected: bool,
    pub messages: VecDeque<Message>,
    pub prompts: BTreeMap<String, Prompt>,
    pub info: Value,
    pub display_truncated: bool,
    pub terminal: Option<Value>,
    pub admissions: BTreeMap<String, String>,
    pub status: String,
    suspended: bool,
    unknown: bool,
    awaiting_admission: bool,
    own_admission: Option<String>,
    stream_id: Option<String>,
    segment: u64,
    observed_completion: Option<String>,
    completed: VecDeque<String>,
}
fn id(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(Error::Protocol("missing or invalid identity"))
}
fn number(v: &Value, key: &str) -> Result<u64> {
    v[key]
        .as_u64()
        .ok_or(Error::Protocol("missing numeric fence"))
}
fn short(text: &str, limit: usize) -> String {
    bounded_text(text, limit).0
}
fn bounded_text(text: &str, limit: usize) -> (String, bool) {
    let s = display_text(text);
    let truncated = s.len() > limit;
    let mut end = s.len().min(limit);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    // Do not retain the discarded input's allocation behind a short prefix.
    (s[..end].to_owned(), truncated)
}
fn content(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_owned();
    }
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
impl SessionState {
    pub fn reconcile(&mut self, snapshot: Value) -> Result<()> {
        let mut fresh = Self::from_snapshot(&self.profile, &self.instance, snapshot)?;
        if fresh.session_id != self.session_id {
            return Err(Error::Protocol("snapshot changed session identity"));
        }
        fresh.own_admission = self.own_admission.clone();
        fresh.awaiting_admission = self.awaiting_admission;
        fresh.unknown |= self.unknown;
        fresh.terminal = self.terminal.clone();
        fresh.completed = self.completed.clone();
        // Missing pending rows may have settled since the snapshot. Keep their
        // identities for receipt queries, never infer they were not admitted.
        for (id, status) in &self.admissions {
            fresh
                .admissions
                .entry(id.clone())
                .or_insert_with(|| status.clone());
        }
        *self = fresh;
        Ok(())
    }
    pub fn from_snapshot(profile: &str, instance: &str, v: Value) -> Result<Self> {
        let mut s = Self {
            profile: profile.into(),
            instance: instance.into(),
            session_id: id(&v, "session_id")?,
            subscription_id: id(&v, "subscription_id")?,
            authority_epoch: number(&v, "authority_epoch")?,
            generation: number(&v, "execution_generation")?,
            revision: number(&v, "revision")?,
            replay_epoch: id(&v, "replay_epoch")?,
            last_sequence: number(&v, "last_sequence")?,
            running: v["running"]
                .as_bool()
                .ok_or(Error::Protocol("missing execution state"))?,
            connected: true,
            messages: VecDeque::new(),
            prompts: BTreeMap::new(),
            info: v["info"].clone(),
            display_truncated: false,
            terminal: None,
            admissions: BTreeMap::new(),
            status: "attached".into(),
            suspended: false,
            unknown: false,
            awaiting_admission: false,
            own_admission: None,
            stream_id: None,
            segment: 0,
            observed_completion: None,
            completed: VecDeque::new(),
        };
        let history = v
            .get("display_messages")
            .or_else(|| v.get("messages"))
            .and_then(Value::as_array)
            .ok_or(Error::Protocol("missing history"))?;
        for (n, row) in history.iter().enumerate() {
            let role = row["role"].as_str().unwrap_or("unknown");
            let text = row["text"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| content(&row["content"]));
            let key = row["row_id"]
                .as_u64()
                .map(|n| format!("row-{n}"))
                .unwrap_or_else(|| format!("history-{n}"));
            if !text.is_empty() {
                s.push(key, role, &text);
            }
        }
        for receipt in v["pending"]
            .as_array()
            .ok_or(Error::Protocol("missing pending admissions"))?
        {
            s.record_receipt(receipt)?;
        }
        for p in v["prompts"]
            .as_array()
            .ok_or(Error::Protocol("missing prompts"))?
        {
            s.add_prompt(p, false, None)?;
        }
        Ok(s)
    }
    pub fn can_submit(&self) -> bool {
        self.connected
            && !self.suspended
            && !self.unknown
            && !self.running
            && !self.awaiting_admission
            && self.admissions.is_empty()
    }
    pub fn outcome_unknown(&self) -> bool {
        self.unknown
    }
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }
    pub fn submit(&mut self, text: &str, input_id: &str) -> Result<Action> {
        if !self.can_submit() {
            return Err(Error::Protocol(
                "session not ready; reconcile before submitting",
            ));
        }
        if text.trim().is_empty()
            || text.len() > TEXT_BYTES
            || input_id.is_empty()
            || input_id.len() > 256
        {
            return Err(Error::Config("empty or oversized prompt identity/text"));
        }
        self.awaiting_admission = true;
        self.terminal = None;
        self.own_admission = None;
        self.push(format!("input-{input_id}"), "user", text);
        self.status = "submitting; admission not yet known".into();
        Ok(Action::Submit {
            text: text.into(),
            input_id: input_id.into(),
        })
    }
    pub fn submission_refused(&mut self) {
        self.awaiting_admission = false;
        self.status = "submission refused, not admitted".into();
    }
    pub fn mark_disconnected(&mut self) {
        self.unknown = self.awaiting_admission || !self.admissions.is_empty() || self.running;
        self.connected = false;
        self.suspended = true;
        self.prompts.clear();
        self.status = "disconnected; resume to reconcile, do not resend blindly".into();
    }
    pub fn accept_receipt(&mut self, receipt: Value) -> Result<()> {
        self.record_receipt(&receipt)?;
        let admission = id(&receipt, "admission_id")?;
        if self.awaiting_admission {
            self.awaiting_admission = false;
            self.own_admission = Some(admission.clone());
        }
        if receipt["status"] == "terminal" {
            if self.own_admission.as_deref() == Some(&admission) {
                self.terminal = Some(receipt.clone());
            }
            let current = receipt["execution_generation"].as_u64() == Some(self.generation);
            if current && self.admissions.is_empty() {
                self.running = false;
            }
            if current {
                self.status = format!(
                    "terminal: {}",
                    short(receipt["outcome"].as_str().unwrap_or("unspecified"), 80)
                );
            }
            // Stored results may differ from live completion (for example redaction).
            // Recover a missing completion, never overwrite one we actually observed
            // or address a stream belonging to a newer execution generation.
            if current
                && self.own_admission.as_deref() == Some(&admission)
                && self.observed_completion.as_deref() != Some(&admission)
                && let Some(text) = receipt["result"]["final_response"].as_str()
            {
                self.update_stream(&admission, text, true);
            }
        }
        Ok(())
    }
    fn record_receipt(&mut self, r: &Value) -> Result<()> {
        if r["ref"]["session_id"] != self.session_id
            || r["ref"]["profile_id"] != self.profile
            || number(r, "authority_epoch")? != self.authority_epoch
        {
            return Err(Error::Protocol("receipt identity mismatch"));
        }
        let admission = id(r, "admission_id")?;
        number(r, "sequence")?;
        if self.completed.contains(&admission) && r["status"] != "terminal" {
            return Ok(());
        }
        match r["status"].as_str() {
            Some("terminal") => {
                self.admissions.remove(&admission);
                if !self.completed.contains(&admission) {
                    self.completed.push_back(admission);
                    if self.completed.len() > 256 {
                        self.completed.pop_front();
                    }
                }
            }
            Some(status @ ("queued" | "started" | "unknown")) => {
                if self.admissions.len() >= 64 && !self.admissions.contains_key(&admission) {
                    return Err(Error::Protocol("pending admission capacity exceeded"));
                }
                self.admissions.insert(admission, status.into());
                if status == "unknown" {
                    self.unknown = true;
                    self.status = "admission outcome unknown".into();
                }
                if status == "started" {
                    self.running = true;
                }
            }
            _ => return Err(Error::Protocol("invalid admission state")),
        }
        if let Some(g) = r["execution_generation"].as_u64() {
            self.advance_generation(g);
        }
        Ok(())
    }
    fn advance_generation(&mut self, generation: u64) {
        if generation > self.generation {
            self.generation = generation;
            self.prompts.clear();
            self.stream_id = None;
            self.segment = 0;
            self.observed_completion = None;
        }
    }
    fn push(&mut self, key: String, role: &str, text: &str) {
        let (text, truncated) = bounded_text(text, TEXT_BYTES);
        self.display_truncated |= truncated;
        self.messages.push_back(Message {
            id: key,
            control_id: uuid::Uuid::new_v4().to_string(),
            role: short(role, 80),
            text,
            truncated,
        });
        self.enforce_display_budget();
    }
    fn enforce_display_budget(&mut self) {
        while self.messages.len() > ROWS
            || self.messages.iter().map(|m| m.text.len()).sum::<usize>() > DISPLAY_BYTES
        {
            self.messages.pop_front();
            self.display_truncated = true;
        }
    }
    fn update_stream(&mut self, admission: &str, text: &str, replace: bool) {
        let continued = self.stream_id.is_some();
        let key = self
            .stream_id
            .get_or_insert_with(|| format!("reply-{admission}-{}", self.segment))
            .clone();
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == key) {
            // Once the prefix has lost bytes, later deltas cannot be joined
            // across that gap. Only a full replacement can recover this row.
            if m.truncated && !replace {
                return;
            }
            let new = if replace {
                text.to_owned()
            } else {
                format!("{}{text}", m.text)
            };
            m.replace_text(&new);
            self.display_truncated |= m.truncated;
            self.enforce_display_budget();
        } else if !continued || replace {
            self.push(key, "assistant", text);
        }
    }
    fn resnapshot(&mut self) -> Vec<Action> {
        if self.suspended {
            return vec![];
        }
        self.suspended = true;
        self.prompts.clear();
        self.status = "replay gap; controls suspended until snapshot".into();
        vec![Action::Snapshot]
    }
    pub fn apply(&mut self, inbound: Inbound) -> Result<Vec<Action>> {
        match inbound {
            Inbound::Disconnected => {
                self.mark_disconnected();
                Ok(vec![])
            }
            Inbound::Event(e) => self.event(e),
            Inbound::ServerRequest { id, method, params } => {
                self.server_request(id, method, params)
            }
            Inbound::Response { .. } => {
                Err(Error::Protocol("response requires request correlation"))
            }
        }
    }
    fn event(&mut self, e: Event) -> Result<Vec<Action>> {
        if e.session_id.as_deref() != Some(&self.session_id) {
            return Ok(vec![]);
        }
        if e.kind == "session.replay_gap" {
            return Ok(self.resnapshot());
        }
        if self.suspended {
            return Ok(vec![]);
        }
        if e.replay_epoch.as_deref() != Some(&self.replay_epoch)
            || e.authority_epoch.is_some_and(|n| n != self.authority_epoch)
        {
            return Ok(self.resnapshot());
        }
        let seq = e.seq.ok_or(Error::Protocol("unsequenced session event"))?;
        if seq <= self.last_sequence {
            return Ok(vec![]);
        }
        if self.last_sequence.checked_add(1) != Some(seq) {
            return Ok(self.resnapshot());
        }
        self.last_sequence = seq;
        if let Some(g) = e.generation {
            if g < self.generation {
                return Ok(vec![]);
            }
            self.advance_generation(g);
        }
        let p = &e.payload;
        let admission = e
            .admission_id
            .as_deref()
            .or_else(|| p["admission_id"].as_str())
            .unwrap_or("observed");
        // A durable receipt can overtake queued stream frames. Do not append
        // those partial bytes to its recovered final text or reopen the turn.
        // A late completion is still allowed to supply the live representation.
        if self.completed.iter().any(|id| id == admission)
            && matches!(
                e.kind.as_str(),
                "message.start" | "message.delta" | "message.interim"
            )
        {
            return Ok(vec![]);
        }
        match e.kind.as_str() {
            "message.start" => {
                self.running = true;
                self.status = "running".into();
            }
            "message.delta" => {
                self.running = true;
                if let Some(text) = p["text"].as_str() {
                    self.update_stream(admission, text, false);
                }
                // A null chunk closes a display segment, not the turn.
                else if p["text"].is_null() {
                    self.stream_id = None;
                    self.segment = self.segment.saturating_add(1);
                }
            }
            "message.interim" => {
                if p["already_streamed"] != true
                    && let Some(text) = p["text"].as_str()
                {
                    self.update_stream(admission, text, false);
                }
                self.stream_id = None;
                self.segment = self.segment.saturating_add(1);
            }
            "message.complete" => {
                if p["response_reused"] != true
                    && let Some(text) = p["text"].as_str()
                    && !text.is_empty()
                {
                    self.update_stream(admission, text, true);
                }
                self.observed_completion = Some(admission.to_owned());
                self.running = false;
                self.prompts.retain(|_, p| p.generation != self.generation);
                self.status = "turn ended; reconciling durable receipt".into();
                if admission != "observed" {
                    return Ok(vec![Action::Receipt {
                        id: admission.into(),
                    }]);
                }
            }
            "approval.request" | "clarify.request" => {
                self.running = true;
                self.add_prompt(p, false, None)?;
            }
            "approval.settled" | "clarify.settled" => {
                if let Some(id) = p["prompt_id"].as_str() {
                    self.prompts.remove(id);
                }
            }
            "request.cancel" => {
                if let Some(id) = p["id"].as_str() {
                    self.prompts.remove(id);
                }
            }
            "session.info" => {
                if let Some(g) = p["execution_generation"].as_u64() {
                    self.advance_generation(g);
                }
                if let Some(r) = p["running"].as_bool() {
                    self.running = r;
                }
                if let Some(r) = p["revision"].as_u64() {
                    self.revision = self.revision.max(r);
                }
                if let Some(fields) = p.as_object()
                    && let Some(info) = self.info.as_object_mut()
                {
                    info.extend(fields.clone());
                }
                if let Some(pending) = p["pending"].as_array() {
                    for r in pending {
                        self.record_receipt(r)?;
                    }
                }
            }
            "tool.start" | "tool.complete" => {
                let tool = id(p, "tool_id")?;
                let key = format!("tool-{}-{tool}", self.generation);
                let text = format!(
                    "{}: {}\n{}",
                    if e.kind == "tool.start" {
                        "running"
                    } else {
                        "finished"
                    },
                    p["name"].as_str().unwrap_or("tool"),
                    p["summary"]
                        .as_str()
                        .or_else(|| p["result_text"].as_str())
                        .unwrap_or("")
                );
                if let Some(m) = self.messages.iter_mut().find(|m| m.id == key) {
                    m.replace_text(&text);
                    self.display_truncated |= m.truncated;
                    self.enforce_display_budget();
                } else {
                    self.push(key, "tool", &text);
                }
            }
            "status.update" | "notice" => {
                self.status = short(
                    p["text"]
                        .as_str()
                        .or_else(|| p["message"].as_str())
                        .unwrap_or("status changed"),
                    256,
                );
            }
            "error" => {
                self.status = "runtime error; inspect session in Hermes".into();
            }
            _ => {} // Unknown non-authority events do not change control state.
        }
        Ok(vec![])
    }
    fn add_prompt(&mut self, p: &Value, server: bool, server_id: Option<String>) -> Result<()> {
        if self.prompts.len() >= 32 {
            return Err(Error::Protocol("prompt capacity exceeded"));
        }
        let generation = if server {
            self.generation
        } else {
            number(p, "execution_generation")?
        };
        if generation != self.generation {
            return Err(Error::Protocol("stale prompt generation"));
        }
        let id = match server_id {
            Some(id) => id,
            None => id(p, "prompt_id")?,
        };
        let kind = p["kind"].as_str().unwrap_or("clarify");
        if !matches!(kind, "approval" | "clarify") {
            return Err(Error::Unsupported("prompt kind"));
        }
        let choices = p["choices"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| short(s, 1024))
                    .take(16)
                    .collect()
            })
            .unwrap_or_default();
        let text = if kind == "approval" {
            format!(
                "{}\n{}",
                p["description"].as_str().unwrap_or(""),
                p["command"].as_str().unwrap_or("")
            )
        } else {
            p["question"]
                .as_str()
                .unwrap_or("Questions require your answer")
                .into()
        };
        let seconds = p["timeout_s"].as_u64().unwrap_or(120).min(600);
        self.prompts.insert(
            id.clone(),
            Prompt {
                id,
                control_id: uuid::Uuid::new_v4().to_string(),
                generation,
                kind: kind.into(),
                text: short(&text, TEXT_BYTES),
                choices,
                questions: p.get("questions").cloned().unwrap_or(Value::Null),
                server,
                answered: false,
                expires: Instant::now() + Duration::from_secs(seconds),
            },
        );
        Ok(())
    }
    fn server_request(&mut self, id: String, method: String, params: Value) -> Result<Vec<Action>> {
        if !self.connected || self.suspended || params["session_id"] != self.session_id {
            return Ok(vec![Action::Decline { id }]);
        }
        match method.as_str() {
            "clarify" => {
                let questions = params["questions"]
                    .as_array()
                    .filter(|a| !a.is_empty() && a.len() <= 5)
                    .ok_or(Error::Protocol("invalid clarify questions"))?;
                let mut seen = std::collections::HashSet::new();
                for q in questions {
                    if !seen.insert(id_value(q, "qid")?) || !q["question"].is_string() {
                        return Err(Error::Protocol("invalid clarify identity"));
                    }
                }
                self.add_prompt(&params, true, Some(id))?;
                Ok(vec![])
            }
            "approval" => {
                let mut p = params;
                p["kind"] = json!("approval");
                self.add_prompt(&p, true, Some(id))?;
                Ok(vec![])
            }
            "sudo" | "secret" | "vault.unlock_prompt" | "vault.save_login" => {
                self.status =
                    "sensitive request declined; use supported Hermes credential UI".into();
                Ok(vec![Action::ServerReply {
                    id,
                    result: json!({"value":""}),
                }])
            }
            _ => {
                self.status = "unsupported viewer request explicitly declined".into();
                Ok(vec![Action::Decline { id }])
            }
        }
    }
    fn live_prompt(&mut self, id: &str, generation: u64) -> Result<&mut Prompt> {
        if !self.connected || self.suspended || self.generation != generation {
            return Err(Error::Protocol("stale or disconnected prompt"));
        }
        self.prompts
            .get_mut(id)
            .filter(|p| p.generation == generation && !p.answered && Instant::now() < p.expires)
            .ok_or(Error::Protocol("prompt expired, answered or absent"))
    }
    pub fn answer_approval(&mut self, id: &str, generation: u64, choice: &str) -> Result<Action> {
        let p = self.live_prompt(id, generation)?;
        if p.kind != "approval"
            || !p.choices.iter().any(|c| c == choice)
            || !matches!(choice, "once" | "deny" | "session" | "always")
        {
            return Err(Error::Protocol("approval choice not offered"));
        }
        p.answered = true;
        Ok(if p.server {
            Action::ServerReply {
                id: id.into(),
                result: json!({"choice":choice}),
            }
        } else {
            Action::Approval {
                id: id.into(),
                generation,
                choice: choice.into(),
            }
        })
    }
    pub fn answer_clarify(&mut self, id: &str, generation: u64, answer: &str) -> Result<Action> {
        if answer.len() > 16384 {
            return Err(Error::Config("clarify answer too long"));
        }
        let p = self.live_prompt(id, generation)?;
        if p.kind != "clarify" || p.server {
            return Err(Error::Protocol("requires batch clarification response"));
        }
        p.answered = true;
        Ok(Action::Clarify {
            id: id.into(),
            generation,
            answer: answer.into(),
        })
    }
    pub fn answer_batch(&mut self, id: &str, generation: u64, answers: Value) -> Result<Action> {
        let p = self.live_prompt(id, generation)?;
        if p.kind != "clarify" || !p.server {
            return Err(Error::Protocol("not a batch clarification"));
        }
        let map = answers
            .as_object()
            .ok_or(Error::Protocol("answers must be object"))?;
        let questions = p
            .questions
            .as_array()
            .ok_or(Error::Protocol("missing questions"))?;
        if map.len() != questions.len()
            || questions.iter().any(|q| {
                map.get(q["qid"].as_str().unwrap_or(""))
                    .is_none_or(|v| !v.is_null() && v.as_str().is_none_or(|s| s.len() > 16384))
            })
        {
            return Err(Error::Protocol("answers must match every question"));
        }
        p.answered = true;
        Ok(Action::ServerReply {
            id: id.into(),
            result: json!({"answers":answers,"outcome":"submitted"}),
        })
    }
    pub fn skip_prompt(&mut self, id: &str, generation: u64) -> Result<Action> {
        let p = self.live_prompt(id, generation)?;
        if p.kind == "approval" {
            return self.answer_approval(id, generation, "deny");
        }
        if p.server {
            p.answered = true;
            Ok(Action::ServerReply {
                id: id.into(),
                result: json!({"outcome":"cancelled"}),
            })
        } else {
            self.answer_clarify(id, generation, "")
        }
    }
    pub fn interrupt(&self) -> Result<Action> {
        if !self.connected || self.suspended || !self.running {
            return Err(Error::Protocol("no reconciled running turn"));
        }
        Ok(Action::Interrupt {
            generation: self.generation,
        })
    }
    pub fn detach(&self) -> Action {
        Action::Detach
    }
}
fn id_value(v: &Value, key: &str) -> Result<String> {
    id(v, key)
}
