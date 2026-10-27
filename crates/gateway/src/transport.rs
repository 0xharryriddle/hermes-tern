//! One socket owner with bounded queues. A transport fault never retries a request.
use crate::{
    bootstrap::{ConnectionGrant, WS_PROTOCOL},
    config::Limits,
    protocol::{Inbound, parse_frame},
};
use hermes_tern_core::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    net::{Shutdown, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tungstenite::{
    Message, WebSocket,
    client::{IntoClientRequest, client_with_config},
    protocol::WebSocketConfig,
};

const QUEUE_BYTES: usize = 16 * 1024 * 1024;
type Wake = Arc<dyn Fn() + Send + Sync>;
struct Shared {
    stop: AtomicBool,
    incoming_bytes: AtomicUsize,
    outgoing_bytes: AtomicUsize,
    error: Mutex<Option<Error>>,
    wake: Mutex<Option<Wake>>,
}
impl Shared {
    fn notify(&self) {
        if let Ok(w) = self.wake.lock()
            && let Some(w) = w.as_ref()
        {
            w();
        }
    }
}
struct Command {
    text: String,
    request: Option<u64>,
    server_response: Option<String>,
}
pub struct RpcClient {
    tx: SyncSender<Command>,
    rx: Receiver<(Inbound, usize)>,
    shared: Arc<Shared>,
    shutdown: TcpStream,
    worker: Option<JoinHandle<()>>,
    next_id: u64,
    limits: Limits,
    pub profile_id: String,
    pub instance_id: String,
}
impl RpcClient {
    pub fn connect(grant: ConnectionGrant, limits: Limits) -> Result<Self> {
        limits.validate()?;
        let stream = TcpStream::connect_timeout(&grant.address()?, limits.connect_timeout)
            .map_err(|_| Error::Transport("loopback connect failed"))?;
        stream.set_read_timeout(Some(limits.connect_timeout))?;
        stream.set_write_timeout(Some(limits.connect_timeout))?;
        stream.set_nodelay(true)?;
        let mut request = grant
            .url
            .as_str()
            .into_client_request()
            .map_err(|_| Error::Transport("invalid upgrade request"))?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            grant
                .protocols
                .join(", ")
                .parse()
                .map_err(|_| Error::Transport("invalid protocol header"))?,
        );
        let config = WebSocketConfig::default()
            .read_buffer_size(8192)
            .write_buffer_size(0)
            .max_write_buffer_size(limits.max_frame_bytes + 1024)
            .max_message_size(Some(limits.max_frame_bytes))
            .max_frame_size(Some(limits.max_frame_bytes));
        let (socket, response) = client_with_config(request, stream, Some(config))
            .map_err(|_| Error::Transport("WebSocket upgrade failed"))?;
        if response
            .headers()
            .get("Sec-WebSocket-Protocol")
            .and_then(|v| v.to_str().ok())
            != Some(WS_PROTOCOL)
        {
            return Err(Error::Protocol("gateway subprotocol not echoed"));
        }
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))?;
        socket
            .get_ref()
            .set_write_timeout(Some(limits.request_timeout))?;
        let shutdown = socket.get_ref().try_clone()?;
        let (tx, commands) = mpsc::sync_channel(limits.max_pending_requests);
        let (events, rx) = mpsc::sync_channel(limits.max_events);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            incoming_bytes: AtomicUsize::new(0),
            outgoing_bytes: AtomicUsize::new(0),
            error: Mutex::new(None),
            wake: Mutex::new(None),
        });
        let state = shared.clone();
        let worker_limits = limits.clone();
        let worker = thread::spawn(move || {
            if let Err(error) = run(socket, commands, events, &state, &worker_limits)
                && let Ok(mut slot) = state.error.lock()
            {
                *slot = Some(error);
            }
            state.stop.store(true, Ordering::Release);
            state.notify();
        });
        Ok(Self {
            tx,
            rx,
            shared,
            shutdown,
            worker: Some(worker),
            next_id: 1,
            limits,
            profile_id: grant.profile_id,
            instance_id: grant.instance_id,
        })
    }
    pub fn set_waker(&mut self, wake: Wake) {
        if let Ok(mut slot) = self.shared.wake.lock() {
            *slot = Some(wake);
        }
    }
    pub fn request(&mut self, method: &str, params: Value) -> Result<u64> {
        if !matches!(
            method,
            "ping"
                | "runtime.describe"
                | "profiles.list"
                | "commands.catalog"
                | "tools.show"
                | "plugins.list"
                | "subagent.list"
                | "file.attach"
                | "reload.mcp"
                | "command.dispatch"
                | "client.capabilities"
                | "session.create"
                | "session.list"
                | "session.resume"
                | "session.info"
                | "model.options"
                | "session.mutate"
                | "session.history"
                | "session.interrupt"
                | "session.detach"
                | "prompt.submit"
                | "prompt.receipt"
                | "approval.respond"
                | "clarify.respond"
        ) {
            return Err(Error::Unsupported("RPC method"));
        }
        if !params.is_object() {
            return Err(Error::Protocol("request params must be object"));
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or(Error::Protocol("request id exhausted"))?;
        self.enqueue(Command {
            text: json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string(),
            request: Some(id),
            server_response: None,
        })?;
        Ok(id)
    }
    pub fn respond_server(&mut self, id: &str, result: Value) -> Result<()> {
        self.server_reply(id, json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    pub fn decline_server(&mut self, id: &str) -> Result<()> {
        self.server_reply(id, json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unsupported_request"}}))
    }
    fn server_reply(&mut self, id: &str, value: Value) -> Result<()> {
        if !id.starts_with("srq-") || id.len() > 160 {
            return Err(Error::Protocol("invalid server request id"));
        }
        self.enqueue(Command {
            text: value.to_string(),
            request: None,
            server_response: Some(id.to_owned()),
        })
    }
    fn enqueue(&self, command: Command) -> Result<()> {
        if self.shared.stop.load(Ordering::Acquire) {
            return Err(Error::Disconnected);
        }
        let size = command.text.len();
        if size > self.limits.max_frame_bytes {
            return Err(Error::Protocol("outgoing frame exceeds limit"));
        }
        reserve(&self.shared.outgoing_bytes, size)?;
        if self.tx.try_send(command).is_err() {
            self.shared.outgoing_bytes.fetch_sub(size, Ordering::AcqRel);
            return Err(Error::Transport(
                "outgoing queue full or disconnected; request not accepted",
            ));
        }
        Ok(())
    }
    pub fn try_recv(&mut self) -> Result<Option<Inbound>> {
        match self.rx.try_recv() {
            Ok((event, size)) => {
                self.shared.incoming_bytes.fetch_sub(size, Ordering::AcqRel);
                Ok(Some(event))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(self.failure()),
        }
    }
    pub fn recv(&mut self, timeout: Duration) -> Result<Inbound> {
        match self.rx.recv_timeout(timeout) {
            Ok((event, size)) => {
                self.shared.incoming_bytes.fetch_sub(size, Ordering::AcqRel);
                Ok(event)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout("receive")),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.failure()),
        }
    }
    fn failure(&self) -> Error {
        self.shared
            .error
            .lock()
            .ok()
            .and_then(|mut v| v.take())
            .unwrap_or(Error::Disconnected)
    }
    pub fn close(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        let _ = self.shutdown.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for RpcClient {
    fn drop(&mut self) {
        self.close();
    }
}
fn reserve(counter: &AtomicUsize, size: usize) -> Result<()> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            n.checked_add(size).filter(|n| *n <= QUEUE_BYTES)
        })
        .map(|_| ())
        .map_err(|_| Error::Transport("queue byte budget exceeded"))
}
fn run(
    mut socket: WebSocket<TcpStream>,
    commands: Receiver<Command>,
    events: SyncSender<(Inbound, usize)>,
    shared: &Shared,
    limits: &Limits,
) -> Result<()> {
    let mut pending = HashMap::new();
    let mut server_requests = HashSet::new();
    while !shared.stop.load(Ordering::Acquire) {
        // Fairness: bounded command batch, then service incoming events/decisions.
        for _ in 0..8 {
            let command = match commands.try_recv() {
                Ok(c) => c,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            };
            shared
                .outgoing_bytes
                .fetch_sub(command.text.len(), Ordering::AcqRel);
            if let Some(id) = command.request {
                if pending.len() >= limits.max_pending_requests {
                    return Err(Error::Transport("pending request limit"));
                }
                pending.insert(id, Instant::now() + limits.request_timeout);
            }
            if let Some(id) = command.server_response
                && !server_requests.remove(&id)
            {
                return Err(Error::Protocol(
                    "unknown or already answered server request",
                ));
            }
            socket
                .send(Message::Text(command.text.into()))
                .map_err(|_| Error::Disconnected)?;
        }
        if pending.values().any(|deadline| Instant::now() >= *deadline) {
            return Err(Error::Timeout("request"));
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let event = parse_frame(&text)?;
                match &event {
                    Inbound::Response { id, .. } if pending.remove(id).is_none() => {
                        return Err(Error::Protocol("uncorrelated response"));
                    }
                    Inbound::ServerRequest { id, .. }
                        if server_requests.len() >= limits.max_pending_requests
                            || !server_requests.insert(id.clone()) =>
                    {
                        return Err(Error::Protocol("duplicate or excessive server requests"));
                    }
                    _ => {}
                }
                let size = text.len();
                reserve(&shared.incoming_bytes, size)?;
                if events.try_send((event, size)).is_err() {
                    shared.incoming_bytes.fetch_sub(size, Ordering::AcqRel);
                    return Err(Error::Transport(
                        "consumer stalled; reconnect required, no events silently dropped",
                    ));
                }
                shared.notify();
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                socket.flush().map_err(|_| Error::Disconnected)?;
            }
            Ok(Message::Close(_)) => return Err(Error::Disconnected),
            Ok(_) => return Err(Error::Protocol("binary or unexpected frame")),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err(Error::Transport("invalid frame or connection closed")),
        }
    }
    Ok(())
}
