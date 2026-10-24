use hermes_tern_ui::native_visibility::Visibility;
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc, time::Duration};
use tern_sdk::{Input, Options, Session, SurfaceOptions, term::Terminal, ui};

struct Script {
    chunks: Rc<RefCell<VecDeque<Vec<u8>>>>,
}
impl Terminal for Script {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.starts_with(b"\x1b_tsp;q;") {
            self.chunks.borrow_mut().push_back(b"\x1b_tsp;r;{\"r\":\"hello\",\"v\":1,\"credits\":1,\"apc\":64,\"kinds\":[\"col\",\"text\"],\"features\":[]}\x1b\\".to_vec());
        }
        Ok(())
    }
    fn read(&mut self, buf: &mut [u8], _: Option<Duration>) -> io::Result<Option<usize>> {
        let Some(mut chunk) = self.chunks.borrow_mut().pop_front() else {
            return Ok(None);
        };
        if chunk.len() > buf.len() {
            let rest = chunk.split_off(buf.len());
            self.chunks.borrow_mut().push_front(rest);
        }
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok(Some(chunk.len()))
    }
}
fn ack(sf: &str, s: u64) -> Vec<u8> {
    format!("\x1b_tsp;e;{{\"ev\":\"ack\",\"sf\":\"{sf}\",\"s\":{s}}}\x1b\\").into_bytes()
}
fn key(session: &mut Session<()>, seen: &Visibility) -> Option<u64> {
    let Some(Input::Key(k)) = session.next(Some(Duration::ZERO)).unwrap() else {
        panic!("no key");
    };
    seen.key_frame(&k).unwrap()
}

#[test]
fn visibility_observes_drawn_frames_not_requested_views_or_later_acks() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(1);
    session.render(sf, ui::text("first".repeat(40))).unwrap(); // chunked, still one frame
    seen.rendering(2);
    session.render(sf, ui::text("second".repeat(40))).unwrap(); // credit-blocked
    let mut input = b"\r".to_vec();
    input.extend(ack("s", 1));
    input.extend(b"\r");
    chunks.borrow_mut().push_back(input);
    assert_eq!(
        key(&mut session, &seen),
        None,
        "later ACK made an earlier key visible"
    );
    assert_eq!(
        key(&mut session, &seen),
        Some(1),
        "requested view was confused with drawn view"
    );
    chunks.borrow_mut().push_back(ack("s", 2));
    session.next(Some(Duration::ZERO)).unwrap();
    chunks.borrow_mut().push_back(b"\r".to_vec());
    assert_eq!(key(&mut session, &seen), Some(2));
    assert!(seen.settled());
}

#[test]
fn visibility_refuses_foreign_future_and_pasted_acks_and_preserves_key_order() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(7);
    session.render(sf, ui::text("current")).unwrap();
    let mut bytes = ack("other", 1);
    bytes.extend(ack("s", 999));
    bytes.extend(b"\x1b[200~");
    bytes.extend(ack("s", 1));
    bytes.extend(b"\x1b[201~\r");
    chunks.borrow_mut().push_back(bytes);
    assert_eq!(key(&mut session, &seen), None); // paste is data, never an ACK
    assert_eq!(key(&mut session, &seen), None);
    let valid = ack("s", 1);
    chunks.borrow_mut().extend([
        valid[..9].to_vec(),
        valid[9..].to_vec(),
        b"\x1b[Z\x1b[32u".to_vec(),
    ]);
    assert_eq!(key(&mut session, &seen), Some(7));
    assert_eq!(key(&mut session, &seen), Some(7));
}

#[test]
fn visibility_bounds_unconsumed_key_metadata() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session: Session<()> = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    chunks
        .borrow_mut()
        .extend([vec![b'x'; 8192], b"y".to_vec()]);
    assert!(
        session.pump(Duration::ZERO).is_err(),
        "unbounded key observation queue accepted"
    );
}

