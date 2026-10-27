//! Bounded, read-only projection of an owner's complete reported task plan.
//!
//! Pinned contract: Hermes bf0203a794a07aac3ff764972ab9087baa4e4f4e,
//! gateway/session_controls.py:630-648, gateway/run_turn_progress.py:859-873,
//! tui_gateway/tool_progress.py:116-129, and tools/todo_tool.py:9-17,26-28,89-100.
//! The caller owns transport bounds and owner/session/generation fencing. Task
//! statuses grant no control authority and establish neither success nor liveness.

use hermes_tern_core::{
    error::{Error, Result},
    text::is_bidi_control,
};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

// Local complete-projection bounds; never return truncated or partial rows.
const MAX_ROWS: usize = 256;
const MAX_ID_BYTES: usize = 256;
const MAX_CONTENT_CHARS: usize = 4000;
const MAX_CONTENT_BYTES: usize = 16_000;
const MAX_TEXT_BYTES: usize = 65_536;

/// An exact reported identity and status paired with complete safe display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Exact upstream identity, never normalized or recovered from rendered text.
    pub id: String,
    /// Optional exact upstream parent identity, validated against the complete list.
    pub parent: Option<String>,
    /// One of the four reported statuses, not an independent completion assertion.
    pub status: String,
    /// Deterministic human-readable projection of only the known public fields.
    pub text: String,
}

/// A validated complete task snapshot in its original reported order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// Strict unsigned upstream revision, independent of transport sequencing.
    pub revision: u64,
    /// Complete validated rows; an empty list at a positive revision reports clear.
    pub rows: Vec<Row>,
}

impl Catalog {
    /// Validate known fields and project the complete owner-reported task state.
    ///
    /// Null or an empty revision-zero state means unreported, not a reported clear.
    /// Missing optional parents are equivalent to null. Unknown private fields
    /// are neither retained nor charged to the rendered-text budget. The caller
    /// must enforce transport bounds before allocating the input JSON value.
    ///
    /// # Errors
    /// Returns data-independent protocol errors for malformed known fields,
    /// unsafe/duplicate identities, invalid parent graphs, or exceeded bounds.
    pub fn parse(value: &Value) -> Result<Option<Self>> {
        if value.is_null() {
            return Ok(None);
        }
        let object = value
            .as_object()
            .ok_or(Error::Protocol("invalid task plan envelope"))?;
        let revision = object
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or(Error::Protocol("invalid task plan revision"))?;
        let todos = object
            .get("todos")
            .and_then(Value::as_array)
            .filter(|rows| rows.len() <= MAX_ROWS)
            .ok_or(Error::Protocol("invalid or oversized task plan rows"))?;
        if revision == 0 && todos.is_empty() {
            return Ok(None);
        }

        let mut indices = HashMap::with_capacity(todos.len());
        let mut rows = Vec::with_capacity(todos.len());
        let mut text_bytes = 0usize;
        for todo in todos {
            let object = todo
                .as_object()
                .ok_or(Error::Protocol("invalid task plan row"))?;
            let id = identity(object.get("id"))?;
            if indices.insert(id, rows.len()).is_some() {
                return Err(Error::Protocol("duplicate task plan identity"));
            }
            let parent = match object.get("parent") {
                None | Some(Value::Null) => None,
                value => Some(identity(value)?),
            };
            let status = object
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| {
                    matches!(
                        *status,
                        "pending" | "in_progress" | "completed" | "cancelled"
                    )
                })
                .ok_or(Error::Protocol("invalid task plan status"))?;
            let content = object
                .get("content")
                .and_then(Value::as_str)
                .filter(|content| {
                    content.len() <= MAX_CONTENT_BYTES
                        && !content.trim().is_empty()
                        && content.chars().count() <= MAX_CONTENT_CHARS
                })
                .ok_or(Error::Protocol("invalid or oversized task plan content"))?;
            let text = format!(
                "Task: {id}\nStatus: {status}\nParent: {}\n{}",
                parent.unwrap_or("not reported"),
                escape_content(content),
            );
            text_bytes = text_bytes
                .checked_add(text.len())
                .filter(|bytes| *bytes <= MAX_TEXT_BYTES)
                .ok_or(Error::Protocol("task plan display limit exceeded"))?;
            rows.push(Row {
                id: id.to_owned(),
                parent: parent.map(str::to_owned),
                status: status.to_owned(),
                text,
            });
        }
        validate_parents(&rows, &indices)?;
        Ok(Some(Self { revision, rows }))
    }
}

