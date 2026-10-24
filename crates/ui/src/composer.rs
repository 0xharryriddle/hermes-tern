use hermes_tern_core::{
    error::{Error, Result},
    text::display_text,
};
#[derive(Default, Debug, Clone)]
pub struct Composer {
    text: String,
    cursor: usize,
}
impl Composer {
    pub fn text(&self) -> &str {
        &self.text
    }
    fn byte(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(i, _)| i)
    }
    pub fn insert(&mut self, text: &str) -> Result<()> {
        let text = display_text(text);
        if self.text.len().saturating_add(text.len()) > 65536 {
            return Err(Error::Config("draft exceeds 64 KiB"));
        }
        self.text.insert_str(self.byte(self.cursor), &text);
        self.cursor += text.chars().count();
        Ok(())
    }
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.text.remove(self.byte(self.cursor));
        }
    }
    pub fn delete(&mut self) {
        let at = self.byte(self.cursor);
        if at < self.text.len() {
            self.text.remove(at);
        }
    }
    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
    }
    pub fn utf16_cursor(&self) -> usize {
        self.text
            .chars()
            .take(self.cursor)
            .map(char::len_utf16)
            .sum()
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }
}