#[test]
fn fragmented_keys_and_escape_flush_keep_sdk_decode_in_lockstep() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(1);
    session.render(sf, ui::text("one")).unwrap();
    chunks.borrow_mut().push_back(ack("s", 1));
    session.next(Some(Duration::ZERO)).unwrap();
    for bytes in [
        b"\x1b[13u".to_vec(),
        "😀".as_bytes().to_vec(),
        b"\x1b[200~paste\x1b[201~".to_vec(),
    ] {
        chunks
            .borrow_mut()
            .extend(bytes.into_iter().map(|b| vec![b]));
        assert_eq!(key(&mut session, &seen), Some(1));
    }
    chunks.borrow_mut().push_back(vec![0x1b]);
    let Some(Input::Key(k)) = session.next(Some(Duration::from_millis(100))).unwrap() else {
        panic!("escape not flushed")
    };
    assert!(k.is("escape"));
    assert_eq!(seen.key_frame(&k).unwrap(), Some(1));
}

// Force the observer's old timestamp to precede the SDK's by more than its
// flush interval. This is a test transport, not a change to production clocks.
struct DelayAfterRead<T> {
    inner: T,
}
impl<T: Terminal> Terminal for DelayAfterRead<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(bytes)
    }
    fn read(&mut self, buf: &mut [u8], timeout: Option<Duration>) -> io::Result<Option<usize>> {
        let n = self.inner.read(buf, timeout)?;
        if n == Some(1) && buf[0] == 0x1b {
            std::thread::sleep(Duration::from_millis(45));
        }
        Ok(n)
    }
}

#[test]
fn sdk_is_the_only_flush_authority_even_when_observer_and_sdk_timestamps_diverge() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session: Session<()> = Session::with_terminal(
        DelayAfterRead {
            inner: seen.wrap(Script {
                chunks: chunks.clone(),
            }),
        },
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(1);
    session.render(sf, ui::text("view")).unwrap();
    chunks.borrow_mut().push_back(ack("s", 1));
    session.next(Some(Duration::ZERO)).unwrap();
    chunks.borrow_mut().push_back(vec![0x1b]);
    assert!(session.next(Some(Duration::ZERO)).unwrap().is_none());
    chunks.borrow_mut().push_back(b"[Z".to_vec());
    let Some(Input::Key(k)) = session.next(Some(Duration::ZERO)).unwrap() else {
        panic!("no key")
    };
    assert!(k.is("shift+tab"));
    assert_eq!(seen.key_frame(&k).unwrap(), Some(1));
}

#[test]
fn positioning_ack_is_ordered_and_raw_focus_does_not_promote_requested_view() {
    use tern_sdk::wire::{Op, RevealAt};
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session: Session<()> = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(1);
    session.render(sf, ui::text("one")).unwrap();
    assert!(seen.positioning(1).is_err());
    chunks.borrow_mut().push_back(ack("s", 1));
    session.next(Some(Duration::ZERO)).unwrap();
    seen.rendering(2); // no document 2 was rendered; raw focus must not label it 2
    session.focus(sf, None).unwrap();
    chunks.borrow_mut().push_back(ack("s", 2));
    session.next(Some(Duration::ZERO)).unwrap();
    assert_eq!(seen.drawn(), Some(1));
    seen.rendering(1);
    seen.positioning(1).unwrap();
    session
        .send(
            sf,
            vec![
                Op::Focus { id: None },
                Op::Reveal {
                    id: "main".into(),
                    at: RevealAt::Nearest,
                },
            ],
        )
        .unwrap();
    let mut bytes = b"\r".to_vec();
    bytes.extend(ack("s", 3));
    bytes.extend(b"\r");
    chunks.borrow_mut().push_back(bytes);
    let Some(Input::Key(k)) = session.next(Some(Duration::ZERO)).unwrap() else {
        panic!("key")
    };
    assert_eq!(seen.key_visibility(&k).unwrap().positioned, None);
    let Some(Input::Key(k)) = session.next(Some(Duration::ZERO)).unwrap() else {
        panic!("key")
    };
    assert_eq!(seen.key_visibility(&k).unwrap().positioned, Some(1));
    seen.rendering(2);
    session.render(sf, ui::text("two")).unwrap();
    chunks.borrow_mut().push_back(ack("s", 4));
    session.next(Some(Duration::ZERO)).unwrap();
    assert!(seen.needs_positioning(2));
}

