//! Explicit profile-wide MCP reload consent followed by a separately correlated session reattach.
//! Local fences are not owner CAS or durable deduplication. The driver owns connection/request
//! correlation and must reconcile the validated, dedicated resume before calling `reconciled`.
use crate::session::SessionState;
use hermes_tern_core::{
    error::{Error, Result},
    text::is_bidi_control,
};
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_NAMES: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_MESSAGE_BYTES: usize = 16_384;
const MAX_PROJECTION_BYTES: usize = 65_536;
const MAX_ATTEMPTS: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Empty,
    Review,
    Pending,
    Reconciling,
    Reconciled,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOutcome {
    Applied,
    Unchanged,
    Busy,
}
/// Public owner reports only; no raw diagnostics, host acknowledgement, revision or config.
#[derive(Debug)]
pub struct Report {
    pub session: SessionOutcome,
    pub unavailable: Vec<String>,
    pub session_servers: Option<Vec<String>>,
}
fn safe_name(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|name| {
            !name.trim().is_empty()
                && name.len() <= MAX_NAME_BYTES
                && !name.chars().any(|c| c.is_control() || is_bidi_control(c))
        })
        .ok_or(Error::Protocol("invalid MCP report identity"))
}
fn names(value: Option<&Value>) -> Result<Option<Vec<String>>> {
    let rows = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(rows)) if rows.len() <= MAX_NAMES => rows,
        _ => return Err(Error::Protocol("invalid or oversized MCP server names")),
    };
    let mut seen = HashSet::with_capacity(rows.len());
    let mut names = Vec::with_capacity(rows.len());
    for row in rows {
        let name = safe_name(row)?;
        if !seen.insert(name) {
            return Err(Error::Protocol("duplicate MCP server name"));
        }
        names.push(name.to_owned());
    }
    Ok(Some(names))
}
impl Report {
    /// Validate all known field types, retaining only the complete bounded public projection.
    ///
    /// # Errors
    /// Rejects malformed known fields, unsafe/duplicate names and exceeded local bounds.
    /// Transport bounds must already have been enforced by the caller.
    pub fn parse(value: Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or(Error::Protocol("invalid MCP reload acknowledgement"))?;
        if value["status"] != "reloaded" {
            return Err(Error::Protocol("MCP reload was not acknowledged"));
        }
        let session = match value["session"].as_str() {
            Some("applied") => SessionOutcome::Applied,
            Some("unchanged") => SessionOutcome::Unchanged,
            Some("busy") => SessionOutcome::Busy,
            _ => return Err(Error::Protocol("invalid MCP session outcome")),
        };
        match object.get("message") {
            None | Some(Value::Null) => {}
            Some(Value::String(message)) if message.len() <= MAX_MESSAGE_BYTES => {}
            _ => return Err(Error::Protocol("invalid or oversized MCP message")),
        }
        match object.get("loaded_rev") {
            None | Some(Value::Null) => {}
            Some(Value::String(rev))
                if rev.len() == 12
                    && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) => {}
            _ => return Err(Error::Protocol("invalid MCP revision acknowledgement")),
        }
        for key in ["coalesced", "turn_isolation"] {
            if object
                .get(key)
                .is_some_and(|v| !v.is_null() && !v.is_boolean())
            {
                return Err(Error::Protocol("invalid MCP acknowledgement flag"));
            }
        }
        let unavailable = names(object.get("unavailable"))?.unwrap_or_default();
        let session_servers = names(object.get("session_servers"))?;
        // Charge the complete escaped public JSON, including punctuation and labels.
        // Private fields and the validated-but-discarded diagnostic never enter it.
        let projection = json!({"session":value["session"],"unavailable":unavailable,"session_servers":session_servers});
        if serde_json::to_vec(&projection)
            .map_err(|_| Error::Protocol("invalid MCP report projection"))?
            .len()
            > MAX_PROJECTION_BYTES
        {
            return Err(Error::Protocol("MCP report display limit exceeded"));
        }
        Ok(Self {
            session,
            unavailable,
            session_servers,
        })
    }
}
#[derive(PartialEq, Eq)]
struct Fence {
    profile: String,
    instance: String,
    session: String,
    authority: u64,
    revision: u64,
    generation: u64,
    replay: String,
    sequence: u64,
    subscription: String,
    model: Value,
    provider: Value,
}
impl Fence {
    fn of(s: &SessionState) -> Self {
        Self {
            profile: s.profile.clone(),
            instance: s.instance.clone(),
            session: s.session_id.clone(),
            authority: s.authority_epoch,
            revision: s.revision,
            generation: s.generation,
            replay: s.replay_epoch.clone(),
            sequence: s.last_sequence,
            subscription: s.subscription_id.clone(),
            model: s.info["model"].clone(),
            provider: s.info["provider"].clone(),
        }
    }
    fn same_owner(&self, s: &SessionState) -> bool {
        s.connected
            && self.profile == s.profile
            && self.instance == s.instance
            && self.session == s.session_id
            && self.authority == s.authority_epoch
    }
}
#[derive(Default)]
pub struct McpReload {
    phase: Phase,
    id: String,
    fence: Option<Fence>,
    report: Option<Report>,
    attempts: usize,
    pub notice: &'static str,
}
impl McpReload {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }
    pub fn attempts(&self) -> usize {
        self.attempts
    }
    pub fn blocked(&self) -> bool {
        matches!(
            self.phase,
            Phase::Pending | Phase::Reconciling | Phase::Unknown
        )
    }
    /// Capture local review authority only; no owner request is issued.
    ///
    /// # Errors
    /// Rejects unsettled reloads, non-idle/unsafe sessions or exhausted admission capacity.
    pub fn review(&mut self, s: &SessionState) -> Result<()> {
        if self.blocked() || !s.can_submit() || s.info["yolo"] == true {
            return Err(Error::Protocol(
                "MCP review unavailable until session is idle and reconciled",
            ));
        }
        if self.attempts >= MAX_ATTEMPTS {
            return Err(Error::Protocol(
                "eight admitted MCP reloads reached for this connection",
            ));
        }
        self.id = uuid::Uuid::new_v4().to_string();
        self.fence = Some(Fence::of(s));
        self.report = None;
        self.phase = Phase::Review;
        self.notice = "Local review only; Confirm reconciles MCP across every profile chat, may change server processes/network, then reattaches this session (policy/adoption/hooks). External changes may race; no CAS or automatic retry";
        Ok(())
    }
    pub fn cancel_review(&mut self) {
        if self.phase == Phase::Review {
            self.phase = Phase::Empty;
            self.id = uuid::Uuid::new_v4().to_string();
            self.fence = None;
            self.report = None;
            self.notice = "Local MCP review cancelled; no reload or session reattach sent";
        }
    }
    /// Admit one minimal request for the exact still-current local consent.
    ///
    /// # Errors
    /// Rejects stale/repeated consent, changed fences or exhausted admission capacity.
    pub fn confirm(&mut self, s: &SessionState, id: &str) -> Result<Value> {
        if self.phase != Phase::Review
            || id != self.id
            || !s.can_submit()
            || s.info["yolo"] == true
            || self.attempts >= MAX_ATTEMPTS
            || !self.fence.as_ref().is_some_and(|f| *f == Fence::of(s))
        {
            return Err(Error::Protocol(
                "MCP consent changed; review the current owner explicitly",
            ));
        }
        self.attempts += 1;
        self.phase = Phase::Pending;
        self.notice =
            "MCP reload pending; no automatic retry; prompt and mutation controls withheld";
        Ok(json!({"session_id":s.session_id,"confirm":true}))
    }
    /// Only a definite local pre-enqueue refusal may restore admission capacity.
    pub fn not_enqueued(&mut self) {
        if self.phase == Phase::Pending {
            self.attempts -= 1;
            self.phase = Phase::Failed;
            self.id = uuid::Uuid::new_v4().to_string();
            self.fence = None;
            self.notice =
                "MCP reload was not enqueued; a fresh explicit Review and Confirm is required";
        }
    }
    /// Accept only the driver's one correlated reply; this is not a readback.
    ///
    /// # Errors
    /// A changed qualified owner or malformed admitted reply leaves outcome Unknown.
    pub fn result(&mut self, s: &SessionState, value: Value) -> Result<()> {
        if self.phase != Phase::Pending {
            return Err(Error::Protocol("MCP result without a pending request"));
        }
        if !self.fence.as_ref().is_some_and(|f| f.same_owner(s)) {
            self.unknown();
            return Err(Error::Protocol("MCP result owner changed; outcome unknown"));
        }
        match Report::parse(value) {
            Ok(report) => {
                self.report = Some(report);
                self.id = uuid::Uuid::new_v4().to_string();
                self.phase = Phase::Reconciling;
                self.notice = "Profile MCP result reported; awaiting the consented same-connection session reattach, not a health or atomic revision check";
                Ok(())
            }
            Err(error) => {
                self.unknown();
                Err(error)
            }
        }
    }
    /// Check a dedicated post-result resume before the driver applies `SessionState::reconcile`.
    /// New replay epochs may restart sequence numbers; the reducer must also accept the snapshot.
    ///
    /// # Errors
    /// Rejects changed owner/session authority, unsafe policy, malformed fences or regression.
    pub fn validate_readback(&self, s: &SessionState, snapshot: &Value) -> Result<()> {
        let fence = self
            .fence
            .as_ref()
            .ok_or(Error::Protocol("MCP readback without review"))?;
        let report = self
            .report
            .as_ref()
            .ok_or(Error::Protocol("MCP readback without result"))?;
        let generation = if report.session == SessionOutcome::Applied {
            fence
                .generation
                .checked_add(1)
                .ok_or(Error::Protocol("MCP generation floor overflow"))?
        } else {
            fence.generation
        };
        let replay = safe_name(&snapshot["replay_epoch"])?;
        safe_name(&snapshot["subscription_id"])?;
        let sequence = snapshot["last_sequence"]
            .as_u64()
            .ok_or(Error::Protocol("invalid MCP readback sequence"))?;
        if self.phase != Phase::Reconciling
            || !fence.same_owner(s)
            || snapshot["session_id"] != fence.session
            || snapshot["authority_epoch"].as_u64() != Some(fence.authority)
            || snapshot["info"]["profile_id"] != fence.profile
            || snapshot["info"]["yolo"].as_bool() != Some(false)
            || snapshot["revision"]
                .as_u64()
                .is_none_or(|n| n < fence.revision.max(s.revision))
            || snapshot["execution_generation"]
                .as_u64()
                .is_none_or(|n| n < generation.max(s.generation))
            || (replay == fence.replay && sequence < fence.sequence)
            || (replay == s.replay_epoch && sequence < s.last_sequence)
        {
            return Err(Error::Protocol(
                "MCP session reattach changed authority, policy or regressed fences",
            ));
        }
        Ok(())
    }
    /// Call only after the dedicated resume passed validation and reducer reconciliation.
    pub fn reconciled(&mut self) {
        if self.phase != Phase::Reconciling {
            return;
        }
        let Some(report) = &self.report else { return };
        self.phase = Phase::Reconciled;
        self.id = uuid::Uuid::new_v4().to_string();
        self.notice = match (report.session, report.unavailable.is_empty()) {
            (SessionOutcome::Busy, true) => {
                "MCP reattach readback complete. Profile MCP reconcile reported; chat policy was busy; reattach reported state. Server health and atomic MCP revision/config match are not established"
            }
            (SessionOutcome::Busy, false) => {
                "MCP reattach readback complete. Profile MCP reconcile reported with unavailable servers; chat policy was busy; reattach reported state. Server health and atomic MCP revision/config match are not established"
            }
            (_, false) => {
                "MCP reattach readback complete. Profile MCP reconcile reported with unavailable servers. Server health and atomic MCP revision/config match are not established"
            }
            (SessionOutcome::Applied, true) => {
                "MCP reattach readback complete. Profile MCP reconcile and chat policy application reported. Server health and atomic MCP revision/config match are not established"
            }
            (SessionOutcome::Unchanged, true) => {
                "MCP reattach readback complete. Profile MCP reconcile reported; chat policy reported unchanged. Server health and atomic MCP revision/config match are not established"
            }
        };
    }
    /// Preserve any bounded report; never retain or echo raw owner error reasons.
    pub fn unknown(&mut self) {
        if self.blocked() {
            self.phase = Phase::Unknown;
            self.id = uuid::Uuid::new_v4().to_string();
            self.notice = "MCP reload or session reattach outcome unknown; do not blindly repeat. Prompt and mutation controls remain withheld; reconnect/reconcile explicitly";
        }
    }
    pub fn disconnected(&mut self) {
        if self.blocked() {
            self.unknown();
        } else {
            self.phase = Phase::Failed;
            self.id = uuid::Uuid::new_v4().to_string();
            self.fence = None;
            self.notice =
                "MCP controls disconnected; reconnect before a fresh explicit Review and Confirm";
        }
    }
}
