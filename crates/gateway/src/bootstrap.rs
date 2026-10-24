use crate::config::{AttachOptions, Limits};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use serde::Deserialize;
use std::{
    fmt,
    io::Read,
    net::{IpAddr, SocketAddr},
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use url::{Host, Url};

pub const WS_PROTOCOL: &str = "hermes-gateway-v1";
const TICKET_PREFIX: &str = "hermes-gateway-ticket.";
const HELPER: &str = include_str!("../bridge/hermes_discover.py");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionGrant {
    pub(crate) url: String,
    pub(crate) protocols: Vec<String>,
    pub profile_id: String,
    pub instance_id: String,
}
impl fmt::Debug for ConnectionGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConnectionGrant([redacted])")
    }
}
impl ConnectionGrant {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
            return Err(Error::Bootstrap("oversized grant"));
        }
        let g: Self = serde_json::from_value(strict_json(raw)?)
            .map_err(|_| Error::Bootstrap("invalid grant shape"))?;
        g.address()?;
        if g.protocols.len() != 2
            || g.protocols[0] != WS_PROTOCOL
            || !g.protocols[1].starts_with(TICKET_PREFIX)
            || g.protocols[1].len() <= TICKET_PREFIX.len()
            || g.protocols[1].len() > 4096
            || !g.protocols[1]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || [&g.profile_id, &g.instance_id]
                .iter()
                .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        {
            return Err(Error::Bootstrap("invalid grant identity or protocol"));
        }
        Ok(g)
    }
    pub(crate) fn address(&self) -> Result<SocketAddr> {
        let u = Url::parse(&self.url).map_err(|_| Error::Bootstrap("invalid endpoint"))?;
        if u.scheme() != "ws"
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.path() != "/api/ws"
        {
            return Err(Error::Bootstrap(
                "endpoint must be credential-free loopback ws /api/ws",
            ));
        }
        let ip = match u.host() {
            Some(Host::Ipv4(v)) => IpAddr::V4(v),
            Some(Host::Ipv6(v)) => IpAddr::V6(v),
            _ => return Err(Error::Bootstrap("literal loopback address required")),
        };
        let port = u.port().unwrap_or(80);
        if !ip.is_loopback() || port == 0 {
            return Err(Error::Bootstrap("non-loopback endpoint"));
        }
        Ok(SocketAddr::new(ip, port))
    }
}

pub fn discover(options: &AttachOptions, limits: &Limits) -> Result<ConnectionGrant> {
    limits.validate()?;
    // An anonymous socket pair gives bounded nonblocking reads without a reader
    // thread that could hang forever if a buggy helper leaks its pipe to a child.
    let (mut reader, writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    let fd: OwnedFd = writer.into();
    let mut command = Command::new(&options.python);
    command
        .args(["-I", "-B", "-c", HELPER])
        .arg(&options.hermes_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(fd))
        .stderr(Stdio::null());
    if let Some(home) = &options.home {
        command.arg(home);
    }
    let mut child = command
        .spawn()
        .map_err(|_| Error::Bootstrap("cannot execute selected interpreter"))?;
    // Command retains the configured stdout FD after spawn. Drop our copy so
    // helper exit closes the last writer and the reader can observe EOF.
    drop(command);
    let deadline = Instant::now() + limits.connect_timeout;
    let mut raw = Vec::new();
    let result = (|| {
        loop {
            let mut bytes = [0; 4096];
            match reader.read(&mut bytes) {
                Ok(0) => {
                    if let Some(status) = child.try_wait()? {
                        return if status.success() {
                            ConnectionGrant::parse(&raw)
                        } else {
                            Err(Error::Bootstrap(
                                "discovery_failed; existing owner required",
                            ))
                        };
                    }
                }
                Ok(n) => {
                    if raw.len() + n > 65536 {
                        return Err(Error::Bootstrap("oversized helper output"));
                    }
                    raw.extend_from_slice(&bytes[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout("discovery"));
            }
            thread::sleep(Duration::from_millis(5));
        }
    })();
    // Only this helper is ours. Never signal the gateway owner.
    if child.try_wait()?.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