#[test]
fn sdk_flush_decision_preserves_ack_and_fragment_order_across_many_inputs() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session: Session<()> = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(1);
    session.render(sf, ui::text("current")).unwrap();
    // Lone ESC must keep its pre-ACK visibility even if flushed after the ACK.
    chunks.borrow_mut().extend([vec![0x1b], ack("s", 1)]);
    let Some(Input::Key(k)) = session.next(Some(Duration::from_millis(100))).unwrap() else {
        panic!("escape")
    };
    assert!(k.is("escape"));
    assert_eq!(seen.key_frame(&k).unwrap(), None);
    for i in 0..24 {
        chunks.borrow_mut().push_back(vec![0x1b]);
        assert!(session.next(Some(Duration::ZERO)).unwrap().is_none());
        if i % 2 == 0 {
            chunks.borrow_mut().push_back(b"[Z".to_vec());
        }
        let Some(Input::Key(k)) = session.next(Some(Duration::from_millis(100))).unwrap() else {
            panic!("key")
        };
        assert!(k.is(if i % 2 == 0 { "shift+tab" } else { "escape" }));
        assert_eq!(seen.key_frame(&k).unwrap(), Some(1));
    }
}

#[test]
fn only_exact_ready_ring_publication_preserves_positioning() {
    use tern_sdk::wire::Op;
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session: Session<()> = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline().id("s")).unwrap();
    seen.bind("s");
    seen.rendering(1);
    let base = "ht-button ht-transcript-toggle ht-keyboard-focus";
    let ready = "ht-button ht-transcript-toggle ht-keyboard-focus ht-keyboard-ready";
    session
        .render(
            sf,
            ui::col().child(ui::html::button("Show").key("control").class(base)),
        )
        .unwrap();
    chunks.borrow_mut().push_back(ack("s", 1));
    session.next(Some(Duration::ZERO)).unwrap();
    seen.positioning(1).unwrap();
    session.send(sf, vec![Op::Focus { id: None }]).unwrap();
    chunks.borrow_mut().push_back(ack("s", 2));
    session.next(Some(Duration::ZERO)).unwrap();
    assert_eq!(seen.positioned(), Some(1));
    session
        .render(
            sf,
            ui::col().child(ui::html::button("Show").key("control").class(ready)),
        )
        .unwrap();
    chunks.borrow_mut().push_back(ack("s", 3));
    session.next(Some(Duration::ZERO)).unwrap();
    assert_eq!(seen.positioned(), Some(1));
    assert!(
        !seen.needs_positioning(1),
        "ready publication caused reveal loop"
    );
    session
        .render(
            sf,
            ui::col().child(
                ui::html::button("different geometry")
                    .key("control")
                    .class(ready),
            ),
        )
        .unwrap();
    assert_eq!(
        seen.positioned(),
        None,
        "unacknowledged geometry change retained ready state"
    );
    chunks.borrow_mut().push_back(ack("s", 4));
    session.next(Some(Duration::ZERO)).unwrap();
    assert_eq!(seen.positioned(), None);
    assert!(seen.needs_positioning(1));
}

#[test]
fn unfinished_paste_is_bounded_before_sdk_accumulates_unbounded_text() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let seen = Visibility::default();
    let mut session: Session<()> = Session::with_terminal(
        seen.wrap(Script {
            chunks: chunks.clone(),
        }),
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    chunks.borrow_mut().push_back(b"\x1b[200~".to_vec());
    chunks
        .borrow_mut()
        .extend((0..20).map(|_| vec![b'x'; 8192]));
    assert!(
        session.pump(Duration::ZERO).is_err(),
        "unfinished paste observation was not bounded"
    );
}
