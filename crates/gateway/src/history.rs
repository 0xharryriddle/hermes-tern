//! Immutable bounded display projection, not raw transcript or upstream pagination.
use crate::{config::Limits, inspect, transport::RpcClient};
use hermes_tern_core::{
    error::{Error, Result},
    text::is_bidi_control,
};
use serde_json::{Value, json};
use std::io::{self, Write};

pub const MAX_HISTORY_ROWS: usize = 4096;
pub const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
pub const PAGE_BYTES: usize = 16 * 1024;
pub const PAGE_ROWS: usize = 32;

#[derive(Debug)]
struct Span {
    row: usize,
    start: usize,
    end: usize,
}
#[derive(Debug)]
pub struct HistorySnapshot {
    source_count: u64,
    rows: Vec<String>,
    pages: Vec<Vec<Span>>,
}
pub struct Fragment<'a> {
    /// Projection ordinal only; never a durable row identity or authority.
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub total_bytes: usize,
    pub text: &'a str,
}
struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("history projection limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn escaped_row(row: &Value, limit: usize) -> Result<String> {
    let mut writer = BoundedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, row)
        .map_err(|_| Error::Protocol("history projection exceeds 8 MiB escaped retention limit"))?;
    let json = String::from_utf8(writer.bytes)
        .map_err(|_| Error::Protocol("invalid history projection encoding"))?;
    let mut text = String::new();
    for ch in json.chars() {
        if ch.is_control() || is_bidi_control(ch) {
            if text.len() + 6 > limit {
                return Err(Error::Protocol(
                    "history projection exceeds 8 MiB escaped retention limit",
                ));
            }
            use std::fmt::Write as _;
            write!(text, "\\u{:04x}", ch as u32).expect("writing to String");
        } else {
            if text.len() + ch.len_utf8() > limit {
                return Err(Error::Protocol(
                    "history projection exceeds 8 MiB escaped retention limit",
                ));
            }
            text.push(ch);
        }
    }
    Ok(text)
}
impl HistorySnapshot {
    pub fn from_response(value: Value) -> Result<Self> {
        let source_count = value["count"]
            .as_u64()
            .ok_or(Error::Protocol("missing history source row count"))?;
        let messages = value["messages"]
            .as_array()
            .ok_or(Error::Protocol("missing history projection"))?;
        if messages.len() > MAX_HISTORY_ROWS {
            return Err(Error::Protocol("history projection exceeds 4096 row limit"));
        }
        let mut rows = Vec::new();
        let mut bytes = 0;
        for row in messages {
            if !row.is_object() {
                return Err(Error::Protocol("history projection row must be an object"));
            }
            let text = escaped_row(row, MAX_HISTORY_BYTES - bytes)?;
            bytes += text.len();
            rows.push(text);
        }
        let mut pages = vec![Vec::new()];
        let mut page_bytes = 0;
        for (row, text) in rows.iter().enumerate() {
            let mut start = 0;
            while start < text.len() {
                let mut end = (start + PAGE_BYTES - page_bytes).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                if pages.last().expect("initial page").len() == PAGE_ROWS || end == start {
                    pages.push(Vec::new());
                    page_bytes = 0;
                    continue;
                }
                pages
                    .last_mut()
                    .expect("initial page")
                    .push(Span { row, start, end });
                page_bytes += end - start;
                start = end;
            }
        }
        Ok(Self {
            source_count,
            rows,
            pages,
        })
    }
    pub fn source_count(&self) -> u64 {
        self.source_count
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn page(&self, page: usize) -> Result<Vec<Fragment<'_>>> {
        let spans = self
            .pages
            .get(page)
            .ok_or(Error::Config("history page is out of range"))?;
        Ok(spans
            .iter()
            .map(|s| Fragment {
                row: s.row,
                start: s.start,
                end: s.end,
                total_bytes: self.rows[s.row].len(),
                text: &self.rows[s.row][s.start..s.end],
            })
            .collect())
    }
}

pub fn validate_session(session: &str) -> Result<()> {
    if session.is_empty()
        || session.len() > 256
        || session.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(Error::Config("invalid history session identity"));
    }
    Ok(())
}
/// Unbound read of an already live session; never resume/create or add a subscription.
pub fn fetch(client: &mut RpcClient, limits: &Limits, session: &str) -> Result<HistorySnapshot> {
    validate_session(session)?;
    inspect::qualify(client, limits)?;
    let result = inspect::call(
        client,
        "session.history",
        json!({"session_id":session}),
        limits.request_timeout,
    )?;
    HistorySnapshot::from_response(result)
}
