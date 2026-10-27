//! Local presentation state only; never dispatches requests to an agent.
use hermes_tern_core::error::{Error, Result};
use hermes_tern_gateway::session::SessionState;
use tern_sdk::keys::Key;

const PREVIEW_BYTES: usize = 1024;
const PREVIEW_LINES: usize = 12;

#[derive(Default, Debug)]
pub struct Transcript {
    expanded: Option<(String, String)>,
    selected: Option<(String, String)>,
    revision: u64,
    ready: bool,
}
impl Transcript {
    pub fn navigation_revision(&self) -> u64 {
        self.revision + 1
    }
    fn changed(&mut self) -> Result<()> {
        self.ready = false;
        self.revision = self
            .revision
            .checked_add(1)
            .filter(|v| *v < u64::MAX)
            .ok_or(Error::Protocol("local navigation exhausted"))?;
        Ok(())
    }
    pub fn inspecting(&self) -> bool {
        self.selected.is_some()
    }
    pub fn keyboard_ready(&self) -> bool {
        self.ready
    }
    pub fn positioned(&mut self, revision: Option<u64>) -> bool {
        let ready = self.inspecting() && revision == Some(self.navigation_revision());
        let changed = self.ready != ready;
        self.ready = ready;
        changed
    }
    pub fn layout_changed(&mut self) -> Result<()> {
        if self.inspecting() {
            self.changed()?;
        }
        Ok(())
    }
    pub fn is_selected(&self, id: &str, control_id: &str) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|(row, control)| row == id && control == control_id)
    }
    pub fn composer(&mut self) -> Result<()> {
        if self.selected.is_some() {
            self.changed()?;
            self.selected = None;
        }
        Ok(())
    }
    pub fn reveal_target(&self, state: &SessionState) -> Option<String> {
        let (id, control_id) = self.selected.as_ref()?;
        validate_row(state, id, control_id).ok()?;
        Some(format!(
            "main.{id}.{}-{control_id}",
            if self.is_expanded(id, control_id) {
                "collapse"
            } else {
                "show"
            }
        ))
    }
    /// Consumed local inspection keys never become composer edits or upstream actions.
    /// Stale selection stays inert until explicit navigation, not implicit Submit.
    pub fn keyboard(
        &mut self,
        state: &SessionState,
        key: &Key,
        drawn: Option<u64>,
    ) -> Result<bool> {
        if key.is("ctrl+c") || key.is("ctrl+d") {
            return Ok(false);
        }
        if key.is("tab") || key.is("shift+tab") {
            let rows: Vec<_> = state
                .messages
                .iter()
                .filter(|m| validate_row(state, &m.id, &m.control_id).is_ok())
                .collect();
            let at = self
                .selected
                .as_ref()
                .and_then(|(id, c)| rows.iter().position(|m| &m.id == id && &m.control_id == c));
            let next = if key.shift {
                match at {
                    Some(0) => None,
                    Some(n) => Some(n - 1),
                    None => rows.len().checked_sub(1),
                }
            } else {
                match at {
                    Some(n) if n + 1 < rows.len() => Some(n + 1),
                    Some(_) => None,
                    None if !rows.is_empty() => Some(0),
                    None => None,
                }
            };
            self.changed()?;
            self.selected = next.map(|n| (rows[n].id.clone(), rows[n].control_id.clone()));
            return Ok(true);
        }
        if !self.inspecting() {
            return Ok(false);
        }
        if key.is("escape") {
            self.composer()?;
            return Ok(true);
        }
        if key.is("enter") || key.is("space") {
            let (id, control_id) = self
                .selected
                .clone()
                .ok_or(Error::Protocol("no retained row selected"))?;
            validate_row(state, &id, &control_id)?;
            if drawn != Some(self.navigation_revision()) {
                return Err(Error::Protocol(
                    "inspection view not acknowledged; use current controls",
                ));
            }
            self.select(state, &id, &control_id, !self.is_expanded(&id, &control_id))?;
        }
        Ok(true)
    }
    pub fn is_expanded(&self, id: &str, control_id: &str) -> bool {
        self.expanded
            .as_ref()
            .is_some_and(|(row, control)| row == id && control == control_id)
    }
    pub fn reconcile(&mut self, state: &SessionState) {
        if self
            .expanded
            .as_ref()
            .is_some_and(|(id, control_id)| validate_row(state, id, control_id).is_err())
        {
            self.expanded = None;
        }
    }
    /// Explicit desired state makes duplicate callbacks inert. A late collapse
    /// for a different row must not close the currently inspected row.
    pub fn select(
        &mut self,
        state: &SessionState,
        id: &str,
        control_id: &str,
        expand: bool,
    ) -> Result<()> {
        validate_row(state, id, control_id)?;
        let before = self.expanded.clone();
        if expand {
            self.expanded = Some((id.to_owned(), control_id.to_owned()));
        } else if self.is_expanded(id, control_id) {
            self.expanded = None;
        }
        if self.expanded != before {
            self.changed()?;
        }
        Ok(())
    }
}

/// Local inspection remains available offline, but only for this retained row instance.
pub fn validate_row(state: &SessionState, id: &str, control_id: &str) -> Result<()> {
    let mut rows = state.messages.iter().filter(|m| m.id == id);
    let row = rows
        .next()
        .ok_or(Error::Protocol("transcript row no longer retained"))?;
    if rows.next().is_some()
        || row.control_id != control_id
        || (row.role != "reasoning" && preview(&row.text).len() == row.text.len())
    {
        return Err(Error::Protocol(
            "transcript row changed; use current controls",
        ));
    }
    Ok(())
}

/// A contiguous Unicode-safe prefix; no ellipsis is inserted into actual data.
pub fn preview(text: &str) -> &str {
    let mut end = text.len().min(PREVIEW_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some((at, _)) = text[..end].match_indices('\n').nth(PREVIEW_LINES - 1) {
        end = at + 1;
    }
    &text[..end]
}
