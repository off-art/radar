//! Действия, комбинации клавиш и их настройка.
//!
//! Модель как в Herdr: один префикс (по умолчанию Ctrl+b) включает «режим навигации», в котором
//! клавиши не нужно зажимать с Ctrl. Плюс необязательные «прямые» сочетания без префикса.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Всё, что можно сделать в интерфейсе. Используется меню, палитрой и клавишами.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewAgent,
    /// Новый агент указанного типа (индекс в конфиге).
    NewAgentOf(usize),
    /// Новый агент в папке текущего.
    NewHere,
    Close,
    Rename,
    Restart,
    ToggleMute,
    ToggleGrid,
    ToggleNotifications,
    /// Звук уведомлений вкл/выкл.
    ToggleSound,
    /// Всплывающие уведомления вкл/выкл.
    TogglePopups,
    /// Выбрать звуковую тему (индекс в notify::theme_names).
    SetTheme(usize),
    PickSound,
    Settings,
    Integrations,
    Next,
    Prev,
    Select(usize),
    NextWaiting,
    ScrollUp,
    ScrollDown,
    Palette,
    Help,
    Quit,
}

impl Action {
    /// Имя для конфига.
    pub fn parse(s: &str) -> Option<Action> {
        let s = s.trim().to_lowercase();
        if let Some(n) = s.strip_prefix("select_") {
            let n: usize = n.parse().ok()?;
            return (1..=9).contains(&n).then(|| Action::Select(n - 1));
        }
        Some(match s.as_str() {
            "new" => Action::NewAgent,
            "new_here" => Action::NewHere,
            "close" => Action::Close,
            "rename" => Action::Rename,
            "restart" => Action::Restart,
            "mute" => Action::ToggleMute,
            "grid" => Action::ToggleGrid,
            "notifications" => Action::ToggleNotifications,
            "sound" => Action::ToggleSound,
            "popups" => Action::TogglePopups,
            "pick_sound" => Action::PickSound,
            "settings" => Action::Settings,
            "integrations" => Action::Integrations,
            "next" => Action::Next,
            "prev" => Action::Prev,
            "waiting" => Action::NextWaiting,
            "scroll_up" => Action::ScrollUp,
            "scroll_down" => Action::ScrollDown,
            "palette" => Action::Palette,
            "help" => Action::Help,
            "quit" => Action::Quit,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    /// «ctrl+b», «alt+n», «shift+down», «ctrl+space», «f2».
    pub fn parse(s: &str) -> Option<Chord> {
        let mut mods = KeyModifiers::empty();
        let parts: Vec<String> = s.split('+').map(|p| p.trim().to_lowercase()).collect();
        let (key, mod_parts) = parts.split_last()?;
        for m in mod_parts {
            match m.as_str() {
                "ctrl" | "control" | "c" => mods |= KeyModifiers::CONTROL,
                "alt" | "opt" | "option" | "meta" | "a" => mods |= KeyModifiers::ALT,
                "shift" | "s" => mods |= KeyModifiers::SHIFT,
                _ => return None,
            }
        }
        let code = match key.as_str() {
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "esc" | "escape" => KeyCode::Esc,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" | "pgdown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok() => {
                KeyCode::F(k[1..].parse().ok()?)
            }
            k if k.chars().count() == 1 => KeyCode::Char(k.chars().next()?),
            _ => return None,
        };
        Some(Chord { code, mods })
    }

    pub fn matches(&self, k: &KeyEvent) -> bool {
        let strip = |m: KeyModifiers, code: KeyCode| {
            // для символов Shift уже учтён в самом символе
            if matches!(code, KeyCode::Char(_)) {
                m - KeyModifiers::SHIFT
            } else {
                m
            }
        };
        let norm = |c: KeyCode| match c {
            KeyCode::Char(ch) => KeyCode::Char(ch.to_ascii_lowercase()),
            o => o,
        };
        norm(k.code) == norm(self.code)
            && strip(k.modifiers, k.code) == strip(self.mods, self.code)
    }

    /// Подпись для интерфейса: «Ctrl+b».
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            s.push_str("Ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            s.push_str("Alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            s.push_str("Shift+");
        }
        s.push_str(&match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::PageUp => "PgUp".into(),
            KeyCode::PageDown => "PgDn".into(),
            KeyCode::F(n) => format!("F{n}"),
            other => format!("{other:?}"),
        });
        s
    }
}

#[derive(Clone, Debug)]
pub struct Keys {
    pub prefix: Chord,
    /// Сочетания без префикса. Должны быть такими, которые агенты не используют.
    pub direct: Vec<(Chord, Action)>,
    /// Через сколько секунд бездействия режим навигации выключается сам (0 — не выключается).
    pub nav_timeout: u64,
}

impl Default for Keys {
    fn default() -> Self {
        let d = |s: &str, a: Action| (Chord::parse(s).unwrap(), a);
        Keys {
            prefix: Chord::parse("ctrl+b").unwrap(),
            direct: vec![
                d("shift+down", Action::Next),
                d("shift+up", Action::Prev),
                d("shift+pageup", Action::ScrollUp),
                d("shift+pagedown", Action::ScrollDown),
            ],
            nav_timeout: 0,
        }
    }
}

impl Keys {
    pub fn direct_action(&self, k: &KeyEvent) -> Option<Action> {
        self.direct.iter().find(|(c, _)| c.matches(k)).map(|(_, a)| *a)
    }

