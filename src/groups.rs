//! Группы агентов в списке: порядок, строки списка, сворачивание.
//!
//! Группа задаётся вручную («Группа…») или автоматически — по имени папки агента.
//! Заголовок показывается, если в группе два и больше агентов или она задана вручную.
//! Агенты одной группы всегда идут подряд (см. `order`).

use std::collections::HashSet;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Header { key: String, first: usize, count: usize, collapsed: bool },
    Item(usize),
}

/// Группа по умолчанию — имя папки.
pub fn auto_key(cwd: &Path) -> String {
    cwd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into())
}

/// Новый порядок агентов: группы идут блоками в порядке первого появления,
/// внутри группы порядок сохраняется. Возвращает индексы старого списка.
pub fn order(keys: &[String]) -> Vec<usize> {
    let mut uniq: Vec<&String> = vec![];
    for k in keys {
        if !uniq.contains(&k) {
            uniq.push(k);
        }
    }
    uniq.iter().flat_map(|u| keys.iter().enumerate().filter(move |(_, k)| k == u).map(|(i, _)| i)).collect()
}

/// Строки списка. `items` — (ключ группы, задана ли вручную) для каждого агента по порядку.
pub fn rows(items: &[(String, bool)], collapsed: &HashSet<String>) -> Vec<Row> {
    let mut out = vec![];
    let mut i = 0;
    while i < items.len() {
        let mut j = i;
        while j < items.len() && items[j].0 == items[i].0 {
            j += 1;
        }
        let header = j - i >= 2 || items[i..j].iter().any(|x| x.1);
        let is_collapsed = header && collapsed.contains(&items[i].0);
        if header {
            out.push(Row::Header { key: items[i].0.clone(), first: i, count: j - i, collapsed: is_collapsed });
        }
        if !is_collapsed {
            out.extend((i..j).map(Row::Item));
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn order_groups_blocks() {
        assert_eq!(order(&k(&["a", "b", "a", "c", "b"])), vec![0, 2, 1, 4, 3]);
        assert_eq!(order(&k(&[])), Vec::<usize>::new());
    }

    #[test]
    fn single_auto_has_no_header() {
        let items = vec![("a".to_string(), false), ("b".to_string(), false)];
        let r = rows(&items, &HashSet::new());
        assert_eq!(r, vec![Row::Item(0), Row::Item(1)]);
    }

    #[test]
    fn header_for_pair_and_manual() {
        let items = vec![("a".to_string(), false), ("a".to_string(), false), ("m".to_string(), true)];
        let r = rows(&items, &HashSet::new());
        assert_eq!(r.len(), 5);
        assert!(matches!(&r[0], Row::Header { key, first: 0, count: 2, collapsed: false } if key == "a"));
        assert!(matches!(&r[3], Row::Header { key, count: 1, .. } if key == "m"));
    }

    #[test]
    fn collapsed_hides_items() {
        let items = vec![("a".to_string(), false), ("a".to_string(), false), ("b".to_string(), false)];
        let c: HashSet<String> = ["a".to_string()].into();
        let r = rows(&items, &c);
        assert_eq!(r.len(), 2);
        assert!(matches!(&r[0], Row::Header { collapsed: true, .. }));
        assert_eq!(r[1], Row::Item(2));
        // одиночная автогруппа не сворачивается
        let c: HashSet<String> = ["b".to_string()].into();
        assert_eq!(rows(&items, &c).len(), 4);
    }
}
