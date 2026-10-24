use hermes_tern_core::text::display_text;

#[test]
fn plain_output_cannot_inject_terminal_commands() {
    assert_eq!(
        display_text("ok\u{1b}]52;secret\u{7}\nhi\r!"),
        "ok]52;secret\nhi!"
    );
}

#[test]
fn every_bidi_control_is_removed_but_rtl_letters_are_preserved() {
    let controls = "\u{061c}\u{200e}\u{200f}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}";
    assert_eq!(display_text(&format!("ع{controls}ב\n\t")), "عב\n\t");
}
