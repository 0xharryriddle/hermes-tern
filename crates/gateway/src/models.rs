//! Session-scoped model choices. No provider inference, profile config or optimistic route writes.
use crate::session::SessionState;
use hermes_tern_core::{
    error::{Error, Result},
    text::{display_text, is_bidi_control},
};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug)]
pub struct ModelRow {
    pub provider: String,
    pub provider_name: String,
    pub model: String,
    pub selectable: bool,
    pub availability: &'static str,
}
#[derive(Debug)]
pub struct Catalog {
    pub rows: Vec<ModelRow>,
}
fn token(v: &Value, max: usize) -> Result<String> {
    v.as_str()
        .filter(|s| {
            !s.trim().is_empty()
                && s.len() <= max
                && !s.chars().any(|c| c.is_control() || is_bidi_control(c))
        })
        .map(str::to_owned)
        .ok_or(Error::Protocol("invalid model inventory identity"))
}
fn optional_bool(row: &Value, key: &str) -> Result<Option<bool>> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        _ => Err(Error::Protocol("invalid model availability")),
    }
}
impl Catalog {
    pub fn parse(value: Value) -> Result<Self> {
        let providers = value["providers"]
            .as_array()
            .filter(|p| p.len() <= 64)
            .ok_or(Error::Protocol("invalid or oversized provider inventory"))?;
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        let mut slugs = HashSet::new();
        for p in providers {
            let slug = token(&p["slug"], 256)?;
            if !slugs.insert(slug.clone()) {
                return Err(Error::Protocol("duplicate model provider"));
            }
            let name = p["name"]
                .as_str()
                .filter(|s| s.len() <= 1024)
                .ok_or(Error::Protocol("invalid provider label"))?;
            let models = p["models"]
                .as_array()
                .ok_or(Error::Protocol("invalid provider models"))?;
            if rows.len() + models.len() > 4096 {
                return Err(Error::Protocol("model inventory limit exceeded"));
            }
            let auth = optional_bool(p, "authenticated")?;
            let unavailable = match p.get("unavailable_models") {
                None | Some(Value::Null) => HashSet::new(),
                Some(Value::Array(a)) if a.len() <= 4096 => a
                    .iter()
                    .map(|m| token(m, 1024))
                    .collect::<Result<HashSet<_>>>()?,
                _ => return Err(Error::Protocol("invalid model availability")),
            };
            let limited = match p.get("limit") {
                None | Some(Value::Null) => false,
                Some(v) if v["scope"] == "account" => true,
                Some(v) if v["scope"] == "models" && v["models"].is_object() => false,
                _ => return Err(Error::Protocol("invalid provider limit")),
            };
            for m in models {
                let model = token(m, 1024)?;
                if !seen.insert((slug.clone(), model.clone())) {
                    return Err(Error::Protocol("duplicate model identity"));
                }
                let blocked =
                    unavailable.contains(&model) || p["limit"]["models"].get(&model).is_some();
                let availability = if auth == Some(false) {
                    "not authenticated"
                } else if auth.is_none() {
                    "authentication unreported"
                } else if limited || blocked {
                    "reported unavailable or limited"
                } else {
                    "reported available; not a health check"
                };
                rows.push(ModelRow {
                    provider: slug.clone(),
                    provider_name: display_text(name),
                    model,
                    selectable: auth == Some(true) && !limited && !blocked,
                    availability,
                });
            }
        }
        Ok(Self { rows })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Empty,
    Loading,
    Ready,
    Selected,
    Sending,
    Confirmation,
    Deferred,
    Reconciling,
    Committed,
    Failed,
    Unknown,
}
struct Pick {
    provider: String,
    model: String,
    profile: String,
    instance: String,
    session: String,
    authority: u64,
    revision: u64,
    generation: u64,
    route_model: Value,
    route_provider: Value,
    control: String,
    confirm: Option<String>,
    resolved: Option<(String, String)>,
}
impl Pick {
    fn current(&self, s: &SessionState) -> bool {
        s.can_submit()
            && self.profile == s.profile
            && self.instance == s.instance
            && self.session == s.session_id
            && self.authority == s.authority_epoch
            && self.revision == s.revision
            && self.generation == s.generation
            && self.route_model == s.info["model"]
            && self.route_provider == s.info["provider"]
    }
}
#[derive(Default)]
pub struct ModelControl {
    catalog: Option<Catalog>,
    catalog_id: String,
    pick: Option<Pick>,
    phase: Phase,
    readback_floor: Option<(u64, u64)>,
    pub notice: String,
    pub warning: Option<String>,
}
impl ModelControl {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn validate_readback(&self, snapshot: &Value) -> Result<()> {
        let pick = self
            .pick
            .as_ref()
            .ok_or(Error::Protocol("model readback without selection"))?;
        let (revision, generation) = self
            .readback_floor
            .ok_or(Error::Protocol("model readback without commit fences"))?;
        if self.phase != Phase::Reconciling
            || snapshot["session_id"] != pick.session
            || snapshot["revision"].as_u64().is_none_or(|n| n < revision)
            || snapshot["execution_generation"]
                .as_u64()
                .is_none_or(|n| n < generation)
        {
            return Err(Error::Protocol("model snapshot predates committed change"));
        }
        Ok(())
    }
    pub fn reconciled(&mut self) {
        if self.phase == Phase::Reconciling {
            self.phase = Phase::Committed;
            self.pick = None;
            self.readback_floor = None;
            self.notice =
                "Model control reconciled with owner snapshot; active route is owner-reported"
                    .into();
        }
    }
    pub fn catalog(&self) -> Option<&Catalog> {
        self.catalog.as_ref()
    }
    pub fn catalog_id(&self) -> &str {
        &self.catalog_id
    }
    pub fn control_id(&self) -> Option<&str> {
        self.pick.as_ref().map(|p| p.control.as_str())
    }
    pub fn selected(&self) -> Option<(&str, &str)> {
        self.pick
            .as_ref()
            .map(|p| (p.provider.as_str(), p.model.as_str()))
    }
    pub fn resolved_confirmation(&self) -> Option<(&str, &str)> {
        self.pick
            .as_ref()?
            .resolved
            .as_ref()
            .map(|(provider, model)| (provider.as_str(), model.as_str()))
    }
    pub fn loading(&mut self) -> String {
        self.catalog_id = uuid::Uuid::new_v4().to_string();
        self.catalog = None;
        self.pick = None;
        self.readback_floor = None;
        self.warning = None;
        self.phase = Phase::Loading;
        self.notice = "Loading configured model inventory; no selection applied".into();
        self.catalog_id.clone()
    }
    pub fn loaded(&mut self, id: &str, value: Value) -> Result<()> {
        if id != self.catalog_id || self.phase != Phase::Loading {
            return Err(Error::Protocol("stale model inventory response"));
        }
        match Catalog::parse(value) {
            Ok(c) => {
                self.catalog = Some(c);
                self.phase = Phase::Ready;
                self.notice =
                    "Select a model, then explicitly Apply; active route remains owner-reported"
                        .into();
                Ok(())
            }
            Err(e) => {
                self.failed("model inventory invalid or exceeds supported bounds");
                Err(e)
            }
        }
    }
    pub fn failed(&mut self, notice: &str) {
        self.phase = Phase::Failed;
        self.pick = None;
        self.readback_floor = None;
        self.warning = None;
        self.notice = notice.into();
    }
    pub fn disconnected(&mut self) {
        if matches!(
            self.phase,
            Phase::Sending | Phase::Deferred | Phase::Reconciling | Phase::Unknown
        ) {
            self.unknown("Model change outcome unknown; reconnect/reconcile, never blindly resend");
        } else {
            self.failed("Model controls disconnected; reconnect before selecting");
        }
        self.pick = None;
    }
    pub fn unknown(&mut self, notice: &str) {
        self.phase = Phase::Unknown;
        self.pick = None;
        self.readback_floor = None;
        self.warning = None;
        self.notice = notice.into();
    }
    /// Only owner events that already passed session replay/identity fences reach here.
    pub fn updated(&mut self, s: &SessionState, payload: &Value) -> bool {
        if self.phase != Phase::Deferred
            || payload["operation"] != "model"
            || payload["session_id"] != s.session_id
        {
            return false;
        }
        if payload["status"] == "refused" {
            self.failed("Deferred model selection not applied by Hermes; active route remains owner-reported");
            false
        } else if payload["status"] == "confirmation_required" {
            // The pinned owner retains this pick awaiting confirmation. A local
            // dismissal is not cancellation and must not re-enable prompt admission.
            self.unknown("Deferred guarded model selection remains pending in Hermes; local confirmation unavailable. Reconnect/reconcile before submitting or switching");
            false
        } else if payload.get("status").is_none() || payload["status"] == "committed" {
            self.phase = Phase::Reconciling;
            // The original deferred receipt bounds the later owner's snapshot;
            // event fields may tighten these fences, never erase them.
            if let Some((revision, generation)) = &mut self.readback_floor {
                *revision = (*revision).max(s.revision);
                *generation = (*generation).max(s.generation);
            }
            self.notice = "Deferred model selection committed; reconciling owner snapshot".into();
            true
        } else {
            false
        }
    }
    pub fn select(&mut self, s: &SessionState, catalog: &str, index: usize) -> Result<()> {
        if !s.can_submit()
            || catalog != self.catalog_id
            || matches!(
                self.phase,
                Phase::Sending
                    | Phase::Loading
                    | Phase::Unknown
                    | Phase::Deferred
                    | Phase::Reconciling
            )
        {
            return Err(Error::Protocol(
                "model selection is stale or session not ready",
            ));
        }
        let row = self
            .catalog
            .as_ref()
            .and_then(|c| c.rows.get(index))
            .filter(|r| r.selectable)
            .ok_or(Error::Protocol("model not selectable"))?;
        self.pick = Some(Pick {
            provider: row.provider.clone(),
            model: row.model.clone(),
            profile: s.profile.clone(),
            instance: s.instance.clone(),
            session: s.session_id.clone(),
            authority: s.authority_epoch,
            revision: s.revision,
            generation: s.generation,
            route_model: s.info["model"].clone(),
            route_provider: s.info["provider"].clone(),
            control: uuid::Uuid::new_v4().to_string(),
            confirm: None,
            resolved: None,
        });
        self.phase = Phase::Selected;
        self.warning = None;
        self.notice = "Selection only; press Apply to change this session, not the profile".into();
        Ok(())
    }
    pub fn apply(&mut self, s: &SessionState, control: &str, confirm: bool) -> Result<Value> {
        let expected = if confirm {
            Phase::Confirmation
        } else {
            Phase::Selected
        };
        let pick = self
            .pick
            .as_ref()
            .filter(|p| p.control == control && p.current(s))
            .ok_or(Error::Protocol("model selection changed; select again"))?;
        if self.phase != expected {
            return Err(Error::Protocol("model action is no longer current"));
        }
        let mut payload = json!({"model":pick.model,"provider":pick.provider});
        if confirm {
            payload["confirm"] = json!(
                pick.confirm
                    .as_ref()
                    .ok_or(Error::Protocol("missing model confirmation"))?
            );
        }
        let request = json!({"session_id":s.session_id,"request_id":uuid::Uuid::new_v4().to_string(),"expected_revision":s.revision,"expected_generation":s.generation,"operation":"model","payload":payload});
        self.phase = Phase::Sending;
        self.notice =
            "Model change sent; active route stays owner-reported until reconciliation".into();
        Ok(request)
    }
    pub fn result(&mut self, s: &SessionState, v: Value) -> Result<bool> {
        let Some(pick) = self.pick.as_mut() else {
            return Err(Error::Protocol("model result without selection"));
        };
        if self.phase != Phase::Sending
            || v["session_id"] != pick.session
            || v["operation"] != "model"
        {
            return Err(Error::Protocol("model result identity mismatch"));
        }
        if v["status"] == "confirmation_required" {
            if !pick.current(s) || v["confirm_required"] == false {
                self.failed("Model confirmation changed; select again");
                return Ok(false);
            }
            // Catalog spelling is the exact request identity; Hermes resolves it.
            // Its correlated guard token binds the resolved tuple, shown before
            // the separate human Confirm. Never silently rewrite the request.
            pick.resolved = Some((
                token(&v["target_provider"], 256)?,
                token(&v["target_model"], 1024)?,
            ));
            pick.confirm = Some(token(&v["confirm"], 256)?);
            pick.control = uuid::Uuid::new_v4().to_string();
            let msg = v["confirm_message"]
                .as_str()
                .filter(|s| s.len() <= 16384)
                .ok_or(Error::Protocol("invalid model confirmation message"))?;
            self.warning = Some(display_text(msg));
            self.phase = Phase::Confirmation;
            self.notice = "Hermes requires additional human confirmation; not applied".into();
            return Ok(false);
        }
        let revision = v["revision"]
            .as_u64()
            .ok_or(Error::Protocol("model result missing revision"))?;
        let generation = v["execution_generation"]
            .as_u64()
            .ok_or(Error::Protocol("model result missing generation"))?;
        if revision < pick.revision || generation < pick.generation {
            return Err(Error::Protocol("model result regressed fences"));
        }
        self.readback_floor = Some((revision, generation));
        if v["status"] == "deferred" {
            self.phase = Phase::Deferred;
            self.notice =
                "Model selection deferred by Hermes; active route unchanged until owner update"
                    .into();
            Ok(false)
        } else if v.get("status").is_none() || v["status"] == "committed" {
            self.phase = Phase::Reconciling;
            self.notice = "Model selection committed; reading effective owner route".into();
            Ok(true)
        } else {
            self.failed("Model selection not committed; select again after reconciliation");
            Ok(false)
        }
    }
}
