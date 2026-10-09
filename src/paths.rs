//! Каталоги и пути Radar в одном месте.

use std::path::{Path, PathBuf};

/// Домашняя папка; если её нет, текущая (`.`).
pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.config/radar` — настройки, состояние, журнал хуков, помощник уведомлений.
pub fn config_dir() -> PathBuf {
    home().join(".config").join("radar")
}

/// `~/.radar/run` — сокеты и журнал фоновых хозяев агентов.
pub fn run_dir() -> PathBuf {
    home().join(".radar").join("run")
}

/// `~/.radar/worktrees` — git worktree агентов.
pub fn worktrees_dir() -> PathBuf {
    home().join(".radar").join("worktrees")
}

/// Раскрывает `~` и `~/…` в начале пути.
pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        return home();
    }
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(p),
    }
}

/// Путь для показа: домашняя папка заменяется на `~`.
pub fn short_path(p: &Path) -> String {
    let Some(home) = dirs::home_dir() else {
        return p.display().to_string();
    };
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_roundtrip() {
        let h = home();
        assert_eq!(expand_tilde("~"), h);
        assert_eq!(expand_tilde("~/a/b"), h.join("a/b"));
        assert_eq!(expand_tilde("/tmp/x"), PathBuf::from("/tmp/x"));
        assert_eq!(expand_tilde("~user"), PathBuf::from("~user"));
        assert_eq!(short_path(&h), "~");
        assert_eq!(short_path(&h.join("proj")), "~/proj");
        assert_eq!(short_path(Path::new("/definitely/elsewhere")), "/definitely/elsewhere");
    }
}
