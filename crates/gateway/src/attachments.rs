//! Explicit staging and separately consented draft insertion. Never submits a prompt.
use crate::{
    attachment_file::{AttachmentReceipt, PreparedFile},
    session::SessionState,
};
use hermes_tern_core::error::{Error, Result};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Empty,
    Prepared,
    Pending,
    Staged,
    Added,
    Failed,
    Unknown,
}
#[derive(PartialEq, Eq)]
struct Fence {
    profile: String,
    instance: String,
    session: String,
    authority: u64,
    revision: u64,
    generation: u64,
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
        }
    }
    fn current(&self, s: &SessionState) -> bool {
        s.connected && !s.is_suspended() && *self == Self::of(s)
    }
}
#[derive(Default)]
pub struct Attachments {
    phase: Phase,
    id: String,
    fence: Option<Fence>,
    prepared: Option<PreparedFile>,
    receipt: Option<AttachmentReceipt>,
    attempts: usize,
    pub notice: &'static str,
}
impl Attachments {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn attempts(&self) -> usize {
        self.attempts
    }
    pub fn prepared(&self) -> Option<&PreparedFile> {
        self.prepared.as_ref()
    }
    pub fn receipt(&self) -> Option<&AttachmentReceipt> {
        self.receipt.as_ref()
    }
    pub fn blocked(&self) -> bool {
        matches!(self.phase, Phase::Pending | Phase::Unknown)
    }
    pub fn prepare(&mut self, s: &SessionState, path: &str) -> Result<()> {
        if self.blocked() || !s.can_submit() {
            return Err(Error::Protocol(
                "attachment review unavailable while session is pending or unknown",
            ));
        }
        self.invalidate_prepared();
        match PreparedFile::read(path) {
            Ok(file) => {
                self.id = file.id().into();
                self.fence = Some(Fence::of(s));
                self.prepared = Some(file);
                self.phase = Phase::Prepared;
                self.notice = "Local bytes reviewed only; Stage requires a separate explicit click";
                Ok(())
            }
            Err(error) => {
                self.phase = Phase::Failed;
                self.notice =
                    "Local file unavailable, unsafe, changed or unsupported; nothing uploaded";
                Err(error)
            }
        }
    }
    pub fn invalidate_prepared(&mut self) {
        if !self.blocked() {
            self.id = uuid::Uuid::new_v4().to_string();
            self.prepared = None;
            self.receipt = None;
            self.fence = None;
            self.phase = Phase::Empty;
            self.notice = "Review one explicit local text file; no automatic upload or submission";
        }
    }
    pub fn stage(&mut self, s: &SessionState, id: &str) -> Result<Value> {
        if self.phase != Phase::Prepared
            || id != self.id
            || !s.can_submit()
            || !self.fence.as_ref().is_some_and(|f| f.current(s))
        {
            return Err(Error::Protocol(
                "attachment review changed; review current local bytes explicitly",
            ));
        }
        if self.attempts >= 8 {
            return Err(Error::Protocol(
                "eight staging attempts reached; retained owner files are not automatically removed",
            ));
        }
        let params = self
            .prepared
            .as_ref()
            .ok_or(Error::Protocol("attachment snapshot unavailable"))?
            .params(&s.session_id);
        self.prepared = None;
        self.attempts += 1;
        self.phase = Phase::Pending;
        self.notice = "Staging pending; no automatic retry, prompt or draft insertion";
        Ok(params)
    }
    pub fn not_enqueued(&mut self) {
        if self.phase == Phase::Pending {
            self.attempts -= 1;
            self.phase = Phase::Failed;
            self.id = uuid::Uuid::new_v4().to_string();
            self.fence = None;
            self.notice =
                "Upload was not enqueued; review explicitly after pending requests settle";
        }
    }
    pub fn result(&mut self, s: &SessionState, value: Value) -> Result<()> {
        if self.phase != Phase::Pending || !self.fence.as_ref().is_some_and(|f| f.current(s)) {
            self.unknown();
            return Err(Error::Protocol(
                "attachment receipt stale; staged outcome unknown",
            ));
        }
        match AttachmentReceipt::parse(value) {
            Ok(receipt) => {
                self.receipt = Some(receipt);
                self.id = uuid::Uuid::new_v4().to_string();
                self.phase = Phase::Staged;
                self.notice =
                    "Owner staged a file; Add reference is separate and never sends a prompt";
                Ok(())
            }
            Err(error) => {
                self.unknown();
                Err(error)
            }
        }
    }
    pub fn reference(&self, s: &SessionState, id: &str) -> Result<&str> {
        if self.phase != Phase::Staged
            || id != self.id
            || !s.can_submit()
            || !self.fence.as_ref().is_some_and(|f| f.current(s))
        {
            return Err(Error::Protocol(
                "attachment reference changed; use current staged receipt",
            ));
        }
        self.receipt
            .as_ref()
            .map(AttachmentReceipt::reference)
            .ok_or(Error::Protocol("attachment receipt unavailable"))
    }
    pub fn added(&mut self, s: &SessionState, id: &str) -> Result<()> {
        self.reference(s, id)?;
        self.phase = Phase::Added;
        self.id = uuid::Uuid::new_v4().to_string();
        self.notice = "Reference added locally once; review the draft and choose Send yourself";
        Ok(())
    }
    pub fn unknown(&mut self) {
        self.phase = Phase::Unknown;
        self.prepared = None;
        self.receipt = None;
        self.id = uuid::Uuid::new_v4().to_string();
        self.notice =
            "Staging outcome unknown; do not blindly repeat or assume the owner removed the file";
    }
    pub fn disconnected(&mut self) {
        if self.phase == Phase::Pending {
            self.unknown();
        }
    }
}
