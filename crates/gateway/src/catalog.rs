//! Fixed, bounded read-only session inventory. Titles never identify a session.
use crate::{config::Limits, inspect, transport::RpcClient};
use hermes_tern_core::{
    error::{Error, Result},
    text::is_bidi_control,
};
use serde_json::{Value, json};
use std::collections::HashSet;

pub const LIST_LIMIT: usize = 200;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Live,
    Stored,
}
impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Stored => "stored",
        }
    }
}
#[derive(Debug)]
pub struct Entry {
    pub session_id: String,
    pub title: String,
    pub model: Option<String>,
    pub source: Option<String>,
    pub running: bool,
}
#[derive(Debug)]
pub struct Catalog {
    live: Vec<Entry>,
    stored: Vec<Entry>,
}
impl Catalog {
    pub fn from_responses(live: Value, stored: Value) -> Result<Self> {
        Ok(Self {
            live: parse(live, Scope::Live)?,
            stored: parse(stored, Scope::Stored)?,
        })
    }
    pub fn entries(&self, scope: Scope) -> &[Entry] {
        match scope {
            Scope::Live => &self.live,
            Scope::Stored => &self.stored,
        }
    }
    pub fn at_limit(&self, scope: Scope) -> bool {
        self.entries(scope).len() == LIST_LIMIT
    }
}
fn label(row: &Value, key: &str, limit: usize) -> Result<Option<String>> {
    let Some(value) = row.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .filter(|v| v.len() <= limit)
        .ok_or(Error::Protocol("invalid session catalog metadata"))?;
    let escaped = text
        .chars()
        .flat_map(|c| {
            if c.is_control() || is_bidi_control(c) {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect();
    Ok(Some(escaped))
}
fn parse(value: Value, scope: Scope) -> Result<Vec<Entry>> {
    if value["scope"].as_str() != Some(scope.label()) {
        return Err(Error::Protocol("session catalog scope mismatch"));
    }
    let rows = value["sessions"]
        .as_array()
        .filter(|rows| rows.len() <= LIST_LIMIT)
        .ok_or(Error::Protocol("invalid or oversized session catalog"))?;
    let mut seen = HashSet::new();
    rows.iter()
        .map(|row| {
            let session_id = row["session_id"]
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 256
                        && !s.chars().any(|c| c.is_control() || is_bidi_control(c))
                })
                .ok_or(Error::Protocol("invalid session catalog identity"))?;
            if !seen.insert(session_id.to_owned()) {
                return Err(Error::Protocol("duplicate session catalog identity"));
            }
            let running = match row.get("running") {
                Some(Value::Bool(v)) => *v,
                None | Some(Value::Null) => false,
                _ => return Err(Error::Protocol("invalid catalog execution state")),
            };
            Ok(Entry {
                session_id: session_id.into(),
                title: label(row, "title", 4096)?.unwrap_or_default(),
                model: label(row, "model", 512)?,
                source: label(row, "source", 128)?,
                running,
            })
        })
        .collect()
}
/// Reads two limited inventories; never attaches, creates or mutates a session.
pub fn fetch(client: &mut RpcClient, limits: &Limits) -> Result<Catalog> {
    inspect::qualify(client, limits)?;
    let mut list = |scope: Scope| {
        inspect::call(
            client,
            "session.list",
            json!({"scope":scope.label(),"limit":LIST_LIMIT}),
            limits.request_timeout,
        )
    };
    let live = list(Scope::Live)?;
    let stored = list(Scope::Stored)?;
    Catalog::from_responses(live, stored)
}
