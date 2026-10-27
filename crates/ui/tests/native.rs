use hermes_tern_ui::{
    app::{NativeInput, drain_native_inbox, edit_key, next_native, next_native_for_surface},
    composer::Composer,
    view::UiAction,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc, time::Duration};
use tern_sdk::{
    Options, Session, SurfaceOptions,
    keys::Key,
    term::{Terminal, Waker},
};
struct Script {
    chunks: Rc<RefCell<VecDeque<Vec<u8>>>>,
    closed: Rc<RefCell<bool>>,
}
impl Terminal for Script {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.windows(7).any(|w| w == b"tsp;q;{") {
            self.chunks.borrow_mut().push_back(b"\x1b_tsp;r;{\"r\":\"hello\",\"v\":1,\"term\":\"tern\",\"kinds\":[\"col\",\"text\",\"editor\",\"el\"],\"features\":[\"flow\",\"send\"],\"credits\":1}\x1b\\\x1b[?62;52;c".to_vec());
        }
        Ok(())
    }
    fn read(&mut self, buf: &mut [u8], _: Option<Duration>) -> io::Result<Option<usize>> {
        if let Some(chunk) = self.chunks.borrow_mut().pop_front() {
            assert!(chunk.len() <= buf.len());
            buf[..chunk.len()].copy_from_slice(&chunk);
            Ok(Some(chunk.len()))
        } else if *self.closed.borrow() {
            Ok(Some(0))
        } else {
            Ok(None)
        }
    }
}
#[test]
fn native_timeout_is_not_eof_and_closed_input_is() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let closed = Rc::new(RefCell::new(false));
    let mut session: Session<UiAction> = Session::with_terminal(
        Script {
            chunks,
            closed: closed.clone(),
        },
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        next_native(&mut session).unwrap(),
        NativeInput::Wake
    ));
    *closed.borrow_mut() = true;
    assert!(matches!(
        next_native(&mut session).unwrap(),
        NativeInput::Closed
    ));
}
#[test]
fn keys_edit_draft_but_interrupt_never_sends_it() {
    let mut composer = Composer::default();
    composer.insert("keep me").unwrap();
    let key = Key {
        name: "c".into(),
        text: None,
        ctrl: true,
        alt: false,
        shift: false,
        meta: false,
    };
    assert!(matches!(
        edit_key(&mut composer, &key).unwrap(),
        Some(UiAction::Interrupt)
    ));
    assert_eq!(composer.text(), "keep me");
}

struct WakeScript {
    inner: Script,
    waker: Waker,
    reads: Rc<RefCell<usize>>,
}
impl Terminal for WakeScript {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(bytes)
    }
    fn read(&mut self, buf: &mut [u8], timeout: Option<Duration>) -> io::Result<Option<usize>> {
        *self.reads.borrow_mut() += 1;
        self.inner.read(buf, timeout)
    }
    fn waker(&self) -> Option<Waker> {
        Some(self.waker.clone())
    }
}
fn event(value: Value) -> Vec<u8> {
    format!("\x1b_tsp;e;{value}\x1b\\").into_bytes()
}

#[test]
fn selection_barrier_preserves_decoded_errors_and_leaves_later_input_unread() {
    for scope in ["current", "global", "other"] {
        let chunks = Rc::new(RefCell::new(VecDeque::new()));
        let reads = Rc::new(RefCell::new(0));
        let mut session: Session<UiAction> = Session::with_terminal(
            WakeScript {
                inner: Script {
                    chunks: chunks.clone(),
                    closed: Rc::new(RefCell::new(false)),
                },
                waker: Waker::new().unwrap(),
                reads: reads.clone(),
            },
            Options::new().record(None),
        )
        .unwrap()
        .unwrap();
        let sf = session.open(SurfaceOptions::inline()).unwrap();
        let mut error = json!({"ev":"error","msg":"UNTRUSTED-SELECTION-ERROR"});
        if scope != "global" {
            error["sf"] = json!(if scope == "current" {
                session.surface_id(sf)
            } else {
                "other"
            });
        }
        let mut first = b"\r".to_vec();
        first.extend(event(error));
        chunks.borrow_mut().push_back(first);
        chunks
            .borrow_mut()
            .push_back(event(json!({"ev":"error","msg":"LATER-READ"})));
        assert!(matches!(
            next_native(&mut session).unwrap(),
            NativeInput::Input(_)
        ));
        let before = *reads.borrow();
        let result = drain_native_inbox(&mut session, sf);
        if scope == "other" {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            *reads.borrow(),
            before,
            "selection barrier performed a new read"
        );
        assert_eq!(chunks.borrow().len(), 1);
        assert!(session.is_open(sf));
    }
}

