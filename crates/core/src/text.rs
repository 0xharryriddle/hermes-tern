/// Unicode Bidi_Control characters; normal RTL text is not a control.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Remove controls before plain printing or TSP text projection; never emit ESC/OSC.
pub fn display_text(s: &str) -> String {
    s.chars()
        .filter(|c| (!c.is_control() || *c == '\n' || *c == '\t') && !is_bidi_control(*c))
        .collect()
}
