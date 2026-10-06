//! Цветовые схемы (по образцу Herdr): встроенные темы, `terminal` (палитра терминала) и переопределение
//! отдельных цветов через `[theme.custom]`.

use ratatui::style::Color;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    /// Рисовать фон самим (true) или оставить фон терминала (false: radar, terminal).
    pub paint: bool,
    /// Заменять цвета ANSI 0–15 в окнах агентов на палитру темы.
    pub remap_ansi: bool,
    pub bg: Color,
    pub sidebar_bg: Color,
    pub header_bg: Color,
    pub active_row_bg: Color,
    pub popup_bg: Color,
    pub text: Color,
    pub subtext: Color,
    pub dim: Color,
    pub line: Color,
    pub accent: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
    pub teal: Color,
    pub peach: Color,
    pub mauve: Color,
    /// Текст на цветной плашке (статус, выделение).
    pub on_color: Color,
}

fn rgb(h: u32) -> Color {
    Color::Rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

/// Палитра из токенов (названия — как у Herdr / Catppuccin).
#[allow(clippy::too_many_arguments)]
fn palette(
    name: &'static str,
    [base, mantle, surface0, surface1, overlay0, overlay1]: [u32; 6],
    [text, subtext]: [u32; 2],
    [mauve, green, yellow, red, blue, teal, peach]: [u32; 7],
    dark: bool,
) -> Theme {
    Theme {
        name,
        paint: true,
        remap_ansi: true,
        bg: rgb(base),
        sidebar_bg: rgb(mantle),
        header_bg: rgb(surface0),
        active_row_bg: rgb(surface1),
        popup_bg: rgb(surface0),
        text: rgb(text),
        subtext: rgb(subtext),
        dim: rgb(overlay1),
        line: rgb(overlay0),
        accent: rgb(if dark { blue } else { mauve }),
        green: rgb(green),
        yellow: rgb(yellow),
        red: rgb(red),
        blue: rgb(blue),
        teal: rgb(teal),
        peach: rgb(peach),
        mauve: rgb(mauve),
        on_color: rgb(if dark { base } else { 0xffffff }),
    }
}

pub fn builtin() -> Vec<Theme> {
    vec![
        // как раньше: фон терминала, фирменные яркие цвета
        Theme {
            name: "radar",
            paint: false,
            remap_ansi: false,
            bg: Color::Reset,
            sidebar_bg: Color::Reset,
            header_bg: Color::Indexed(236),
            active_row_bg: Color::Indexed(237),
            popup_bg: Color::Reset,
            text: Color::Reset,
            subtext: Color::Indexed(250),
            dim: Color::Indexed(244),
            line: Color::Indexed(238),
            accent: Color::Rgb(251, 191, 36),
            green: Color::Rgb(74, 222, 128),
            yellow: Color::Rgb(251, 191, 36),
            red: Color::Rgb(248, 113, 113),
            blue: Color::Rgb(56, 189, 248),
            teal: Color::Rgb(45, 212, 191),
            peach: Color::Rgb(217, 119, 87),
            mauve: Color::Rgb(167, 139, 250),
            on_color: Color::Black,
        },
        // цвета берутся из палитры вашего терминала (ANSI)
        Theme {
            name: "terminal",
            paint: false,
            remap_ansi: false,
            bg: Color::Reset,
            sidebar_bg: Color::Reset,
            header_bg: Color::Indexed(8),
            active_row_bg: Color::Indexed(8),
            popup_bg: Color::Reset,
            text: Color::Reset,
            subtext: Color::Indexed(7),
            dim: Color::Indexed(8),
            line: Color::Indexed(8),
            accent: Color::Indexed(3),
            green: Color::Indexed(2),
            yellow: Color::Indexed(3),
            red: Color::Indexed(1),
            blue: Color::Indexed(4),
            teal: Color::Indexed(6),
            peach: Color::Indexed(9),
            mauve: Color::Indexed(5),
            on_color: Color::Indexed(0),
        },
        palette(
            "catppuccin",
            [0x1e1e2e, 0x181825, 0x313244, 0x45475a, 0x6c7086, 0x7f849c],
            [0xcdd6f4, 0xa6adc8],
            [0xcba6f7, 0xa6e3a1, 0xf9e2af, 0xf38ba8, 0x89b4fa, 0x94e2d5, 0xfab387],
            true,
        ),
        palette(
            "catppuccin-latte",
            [0xeff1f5, 0xe6e9ef, 0xccd0da, 0xbcc0cc, 0x9ca0b0, 0x8c8fa1],
            [0x4c4f69, 0x6c6f85],
            [0x8839ef, 0x40a02b, 0xdf8e1d, 0xd20f39, 0x1e66f5, 0x179299, 0xfe640b],
            false,
        ),
        palette(
            "tokyo-night",
            [0x1a1b26, 0x16161e, 0x292e42, 0x3b4261, 0x565f89, 0x737aa2],
            [0xc0caf5, 0xa9b1d6],
            [0xbb9af7, 0x9ece6a, 0xe0af68, 0xf7768e, 0x7aa2f7, 0x73daca, 0xff9e64],
            true,
        ),
        palette(
            "tokyo-night-day",
            [0xe1e2e7, 0xd0d5e3, 0xc4c8da, 0xb4b8cc, 0x9aa5ce, 0x848cb5],
            [0x3760bf, 0x6172b0],
            [0x9854f1, 0x587539, 0x8c6c3e, 0xf52a65, 0x2e7de9, 0x118c74, 0xb15c00],
            false,
        ),
        palette(
            "gruvbox",
            [0x282828, 0x1d2021, 0x3c3836, 0x504945, 0x7c6f64, 0x928374],
            [0xebdbb2, 0xbdae93],
            [0xd3869b, 0xb8bb26, 0xfabd2f, 0xfb4934, 0x83a598, 0x8ec07c, 0xfe8019],
            true,
        ),
        palette(
            "dracula",
            [0x282a36, 0x21222c, 0x343746, 0x44475a, 0x6272a4, 0x7b86b8],
            [0xf8f8f2, 0xbfbfbf],
            [0xff79c6, 0x50fa7b, 0xf1fa8c, 0xff5555, 0x8be9fd, 0x69e0c8, 0xffb86c],
            true,
        ),
        palette(
            "nord",
            [0x2e3440, 0x272c36, 0x3b4252, 0x434c5e, 0x4c566a, 0x616e88],
            [0xeceff4, 0xd8dee9],
            [0xb48ead, 0xa3be8c, 0xebcb8b, 0xbf616a, 0x81a1c1, 0x88c0d0, 0xd08770],
            true,
        ),
        palette(
            "one-dark",
            [0x282c34, 0x21252b, 0x2c313a, 0x3e4451, 0x5c6370, 0x7f848e],
            [0xabb2bf, 0x9da5b4],
            [0xc678dd, 0x98c379, 0xe5c07b, 0xe06c75, 0x61afef, 0x56b6c2, 0xd19a66],
            true,
        ),
        palette(
            "solarized-dark",
            [0x002b36, 0x00212b, 0x073642, 0x0f4753, 0x586e75, 0x657b83],
            [0x93a1a1, 0x839496],
            [0x6c71c4, 0x859900, 0xb58900, 0xdc322f, 0x268bd2, 0x2aa198, 0xcb4b16],
            true,
        ),
        palette(
            "solarized-light",
            [0xfdf6e3, 0xeee8d5, 0xe6dfc8, 0xdad3bb, 0x93a1a1, 0x839496],
            [0x586e75, 0x657b83],
            [0x6c71c4, 0x859900, 0xb58900, 0xdc322f, 0x268bd2, 0x2aa198, 0xcb4b16],
            false,
        ),
    ]
}

pub const DEFAULT: &str = "radar";

pub fn names() -> Vec<&'static str> {
    builtin().iter().map(|t| t.name).collect()
}

