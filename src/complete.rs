//! Автодополнение пути к папке в форме «Новый агент» (клавиша Tab, как в шелле).

use crate::paths::expand_tilde;
use std::path::PathBuf;

/// Результат одного нажатия Tab.
#[derive(Debug, PartialEq)]
pub struct Completion {
    /// Новый текст поля.
    pub text: String,
    /// Подходящие подпапки (если их больше одной) — для подсказки и перебора.
    pub matches: Vec<String>,
    /// Часть пути до последнего `/` (включительно) — сохраняется при переборе.
    pub parent: String,
}

/// Подпапки каталога `parent`, имя которых начинается с `partial`.
fn subdirs(parent: &str, partial: &str) -> Vec<String> {
    let dir: PathBuf = if parent.is_empty() { PathBuf::from(".") } else { expand_tilde(parent) };
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir()) // is_dir идёт по симлинкам
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            n.to_lowercase().starts_with(&partial.to_lowercase()) && (partial.starts_with('.') || !n.starts_with('.'))
        })
        .collect();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

fn common_prefix(names: &[String]) -> String {
    let mut p = names[0].clone();
    for n in &names[1..] {
        let k = p.chars().zip(n.chars()).take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase())).count();
        p = p.chars().take(k).collect();
    }
    p
}

/// Подсказка «серым» при наборе: полный путь с первой подходящей папкой.
/// Пока имя не начато (пусто или путь кончается на `/`) подсказки нет.
pub fn suggest(input: &str) -> Option<String> {
    if input.is_empty() || input.ends_with('/') || input == "~" {
        return None;
    }
    let c = complete(input);
    let full = |names: &[String]| names.first().map(|n| format!("{}{n}/", c.parent));
    if !c.matches.is_empty() {
        return full(&c.matches);
    }
    // единственный вариант: complete() уже вернул его целиком
    (c.text != input && c.text.ends_with('/')).then_some(c.text)
}

/// Дополняет путь: одна подходящая папка — дописывается целиком с `/`,
/// несколько — общее начало, остальные возвращаются в `matches`.
pub fn complete(input: &str) -> Completion {
    let (parent, partial) = match input.rfind('/') {
        Some(i) => (input[..=i].to_string(), input[i + 1..].to_string()),
        None => (String::new(), input.to_string()),
    };
    // «~» без слеша — домашняя папка
    if input == "~" {
        return Completion { text: "~/".into(), matches: vec![], parent: "~/".into() };
    }
    let mut parent = parent;
    let mut names = subdirs(&parent, &partial);
    // «desk» без `/` и `~`: если в текущей папке нет, ищем в домашней (`~/Desktop/`)
    if names.is_empty() && !input.starts_with('/') && !input.starts_with('~') {
        let home_parent = format!("~/{parent}");
        let alt = subdirs(&home_parent, &partial);
        if !alt.is_empty() {
            names = alt;
            parent = home_parent;
        }
    }
    let (text, matches) = match names.len() {
        0 => (input.to_string(), vec![]),
        1 => (format!("{parent}{}/", names[0]), vec![]),
        _ => (format!("{parent}{}", common_prefix(&names)), names),
    };
    Completion { text, matches, parent }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn tmp() -> TempDir {
        let d = TempDir::new("complete");
        for n in ["alpha", "alpine", "beta", ".hidden"] {
            std::fs::create_dir_all(d.join(n)).unwrap();
        }
        std::fs::write(d.join("afile"), "x").unwrap();
        d
    }

    #[test]
    fn single_match_completes_with_slash() {
        let d = tmp();
        let c = complete(&format!("{}/be", d.display()));
        assert_eq!(c.text, format!("{}/beta/", d.display()));
        assert!(c.matches.is_empty());
    }

    #[test]
    fn several_matches_use_common_prefix() {
        let d = tmp();
        let c = complete(&format!("{}/al", d.display()));
        assert_eq!(c.text, format!("{}/alp", d.display()));
        assert_eq!(c.matches, vec!["alpha", "alpine"]);
        let c = complete(&format!("{}/alp", d.display()));
        assert_eq!(c.matches, vec!["alpha", "alpine"]);
        assert_eq!(c.text, format!("{}/alp", d.display()));
    }

    #[test]
    fn hidden_only_with_dot_and_files_skipped() {
        let d = tmp();
        let all = complete(&format!("{}/", d.display()));
        assert_eq!(all.matches, vec!["alpha", "alpine", "beta"]);
        let h = complete(&format!("{}/.h", d.display()));
        assert_eq!(h.text, format!("{}/.hidden/", d.display()));
        let f = complete(&format!("{}/af", d.display()));
        assert_eq!(f.text, format!("{}/af", d.display()));
    }

    #[test]
    fn case_insensitive() {
        let d = tmp();
        let c = complete(&format!("{}/BE", d.display()));
        assert_eq!(c.text, format!("{}/beta/", d.display()));
        let c = complete(&format!("{}/AL", d.display()));
        assert_eq!(c.matches, vec!["alpha", "alpine"]);
    }

    #[test]
    fn bare_name_falls_back_to_home() {
        // в текущей папке теста такого нет, а в домашней — `.` всегда есть, ищем скрытую папку по точке
        let home = dirs::home_dir().unwrap();
        let some = std::fs::read_dir(&home)
            .unwrap()
            .flatten()
            .find(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.file_name().to_string_lossy().to_string());
        if let Some(name) = some {
            let probe: String = name.chars().take(3).collect();
            if !std::path::Path::new(&probe).is_dir() {
                let c = complete(&probe.to_lowercase());
                assert!(c.text.starts_with("~/"), "{c:?}");
            }
        }
    }

    #[test]
    fn suggest_first_match_only_while_typing() {
        let d = tmp();
        let base = d.display();
        assert_eq!(suggest(&format!("{base}/al")), Some(format!("{base}/alpha/")));
        assert_eq!(suggest(&format!("{base}/be")), Some(format!("{base}/beta/")));
        assert_eq!(suggest(&format!("{base}/")), None);
        assert_eq!(suggest(&format!("{base}/zzz")), None);
        assert_eq!(suggest(""), None);
    }

    #[test]
    fn tilde_and_missing() {
        assert_eq!(complete("~").text, "~/");
        assert_eq!(complete("/definitely/not/here/x").text, "/definitely/not/here/x");
    }
}
