//! Local operator proposals and a one-way dispatch fence, never an OMP receipt.
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const MAX_INPUT: usize = 65536;
const MAX_PROPOSALS: usize = 64;
const MAX_FILE: usize = 65536;
const EVENTS: [&str; 3] = ["ask_returned", "idle_observed", "outcome_unknown"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u32,
    request_id: String,
    op: Operation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Offer {
        text: String,
        source: String,
    },
    Prepare {
        handoff_id: String,
    },
    Arm {
        handoff_id: String,
        attempt_id: String,
        target_id: String,
        digest: String,
    },
    Record {
        handoff_id: String,
        attempt_id: String,
        target_id: String,
        event: String,
    },
    Inspect {
        handoff_id: String,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    v: u32,
    request_id: String,
    handoff_id: String,
    attempt_id: String,
    target_id: String,
    digest: String,
    text: String,
    source: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    v: u32,
    handoff_id: String,
    attempt_id: String,
    target_id: String,
    digest: String,
    event: String,
}
impl Marker {
    fn new(p: &Proposal, event: &str) -> Self {
        Self {
            v: 1,
            handoff_id: p.handoff_id.clone(),
            attempt_id: p.attempt_id.clone(),
            target_id: p.target_id.clone(),
            digest: p.digest.clone(),
            event: event.into(),
        }
    }
    fn matches(&self, p: &Proposal) -> bool {
        self.v == 1
            && self.handoff_id == p.handoff_id
            && self.attempt_id == p.attempt_id
            && self.target_id == p.target_id
            && self.digest == p.digest
    }
}
fn canonical_id(id: &str) -> Result<()> {
    if Uuid::parse_str(id)
        .ok()
        .is_none_or(|u| u.is_nil() || u.to_string() != id)
    {
        return Err(Error::Protocol("noncanonical handoff identity"));
    }
    Ok(())
}
fn text_safe(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(|c|
        (c.is_control() && c != '\n' && c != '\t')
            || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
fn valid_offer(text: &str, source: &str) -> Result<()> {
    if !text_safe(text, 8192)
        || !text_safe(source, 256)
        || source
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        || text.trim_start().starts_with(['/', '!', '$', '@'])
    {
        return Err(Error::Protocol("invalid handoff text or source"));
    }
    Ok(())
}
fn digest(text: &str, source: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"hermes-tern-omp-handoff-v1\0");
    for s in [source, text] {
        hash.update((s.len() as u64).to_be_bytes());
        hash.update(s.as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_value(strict_json(bytes)?)
        .map_err(|_| Error::Protocol("invalid handoff document"))
}
fn safe_file(file: &File) -> Result<fs::Metadata> {
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o777 != 0o600
        || m.nlink() != 1
        || m.len() > MAX_FILE as u64
    {
        return Err(Error::Store("unsafe handoff file"));
    }
    Ok(m)
}
fn identity(m: &fs::Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}
struct State {
    path: PathBuf,
    dir: File,
    _lock: File,
}
impl State {
    fn open(path: &Path, create: bool) -> Result<Self> {
        if !path.is_absolute() {
            return Err(Error::Config("handoff state must be absolute"));
        }
        let parent = path
            .parent()
            .ok_or(Error::Config("handoff parent unavailable"))?;
        if parent.canonicalize()? != parent {
            return Err(Error::Store("noncanonical handoff parent"));
        }
        if create && !path.try_exists()? {
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e.into()),
            }
        }
        if path.canonicalize()? != path {
            return Err(Error::Store("noncanonical handoff directory"));
        }
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::DIRECTORY).bits() as i32,
            )
            .open(path)?;
        let m = dir.metadata()?;
        if !m.is_dir()
            || m.uid() != rustix::process::geteuid().as_raw()
            || m.mode() & 0o777 != 0o700
        {
            return Err(Error::Store("handoff directory must be owner-only"));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .truncate(false)
            .mode(0o600)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(path.join("handoff.lock"))?;
        if safe_file(&lock)?.len() != 0 {
            return Err(Error::Store("invalid handoff lock"));
        }
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Store("handoff state busy; no dispatch grant"))?;
        let state = Self {
            path: path.to_owned(),
            dir,
            _lock: lock,
        };
        state.check_dir()?;
        if create {
            state._lock.sync_all()?;
            state.dir.sync_all()?;
            File::open(
                path.parent()
                    .ok_or(Error::Config("handoff parent unavailable"))?,
            )?
            .sync_all()?;
        }
        Ok(state)
    }
    fn check_dir(&self) -> Result<()> {
        let m = fs::symlink_metadata(&self.path)?;
        if !m.is_dir()
            || m.mode() & 0o777 != 0o700
            || m.uid() != rustix::process::geteuid().as_raw()
            || identity(&m) != identity(&self.dir.metadata()?)
            || self.path.canonicalize()? != self.path
        {
            return Err(Error::Store("handoff directory changed"));
        }
        let l = fs::symlink_metadata(self.path.join("handoff.lock"))?;
        if identity(&l) != identity(&safe_file(&self._lock)?)
            || !l.is_file()
            || l.nlink() != 1
            || l.mode() & 0o777 != 0o600
        {
            return Err(Error::Store("handoff lock changed"));
        }
        Ok(())
    }
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.check_dir()?;
        let mut f = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(self.path.join(name))?;
        let before = safe_file(&f)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut f)
            .take((MAX_FILE + 1) as u64)
            .read_to_end(&mut bytes)?;
        let after = safe_file(&f)?;
        let entry = fs::symlink_metadata(self.path.join(name))?;
        if bytes.len() > MAX_FILE
            || bytes.len() as u64 != before.len()
            || before.len() != after.len()
            || identity(&after) != identity(&entry)
            || !entry.is_file()
            || entry.nlink() != 1
            || entry.mode() & 0o777 != 0o600
        {
            return Err(Error::Store("handoff file changed"));
        }
        self.check_dir()?;
        Ok(bytes)
    }
    fn publish(&self, name: &str, value: &impl Serialize) -> Result<()> {
        let bytes =
            serde_json::to_vec(value).map_err(|_| Error::Store("handoff encoding failed"))?;
        if bytes.len() > MAX_FILE {
            return Err(Error::Store("handoff file exceeds bound"));
        }
        self.check_dir()?;
        // Never unlink a partial/unsynced dispatch marker: it may have escaped.
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(self.path.join(name))?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        self.dir.sync_all()?;
        let m = safe_file(&f)?;
        if identity(&m) != identity(&fs::symlink_metadata(self.path.join(name))?)
            || self.read(name)? != bytes
        {
            return Err(Error::Store("handoff publication changed"));
        }
        Ok(())
    }
    fn sync_existing(&self, name: &str) -> Result<()> {
        let before = self.read(name)?;
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(self.path.join(name))?;
        safe_file(&f)?;
        f.sync_all()?;
        self.dir.sync_all()?;
        if self.read(name)? != before {
            return Err(Error::Store("handoff changed during sync"));
        }
        Ok(())
    }
    fn inventory(&self) -> Result<Inventory> {
        let mut names = BTreeSet::new();
        for entry in fs::read_dir(&self.path)? {
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| Error::Store("invalid handoff filename"))?;
            if names.len() > MAX_PROPOSALS * 5 {
                return Err(Error::Store("handoff inventory exceeds bound"));
            }
            names.insert(name);
        }
        let mut proposals = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for name in &names {
            if name == "handoff.lock" {
                continue;
            }
            if name.starts_with("offer-") {
                let p: Proposal = decode(&self.read(name)?)?;
                for id in [&p.request_id, &p.handoff_id, &p.attempt_id, &p.target_id] {
                    canonical_id(id)?;
                }
                valid_offer(&p.text, &p.source)?;
                if p.v != 1
                    || p.digest != digest(&p.text, &p.source)
                    || *name != format!("offer-{}.json", p.request_id)
                    || proposals.len() >= MAX_PROPOSALS
                    || !ids.insert(p.handoff_id.clone())
                    || !ids.insert(p.attempt_id.clone())
                    || !ids.insert(p.target_id.clone())
                {
                    return Err(Error::Store("invalid retained handoff proposal"));
                }
                proposals.insert(p.handoff_id.clone(), p);
            }
        }
        let mut markers = BTreeSet::new();
        for name in &names {
            if name == "handoff.lock" || name.starts_with("offer-") {
                continue;
            }
            let m: Marker = decode(&self.read(name)?)?;
            let p = proposals
                .get(&m.handoff_id)
                .ok_or(Error::Store("orphan handoff marker"))?;
            if !m.matches(p)
                || !(m.event == "arm" || EVENTS.contains(&m.event.as_str()))
                || *name != format!("{}-{}.json", m.event, m.handoff_id)
            {
                return Err(Error::Store("invalid retained handoff marker"));
            }
            markers.insert((m.handoff_id.clone(), m.event.clone()));
        }
        for (id, event) in &markers {
            if event != "arm" && !markers.contains(&(id.clone(), "arm".into())) {
                return Err(Error::Store("unarmed handoff observation"));
            }
        }
        Ok(Inventory { proposals, markers })
    }
}
struct Inventory {
    proposals: BTreeMap<String, Proposal>,
    markers: BTreeSet<(String, String)>,
}
impl Inventory {
    fn armed(&self, id: &str) -> bool {
        self.markers.contains(&(id.into(), "arm".into()))
    }
}

