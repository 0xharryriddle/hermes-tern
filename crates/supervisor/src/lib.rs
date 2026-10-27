//! Bounded Linux Unix-socket service. Credentials travel only in private frames/files.
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use hermes_tern_store::PeerIdentity;
use hermes_tern_store::{
    DispositionInput, MessageInput, Registration, Store, StoreLimits, private_dir,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    os::{
        linux::net::SocketAddrExt,
        unix::net::{SocketAddr, UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const FRAME: usize = 262144;
const RESPONSE: usize = 524288;

mod credential;
mod operator_key;
pub mod operator_key_recovery;
pub mod peer;
pub mod recovery;
mod secrets;
use peer::{PeerBinding, PeerOperation};
use secrets::create as create_secret;

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Hello {},
    BindPeer {
        actor: String,
        hermes_home: PathBuf,
        physical_session_id: String,
        attest_binding: bool,
    },
    BoundPeer {
        identity: PeerIdentity,
        operation: PeerOperation,
    },
    Register {
        kind: String,
        profile: String,
        session: String,
        model: String,
        provider: String,
    },
    Rotate {
        actor: String,
    },
    InspectStagedCredential {
        staging_id: String,
        actor: String,
        incarnation: String,
    },
    RecoverCredential {
        staging_id: String,
        actor: String,
        incarnation: String,
    },
    Route {
        sender: String,
        recipient: String,
        allow: bool,
    },
    Actors {},
    Messages {},
    Snapshot {},
    StateIndex {},
    StatePage {
        workspace: String,
        epoch: String,
        revision: String,
        section: String,
        offset: String,
        limit: u16,
    },
    Archive {
        archive_id: String,
    },
    InspectArchive {
        archive_id: String,
    },
    InspectStagedArchive {
        staging_id: String,
        archive_id: String,
    },
    RecoverArchive {
        staging_id: String,
        archive_id: String,
        database_sha256: String,
    },
    Events {
        workspace: String,
        epoch: String,
        after: String,
        limit: u16,
    },
    InspectMessage {
        sender: String,
        message: String,
    },
    DisposeMessage {
        disposition: DispositionInput,
    },
    InspectTask {
        task: String,
    },
    Send {
        message: MessageInput,
    },
    Receive {},
    Ack {
        sender: String,
        message: String,
    },
    Claim {
        task: String,
        ttl_seconds: u64,
    },
    Complete {
        task: String,
        generation: String,
        result: String,
    },
    ReleaseStopped {
        task: String,
        owner: String,
        incarnation: String,
        generation: String,
        attest_stopped: bool,
    },
    Shutdown {},
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    v: u32,
    id: String,
    auth: String,
    op: Operation,
}
pub fn validate_request(bytes: &[u8]) -> Result<Request> {
    if bytes.len() > FRAME {
        return Err(Error::Protocol("IPC frame exceeds limit"));
    }
    let request: Request = serde_json::from_value(strict_json(bytes)?)
        .map_err(|_| Error::Protocol("invalid IPC request"))?;
    if request.v != 1
        || request.id.is_empty()
        || request.id.len() > 128
        || request.auth.len() != 64
        || !request.auth.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Protocol("invalid IPC version or credential"));
    }
    Ok(request)
}
fn endpoint(dir: &Path) -> Result<SocketAddr> {
    let path = dir.canonicalize()?;
    let hash = Sha256::digest(path.as_os_str().as_encoded_bytes());
    let suffix: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    // Abstract sockets vanish with their owner; no stale socket is ever deleted.
    // Linux scope is explicit. Token authentication remains mandatory for all calls.
    SocketAddr::from_abstract_name(format!(
        "hermes-tern-{}-{suffix}",
        rustix::process::geteuid().as_raw()
    ))
    .map_err(Error::from)
}
fn read_secret(path: &Path) -> Result<String> {
    Ok(secrets::PrivateFile::open(path)?.into_text())
}
fn valid_token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn equal_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |x, (a, b)| x | (a ^ b)) == 0
}
fn read_frame(stream: &mut UnixStream, limit: usize, deadline: Instant) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or(Error::Timeout("IPC frame"))?;
        stream.set_read_timeout(Some(left))?;
        let mut chunk = [0; 4096];
        let n = match stream.read(&mut chunk) {
            Ok(n) => n,
            // Continue this frame, not the operation that produced it. Keep both
            // the accumulated prefix and the original absolute read deadline.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        if n == 0 {
            return Err(Error::Protocol("partial IPC frame at EOF"));
        }
        if bytes.len() + n > limit {
            return Err(Error::Protocol("IPC byte limit"));
        }
        if let Some(end) = chunk[..n].iter().position(|b| *b == b'\n') {
            if end + 1 != n {
                return Err(Error::Protocol("one request per connection"));
            }
            bytes.extend_from_slice(&chunk[..end]);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
}
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::Store("system clock before epoch"))
}

