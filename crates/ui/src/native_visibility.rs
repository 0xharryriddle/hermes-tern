//! Bounded frame/ACK metadata at the SDK's public Terminal seam.
//!
//! The pinned SDK consumes ACKs before returning inputs. Observe the original
//! input order so an ACK later in one read cannot authorize an earlier key.
//! No raw traffic or text is recorded; SDK still owns handshake and rendering.
use crate::native_input::InputObservation;
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc, time::Duration};
use tern_sdk::{
    keys::Key,
    term::{Terminal, Waker},
};

const MAX_FRAMES: usize = 64;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyVisibility {
    pub drawn: Option<u64>,
    pub positioned: Option<u64>,
}
impl KeyVisibility {
    pub(super) fn intersection(self, other: Self) -> Self {
        Self {
            drawn: self.drawn.filter(|v| other.drawn == Some(*v)),
            positioned: self.positioned.filter(|v| other.positioned == Some(*v)),
        }
    }
}
pub(super) struct Frame {
    pub seq: u64,
    pub visible: KeyVisibility,
}

#[derive(Clone, Default)]
pub struct Visibility(Rc<RefCell<State>>);
#[derive(Default)]
struct State {
    surface: Option<String>,
    render: u64,
    sent: u64,
    emitted: KeyVisibility,
    positioning: Option<u64>,
    frames: VecDeque<Frame>,
    input: InputObservation,
}
impl Visibility {
    pub fn wrap<T: Terminal>(&self, inner: T) -> Observed<T> {
        Observed {
            inner,
            visibility: self.clone(),
        }
    }
    pub fn bind(&self, surface: &str) {
        let mut s = self.0.borrow_mut();
        s.surface = Some(surface.into());
        s.sent = 0;
        s.emitted = KeyVisibility::default();
        s.positioning = None;
        s.frames.clear();
    }
    /// Token belonging to the latest requested view (not a drawn-frame claim).
    pub fn rendering(&self, token: u64) {
        self.0.borrow_mut().render = token;
    }
    pub fn key_frame(&self, key: &Key) -> io::Result<Option<u64>> {
        Ok(self.key_visibility(key)?.drawn)
    }
    pub fn key_visibility(&self, key: &Key) -> io::Result<KeyVisibility> {
        self.0.borrow_mut().input.key(key)
    }
    pub fn settled(&self) -> bool {
        let s = self.0.borrow();
        s.sent != 0 && s.input.acked() == s.sent
    }
    pub fn drawn(&self) -> Option<u64> {
        self.0.borrow().input.visible().drawn
    }
    pub fn positioned(&self) -> Option<u64> {
        let s = self.0.borrow();
        s.input
            .visible()
            .positioned
            .filter(|v| s.emitted.positioned == Some(*v))
    }
    pub fn needs_positioning(&self, token: u64) -> bool {
        self.settled() && self.0.borrow().emitted.positioned != Some(token)
    }
    /// Call only immediately before the bounded raw positioning send. A failed
    /// send terminates the session, never retrying this marker on another frame.
    pub fn positioning(&self, token: u64) -> io::Result<()> {
        if !self.settled() {
            return Err(invalid("native positioning before settled view"));
        }
        let mut s = self.0.borrow_mut();
        if s.positioning.is_some() || s.render != token {
            return Err(invalid("native positioning revision mismatch"));
        }
        s.positioning = Some(token);
        Ok(())
    }
    fn input(&self, bytes: Option<&[u8]>) -> io::Result<()> {
        let mut s = self.0.borrow_mut();
        let State {
            input,
            surface,
            frames,
            ..
        } = &mut *s;
        match bytes {
            Some(bytes) => input.feed(bytes, surface.as_deref(), frames)?,
            None => input.idle(surface.as_deref(), frames)?,
        }
        let acked = input.acked();
        while frames.front().is_some_and(|f| f.seq <= acked) {
            frames.pop_front();
        }
        Ok(())
    }
    fn frame(&self, bytes: &[u8]) -> io::Result<()> {
        // Encoder emits one complete message (possibly several chunks) per
        // Terminal::write. Only frame bodies are materialized transiently.
        if !bytes.starts_with(b"\x1b_tsp;f;") {
            return Ok(());
        }
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(invalid("native frame observation bound"));
        }
        let mut body = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let Some(payload) = rest.strip_prefix(b"\x1b_tsp;f;") else {
                return Err(invalid("native frame encoding changed"));
            };
            let end = payload
                .windows(2)
                .position(|w| w == b"\x1b\\")
                .ok_or_else(|| invalid("incomplete native frame"))?;
            let mut part = &payload[..end];
            // Only pinned Encoder's own chunk headers, not arbitrary peer data.
            while part.starts_with(b"c=") || part.starts_with(b"m=") {
                let semi = part
                    .iter()
                    .position(|b| *b == b';')
                    .ok_or_else(|| invalid("native chunk header"))?;
                part = &part[semi + 1..];
            }
            body.extend_from_slice(part);
            rest = &payload[end + 2..];
        }
        let v: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| invalid("native frame metadata"))?;
        let sf = v["sf"]
            .as_str()
            .ok_or_else(|| invalid("native frame surface"))?;
        let seq = v["s"]
            .as_u64()
            .ok_or_else(|| invalid("native frame sequence"))?;
        let mut s = self.0.borrow_mut();
        if s.surface.as_deref() != Some(sf) {
            return Ok(());
        }
        if seq
            != s.sent
                .checked_add(1)
                .ok_or_else(|| invalid("native frame sequence exhausted"))?
            || s.frames.len() >= MAX_FRAMES
        {
            return Err(invalid("native frame visibility capacity/order"));
        }
        let ops = v["ops"]
            .as_array()
            .ok_or_else(|| invalid("native frame operations"))?;
        // The only layout-neutral publication is our exact ready-ring class.
        // Any other property/operation invalidates the positioning fence.
        let ready_ring_only = !ops.is_empty() && ops.iter().all(|op| {
            op[0] == "set" && op[1].as_str().is_some_and(|id| id.starts_with("main."))
                && op[2].as_object().is_some_and(|p| p.len() == 1
                    && p.get("class").and_then(|v| v.as_str()) == Some("ht-button ht-transcript-toggle ht-keyboard-focus ht-keyboard-ready"))
        });
        let document = !ready_ring_only
            && ops.iter().any(|op| {
                matches!(
                    op[0].as_str(),
                    Some("add" | "set" | "text" | "del" | "move")
                )
            });
        let mut visible = if document {
            KeyVisibility {
                drawn: Some(s.render),
                positioned: None,
            }
        } else {
            s.emitted
        };
        if let Some(token) = s.positioning.take() {
            if document
                || !ops.iter().any(|op| op[0] == "focus")
                || !ops
                    .iter()
                    .all(|op| matches!(op[0].as_str(), Some("focus" | "reveal")))
            {
                return Err(invalid("native positioning frame differs"));
            }
            // A resize may need no document diff. The explicit positioning send
            // is still a new acknowledged layout fence for the current revision.
            visible = KeyVisibility {
                drawn: Some(token),
                positioned: Some(token),
            };
        }
        s.frames.push_back(Frame { seq, visible });
        s.emitted = visible;
        s.sent = seq;
        Ok(())
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub struct Observed<T> {
    inner: T,
    visibility: Visibility,
}
impl<T: Terminal> Terminal for Observed<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        // Failure refuses the native session; never mark a failed write drawn.
        self.inner.write(bytes)?;
        self.visibility.frame(bytes)
    }
    fn read(&mut self, buf: &mut [u8], timeout: Option<Duration>) -> io::Result<Option<usize>> {
        let n = self.inner.read(buf, timeout)?;
        match n {
            Some(n) if n > 0 => {
                self.visibility.input(Some(&buf[..n]))?;
            }
            None => self.visibility.input(None)?,
            _ => {}
        }
        Ok(n)
    }
    fn restore(&mut self) -> io::Result<()> {
        self.inner.restore()
    }
    fn modes(&mut self, paste: bool, kitty: bool) {
        self.inner.modes(paste, kitty);
    }
    fn waker(&self) -> Option<Waker> {
        self.inner.waker()
    }
}
