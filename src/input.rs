//! Преобразование нажатий клавиш в байты для pty.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

fn modifier_param(m: KeyModifiers) -> u8 {
    1 + m.contains(KeyModifiers::SHIFT) as u8
        + 2 * m.contains(KeyModifiers::ALT) as u8
        + 4 * m.contains(KeyModifiers::CONTROL) as u8
}

pub fn key_to_bytes(k: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let m = k.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    let has_mods = ctrl || alt || shift;

    let csi = |final_byte: char, app: bool| -> Vec<u8> {
        if has_mods && !matches!(k.code, KeyCode::Char(_)) {
            format!("\x1b[1;{}{}", modifier_param(m), final_byte).into_bytes()
        } else if app {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if has_mods {
            format!("\x1b[{};{}~", n, modifier_param(m)).into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };

    let mut out = match k.code {
        KeyCode::Char(c) => {
            if ctrl {
                let b = match c.to_ascii_lowercase() {
                    'a'..='z' => c.to_ascii_lowercase() as u8 - b'a' + 1,
                    ' ' | '@' | '2' => 0,
                    '[' | '3' => 27,
                    '\\' | '4' => 28,
                    ']' | '5' => 29,
                    '^' | '6' => 30,
                    '_' | '7' | '/' => 31,
                    '?' | '8' => 127,
                    _ => return None,
                };
                vec![b]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => csi('A', app_cursor),
        KeyCode::Down => csi('B', app_cursor),
        KeyCode::Right => csi('C', app_cursor),
        KeyCode::Left => csi('D', app_cursor),
        KeyCode::Home => csi('H', app_cursor),
        KeyCode::End => csi('F', app_cursor),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => tilde(15),
            6 => tilde(17),
            7 => tilde(18),
            8 => tilde(19),
            9 => tilde(20),
            10 => tilde(21),
            11 => tilde(23),
            12 => tilde(24),
            _ => return None,
        },
        _ => return None,
    };

    // Alt+символ / Alt+Enter / Alt+Backspace → ESC-префикс (как в большинстве терминалов).
    if alt && matches!(k.code, KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace | KeyCode::Tab) {
        out.insert(0, 0x1b);
    }
    Some(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEv {
    /// Кнопка: 0 — левая, 1 — средняя, 2 — правая.
    Down(u8),
    Up(u8),
    Drag(u8),
    Move,
    WheelUp,
    WheelDown,
}

/// Кодирует событие мыши так, как просил агент (режим и кодировка из его escape-последовательностей).
/// `col`/`row` — с нуля, относительно панели агента.
pub fn mouse_to_bytes(
    ev: MouseEv,
    mods: KeyModifiers,
    col: u16,
    row: u16,
    mode: MouseProtocolMode,
    enc: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    use MouseProtocolMode as M;
    let wanted = match ev {
        MouseEv::Down(_) | MouseEv::WheelUp | MouseEv::WheelDown => mode != M::None,
        MouseEv::Up(_) => matches!(mode, M::PressRelease | M::ButtonMotion | M::AnyMotion),
        MouseEv::Drag(_) => matches!(mode, M::ButtonMotion | M::AnyMotion),
        MouseEv::Move => mode == M::AnyMotion,
    };
    if !wanted {
        return None;
    }
    let sgr = enc == MouseProtocolEncoding::Sgr;
    let mut cb: u32 = match ev {
        MouseEv::Down(b) => b as u32,
        MouseEv::Up(b) => {
            if sgr {
                b as u32
            } else {
                3
            }
        }
        MouseEv::Drag(b) => b as u32 + 32,
        MouseEv::Move => 35,
        MouseEv::WheelUp => 64,
        MouseEv::WheelDown => 65,
    };
    if mode != M::Press {
        if mods.contains(KeyModifiers::SHIFT) {
            cb += 4;
        }
        if mods.contains(KeyModifiers::ALT) {
            cb += 8;
        }
        if mods.contains(KeyModifiers::CONTROL) {
            cb += 16;
        }
    }
    let (x, y) = (col as u32 + 1, row as u32 + 1);
    if sgr {
        let fin = if matches!(ev, MouseEv::Up(_)) { 'm' } else { 'M' };
        return Some(format!("\x1b[<{cb};{x};{y}{fin}").into_bytes());
    }
    let mut out = b"\x1b[M".to_vec();
    for v in [cb + 32, x + 32, y + 32] {
        if enc == MouseProtocolEncoding::Utf8 {
            let mut buf = [0u8; 4];
            out.extend_from_slice(char::from_u32(v)?.encode_utf8(&mut buf).as_bytes());
        } else {
            if v > 255 {
                return None;
            }
            out.push(v as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: m, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    #[test]
    fn plain_and_unicode() {
        assert_eq!(key_to_bytes(key(KeyCode::Char('a'), KeyModifiers::NONE), false), Some(b"a".to_vec()));
        assert_eq!(key_to_bytes(key(KeyCode::Char('я'), KeyModifiers::NONE), false), Some("я".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_c_and_alt() {
        assert_eq!(key_to_bytes(key(KeyCode::Char('c'), KeyModifiers::CONTROL), false), Some(vec![3]));
        assert_eq!(key_to_bytes(key(KeyCode::Char('f'), KeyModifiers::ALT), false), Some(vec![0x1b, b'f']));
        assert_eq!(key_to_bytes(key(KeyCode::Enter, KeyModifiers::ALT), false), Some(vec![0x1b, b'\r']));
    }

    #[test]
    fn arrows_respect_application_mode() {
        assert_eq!(key_to_bytes(key(KeyCode::Up, KeyModifiers::NONE), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_to_bytes(key(KeyCode::Up, KeyModifiers::NONE), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_to_bytes(key(KeyCode::Left, KeyModifiers::CONTROL), false), Some(b"\x1b[1;5D".to_vec()));
    }

    #[test]
    fn special_keys() {
        assert_eq!(key_to_bytes(key(KeyCode::Backspace, KeyModifiers::NONE), false), Some(vec![0x7f]));
        assert_eq!(key_to_bytes(key(KeyCode::BackTab, KeyModifiers::SHIFT), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_to_bytes(key(KeyCode::Delete, KeyModifiers::NONE), false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(key_to_bytes(key(KeyCode::F(5), KeyModifiers::NONE), false), Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn mouse_sgr_and_default() {
        use MouseProtocolEncoding as E;
        use MouseProtocolMode as M;
        let none = KeyModifiers::empty();
        let down = mouse_to_bytes(MouseEv::Down(0), none, 9, 4, M::PressRelease, E::Sgr);
        assert_eq!(down.unwrap(), b"\x1b[<0;10;5M");
        let up = mouse_to_bytes(MouseEv::Up(0), none, 9, 4, M::PressRelease, E::Sgr);
        assert_eq!(up.unwrap(), b"\x1b[<0;10;5m");
        let wheel = mouse_to_bytes(MouseEv::WheelUp, none, 0, 0, M::Press, E::Default);
        assert_eq!(wheel.unwrap(), vec![0x1b, b'[', b'M', 96, 33, 33]);
        // режим не просил движения — не отправляем
        assert!(mouse_to_bytes(MouseEv::Move, none, 1, 1, M::PressRelease, E::Sgr).is_none());
        assert!(mouse_to_bytes(MouseEv::Down(0), none, 1, 1, M::None, E::Sgr).is_none());
        let ctrl = mouse_to_bytes(MouseEv::Down(0), KeyModifiers::CONTROL, 0, 0, M::PressRelease, E::Sgr);
        assert_eq!(ctrl.unwrap(), b"\x1b[<16;1;1M");
    }
}
