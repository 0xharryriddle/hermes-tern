//! Narrow pipe bridge for an operator-bound Hermes physical session.
use crate::credential::Credential;
use crate::{Client, FRAME, Operation, read_secret};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use hermes_tern_store::{MessageInput, PeerIdentity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerBinding {
    pub schema_version: u32,
    pub state_dir: PathBuf,
    pub credential_file: PathBuf,
    pub identity: PeerIdentity,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PeerOperation {
    Status {},
    Send {
        message_id: String,
        recipient: String,
        kind: String,
        text: String,
        ttl_seconds: u64,
        correlation_id: Option<String>,
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
}
impl PeerOperation {
    pub(crate) fn into_operation(self) -> Result<Operation> {
        Ok(match self {
            Self::Status {} => return Err(Error::Protocol("status requires bound identity")),
            Self::Send {
                message_id,
                recipient,
                kind,
                text,
                ttl_seconds,
                correlation_id,
            } => {
                if text.is_empty() || text.len() > 16384 {
                    return Err(Error::Protocol("peer text must be 1..16384 bytes"));
                }
                Operation::Send {
                    message: MessageInput {
                        message_id,
                        recipient,
                        kind,
                        payload: json!({"text":text}),
                        ttl_seconds,
                        correlation_id,
                    },
                }
            }
            Self::Receive {} => Operation::Receive {},
            Self::Ack { sender, message } => Operation::Ack { sender, message },
            Self::Claim { task, ttl_seconds } => Operation::Claim { task, ttl_seconds },
            Self::Complete {
                task,
                generation,
                result,
            } => Operation::Complete {
                task,
                generation,
                result,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    v: u32,
    physical_session_id: String,
    hermes_home: PathBuf,
    op: PeerOperation,
}
pub fn invoke(binding_file: &Path, input: &[u8]) -> Result<Value> {
    if input.len() > FRAME || !binding_file.is_absolute() {
        return Err(Error::Protocol("invalid peer invocation bounds"));
    }
    let call: Invocation = serde_json::from_value(strict_json(input)?)
        .map_err(|_| Error::Protocol("invalid peer invocation"))?;
    let binding: PeerBinding =
        serde_json::from_value(strict_json(read_secret(binding_file)?.as_bytes())?)
            .map_err(|_| Error::Protocol("invalid peer binding"))?;
    let identity = &binding.identity;
    for id in [
        &identity.workspace_id,
        &identity.actor_id,
        &identity.incarnation,
    ] {
        if uuid::Uuid::parse_str(id)
            .ok()
            .is_none_or(|v| v.to_string() != *id)
        {
            return Err(Error::Protocol("noncanonical binding identity"));
        }
    }
    if call.v != 1
        || binding.schema_version != 1
        || call.physical_session_id != identity.physical_session_id
        || call.hermes_home != Path::new(&identity.hermes_home)
        || !call.hermes_home.is_absolute()
        || !call.hermes_home.is_dir()
        || call.hermes_home.canonicalize()? != call.hermes_home
        || !binding.state_dir.is_absolute()
        || binding.state_dir.canonicalize()? != binding.state_dir
        || binding.credential_file
            != binding.state_dir.join("credentials").join(format!(
                "{}-{}.json",
                identity.actor_id, identity.incarnation
            ))
    {
        return Err(Error::Store(
            "peer invocation does not match provisioned binding",
        ));
    }
    let credential = Credential::parse(&read_secret(&binding.credential_file)?)?;
    if credential.workspace_id != identity.workspace_id
        || credential.actor_id != identity.actor_id
        || credential.incarnation != identity.incarnation
    {
        return Err(Error::Store("credential and binding identity differ"));
    }
    crate::private_dir_existing(&binding.state_dir)?;
    let client = Client {
        dir: binding.state_dir,
        credential: credential.credential,
    };
    client.call(Operation::BoundPeer {
        identity: binding.identity,
        operation: call.op,
    })
}

/// One bounded JSON document, terminated by EOF; never block on an open pipe forever.
pub fn run_stdin(binding: &Path) -> Result<Value> {
    let stdin = std::io::stdin();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut raw = Vec::new();
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or(Error::Timeout("peer input"))?;
        let mut fds = [rustix::event::PollFd::new(
            &stdin,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(&mut fds, left.as_millis().min(100) as i32)
            .map_err(|_| Error::Transport("peer input poll failed"))?;
        if fds[0].revents().is_empty() {
            continue;
        }
        let mut bytes = [0; 4096];
        let n = rustix::io::read(&stdin, &mut bytes)
            .map_err(|_| Error::Transport("peer input read failed"))?;
        if n == 0 {
            return invoke(binding, &raw);
        }
        if raw.len() + n > FRAME {
            return Err(Error::Protocol("peer input exceeds limit"));
        }
        raw.extend_from_slice(&bytes[..n]);
    }
}
