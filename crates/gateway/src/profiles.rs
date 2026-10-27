//! Owner-reported existing profiles. Roster metadata grants no attach authority.
use crate::{config::Limits, inspect, transport::RpcClient};
use hermes_tern_core::{
    error::{Error, Result},
    text::{display_text, is_bidi_control},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Component, PathBuf},
};

#[derive(Debug)]
pub struct Row {
    pub name: String,
    pub home: PathBuf,
    pub label: String,
}
#[derive(Debug)]
pub struct Catalog {
    pub rows: Vec<Row>,
}
impl Catalog {
    pub fn parse(value: Value, current: &str) -> Result<Self> {
        let entries = value["profiles"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 64)
            .ok_or(Error::Protocol("invalid or oversized profile roster"))?;
        let mut names = HashSet::new();
        let mut paths = HashSet::new();
        let mut rows = Vec::new();
        for entry in entries {
            let name = entry["name"]
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 64
                        && s.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b)
                        })
                        && s.as_bytes()[0].is_ascii_alphanumeric()
                })
                .ok_or(Error::Protocol("invalid profile name"))?;
            let path = entry["path"]
                .as_str()
                .filter(|s| {
                    s.len() <= 256
                        && !s.chars().any(|c| c.is_control() || is_bidi_control(c))
                        && s.starts_with('/')
                        && !s.ends_with('/')
                        && !s.contains("//")
                        && !s.split('/').any(|part| part == "." || part == "..")
                })
                .ok_or(Error::Protocol("invalid profile home"))?;
            let home = PathBuf::from(path);
            if !home.is_absolute()
                || home
                    .components()
                    .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
                || !names.insert(name.to_owned())
                || !paths.insert(path.to_owned())
            {
                return Err(Error::Protocol(
                    "duplicate or noncanonical profile identity",
                ));
            }
            let label = match entry.get("display_name") {
                None | Some(Value::Null) => name,
                Some(Value::String(s)) if s.len() <= 256 => s,
                _ => return Err(Error::Protocol("invalid profile display name")),
            };
            rows.push(Row {
                name: name.into(),
                home,
                label: display_text(label),
            });
        }
        if !paths.contains(current) {
            return Err(Error::Protocol(
                "profile roster omits qualified current owner",
            ));
        }
        Ok(Self { rows })
    }
}
/// This optional read never creates a profile, starts an owner or lists sessions.
pub fn fetch(client: &mut RpcClient, limits: &Limits) -> Result<Catalog> {
    let value = inspect::call(
        client,
        "profiles.list",
        json!({"include_sessions":false}),
        limits.request_timeout,
    )?;
    Catalog::parse(value, &client.profile_id)
        .map_err(|_| Error::Unsupported("invalid profile inventory"))
}
