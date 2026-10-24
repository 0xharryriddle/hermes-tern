//! Bounded observation of the pinned SDK's possible timeout decisions.
//!
//! Terminal::read returns before Session sets its private last-input clock.
//! Therefore this observer must not decide when the SDK flushes an escape.
//! Keep both possibilities at an idle boundary, then retain only candidates
//! agreeing with the SDK's returned keys. Ambiguous visibility never grants.
use super::native_visibility::{Frame, KeyVisibility};
use std::{collections::VecDeque, io};
use tern_sdk::{
    input::{Item, Parser},
    keys::{Decoder, Key},
    wire::Event,
};

const MAX_KEYS: usize = 8192;
const MAX_BRANCHES: usize = 16;
const MAX_UNDECIDED: usize = 128 * 1024;

#[derive(Clone, Default)]
struct Branch {
    parser: Parser,
    decoder: Decoder,
    keys: VecDeque<(String, u8, KeyVisibility)>,
    held: KeyVisibility,
    visible: KeyVisibility,
    acked: u64,
    undecided: usize,
    key_bytes: usize,
}
impl Branch {
    fn items(
        &mut self,
        items: Vec<Item>,
        surface: Option<&str>,
        frames: &VecDeque<Frame>,
    ) -> io::Result<()> {
        for item in items {
            match item {
                Item::Event(Event::Ack(ack))
                    if surface == Some(ack.sf.as_str()) && ack.s > self.acked =>
                {
                    if let Some(frame) = frames.iter().find(|f| f.seq == ack.s) {
                        self.visible = frame.visible;
                        self.acked = ack.s;
                    }
                }
                Item::Keys(bytes) => {
                    let stamp = if self.decoder.pending() {
                        self.held
                    } else {
                        self.visible
                    };
                    self.key_bytes = self.key_bytes.saturating_add(bytes.len());
                    if self.key_bytes > MAX_UNDECIDED {
                        return Err(invalid("native partial key capacity"));
                    }
                    let keys = self.decoder.feed(&bytes);
                    if !keys.is_empty() {
                        self.key_bytes = 0;
                    }
                    self.enqueue(keys, stamp)?;
                    self.held = stamp;
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn enqueue(&mut self, keys: Vec<Key>, stamp: KeyVisibility) -> io::Result<()> {
        if self.keys.len().saturating_add(keys.len()) > MAX_KEYS {
            return Err(invalid("native key visibility capacity"));
        }
        for key in keys {
            self.keys
                .push_back((key.name.clone(), modifiers(&key), stamp));
        }
        Ok(())
    }
    fn feed(
        &mut self,
        bytes: &[u8],
        surface: Option<&str>,
        frames: &VecDeque<Frame>,
    ) -> io::Result<()> {
        self.undecided = self.undecided.saturating_add(bytes.len());
        if self.undecided > MAX_UNDECIDED {
            return Err(invalid("native input observation capacity"));
        }
        let items = self.parser.feed(bytes);
        if !items.is_empty() {
            self.undecided = 0;
        }
        self.items(items, surface, frames)
    }
    fn flush(&mut self, surface: Option<&str>, frames: &VecDeque<Frame>) -> io::Result<()> {
        let items = self.parser.flush();
        self.items(items, surface, frames)?;
        let keys = self.decoder.flush();
        self.enqueue(keys, self.held)?;
        self.undecided = 0;
        self.key_bytes = 0;
        Ok(())
    }
}

pub(super) struct InputObservation {
    branches: Vec<Branch>,
    idle: bool,
}
impl Default for InputObservation {
    fn default() -> Self {
        Self {
            branches: vec![Branch::default()],
            idle: false,
        }
    }
}
impl InputObservation {
    pub fn feed(
        &mut self,
        bytes: &[u8],
        surface: Option<&str>,
        frames: &VecDeque<Frame>,
    ) -> io::Result<()> {
        self.idle = false;
        for branch in &mut self.branches {
            branch.feed(bytes, surface, frames)?;
        }
        Ok(())
    }
    pub fn idle(&mut self, surface: Option<&str>, frames: &VecDeque<Frame>) -> io::Result<()> {
        if self.idle {
            return Ok(());
        }
        self.idle = true;
        let mut flushed = Vec::new();
        for branch in &self.branches {
            if branch.parser.pending() || branch.decoder.pending() {
                let mut next = branch.clone();
                next.flush(surface, frames)?;
                flushed.push(next);
            }
        }
        if self.branches.len() + flushed.len() > MAX_BRANCHES {
            return Err(invalid("native input ambiguity capacity"));
        }
        self.branches.extend(flushed);
        Ok(())
    }
    pub fn key(&mut self, key: &Key) -> io::Result<KeyVisibility> {
        self.branches.retain(|b| {
            b.keys
                .front()
                .is_some_and(|(name, mods, _)| name == &key.name && *mods == modifiers(key))
        });
        if self.branches.is_empty() {
            return Err(invalid("native key visibility mismatch"));
        }
        let mut stamps = self
            .branches
            .iter_mut()
            .map(|b| b.keys.pop_front().expect("matched key").2);
        let first = stamps.next().expect("nonempty branches");
        Ok(stamps.fold(first, KeyVisibility::intersection))
    }
    pub fn visible(&self) -> KeyVisibility {
        self.branches
            .iter()
            .map(|b| b.visible)
            .reduce(KeyVisibility::intersection)
            .unwrap_or_default()
    }
    pub fn acked(&self) -> u64 {
        self.branches.iter().map(|b| b.acked).min().unwrap_or(0)
    }
}
fn modifiers(key: &Key) -> u8 {
    u8::from(key.ctrl)
        | (u8::from(key.alt) << 1)
        | (u8::from(key.shift) << 2)
        | (u8::from(key.meta) << 3)
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
