//! Bounded, read-only projection of the owner's `tools.show` response.
//! Catalog entries describe reported tools; they grant no execution authority.
use hermes_tern_core::{
    error::{Error, Result},
    text::{display_text, is_bidi_control},
};
use serde_json::Value;
use std::collections::HashSet;

const MAX_SECTIONS: usize = 128;
const MAX_TOOLS: usize = 4096;
const MAX_OUTPUT: usize = 64 * 1024;

fn identity(value: &Value, max_bytes: usize) -> Result<&str> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= max_bytes
                && s.trim() == *s
                && !s.chars().any(|c| {
                    c.is_control() || is_bidi_control(c) || matches!(c, '\u{2028}' | '\u{2029}')
                })
        })
        .ok_or(Error::Protocol("invalid owner tool catalog identity"))
}

fn append(out: &mut String, part: &str) -> Result<()> {
    if out
        .len()
        .checked_add(part.len())
        .is_none_or(|n| n > MAX_OUTPUT)
    {
        return Err(Error::Protocol("owner tool catalog output too large"));
    }
    out.push_str(part);
    Ok(())
}

/// Render only the reported section, tool, and description fields. Unavailable or
/// malformed owner responses are errors, never a fabricated empty catalog.
pub fn render(value: Value) -> Result<String> {
    let sections = value
        .get("sections")
        .and_then(Value::as_array)
        .filter(|sections| sections.len() <= MAX_SECTIONS)
        .ok_or(Error::Protocol("invalid owner tool catalog sections"))?;
    let total = value
        .get("total")
        .and_then(Value::as_u64)
        .filter(|total| *total <= MAX_TOOLS as u64)
        .ok_or(Error::Protocol("invalid owner tool catalog total"))? as usize;

    let mut output = String::new();
    append(&mut output, "Owner tool catalog · ")?;
    append(&mut output, &total.to_string())?;
    append(&mut output, " reported tools\n")?;
    append(
        &mut output,
        "Catalog entries are data, not execution permission or health checks.\n",
    )?;

    let mut groups_seen = HashSet::new();
    let mut tools_seen = HashSet::new();
    let mut count = 0;
    for section in sections {
        let group = identity(&section["name"], 128)?;
        if !groups_seen.insert(group) {
            return Err(Error::Protocol("duplicate owner tool catalog group"));
        }
        let tools = section
            .get("tools")
            .and_then(Value::as_array)
            .ok_or(Error::Protocol("invalid owner tool catalog tools"))?;
        append(&mut output, "Group: ")?;
        append(&mut output, group)?;
        append(&mut output, "\n")?;
        for tool in tools {
            if count >= total {
                return Err(Error::Protocol("owner tool catalog count mismatch"));
            }
            let name = identity(&tool["name"], 256)?;
            if !tools_seen.insert(name) {
                return Err(Error::Protocol("duplicate owner tool catalog tool"));
            }
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .filter(|s| s.len() <= 1024)
                .ok_or(Error::Protocol("invalid owner tool catalog description"))?;
            append(&mut output, "  Tool: ")?;
            append(&mut output, name)?;
            append(&mut output, "\n")?;
            let safe_description = display_text(description);
            for line in safe_description.split('\n') {
                append(&mut output, "    Description: ")?;
                append(&mut output, line)?;
                append(&mut output, "\n")?;
            }
            count += 1;
        }
    }
    if count != total {
        return Err(Error::Protocol("owner tool catalog count mismatch"));
    }
    if total == 0 {
        append(&mut output, "No tools reported by this owner\n")?;
    }
    Ok(output)
}
