//! Запуск агентов и поиск программ на Windows: PowerShell вместо login-шелла, поиск по `PATH` и `PATHEXT`.
//! Чистые функции без обращения к системе (кроме `ps_exe`), поэтому проверяются тестами и на unix.

use std::ffi::OsStr;
use std::path::PathBuf;

/// Строка в одинарных кавычках PowerShell. Кроме `'` кавычками считаются «умные» ‘ ’ ‚ ‛ — их тоже удваиваем.
pub fn ps_quote(s: &str) -> String {
    let mut o = String::from("'");
    for c in s.chars() {
        o.push(c);
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            o.push(c);
        }
    }
    o.push('\'');
    o
}

/// Команда для `powershell -Command`: запускает `command` с аргументами и возвращает его код выхода.
/// `command` вставляется как есть (может содержать свои аргументы), остальные аргументы квотируются.
pub fn agent_line(command: &str, args: &[String]) -> String {
    let mut line = format!("& {command}");
    for a in args {
        line.push(' ');
        line.push_str(&ps_quote(a));
    }
    line.push_str("; exit $LASTEXITCODE");
    line
}

/// Ищет программу как `where`: в каждой папке `PATH` пробует имя как есть (если у него уже есть
/// расширение из `PATHEXT`), затем имя с каждым расширением по порядку.
pub fn find_in_path(name: &str, path: &OsStr, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    let has_ext = exts.iter().any(|e| name.len() > e.len() && name[name.len() - e.len()..].eq_ignore_ascii_case(e));
    for dir in std::env::split_paths(path) {
        if has_ext && dir.join(name).is_file() {
            return Some(dir.join(name));
        }
        for e in &exts {
            let p = dir.join(format!("{name}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// PowerShell 7 (`pwsh`), иначе встроенный Windows PowerShell.
#[cfg(windows)]
pub fn ps_exe() -> String {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if find_in_path("pwsh", &path, ".EXE").is_some() {
        "pwsh.exe".into()
    } else {
        "powershell.exe".into()
    }
}

#[cfg(windows)]
pub fn pathext() -> String {
    std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn quoting_doubles_quotes() {
        assert_eq!(ps_quote("a b"), "'a b'");
        assert_eq!(ps_quote("it's"), "'it''s'");
        assert_eq!(ps_quote("‘x’"), "'‘‘x’’'");
        assert_eq!(ps_quote(r"C:\Users\a b\x.json"), r"'C:\Users\a b\x.json'");
    }

    #[test]
    fn agent_line_quotes_args_and_keeps_exit_code() {
        let args = vec!["--settings".to_string(), r"C:\Users\a b\s.json".to_string()];
        assert_eq!(agent_line("claude", &args), r"& claude '--settings' 'C:\Users\a b\s.json'; exit $LASTEXITCODE");
        assert_eq!(agent_line("codex", &[]), "& codex; exit $LASTEXITCODE");
    }

    #[test]
    fn path_search_follows_pathext_order() {
        let a = TempDir::new("winsh-a");
        let b = TempDir::new("winsh-b");
        std::fs::write(a.join("claude.cmd"), "").unwrap();
        std::fs::write(b.join("claude.exe"), "").unwrap();
        std::fs::write(b.join("codex.exe"), "").unwrap();
        let path = std::env::join_paths([&*a, &*b]).unwrap();
        // первая папка PATH важнее порядка расширений
        assert_eq!(find_in_path("claude", &path, ".exe;.cmd"), Some(a.join("claude.cmd")));
        assert_eq!(find_in_path("codex", &path, ".exe;.cmd"), Some(b.join("codex.exe")));
        // имя уже с расширением
        assert_eq!(find_in_path("codex.exe", &path, ".exe;.cmd"), Some(b.join("codex.exe")));
        assert_eq!(find_in_path("nothing", &path, ".exe;.cmd"), None);
    }
}
