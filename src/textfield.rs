//! Однострочное поле ввода с нормальным редактированием: курсор, Home/End, слова, Ctrl+u/k/w.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default)]
pub struct TextField {
    chars: Vec<char>,
    cur: usize,
}

impl TextField {
    pub fn new(s: &str) -> TextField {
        let chars: Vec<char> = s.chars().collect();
        let cur = chars.len();
        TextField { chars, cur }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    #[allow(dead_code)]
    pub fn cursor(&self) -> usize {
        self.cur
    }

    /// (до курсора, символ под курсором, после курсора) — для отрисовки.
    pub fn parts(&self) -> (String, Option<char>, String) {
        let before: String = self.chars[..self.cur].iter().collect();
        let at = self.chars.get(self.cur).copied();
        let after: String = self.chars.iter().skip(self.cur + 1).collect();
        (before, at, after)
    }

    pub fn insert(&mut self, c: char) {
        self.chars.insert(self.cur, c);
        self.cur += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            self.insert(c);
        }
    }

    fn word_start(&self) -> usize {
        let mut i = self.cur;
        while i > 0 && self.chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !self.chars[i - 1].is_whitespace() && self.chars[i - 1] != '/' {
            i -= 1;
        }
        i
    }

    fn word_end(&self) -> usize {
        let n = self.chars.len();
        let mut i = self.cur;
        while i < n && self.chars[i].is_whitespace() {
            i += 1;
        }
        while i < n && !self.chars[i].is_whitespace() && self.chars[i] != '/' {
            i += 1;
        }
        i
    }

    /// Обрабатывает клавишу. `true` — клавиша была редактированием поля.
    pub fn handle_key(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Left if alt || ctrl => self.cur = self.word_start(),
            KeyCode::Right if alt || ctrl => self.cur = self.word_end(),
            KeyCode::Left => self.cur = self.cur.saturating_sub(1),
            KeyCode::Right => self.cur = (self.cur + 1).min(self.chars.len()),
            KeyCode::Home => self.cur = 0,
            KeyCode::End => self.cur = self.chars.len(),
            KeyCode::Backspace if alt => {
                let s = self.word_start();
                self.chars.drain(s..self.cur);
                self.cur = s;
            }
            KeyCode::Backspace => {
                if self.cur > 0 {
                    self.cur -= 1;
                    self.chars.remove(self.cur);
                }
            }
            KeyCode::Delete => {
                if self.cur < self.chars.len() {
                    self.chars.remove(self.cur);
                }
            }
            KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
                'a' => self.cur = 0,
                'e' => self.cur = self.chars.len(),
                'b' => self.cur = self.cur.saturating_sub(1),
                'f' => self.cur = (self.cur + 1).min(self.chars.len()),
                'u' => {
                    self.chars.drain(..self.cur);
                    self.cur = 0;
                }
                'k' => self.chars.truncate(self.cur),
                'w' => {
                    let s = self.word_start();
                    self.chars.drain(s..self.cur);
                    self.cur = s;
                }
                'd' => {
                    if self.cur < self.chars.len() {
                        self.chars.remove(self.cur);
                    }
                }
                _ => return false,
            },
            KeyCode::Char('b') if alt => self.cur = self.word_start(),
            KeyCode::Char('f') if alt => self.cur = self.word_end(),
            KeyCode::Char('d') if alt => {
                let e = self.word_end();
                self.chars.drain(self.cur..e);
            }
            KeyCode::Char(c) if !alt => self.insert(c),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(c, m)
    }

    #[test]
    fn insert_and_move() {
        let mut t = TextField::new("abc");
        t.handle_key(&k(KeyCode::Left, KeyModifiers::empty()));
        t.handle_key(&k(KeyCode::Char('X'), KeyModifiers::SHIFT));
        assert_eq!(t.text(), "abXc");
        t.handle_key(&k(KeyCode::Home, KeyModifiers::empty()));
        t.handle_key(&k(KeyCode::Delete, KeyModifiers::empty()));
        assert_eq!(t.text(), "bXc");
        assert_eq!(t.cursor(), 0);
    }

    #[test]
    fn readline_keys() {
        let mut t = TextField::new("~/work/api server");
        t.handle_key(&k(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(t.text(), "~/work/api ");
        t.handle_key(&k(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(t.text(), "~/work/");
        t.handle_key(&k(KeyCode::Char('a'), KeyModifiers::CONTROL));
        t.handle_key(&k(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert!(t.is_empty());
    }

    #[test]
    fn utf8_and_paste() {
        let mut t = TextField::default();
        t.insert_str("папка\n1");
        assert_eq!(t.text(), "папка1");
        t.handle_key(&k(KeyCode::Backspace, KeyModifiers::empty()));
        t.handle_key(&k(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(t.text(), "папк");
        let (b, at, a) = t.parts();
        assert_eq!((b.as_str(), at, a.as_str()), ("папк", None, ""));
    }

    #[test]
    fn unknown_ctrl_not_consumed() {
        let mut t = TextField::new("x");
        assert!(!t.handle_key(&k(KeyCode::Char('z'), KeyModifiers::CONTROL)));
        assert!(!t.handle_key(&k(KeyCode::Esc, KeyModifiers::empty())));
    }
}
