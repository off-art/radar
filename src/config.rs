//! Конфигурация: ~/.config/radar/config.toml

use crate::keys::{Action, Chord, Keys};
use ratatui::style::Color;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Claude Code: точные статусы через хуки.
    Claude,
    /// Любой другой CLI-агент: статусы по эвристике.
    Generic,
    /// Обычный shell.
    Shell,
}

#[derive(Clone, Debug)]
pub struct AgentDef {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub kind: Kind,
    pub color: Color,
}

#[derive(Deserialize, Default)]
struct RawAgent {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    kind: Option<String>,
    color: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawTheme {
    name: Option<String>,
    custom: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize, Default)]
struct RawKeys {
    prefix: Option<String>,
    nav_timeout: Option<u64>,
    direct: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize, Default)]
struct RawConfig {
    notifications: Option<bool>,
    sound: Option<bool>,
    popups: Option<bool>,
    restore: Option<bool>,
    sound_theme: Option<String>,
    volume: Option<f32>,
    sound_done: Option<String>,
    sound_waiting: Option<String>,
    mouse: Option<bool>,
    sidebar_width: Option<u16>,
    theme: Option<RawTheme>,
    keys: Option<RawKeys>,
    #[serde(default)]
    agent: Vec<RawAgent>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub notifications: bool,
    pub sound: bool,
    /// Всплывающие уведомления macOS (звук включается отдельно).
    pub popups: bool,
    /// Восстанавливать список агентов при запуске.
    pub restore: bool,
    /// Звуковая тема: bell, sonar, retro, harp, knock, thump, drop.
    pub sound_theme: String,
    /// Громкость уведомлений, 0.0–1.0.
    pub volume: f32,
    pub sound_done: Option<PathBuf>,
    pub sound_waiting: Option<PathBuf>,
    pub mouse: bool,
    /// Ширина списка агентов; 0 — автоматически.
    pub sidebar_width: u16,
    pub theme_name: String,
    pub theme_custom: BTreeMap<String, String>,
    pub keys: Keys,
    pub agents: Vec<AgentDef>,
    /// Замечания к конфигу (неизвестные сочетания и т. п.) — показываются при запуске.
    pub warnings: Vec<String>,
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("radar")
}