fn identity(value: Option<&Value>) -> Result<&str> {
    value
        .and_then(Value::as_str)
        .filter(|id| {
            id.len() <= MAX_ID_BYTES
                && !id.trim().is_empty()
                && !id
                    .chars()
                    .any(|character| character.is_control() || is_bidi_control(character))
        })
        .ok_or(Error::Protocol("invalid task plan identity"))
}

fn escape_content(content: &str) -> String {
    let mut escaped = String::with_capacity(content.len());
    for character in content.chars() {
        // Escape backslashes too: literal escape-looking text must not collide
        // with controls when equal-revision known projections are compared.
        if character == '\\' || character.is_control() || is_bidi_control(character) {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn validate_parents(rows: &[Row], indices: &HashMap<&str, usize>) -> Result<()> {
    let parents: Vec<Option<usize>> = rows
        .iter()
        .map(|row| {
            row.parent
                .as_deref()
                .map(|parent| {
                    indices
                        .get(parent)
                        .copied()
                        .ok_or(Error::Protocol("unknown task plan parent"))
                })
                .transpose()
        })
        .collect::<Result<_>>()?;
    // Each bounded chain is visited and finished once, without recursion or
    // reordering rows. A visiting node encountered again is a cycle.
    let mut states = vec![0u8; rows.len()];
    for start in 0..rows.len() {
        let mut current = Some(start);
        while let Some(index) = current {
            match states[index] {
                0 => {
                    states[index] = 1;
                    current = parents[index];
                }
                1 => return Err(Error::Protocol("cyclic task plan parent references")),
                _ => break,
            }
        }
        let mut current = Some(start);
        while let Some(index) = current {
            if states[index] != 1 {
                break;
            }
            states[index] = 2;
            current = parents[index];
        }
    }
    Ok(())
}

/// Monotonic task projection for one caller-qualified owner/session scope.
///
/// A missing catalog with a retained watermark cannot be revived by that same
/// revision. Only a strictly newer valid report or an explicit scope invalidation
/// permits recovery. The caller must invalidate on generation/authority changes.
#[derive(Debug, Clone, Default)]
pub struct Projection {
    catalog: Option<Catalog>,
    watermark: Option<u64>,
    snapshot_id: String,
}

impl Projection {
    /// Apply a complete report without allowing older revisions to regress it.
    ///
    /// Malformed current/newer reports remove rows and retain/advance the revision
    /// watermark. Malformed older typed revisions are ignored. Absence removes
    /// rows without clearing the watermark, including the unused empty-zero form.
    /// Equal-revision conflicts remain unavailable until a strictly newer report.
    pub fn update(&mut self, value: &Value) {
        let revision = value.get("revision").and_then(Value::as_u64);
        let unused_zero = revision == Some(0)
            && value
                .get("todos")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
        // Unreported zero is absence, not an old reported clear to ignore.
        if value.is_null() || unused_zero {
            self.replace(self.watermark, None);
            return;
        }
        if let (Some(incoming), Some(current)) = (revision, self.watermark)
            && incoming < current
        {
            return;
        }
        match Catalog::parse(value) {
            Ok(Some(catalog)) if revision != self.watermark => {
                self.replace(Some(catalog.revision), Some(catalog));
            }
            Ok(Some(catalog)) if self.catalog.as_ref() == Some(&catalog) => {}
            Ok(_) | Err(_) => self.replace(revision.or(self.watermark), None),
        }
    }

    /// Clear rows and revision history when the caller's qualified scope changes.
    pub fn invalidate(&mut self) {
        self.replace(None, None);
    }

    /// Return a complete reported snapshot, or no current trustworthy projection.
    pub fn catalog(&self) -> Option<&Catalog> {
        self.catalog.as_ref()
    }

    /// Return a local callback-fencing token, empty until the first state change.
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// Describe report availability without asserting task success or liveness.
    pub fn notice(&self) -> &'static str {
        match &self.catalog {
            None => "Owner-reported task plan unavailable; no task state established.",
            Some(catalog) if catalog.rows.is_empty() => {
                "Owner-reported task plan is empty; statuses are reports, not proof of success."
            }
            Some(_) => {
                "Owner-reported task statuses are untrusted reports, not proof of success or agent liveness."
            }
        }
    }

    fn replace(&mut self, watermark: Option<u64>, catalog: Option<Catalog>) {
        if self.watermark != watermark || self.catalog != catalog {
            self.watermark = watermark;
            self.catalog = catalog;
            self.snapshot_id = Uuid::new_v4().to_string();
        }
    }
}