    /// Подпись прямого сочетания для действия (если есть).
    pub fn direct_label(&self, a: Action) -> Option<String> {
        self.direct.iter().find(|(_, x)| *x == a).map(|(c, _)| c.label())
    }
}

/// Действие для клавиши в режиме навигации. `stay` — остаться в режиме после выполнения.
pub fn nav_action(k: &KeyEvent) -> Option<(Action, bool)> {
    use Action::*;
    Some(match k.code {
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab | KeyCode::Char(')') => (Next, true),
        KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab | KeyCode::Char('(') => (Prev, true),
        KeyCode::Char(d @ '1'..='9') => (Select(d as usize - '1' as usize), true),
        KeyCode::PageUp | KeyCode::Char('[') | KeyCode::Char('u') => (ScrollUp, true),
        KeyCode::PageDown | KeyCode::Char(']') | KeyCode::Char('d') => (ScrollDown, true),
        KeyCode::Char('g') | KeyCode::Char('z') => (ToggleGrid, true),
        KeyCode::Char('w') => (NextWaiting, true),
        KeyCode::Char('m') => (ToggleMute, true),
        KeyCode::Char('M') => (ToggleNotifications, true),
        KeyCode::Char('S') => (PickSound, false),
        KeyCode::Char(',') => (Settings, false),
        KeyCode::Char('n') | KeyCode::Char('c') => (NewAgent, false),
        KeyCode::Char('N') => (NewHere, false),
        KeyCode::Char('x') | KeyCode::Char('&') => (Close, false),
        KeyCode::Char('r') => (Rename, false),
        KeyCode::Char('R') => (Restart, false),
        KeyCode::Char('p') | KeyCode::Char(' ') | KeyCode::Char('/') => (Palette, false),
        KeyCode::Char('?') => (Help, false),
        KeyCode::Char('q') => (Quit, false),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(c, m)
    }

    #[test]
    fn parse_chords() {
        let c = Chord::parse("Ctrl+B").unwrap();
        assert!(c.matches(&key(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        assert!(!c.matches(&key(KeyCode::Char('b'), KeyModifiers::empty())));
        let s = Chord::parse("shift+down").unwrap();
        assert!(s.matches(&key(KeyCode::Down, KeyModifiers::SHIFT)));
        assert!(!s.matches(&key(KeyCode::Down, KeyModifiers::empty())));
        assert_eq!(Chord::parse("ctrl+space").unwrap().code, KeyCode::Char(' '));
        assert_eq!(Chord::parse("f5").unwrap().code, KeyCode::F(5));
        assert!(Chord::parse("hyper+x").is_none());
    }

    #[test]
    fn uppercase_char_ignores_shift() {
        let c = Chord::parse("alt+n").unwrap();
        assert!(c.matches(&key(KeyCode::Char('N'), KeyModifiers::ALT | KeyModifiers::SHIFT)));
    }

    #[test]
    fn actions_parse() {
        assert_eq!(Action::parse("new_here"), Some(Action::NewHere));
        assert_eq!(Action::parse("select_3"), Some(Action::Select(2)));
        assert_eq!(Action::parse("select_0"), None);
        assert_eq!(Action::parse("bogus"), None);
    }

    #[test]
    fn nav_keys() {
        let k = |c| key(KeyCode::Char(c), KeyModifiers::empty());
        assert_eq!(nav_action(&k('j')), Some((Action::Next, true)));
        assert_eq!(nav_action(&k('x')), Some((Action::Close, false)));
        assert_eq!(nav_action(&k('~')), None);
    }

    #[test]
    fn default_direct() {
        let keys = Keys::default();
        let a = keys.direct_action(&key(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(a, Some(Action::Prev));
        assert_eq!(keys.direct_label(Action::Next).as_deref(), Some("Shift+↓"));
    }
}