/// Тема по имени (неизвестное имя — тема по умолчанию).
pub fn find(name: &str) -> Theme {
    let all = builtin();
    let n = name.trim().to_lowercase();
    all.iter().find(|t| t.name == n).cloned().unwrap_or_else(|| all[0].clone())
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        if h.len() == 6 {
            return u32::from_str_radix(h, 16).ok().map(rgb);
        }
        return None;
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|x| x.strip_suffix(')')) {
        let v: Vec<u8> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if v.len() == 3 {
            return Some(Color::Rgb(v[0], v[1], v[2]));
        }
        return None;
    }
    Some(match s.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        _ => return None,
    })
}

impl Theme {
    /// Переопределения из `[theme.custom]`. Возвращает сообщения о нераспознанном.
    pub fn apply_custom(&mut self, custom: &BTreeMap<String, String>) -> Vec<String> {
        let mut warns = vec![];
        for (k, v) in custom {
            let Some(c) = parse_color(v) else {
                warns.push(format!("theme.custom.{k}: непонятный цвет «{v}»"));
                continue;
            };
            match k.as_str() {
                "panel_bg" | "bg" => self.bg = c,
                "sidebar_bg" => self.sidebar_bg = c,
                "header_bg" | "surface0" => {
                    self.header_bg = c;
                    self.popup_bg = c;
                }
                "active_row_bg" | "selection_bg" | "surface1" => self.active_row_bg = c,
                "surface_dim" => self.sidebar_bg = c,
                "overlay0" => self.line = c,
                "overlay1" => self.dim = c,
                "text" => self.text = c,
                "subtext0" | "subtext" => self.subtext = c,
                "accent" => self.accent = c,
                "mauve" => self.mauve = c,
                "green" => self.green = c,
                "yellow" => self.yellow = c,
                "red" => self.red = c,
                "blue" => self.blue = c,
                "teal" => self.teal = c,
                "peach" => self.peach = c,
                _ => warns.push(format!("theme.custom.{k}: неизвестный токен")),
            }
        }
        warns
    }

    /// Цвет ANSI 0–15 в палитре темы.
    pub fn ansi(&self, i: u8) -> Color {
        match i {
            0 => self.active_row_bg,
            1 | 9 => self.red,
            2 | 10 => self.green,
            3 | 11 => self.yellow,
            4 | 12 => self.blue,
            5 | 13 => self.mauve,
            6 | 14 => self.teal,
            7 => self.subtext,
            8 => self.line,
            _ => self.text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_falls_back() {
        assert_eq!(find("catppuccin").name, "catppuccin");
        assert_eq!(find("TOKYO-NIGHT").name, "tokyo-night");
        assert_eq!(find("нет такой").name, DEFAULT);
    }

    #[test]
    fn unique_names_and_count() {
        let n = names();
        let mut u = n.clone();
        u.sort();
        u.dedup();
        assert_eq!(n.len(), u.len());
        assert!(n.len() >= 10);
    }

    #[test]
    fn custom_overrides() {
        let mut t = find("catppuccin");
        let mut m = BTreeMap::new();
        m.insert("accent".to_string(), "#a6e3a1".to_string());
        m.insert("blue".to_string(), "rgb(1, 2, 3)".to_string());
        m.insert("bogus".to_string(), "#000000".to_string());
        m.insert("text".to_string(), "не цвет".to_string());
        let w = t.apply_custom(&m);
        assert_eq!(t.accent, Color::Rgb(0xa6, 0xe3, 0xa1));
        assert_eq!(t.blue, Color::Rgb(1, 2, 3));
        assert_eq!(w.len(), 2);
    }
}