pub struct Supervisor {
    store: Store,
    listener: UnixListener,
    operator: String,
}
impl Supervisor {
    pub fn bind(dir: &Path, workspace: &Path, limits: StoreLimits) -> Result<Self> {
        let (store, mut key) = Store::open_with_startup_guard(dir, workspace, limits, |exists| {
            operator_key::prepare(dir, exists)
        })?;
        secrets::prepare_directory(&dir.join("credentials"))?;
        key.recheck()?;
        let operator = key.into_text();
        let listener = UnixListener::bind_addr(&endpoint(dir)?)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            store,
            listener,
            operator,
        })
    }
    pub fn run(mut self, stop: Arc<AtomicBool>) -> Result<()> {
        while !stop.load(Ordering::Acquire) {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
                    let response = match read_frame(
                        &mut stream,
                        FRAME,
                        Instant::now() + Duration::from_millis(500),
                    )
                    .and_then(|b| validate_request(&b))
                    {
                        Ok(request) => {
                            let id = request.id;
                            let admin = equal_secret(&request.auth, &self.operator);
                            match self.dispatch(admin, &request.auth, request.op, &stop) {
                                Ok(value) => json!({"v":1,"id":id,"ok":true,"result":value}),
                                Err(e) => json!({"v":1,"id":id,"ok":false,"error":e.to_string()}),
                            }
                        }
                        Err(_) => {
                            json!({"v":1,"id":null,"ok":false,"error":"invalid_or_incomplete_request"})
                        }
                    };
                    let mut encoded = response.to_string();
                    encoded.push('\n');
                    if encoded.len() <= RESPONSE {
                        let _ = stream.write_all(encoded.as_bytes());
                    }
                    // Lost response is not rollback: durable admission/delivery stays observable.
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn credential_receipt(&self, r: &Registration) -> Value {
        let filename = format!("{}-{}.json", r.actor_id, r.incarnation);
        let path = self.store.state_dir.join("credentials").join(filename);
        json!({"actor_id":r.actor_id,"incarnation":r.incarnation,"credential_file":path})
    }
    fn dispatch(
        &mut self,
        admin: bool,
        credential: &str,
        op: Operation,
        stop: &AtomicBool,
    ) -> Result<Value> {
        match op {
            Operation::BindPeer {
                actor,
                hermes_home,
                physical_session_id,
                attest_binding,
            } if admin => {
                self.bind_peer(&actor, &hermes_home, &physical_session_id, attest_binding)
            }
            Operation::BoundPeer {
                identity,
                operation,
            } if !admin => {
                self.store.verify_peer(credential, &identity)?;
                match operation {
                    PeerOperation::Status {} => self.store.peer_status(credential, &identity),
                    other => self.dispatch(false, credential, other.into_operation()?, stop),
                }
            }
            Operation::Hello {} if admin => Ok(
                json!({"protocol":"hermes-tern-1","workspace_id":self.store.workspace_id,"authority":"operator","sandbox":false,"native_omp":false}),
            ),
            Operation::Register {
                kind,
                profile,
                session,
                model,
                provider,
            } if admin => {
                let dir = self.store.state_dir.clone();
                let workspace = self.store.workspace_id.clone();
                let r = self.store.register_with_publisher(
                    &kind,
                    &profile,
                    &session,
                    &model,
                    &provider,
                    |r| publish_credential(&dir, &workspace, r),
                )?;
                Ok(self.credential_receipt(&r))
            }
            Operation::Rotate { actor } if admin => {
                let dir = self.store.state_dir.clone();
                let workspace = self.store.workspace_id.clone();
                let r = self
                    .store
                    .rotate_with_publisher(&actor, |r| publish_credential(&dir, &workspace, r))?;
                Ok(self.credential_receipt(&r))
            }
            Operation::Route {
                sender,
                recipient,
                allow,
            } if admin => {
                self.store.route(&sender, &recipient, allow)?;
                Ok(json!({"allowed":allow}))
            }
            Operation::InspectStagedCredential {
                staging_id,
                actor,
                incarnation,
            } if admin => self.credential_publication(&staging_id, &actor, &incarnation, false),
            Operation::RecoverCredential {
                staging_id,
                actor,
                incarnation,
            } if admin => self.credential_publication(&staging_id, &actor, &incarnation, true),
            Operation::Actors {} if admin => {
                let mut actors = self.store.actors()?;
                for actor in &mut actors {
                    let id = actor["actor_id"]
                        .as_str()
                        .ok_or(Error::Store("invalid actor"))?;
                    let incarnation = actor["incarnation"]
                        .as_str()
                        .ok_or(Error::Store("invalid incarnation"))?;
                    let path = self
                        .store
                        .state_dir
                        .join("credentials")
                        .join(format!("{id}-{incarnation}.json"));
                    actor["credential_file"] = json!(path);
                }
                Ok(json!({"actors":actors}))
            }
            Operation::Messages {} if admin => {
                Ok(json!({"messages":self.store.messages()?,"limit":128}))
            }
            Operation::Snapshot {} if admin => self.store.observation_snapshot(),
            Operation::StateIndex {} if admin => self.store.state_index(),
            Operation::StatePage {
                workspace,
                epoch,
                revision,
                section,
                offset,
                limit,
            } if admin => self
                .store
                .state_page(&workspace, &epoch, &revision, &section, &offset, limit),
            Operation::Archive { archive_id } if admin => self.store.archive(&archive_id),
            Operation::InspectArchive { archive_id } if admin => {
                self.store.inspect_archive(&archive_id)
            }
            Operation::InspectStagedArchive {
                staging_id,
                archive_id,
            } if admin => self.store.inspect_staged_archive(&staging_id, &archive_id),
            Operation::RecoverArchive {
                staging_id,
                archive_id,
                database_sha256,
            } if admin => self
                .store
                .recover_archive(&staging_id, &archive_id, &database_sha256),
            Operation::Events {
                workspace,
                epoch,
                after,
                limit,
            } if admin => self
                .store
                .observation_events(&workspace, &epoch, &after, limit),
            Operation::InspectMessage { sender, message } if admin => {
                self.store.inspect_message(&sender, &message)
            }
            Operation::DisposeMessage { disposition } if admin => {
                self.store.dispose_message(&disposition, now()?)
            }
            Operation::InspectTask { task } if admin => self.store.inspect_task(&task),
            Operation::ReleaseStopped {
                task,
                owner,
                incarnation,
                generation,
                attest_stopped,
            } if admin => {
                self.store
                    .release_stopped(&task, &owner, &incarnation, &generation, attest_stopped)
            }
            Operation::Shutdown {} if admin => {
                stop.store(true, Ordering::Release);
                Ok(json!({"stopping":true}))
            }
            Operation::Send { message } if !admin => self.store.send(credential, &message, now()?),
            Operation::Receive {} if !admin => {
                Ok(json!({"messages":self.store.receive(credential,now()?)?,"limit":8}))
            }
            Operation::Ack { sender, message } if !admin => {
                self.store.ack(credential, &sender, &message, now()?)
            }
            Operation::Claim { task, ttl_seconds } if !admin => {
                self.store.claim(credential, &task, now()?, ttl_seconds)
            }
            Operation::Complete {
                task,
                generation,
                result,
            } if !admin => self
                .store
                .complete(credential, &task, &generation, &result, now()?),
            _ => Err(Error::Store("operation not allowed for this credential")),
        }
    }
    fn bind_peer(&self, actor: &str, home: &Path, session: &str, attests: bool) -> Result<Value> {
        if !attests || !home.is_absolute() || !home.is_dir() || home.canonicalize()? != home {
            return Err(Error::Config(
                "explicit binding attestation and canonical Hermes home required",
            ));
        }
        let row = self
            .store
            .actors()?
            .into_iter()
            .find(|r| r["actor_id"] == actor)
            .ok_or(Error::Store("actor not found"))?;
        if row["kind"] != "hermes"
            || row["profile"] != home.to_string_lossy().as_ref()
            || row["session_id"] != session
        {
            return Err(Error::Store(
                "actor does not match the attested Hermes context",
            ));
        }
        let identity = PeerIdentity {
            workspace_id: self.store.workspace_id.clone(),
            actor_id: actor.into(),
            incarnation: row["incarnation"]
                .as_str()
                .ok_or(Error::Store("invalid incarnation"))?
                .into(),
            hermes_home: home
                .to_str()
                .ok_or(Error::Config("home must be UTF-8"))?
                .into(),
            physical_session_id: session.into(),
        };
        let dir = self.store.state_dir.canonicalize()?;
        let credential_file = dir
            .join("credentials")
            .join(format!("{}-{}.json", actor, identity.incarnation));
        let client = Client::peer(&dir, &credential_file)?;
        self.store.verify_peer(&client.credential, &identity)?;
        let binding = PeerBinding {
            schema_version: 1,
            state_dir: dir.clone(),
            credential_file,
            identity,
        };
        let body =
            serde_json::to_string(&binding).map_err(|_| Error::Protocol("invalid binding"))?;
        if body.len() > 4096 {
            return Err(Error::Config("binding exceeds private file limit"));
        }
        let path = dir
            .join("bindings")
            .join(format!("{}-{}.json", actor, binding.identity.incarnation));
        if path.symlink_metadata().is_ok() {
            let mut file = secrets::PrivateFile::open(&path)?;
            if file.text() != body {
                return Err(Error::Store(
                    "existing binding differs; explicit operator reconciliation required",
                ));
            }
            secrets::prepare_directory(&dir.join("bindings"))?;
            file.sync()?;
        } else {
            create_secret(&path, &body)?;
        }
        Ok(
            json!({"binding_file":path,"actor_id":actor,"identity_scope":"operator_attested_physical_session"}),
        )
    }
}

fn publish_credential(dir: &Path, workspace: &str, r: &Registration) -> Result<()> {
    let path = dir
        .join("credentials")
        .join(format!("{}-{}.json", r.actor_id, r.incarnation));
    let body = json!({"schema_version":1,"workspace_id":workspace,"actor_id":r.actor_id,"incarnation":r.incarnation,"credential":r.credential}).to_string();
    create_secret(&path, &body)
}

pub struct Client {
    dir: PathBuf,
    credential: String,
}
impl Client {
    pub fn operator(dir: &Path) -> Result<Self> {
        private_dir_existing(dir)?;
        let credential = read_secret(&dir.join("operator.key"))?;
        if !valid_token(&credential) {
            return Err(Error::Store("invalid operator credential"));
        }
        Ok(Self {
            dir: dir.into(),
            credential,
        })
    }
    pub fn peer(dir: &Path, file: &Path) -> Result<Self> {
        private_dir_existing(dir)?;
        let value = strict_json(read_secret(file)?.as_bytes())?;
        let credential = value["credential"]
            .as_str()
            .filter(|s| valid_token(s))
            .ok_or(Error::Store("invalid actor credential file"))?
            .to_owned();
        Ok(Self {
            dir: dir.into(),
            credential,
        })
    }
    pub fn call(&self, op: Operation) -> Result<Value> {
        let mut stream = UnixStream::connect_addr(&endpoint(&self.dir)?)?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let id = uuid::Uuid::new_v4().to_string();
        let request = Request {
            v: 1,
            id: id.clone(),
            auth: self.credential.clone(),
            op,
        };
        let mut bytes =
            serde_json::to_vec(&request).map_err(|_| Error::Protocol("request serialization"))?;
        bytes.push(b'\n');
        if bytes.len() > FRAME {
            return Err(Error::Protocol("IPC outgoing frame exceeds limit"));
        }
        stream.write_all(&bytes)?;
        let response = strict_json(&read_frame(
            &mut stream,
            RESPONSE,
            Instant::now() + Duration::from_secs(3),
        )?)?;
        if response["v"] != 1 || response["id"] != id {
            return Err(Error::Protocol("IPC response identity mismatch"));
        }
        if response["ok"] != true {
            return Err(Error::Store(
                "supervisor refused request; check identity, route, bounds and lifecycle",
            ));
        }
        response
            .get("result")
            .cloned()
            .ok_or(Error::Protocol("missing IPC result"))
    }
}
fn private_dir_existing(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Err(Error::Store("supervisor state not initialized"));
    }
    private_dir(dir)
}
