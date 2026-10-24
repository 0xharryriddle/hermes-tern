use hermes_tern_ui::composer::Composer;
#[test]
fn edits_unicode_without_invalid_slicing_and_converts_utf16() {
    let mut c = Composer::default();
    c.insert("a😀e\u{301}\n").unwrap();
    c.home();
    c.right();
    c.right();
    assert_eq!(c.utf16_cursor(), 3);
    c.insert("界").unwrap();
    assert_eq!(c.text(), "a😀界e\u{301}\n");
    c.backspace();
    c.delete();
    assert_eq!(c.text(), "a😀\u{301}\n");
    c.home();
    c.backspace();
    assert_eq!(c.utf16_cursor(), 0);
    c.end();
    c.delete();
    assert_eq!(c.take(), "a😀\u{301}\n");
    assert_eq!(c.utf16_cursor(), 0);
}
#[test]
fn bounded_paste_is_atomic_and_strips_terminal_controls() {
    let mut c = Composer::default();
    c.insert("ok").unwrap();
    assert!(c.insert(&"x".repeat(65536)).is_err());
    assert_eq!(c.text(), "ok");
    c.insert("\u{1b}\u{7}").unwrap();
    assert_eq!(c.text(), "ok");
}