pub fn config_path() -> PathBuf {
    std::env::var_os("RADAR_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir().join("config.toml"))
}

fn state_path() -> PathBuf {
    config_dir().join("state.toml")
}

#[derive(Deserialize, Default)]
struct RawState {
    notifications: Option<bool>,
    sound: Option<bool>,
    popups: Option<bool>,
    restore: Option<bool>,
    sound_theme: Option<String>,
    theme: Option<String>,
    volume: Option<f32>,
    sidebar_width: Option<u16>,
}

fn parse_color(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn builtin() -> Vec<AgentDef> {
    let mk = |name: &str, cmd: &str, kind: Kind, (r, g, b): (u8, u8, u8)| AgentDef {
        id: slug(name),
        name: name.into(),
        command: cmd.into(),
        args: vec![],
        kind,
        color: Color::Rgb(r, g, b),
    };
    vec![
        mk("Claude Code", "claude", Kind::Claude, (217, 119, 87)),
        mk("Codex", "codex", Kind::Generic, (16, 163, 127)),
        mk("OpenCode", "opencode", Kind::Generic, (96, 165, 250)),
        mk("Qwen Code", "qwen", Kind::Generic, (167, 139, 250)),
        mk("GigaCode", "gigacode", Kind::Generic, (33, 160, 56)),
        mk("Gemini CLI", "gemini", Kind::Generic, (66, 133, 244)),
        mk("Shell", "$SHELL", Kind::Shell, (161, 161, 170)),
    ]
}

impl Config {
    pub fn load() -> Config {
        let mut cfg = Config {
            notifications: true,
            sound: true,
            popups: true,
            restore: true,
            sound_theme: crate::notify::DEFAULT_THEME.to_string(),
            volume: 0.6,
            sound_done: None,
            sound_waiting: None,
            mouse: true,
            sidebar_width: 0,
            theme_name: crate::theme::DEFAULT.to_string(),
            theme_custom: BTreeMap::new(),
            keys: Keys::default(),
            agents: builtin(),
            warnings: vec![],
        };
        let Ok(text) = std::fs::read_to_string(config_path()) else {
            cfg.apply_state();
            return cfg;
        };
        let raw = match toml::from_str::<RawConfig>(&text) {
            Ok(r) => r,
            Err(e) => {
                let msg = e.message().lines().next().unwrap_or("").to_string();
                cfg.warnings.push(format!("config.toml не прочитан: {msg}"));
                return cfg;
            }
        };
        cfg.notifications = raw.notifications.unwrap_or(true);
        cfg.sound = raw.sound.unwrap_or(true);
        cfg.popups = raw.popups.unwrap_or(true);
        cfg.restore = raw.restore.unwrap_or(true);
        if let Some(t) = raw.sound_theme {
            cfg.sound_theme = t;
        }
        cfg.mouse = raw.mouse.unwrap_or(true);
        cfg.volume = raw.volume.unwrap_or(0.6).clamp(0.0, 1.0);
        cfg.sound_done = raw.sound_done.map(|p| crate::app::expand_tilde(&p));
        cfg.sound_waiting = raw.sound_waiting.map(|p| crate::app::expand_tilde(&p));
        cfg.sidebar_width = raw.sidebar_width.unwrap_or(0);
        if let Some(t) = raw.theme {
            if let Some(n) = t.name {
                cfg.theme_name = n;
            }
            cfg.theme_custom = t.custom.unwrap_or_default();
            let mut probe = crate::theme::find(&cfg.theme_name);
            cfg.warnings.extend(probe.apply_custom(&cfg.theme_custom));
        }
        if let Some(k) = raw.keys {
            if let Some(p) = k.prefix {
                match Chord::parse(&p) {
                    Some(c) => cfg.keys.prefix = c,
                    None => cfg.warnings.push(format!("keys.prefix: непонятное сочетание «{p}»")),
                }
            }
            if let Some(t) = k.nav_timeout {
                cfg.keys.nav_timeout = t;
            }
            if let Some(d) = k.direct {
                cfg.keys.direct.clear();
                for (chord, action) in d {
                    match (Chord::parse(&chord), Action::parse(&action)) {
                        (Some(c), Some(a)) => cfg.keys.direct.push((c, a)),
                        _ => cfg.warnings.push(format!("keys.direct: «{chord}» = «{action}» не распознано")),
                    }
                }
            }
        }
        for a in raw.agent {
            let kind = match a.kind.as_deref() {
                Some("claude") => Kind::Claude,
                Some("shell") => Kind::Shell,
                _ => Kind::Generic,
            };
            let def = AgentDef {
                id: slug(&a.name),
                color: a
                    .color
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or(Color::Rgb(148, 163, 184)),
                name: a.name,
                command: a.command,
                args: a.args,
                kind,
            };
            if let Some(slot) = cfg.agents.iter_mut().find(|d| d.id == def.id) {
                *slot = def;
            } else {
                cfg.agents.push(def);
            }
        }
        cfg.apply_state();
        cfg
    }

    /// Настройки, изменённые из интерфейса (меню, палитра), хранятся отдельно и важнее config.toml.
    fn apply_state(&mut self) {
        let Ok(text) = std::fs::read_to_string(state_path()) else {
            return;
        };
        let Ok(st) = toml::from_str::<RawState>(&text) else {
            return;
        };
        if let Some(v) = st.notifications {
            self.notifications = v;
        }
        if let Some(v) = st.sound {
            self.sound = v;
        }
        if let Some(v) = st.popups {
            self.popups = v;
        }
        if let Some(v) = st.restore {
            self.restore = v;
        }
        if let Some(v) = st.sound_theme {
            self.sound_theme = v;
        }
        if let Some(v) = st.theme {
            self.theme_name = v;
        }
        if let Some(v) = st.volume {
            self.volume = v.clamp(0.0, 1.0);
        }
        if let Some(v) = st.sidebar_width {
            self.sidebar_width = v;
        }
    }

    pub fn save_state(&self) {
        let text = format!(
            "# Настройки, изменённые из интерфейса Radar (важнее config.toml)\nnotifications = {}\nsound = {}\npopups = {}\nrestore = {}\nsound_theme = \"{}\"\ntheme = \"{}\"\nvolume = {:.2}\nsidebar_width = {}\n",
            self.notifications,
            self.sound,
            self.popups,
            self.restore,
            self.sound_theme.replace(['"', '\\'], ""),
            self.theme_name.replace(['"', '\\'], ""),
            self.volume,
            self.sidebar_width
        );
        let p = state_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, text);
    }

    /// Активная цветовая схема (с учётом `[theme.custom]`).
    pub fn theme(&self) -> crate::theme::Theme {
        let mut t = crate::theme::find(&self.theme_name);
        t.apply_custom(&self.theme_custom);
        t
    }

    pub fn find(&self, id: &str) -> Option<&AgentDef> {
        let id = id.to_lowercase();
        self.agents
            .iter()
            .find(|a| a.id == id || a.command == id || a.name.to_lowercase() == id)
    }
}

const EXAMPLE: &str = r##"# Radar — конфигурация
# Изменения применяются при следующем запуске.

# Системные уведомления macOS, когда агент закончил или ждёт ответа
notifications = true
sound = true
# Всплывающие уведомления macOS. Можно выключить, оставив только звук: popups = false
popups = true
# Восстанавливать список агентов при следующем запуске Radar
restore = true
# Звуковая тема: bell, sonar, retro, harp, knock, thump, drop (выбор также в палитре: Ctrl+b, p, «Звук»)
sound_theme = "bell"
# Громкость уведомлений (0.0–1.0). Свои звуки: sound_done / sound_waiting = "~/sounds/x.wav"
volume = 0.6
# Управление мышью (клик по списку, прокрутка). Выделение текста — с зажатым Option/Shift.
mouse = true

# Ширина списка агентов в колонках (0 — автоматически)
sidebar_width = 0

# Цветовая схема (меняется и в интерфейсе: Ctrl+b, затем «,» — Настройки).
# radar, terminal (палитра вашего терминала), catppuccin, catppuccin-latte, tokyo-night, tokyo-night-day,
# gruvbox, dracula, nord, one-dark, solarized-dark, solarized-light
[theme]
name = "radar"

# Переопределение отдельных цветов поверх выбранной схемы (hex, rgb(r,g,b) или имя):
# accent, panel_bg, sidebar_bg, active_row_bg, header_bg, text, subtext0, overlay0, overlay1,
# green, yellow, red, blue, teal, peach, mauve
# [theme.custom]
# accent = "#a6e3a1"

# Клавиши. Префикс включает режим навигации (j/k — выбор, n — новый, x — закрыть, ? — помощь),
# выход из него — Esc или Enter. Сочетания без префикса (direct) — только те, что не нужны агентам.
# Действия: new, new_here, close, rename, restart, mute, grid, notifications, sound, popups, pick_sound,
#           settings, integrations, next, prev, waiting, scroll_up, scroll_down, palette, help, quit,
#           select_1 … select_9
[keys]
prefix = "ctrl+b"          # например "ctrl+space" или "ctrl+a"
nav_timeout = 0            # секунд до автовыхода из режима навигации (0 — без таймера)

[keys.direct]
"shift+down" = "next"
"shift+up" = "prev"
"shift+pageup" = "scroll_up"
"shift+pagedown" = "scroll_down"
# "ctrl+space" = "palette"
# "alt+n" = "new"          # Option на macOS должен работать как Meta (iTerm2/Terminal: «Use Option as Meta»)

# Свои агенты (или переопределение встроенных по имени).
# kind: "claude" (статусы через хуки), "generic" (по эвристике), "shell"
#
# [[agent]]
# name = "Claude Code"
# command = "claude"
# args = ["--model", "opus"]
# kind = "claude"
# color = "#d97757"
#
# [[agent]]
# name = "Aider"
# command = "aider"
# color = "#22c55e"
"##;

/// Создаёт пример конфига при первом запуске.
pub fn write_example_if_missing() {
    let p = config_path();
    if p.exists() {
        return;
    }
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, EXAMPLE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_has_claude_and_shell() {
        let c = Config::load();
        assert!(c.find("claude").is_some());
        assert!(c.find("Shell").is_some());
        assert_eq!(c.find("claude").unwrap().kind, Kind::Claude);
    }

    #[test]
    fn keys_from_config() {
        let raw: RawConfig = toml::from_str(
            "[keys]\nprefix = \"ctrl+a\"\n[keys.direct]\n\"alt+n\" = \"new\"\n\"x+y\" = \"zzz\"",
        )
        .unwrap();
        let k = raw.keys.unwrap();
        assert_eq!(k.prefix.as_deref(), Some("ctrl+a"));
        assert_eq!(k.direct.unwrap().len(), 2);
    }

    #[test]
    fn color_parse() {
        assert_eq!(parse_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("ff0000"), None);
    }

    #[test]
    fn example_config_is_valid_toml() {
        assert!(toml::from_str::<RawConfig>(EXAMPLE).is_ok());
    }
}