/// A pipe response may be lost: only the first successfully published arm grants.
pub fn exchange(state: &Path, input: &[u8]) -> Result<Value> {
    if input.len() > MAX_INPUT {
        return Err(Error::Protocol("handoff input exceeds bound"));
    }
    let request: Request = decode(input)?;
    canonical_id(&request.request_id)?;
    if request.v != 1 {
        return Err(Error::Protocol("unsupported handoff version"));
    }
    match &request.op {
        Operation::Offer { text, source } => valid_offer(text, source)?,
        Operation::Prepare { handoff_id } | Operation::Inspect { handoff_id } => {
            canonical_id(handoff_id)?
        }
        Operation::Arm {
            handoff_id,
            attempt_id,
            target_id,
            digest,
        } => {
            for id in [handoff_id, attempt_id, target_id] {
                canonical_id(id)?;
            }
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Protocol("invalid handoff digest"));
            }
        }
        Operation::Record {
            handoff_id,
            attempt_id,
            target_id,
            event,
        } => {
            for id in [handoff_id, attempt_id, target_id] {
                canonical_id(id)?;
            }
            if !EVENTS.contains(&event.as_str()) {
                return Err(Error::Protocol("invalid handoff observation"));
            }
        }
    }
    let state = State::open(state, matches!(&request.op, Operation::Offer { .. }))?;
    let inventory = state.inventory()?;
    let mut reply = match request.op {
        Operation::Offer { text, source } => {
            let name = format!("offer-{}.json", request.request_id);
            let p = if let Some(p) = inventory
                .proposals
                .values()
                .find(|p| p.request_id == request.request_id)
            {
                if p.text != text || p.source != source {
                    return Err(Error::Store("handoff request conflicts"));
                }
                state.sync_existing(&name)?;
                p.clone()
            } else {
                if inventory.proposals.len() >= MAX_PROPOSALS {
                    return Err(Error::Store("handoff quota reached"));
                }
                let p = Proposal {
                    v: 1,
                    request_id: request.request_id.clone(),
                    handoff_id: Uuid::new_v4().to_string(),
                    attempt_id: Uuid::new_v4().to_string(),
                    target_id: Uuid::new_v4().to_string(),
                    digest: digest(&text, &source),
                    text,
                    source,
                };
                state.publish(&name, &p)?;
                p
            };
            json!({"op":"offer", "proposal":p})
        }
        Operation::Prepare { handoff_id } | Operation::Inspect { handoff_id } => {
            let p = inventory
                .proposals
                .get(&handoff_id)
                .ok_or(Error::Store("unknown handoff"))?;
            let events: Vec<_> = EVENTS
                .iter()
                .filter(|e| {
                    inventory
                        .markers
                        .contains(&(handoff_id.clone(), e.to_string()))
                })
                .collect();
            // The operation name is recovered from the already strict parsed input below.
            let armed = inventory.armed(&handoff_id);
            // No available observation is an OMP receipt; lost callbacks cannot
            // turn a durable arm into a clean unsent or completed result.
            json!({"proposal":p, "armed":armed, "observations":events,
                "outcome":if armed { "unknown" } else { "not_armed" }})
        }
        Operation::Arm {
            handoff_id,
            attempt_id,
            target_id,
            digest,
        } => {
            let p = inventory
                .proposals
                .get(&handoff_id)
                .ok_or(Error::Store("unknown handoff"))?;
            if p.attempt_id != attempt_id || p.target_id != target_id || p.digest != digest {
                return Err(Error::Store("handoff fence identity mismatch"));
            }
            let fresh = !inventory.armed(&handoff_id);
            if fresh {
                state.publish(&format!("arm-{handoff_id}.json"), &Marker::new(p, "arm"))?;
            }
            json!({"op":"arm", "proposal":p, "newly_armed":fresh})
        }
        Operation::Record {
            handoff_id,
            attempt_id,
            target_id,
            event,
        } => {
            let p = inventory
                .proposals
                .get(&handoff_id)
                .ok_or(Error::Store("unknown handoff"))?;
            if p.attempt_id != attempt_id
                || p.target_id != target_id
                || !inventory.armed(&handoff_id)
            {
                return Err(Error::Store("unarmed or mismatched handoff observation"));
            }
            let name = format!("{event}-{handoff_id}.json");
            if !inventory
                .markers
                .contains(&(handoff_id.clone(), event.clone()))
            {
                state.publish(&name, &Marker::new(p, &event))?;
            } else {
                state.sync_existing(&name)?;
            }
            json!({"op":"record", "proposal":p, "event":event})
        }
    };
    if reply.get("op").is_none() {
        reply["op"] = strict_json(input)?["op"]["type"].clone();
    }
    state.check_dir()?;
    reply["v"] = json!(1);
    reply["request_id"] = json!(request.request_id);
    Ok(reply)
}

/// One EOF-terminated document with a fixed deadline, not a long-lived writer.
pub fn run_stdin(state: &Path) -> Result<Value> {
    let stdin = std::io::stdin();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut raw = Vec::new();
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or(Error::Timeout("handoff input"))?;
        let mut fds = [rustix::event::PollFd::new(
            &stdin,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(&mut fds, left.as_millis().min(100) as i32)
            .map_err(|_| Error::Transport("handoff input poll failed"))?;
        if fds[0].revents().is_empty() {
            continue;
        }
        let mut buf = [0; 4096];
        let n = rustix::io::read(&stdin, &mut buf)
            .map_err(|_| Error::Transport("handoff input read failed"))?;
        if n == 0 {
            return exchange(state, &raw);
        }
        if raw.len() + n > MAX_INPUT {
            return Err(Error::Protocol("handoff input exceeds bound"));
        }
        raw.extend_from_slice(&buf[..n]);
    }
}
