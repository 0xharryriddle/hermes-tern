//! Explicit owner reads, not a raw slash-command or shell execution channel.
use crate::session::SessionState;
use hermes_tern_core::{
    error::{Error, Result},
    text::display_text,
};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug)]
pub struct Row {
    pub name: String,
    pub label: String,
    pub readable: bool,
}
#[derive(Debug)]
pub struct Catalog {
    pub rows: Vec<Row>,
    pub degraded: bool,
}
impl Catalog {
    pub fn parse(value: Value) -> Result<Self> {
        let pairs = value["pairs"]
            .as_array()
            .filter(|p| !p.is_empty() && p.len() <= 4096)
            .ok_or(Error::Protocol("invalid or oversized command catalog"))?;
        let mut metadata = Vec::new();
        for key in ["canon", "commands", "skills", "plugin_commands"] {
            metadata.push(
                value[key]
                    .as_object()
                    .filter(|v| v.len() <= 4096)
                    .ok_or(Error::Protocol("invalid command metadata"))?,
            );
        }
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        for pair in pairs {
            let pair = pair
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or(Error::Protocol("invalid command pair"))?;
            let name = pair[0]
                .as_str()
                .filter(|s| {
                    s.len() > 1
                        && s.len() <= 128
                        && s.starts_with('/')
                        && s[1..].split(':').all(|part| {
                            !part.is_empty()
                                && part
                                    .chars()
                                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
                        })
                })
                .ok_or(Error::Protocol("invalid command name"))?;
            if !seen.insert(name.to_owned()) {
                return Err(Error::Protocol("duplicate command name"));
            }
            let label = pair[1]
                .as_str()
                .filter(|s| s.len() <= 65536)
                .ok_or(Error::Protocol("invalid command label"))?;
            let readable = matches!(
                name,
                "/help"
                    | "/status"
                    | "/context"
                    | "/version"
                    | "/usage"
                    | "/profile"
                    | "/memory"
                    | "/skills"
                    | "/toolsets"
                    | "/tools"
                    | "/plugins"
                    | "/platforms"
            ) && metadata[0].get(name).and_then(Value::as_str) == Some(name)
                && metadata[1].get(name).is_some_and(Value::is_object)
                && !metadata[2].contains_key(name)
                && !metadata[3].contains_key(&name[1..]);
            if readable && label.len() > 1024 {
                return Err(Error::Protocol("oversized builtin command label"));
            }
            rows.push(Row {
                name: name.into(),
                label: if readable {
                    display_text(label)
                } else {
                    "Not supported in this panel".into()
                },
                readable,
            });
        }
        Ok(Self {
            rows,
            degraded: ["warning", "plugin_discovery_error"].iter().any(|k| {
                value
                    .get(k)
                    .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            }),
        })
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Empty,
    Loading,
    Ready,
    Running,
    Failed,
    Unknown,
}
#[derive(Clone, Copy)]
enum ReadKind {
    Handler,
    Tools,
    Plugins,
}
pub struct ReadRequest {
    pub method: &'static str,
    pub params: Value,
}
#[derive(PartialEq, Eq)]
struct Fence {
    profile: String,
    instance: String,
    session: String,
    authority: u64,
    revision: u64,
    generation: u64,
}
impl Fence {
    fn of(s: &SessionState) -> Self {
        Self {
            profile: s.profile.clone(),
            instance: s.instance.clone(),
            session: s.session_id.clone(),
            authority: s.authority_epoch,
            revision: s.revision,
            generation: s.generation,
        }
    }
    fn current(&self, s: &SessionState) -> bool {
        s.connected && !s.is_suspended() && *self == Self::of(s)
    }
}
#[derive(Default)]
pub struct Commands {
    phase: Phase,
    id: String,
    fence: Option<Fence>,
    catalog: Option<Catalog>,
    output: Vec<String>,
    read_kind: Option<ReadKind>,
    pub notice: &'static str,
}
impl Commands {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn catalog(&self) -> Option<&Catalog> {
        self.catalog.as_ref()
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn output(&self) -> &[String] {
        &self.output
    }
    pub fn loading(&mut self, s: &SessionState) -> Result<String> {
        if !s.connected
            || s.is_suspended()
            || matches!(self.phase, Phase::Loading | Phase::Running | Phase::Unknown)
        {
            return Err(Error::Protocol(
                "commands unavailable until current operation settles",
            ));
        }
        self.id = uuid::Uuid::new_v4().to_string();
        self.fence = Some(Fence::of(s));
        self.catalog = None;
        self.output.clear();
        self.read_kind = None;
        self.phase = Phase::Loading;
        self.notice = "Loading owner command catalog";
        Ok(self.id.clone())
    }
    pub fn loaded(&mut self, s: &SessionState, id: &str, value: Value) -> Result<()> {
        if self.phase != Phase::Loading
            || id != self.id
            || !self.fence.as_ref().is_some_and(|f| f.current(s))
        {
            self.failed("Command catalog changed; reload explicitly");
            return Err(Error::Protocol("stale command catalog"));
        }
        match Catalog::parse(value) {
            Ok(c) => {
                self.catalog = Some(c);
                self.phase = Phase::Ready;
                self.notice = "Choose an explicit owner read; catalog metadata is not permission";
                Ok(())
            }
            Err(e) => {
                self.failed("Command catalog unavailable or malformed");
                Err(e)
            }
        }
    }
    pub fn read(&mut self, s: &SessionState, id: &str, index: usize) -> Result<ReadRequest> {
        if self.phase != Phase::Ready
            || id != self.id
            || !s.can_submit()
            || !self.fence.as_ref().is_some_and(|f| f.current(s))
        {
            return Err(Error::Protocol(
                "command selection changed; reload current catalog",
            ));
        }
        let row = self
            .catalog
            .as_ref()
            .and_then(|c| c.rows.get(index))
            .filter(|r| r.readable)
            .ok_or(Error::Unsupported("command is not an allowed owner read"))?;
        let (kind, request) = if row.name == "/tools" {
            (
                ReadKind::Tools,
                ReadRequest {
                    method: "tools.show",
                    params: json!({"session_id":s.session_id}),
                },
            )
        } else if row.name == "/plugins" {
            (
                ReadKind::Plugins,
                ReadRequest {
                    method: "plugins.list",
                    params: json!({}),
                },
            )
        } else {
            (
                ReadKind::Handler,
                ReadRequest {
                    method: "command.dispatch",
                    params: json!({"session_id":s.session_id,"name":&row.name[1..],"arg":""}),
                },
            )
        };
        self.read_kind = Some(kind);
        self.output.clear();
        self.phase = Phase::Running;
        self.notice = "Owner read pending; no automatic retry";
        Ok(request)
    }
    pub fn result(&mut self, s: &SessionState, value: Value) -> Result<()> {
        if self.phase != Phase::Running || !self.fence.as_ref().is_some_and(|f| f.current(s)) {
            self.unknown("Command result unavailable or changed; no directive was submitted");
            return Err(Error::Protocol("invalid or stale command read response"));
        }
        let text = match self.read_kind {
            Some(ReadKind::Tools) => match crate::tool_catalog::render(value) {
                Ok(text) => text,
                Err(error) => {
                    self.unknown("Tool catalog unavailable or malformed; no output accepted");
                    return Err(error);
                }
            },
            Some(ReadKind::Plugins) => match crate::plugin_catalog::render(value) {
                Ok(text) => text,
                Err(error) => {
                    self.unknown("Plugin inventory unavailable or malformed; no output accepted");
                    return Err(error);
                }
            },
            Some(ReadKind::Handler) if value["type"] == "exec" => {
                let Some(raw) = value["output"].as_str().filter(|s| s.len() <= 65536) else {
                    self.unknown("Command output malformed or exceeds 64 KiB; no output accepted");
                    return Err(Error::Protocol("invalid command output"));
                };
                display_text(raw)
            }
            _ => {
                self.unknown("Command result unavailable or changed; no directive was submitted");
                return Err(Error::Protocol("invalid command read response"));
            }
        };
        let mut start = 0;
        while start < text.len() {
            let mut end = (start + 4096).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            self.output.push(text[start..end].into());
            start = end;
        }
        if self.output.is_empty() {
            self.output.push("(empty owner output)".into());
        }
        // An action already decoded for the previous result cannot run it twice.
        self.id = uuid::Uuid::new_v4().to_string();
        self.read_kind = None;
        self.phase = Phase::Ready;
        self.notice = "Owner read completed; text is data, not approval or a command";
        Ok(())
    }
    pub fn failed(&mut self, notice: &'static str) {
        self.phase = Phase::Failed;
        self.read_kind = None;
        self.catalog = None;
        self.output.clear();
        self.notice = notice;
    }
    pub fn unknown(&mut self, notice: &'static str) {
        self.phase = Phase::Unknown;
        self.read_kind = None;
        self.catalog = None;
        self.output.clear();
        self.notice = notice;
    }
    pub fn disconnected(&mut self) {
        self.unknown("Command outcome unavailable after disconnect; do not blindly repeat");
    }
}
