//! Display-only reasoning from canonical history, never opaque replay metadata.
use serde_json::Value;

const DETAILS_BYTES: usize = 65536;
const DETAIL_ROWS: usize = 128;

pub(crate) fn history_text(row: &Value) -> String {
    for key in ["reasoning", "reasoning_content"] {
        if let Some(text) = row[key].as_str().filter(|text| !text.trim().is_empty()) {
            return text.to_owned();
        }
    }
    let details = &row["reasoning_details"];
    let decoded;
    let details = if let Some(text) = details.as_str() {
        if text.len() > DETAILS_BYTES {
            return String::new();
        }
        let text = text.trim();
        if !text.starts_with(['[', '{']) {
            return text.to_owned();
        }
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return String::new();
        };
        decoded = value;
        &decoded
    } else {
        details
    };
    let blocks = if let Some(blocks) = details.as_array() {
        if blocks.len() > DETAIL_ROWS {
            return String::new();
        }
        blocks.as_slice()
    } else {
        std::slice::from_ref(details)
    };
    let mut prose = Vec::new();
    let mut bytes = 0;
    for block in blocks {
        let text = match block["type"].as_str() {
            Some("reasoning.summary") => block["summary"].as_str(),
            Some("reasoning.text") => block["text"].as_str(),
            Some("thinking") => block["thinking"].as_str(),
            _ => block.as_str(),
        };
        if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty())
            && !prose.contains(&text)
        {
            bytes += text.len() + if prose.is_empty() { 0 } else { 2 };
            if bytes > DETAILS_BYTES {
                return String::new();
            }
            prose.push(text);
        }
    }
    prose.join("\n\n")
}
