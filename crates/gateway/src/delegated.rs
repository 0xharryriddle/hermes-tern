//! Explicit session-scoped child roster snapshots. A read grants no control authority.
use crate::{delegated_catalog::Catalog, session::SessionState};
use hermes_tern_core::error::{Error, Result};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Empty,
    Loading,
    Ready,
    Failed,
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
pub struct Delegated {
    phase: Phase,
    id: String,
    panel: u64,
    invalidated: bool,
    fence: Option<Fence>,
    catalog: Option<Catalog>,
    pub notice: &'static str,
}
impl Delegated {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn catalog(&self, s: &SessionState, panel: u64) -> Option<&Catalog> {
        (self.phase == Phase::Ready
            && !self.invalidated
            && self.panel == panel
            && self.fence.as_ref().is_some_and(|f| f.current(s)))
        .then_some(self.catalog.as_ref())
        .flatten()
    }
    pub fn loading(&mut self, s: &SessionState, panel: u64) -> Result<String> {
        if !s.connected || s.is_suspended() || self.phase == Phase::Loading {
            return Err(Error::Protocol(
                "delegated-agent read unavailable until pending read settles",
            ));
        }
        self.id = uuid::Uuid::new_v4().to_string();
        self.panel = panel;
        self.invalidated = false;
        self.fence = Some(Fence::of(s));
        self.catalog = None;
        self.phase = Phase::Loading;
        self.notice = "Reading delegated agents; no automatic refresh or retry";
        Ok(self.id.clone())
    }
    pub fn loaded(&mut self, s: &SessionState, id: &str, value: Value) -> Result<()> {
        // A duplicate obsolete response must not invalidate a newer explicit read.
        if id != self.id {
            return Err(Error::Protocol("obsolete delegated-agent read"));
        }
        if self.phase != Phase::Loading
            || self.invalidated
            || !self.fence.as_ref().is_some_and(|f| f.current(s))
        {
            self.failed("Delegated agents unavailable or changed; read explicitly again");
            return Err(Error::Protocol("stale delegated-agent inventory"));
        }
        match Catalog::parse(value) {
            Ok(catalog) => {
                self.catalog = Some(catalog);
                self.phase = Phase::Ready;
                self.notice =
                    "Owner-reported snapshot only; not independent peers or control authority";
                Ok(())
            }
            Err(error) => {
                self.failed("Delegated agents unavailable or malformed; not an empty roster");
                Err(error)
            }
        }
    }
    pub fn panel_changed(&mut self, panel: u64) {
        if panel != self.panel {
            self.catalog = None;
            self.invalidated = true;
            if self.phase == Phase::Loading {
                self.notice = "Previous panel read pending; its result will be discarded, no automatic refresh";
            } else {
                self.phase = Phase::Empty;
                self.fence = None;
                self.notice = "Read delegated agents explicitly; no automatic refresh";
            }
            self.panel = panel;
        }
    }
    pub fn reconcile(&mut self, s: &SessionState) {
        if self.fence.as_ref().is_some_and(|f| !f.current(s)) {
            self.catalog = None;
            self.invalidated = true;
            if self.phase != Phase::Loading {
                self.failed("Delegated agents unavailable or changed; read explicitly again");
            }
        }
    }
    pub fn failed(&mut self, notice: &'static str) {
        self.phase = Phase::Failed;
        self.catalog = None;
        self.fence = None;
        self.invalidated = true;
        self.notice = notice;
    }
    pub fn disconnected(&mut self) {
        self.failed("Delegated agents unavailable after disconnect; no automatic retry");
    }
}
