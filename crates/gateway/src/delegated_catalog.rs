//! Bounded, inert projection of the pinned owner's delegated-agent snapshot.
//!
//! Source: Hermes bf0203a794a07aac3ff764972ab9087baa4e4f4e,
//! gateway/session_ancillary.py:16-20,193-217,476-502. Its owner/lineage-filtered
//! rows are reported data, not durable peers, proof of live work, or authority.
//! Transport byte limits and RPC error handling belong to the caller.

use hermes_tern_core::{
    error::{Error, Result},
    text::{display_text, is_bidi_control},
};
use serde_json::{Map, Value};
use std::collections::HashSet;

// Local projection limits; no partial roster or truncated text is returned.
const MAX_ROWS: usize = 128;
const MAX_TEXT_BYTES: usize = 65_536;
const ID_BYTES: usize = 256;
const GOAL_BYTES: usize = 1024;
const STATUS_BYTES: usize = 64;
const LAST_TOOL_BYTES: usize = 128;
const MAX_DEPTH: u64 = 32;
const UNAVAILABLE: &str = "unavailable";

/// An exact reported identity paired with complete, escaped display text.
#[derive(Debug)]
pub struct Row {
    /// Exact upstream identity; never derived from rendered text or normalized.
    pub id: String,
    /// Stable multiline projection of only the known public fields.
    pub text: String,
}

/// A complete read-only snapshot, with no execution or control authority.
#[derive(Debug)]
pub struct Catalog {
    /// Validated rows in upstream order; an empty vector is a successful empty read.
    pub rows: Vec<Row>,
}

impl Catalog {
    /// Validate the complete pinned response before returning any projected rows.
    ///
    /// Missing/null metadata is reported as unavailable. Unknown fields are
    /// neither retained nor charged to the rendered-text budget; the caller must
    /// enforce the global transport bound before decoding JSON.
    ///
    /// # Errors
    /// Returns a data-independent protocol error for malformed known fields,
    /// duplicate/unsafe identities, nonempty delegations, or exceeded limits.
    pub fn parse(value: Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or(Error::Protocol("invalid delegated catalog envelope"))?;
        let agents = object
            .get("subagents")
            .and_then(Value::as_array)
            .filter(|rows| rows.len() <= MAX_ROWS)
            .ok_or(Error::Protocol("invalid or oversized delegated catalog"))?;
        if !object
            .get("delegations")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            return Err(Error::Protocol("unsupported delegated catalog delegations"));
        }

        let mut seen = HashSet::with_capacity(agents.len());
        let mut rows = Vec::with_capacity(agents.len());
        let mut text_bytes = 0usize;
        for agent in agents {
            let object = agent
                .as_object()
                .ok_or(Error::Protocol("invalid delegated catalog row"))?;
            let id = object
                .get("subagent_id")
                .and_then(Value::as_str)
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= ID_BYTES
                        && !id.chars().any(char::is_control)
                        && display_text(id) == *id
                })
                .ok_or(Error::Protocol("invalid delegated catalog identity"))?;
            if !seen.insert(id) {
                return Err(Error::Protocol("duplicate delegated catalog identity"));
            }

            let parent = label(object, "parent_id", ID_BYTES)?;
            let delegation = label(object, "delegation_id", ID_BYTES)?;
            let goal = label(object, "goal", GOAL_BYTES)?;
            let model = label(object, "model", ID_BYTES)?;
            let status = label(object, "status", STATUS_BYTES)?;
            let last_tool = label(object, "last_tool", LAST_TOOL_BYTES)?;
            let depth = unsigned(object, "depth", MAX_DEPTH)?;
            let tools = unsigned(object, "tool_count", u64::MAX)?;
            let started = match present(object, "started_at") {
                None => UNAVAILABLE.to_owned(),
                Some(Value::Number(number))
                    if number
                        .as_f64()
                        .is_some_and(|number| number.is_finite() && number >= 0.0) =>
                {
                    number.to_string()
                }
                _ => return Err(Error::Protocol("invalid delegated catalog start time")),
            };
            let steer = match present(object, "accepting_steer") {
                None => UNAVAILABLE,
                Some(Value::Bool(true)) => "true",
                Some(Value::Bool(false)) => "false",
                _ => return Err(Error::Protocol("invalid delegated catalog steer report")),
            };
            let mut text = format!(
                "Delegated: {id}\nParent: {} · depth {depth}\nGoal: {}\nModel: {} · status {}\nTool calls: {tools} · last tool {}\nStarted epoch: {started}\nSteer reported: {steer} · controls not offered",
                parent.as_deref().unwrap_or(UNAVAILABLE),
                goal.as_deref().unwrap_or(UNAVAILABLE),
                model.as_deref().unwrap_or(UNAVAILABLE),
                status.as_deref().unwrap_or(UNAVAILABLE),
                last_tool.as_deref().unwrap_or(UNAVAILABLE),
            );
            if let Some(delegation) = delegation {
                text.push_str("\nDelegation: ");
                text.push_str(&delegation);
            }
            text_bytes = text_bytes
                .checked_add(text.len())
                .filter(|bytes| *bytes <= MAX_TEXT_BYTES)
                .ok_or(Error::Protocol("delegated catalog display limit exceeded"))?;
            rows.push(Row {
                id: id.to_owned(),
                text,
            });
        }
        Ok(Self { rows })
    }
}

fn present<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    row.get(key).filter(|value| !value.is_null())
}

fn label(row: &Map<String, Value>, key: &str, limit: usize) -> Result<Option<String>> {
    let Some(value) = present(row, key) else {
        return Ok(None);
    };
    let raw = value
        .as_str()
        .filter(|text| text.len() <= limit)
        .ok_or(Error::Protocol("invalid delegated catalog label"))?;
    let mut escaped = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character.is_control() || is_bidi_control(character) {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    Ok(Some(escaped))
}

fn unsigned(row: &Map<String, Value>, key: &str, max: u64) -> Result<String> {
    match present(row, key) {
        None => Ok(UNAVAILABLE.to_owned()),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= max)
            .map(|value| value.to_string())
            .ok_or(Error::Protocol("invalid delegated catalog integer")),
    }
}
