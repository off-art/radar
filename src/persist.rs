//! Сохранение списка агентов между запусками Radar (`~/.config/radar/sessions.toml`).
//!
//! Процессы агентов при закрытии Radar завершаются, поэтому восстанавливается сам список:
//! агент, папка, имя, режим «без уведомлений». Claude Code продолжает прежний диалог.

use crate::config::config_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Saved {
    /// Имя агента из конфига (`Claude Code`, `Codex`…).
    pub agent: String,
    pub dir: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default)]
    pub muted: bool,
    /// Идентификатор диалога для продолжения (`claude --resume`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    /// Группа в списке.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct File {
    #[serde(default)]
    pub selected: usize,
    #[serde(default, rename = "session")]
    pub sessions: Vec<Saved>,
    /// Группы списка по порядку (в том числе пустые).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    /// Свёрнутые группы.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed: Vec<String>,
}

pub fn path() -> PathBuf {
    config_dir().join("sessions.toml")
}

pub fn to_text(f: &File) -> String {
    format!(
        "# Список агентов, который Radar восстанавливает при запуске (отключается в Настройках)\n{}",
        toml::to_string(f).unwrap_or_default()
    )
}

pub fn save_text(text: &str) {
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, text);
}

pub fn load() -> File {
    std::fs::read_to_string(path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let f = File {
            selected: 1,
            sessions: vec![
                Saved {
                    agent: "Claude Code".into(),
                    dir: "/a b".into(),
                    name: "api".into(),
                    resume: Some("abc".into()),
                    ..Default::default()
                },
                Saved {
                    agent: "Codex".into(),
                    dir: "/x".into(),
                    name: "x #2".into(),
                    muted: true,
                    worktree: Some("/w".into()),
                    resume: None,
                    group: Some("Фронт".into()),
                },
            ],
            groups: vec!["Фронт".into(), "пустая".into()],
            collapsed: vec!["api".into()],
        };
        let back: File = toml::from_str(&to_text(&f)).unwrap();
        assert_eq!(back, f);
    }
}
