//! Complete bounded projection of the owner's plugin report, never plugin authority.
use hermes_tern_core::{
    error::{Error, Result},
    text::{display_text, is_bidi_control},
};
use serde_json::Value;
use std::collections::HashSet;

const MAX_ROWS: usize = 128;
const MAX_OUTPUT: usize = 65_536;

fn text(value: &Value, limit: usize) -> Result<&str> {
    value
        .as_str()
        .filter(|s| s.len() <= limit)
        .ok_or(Error::Protocol("invalid or oversized plugin report text"))
}
fn single_line(value: &Value, limit: usize, identity: bool) -> Result<&str> {
    let s = text(value, limit)?;
    if s.chars()
        .any(|c| c.is_control() || is_bidi_control(c) || matches!(c, '\u{2028}' | '\u{2029}'))
        || (identity && (s.is_empty() || s.trim() != s))
    {
        return Err(Error::Protocol("unsafe plugin report identity"));
    }
    Ok(s)
}
fn rows<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    value[key]
        .as_array()
        .filter(|rows| rows.len() <= MAX_ROWS)
        .map(Vec::as_slice)
        .ok_or(Error::Protocol("invalid or oversized plugin report array"))
}
fn count(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|n| *n <= 4096)
        .ok_or(Error::Protocol("invalid plugin registration count"))
}
fn append(out: &mut String, text: &str) -> Result<()> {
    if out
        .len()
        .checked_add(text.len())
        .is_none_or(|n| n > MAX_OUTPUT)
    {
        return Err(Error::Protocol("plugin report output limit exceeded"));
    }
    out.push_str(text);
    Ok(())
}
/// Render only complete canonical metadata; private paths, settings and errors are discarded.
///
/// # Errors
/// Refuses missing/malformed fields, unsafe or duplicate keys and exceeded bounds.
pub fn render(value: Value) -> Result<String> {
    let plugins = rows(&value, "plugins")?;
    let toolsets = rows(&value, "toolsets")?;
    let servers = rows(&value, "portable_mcp_servers")?;
    let mut output = String::new();
    append(
        &mut output,
        &format!(
            "Owner plugins · {} plugins · {} toolsets · {} portable MCP\n",
            plugins.len(),
            toolsets.len(),
            servers.len()
        ),
    )?;
    append(
        &mut output,
        "Report only, not health or execution permission.\n",
    )?;
    let mut seen = HashSet::new();
    for plugin in plugins {
        let key = single_line(&plugin["key"], 256, true)?;
        let name = single_line(&plugin["name"], 256, true)?;
        let version = single_line(&plugin["version"], 256, false)?;
        if !seen.insert(key) {
            return Err(Error::Protocol("duplicate plugin report key"));
        }
        let enabled = plugin["enabled"]
            .as_bool()
            .ok_or(Error::Protocol("invalid plugin enabled flag"))?;
        let tools = count(&plugin["tools"])?;
        let hooks = count(&plugin["hooks"])?;
        let commands = count(&plugin["commands"])?;
        let error = text(&plugin["error"], 16_384)?;
        let status = if enabled { "enabled" } else { "disabled" };
        let error = if error.is_empty() { "no" } else { "reported" };
        append(
            &mut output,
            &format!(
                "Plugin: {name} [{key}] v{version} {status}; tools={tools} hooks={hooks} commands={commands}; load error={error}\n"
            ),
        )?;
    }
    if plugins.is_empty() {
        append(&mut output, "No plugins reported by this owner\n")?;
    }
    seen.clear();
    for toolset in toolsets {
        let key = single_line(&toolset["key"], 128, true)?;
        if !seen.insert(key) {
            return Err(Error::Protocol("duplicate plugin toolset key"));
        }
        let label = display_text(text(&toolset["label"], 256)?)
            .replace(['\u{2028}', '\u{2029}', '\n'], "\\n");
        let description = display_text(text(&toolset["description"], 1024)?)
            .replace(['\u{2028}', '\u{2029}'], "\n");
        append(&mut output, &format!("Toolset: {key} · {label}\n"))?;
        for line in description.split('\n') {
            append(&mut output, &format!("  Description: {line}\n"))?;
        }
    }
    seen.clear();
    for server in servers {
        let name = single_line(server, 256, true)?;
        if !seen.insert(name) {
            return Err(Error::Protocol("duplicate portable MCP name"));
        }
        append(&mut output, &format!("Portable MCP: {name}\n"))?;
    }
    Ok(output)
}