#[test]
fn closed_surface_keeps_queued_rejections_without_reading_more_input() {
    for scope in ["current", "global", "other", "none"] {
        for prefix in [false, true] {
            for error_after_gone in [false, true] {
                let chunks = Rc::new(RefCell::new(VecDeque::new()));
                let reads = Rc::new(RefCell::new(0));
                let mut session: Session<UiAction> = Session::with_terminal(
                    WakeScript {
                        inner: Script {
                            chunks: chunks.clone(),
                            closed: Rc::new(RefCell::new(false)),
                        },
                        waker: Waker::new().unwrap(),
                        reads: reads.clone(),
                    },
                    Options::new().record(None),
                )
                .unwrap()
                .unwrap();
                let sf = session.open(SurfaceOptions::inline()).unwrap();
                let id = session.surface_id(sf).to_owned();
                let mut error = json!({"ev":"error","msg":"UNTRUSTED-DETAIL"});
                match scope {
                    "current" => error["sf"] = json!(id),
                    "other" => error["sf"] = json!("other"),
                    _ => {}
                }
                let gone = json!({"ev":"gone","sf":id,"ids":[id]});
                let mut events = vec![];
                if prefix {
                    events.push(json!({"ev":"theme","dark":true}));
                    // A decoded key must not escape the adapter after closure.
                }
                if error_after_gone {
                    events.push(gone.clone());
                }
                if scope != "none" {
                    events.push(error);
                }
                if !error_after_gone {
                    events.push(gone);
                }
                let mut bytes = if prefix { b"\r".to_vec() } else { vec![] };
                bytes.extend(events.into_iter().flat_map(event));
                chunks.borrow_mut().push_back(bytes);
                // Must remain unread: draining only the decoded inbox is finite.
                chunks
                    .borrow_mut()
                    .push_back(event(json!({"ev":"error","msg":"LATER-READ"})));
                let before = *reads.borrow();
                let result = next_native_for_surface(&mut session, sf);
                if matches!(scope, "current" | "global") {
                    let Err(error) = result else {
                        panic!("queued rejection lost: {scope} {prefix} {error_after_gone}");
                    };
                    assert!(
                        error
                            .to_string()
                            .contains("native terminal rejected output")
                    );
                    assert!(!error.to_string().contains("UNTRUSTED"));
                } else {
                    assert!(matches!(result.unwrap(), NativeInput::Closed));
                }
                assert!(!session.is_open(sf));
                assert_eq!(
                    *reads.borrow() - before,
                    1,
                    "shutdown read beyond decoded inbox"
                );
                assert_eq!(chunks.borrow().len(), 1);
            }
        }
    }
}

#[test]
fn closed_surface_without_a_waker_refuses_unbounded_drain() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let mut session: Session<UiAction> = Session::with_terminal(
        Script {
            chunks: chunks.clone(),
            closed: Rc::new(RefCell::new(false)),
        },
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline()).unwrap();
    let id = session.surface_id(sf).to_owned();
    chunks
        .borrow_mut()
        .push_back(event(json!({"ev":"gone","sf":id,"ids":[id]})));
    let Err(error) = next_native_for_surface(&mut session, sf) else {
        panic!("unbounded drain accepted");
    };
    assert!(
        error
            .to_string()
            .contains("native terminal shutdown requires a waker")
    );
}

#[test]
fn oversized_closed_surface_inbox_fails_closed_without_another_read() {
    let chunks = Rc::new(RefCell::new(VecDeque::new()));
    let reads = Rc::new(RefCell::new(0));
    let mut session: Session<UiAction> = Session::with_terminal(
        WakeScript {
            inner: Script {
                chunks: chunks.clone(),
                closed: Rc::new(RefCell::new(false)),
            },
            waker: Waker::new().unwrap(),
            reads: reads.clone(),
        },
        Options::new().record(None),
    )
    .unwrap()
    .unwrap();
    let sf = session.open(SurfaceOptions::inline()).unwrap();
    let id = session.surface_id(sf).to_owned();
    // pump() can accumulate more than one SDK read. Do not assume the inbox
    // contains only the final read's inputs when deciding shutdown is clean.
    chunks.borrow_mut().extend([
        vec![b'x'; 8192],
        vec![b'y'; 8192],
        event(json!({"ev":"gone","sf":id,"ids":[id]})),
    ]);
    session.pump(Duration::ZERO).unwrap();
    let before = *reads.borrow();
    let Err(error) = next_native_for_surface(&mut session, sf) else {
        panic!("oversized closed-surface inbox reported clean");
    };
    assert!(
        error
            .to_string()
            .contains("native terminal shutdown queue limit exceeded")
    );
    assert_eq!(*reads.borrow(), before);
}
