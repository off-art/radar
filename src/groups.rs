//! Группы агентов в списке. Группы создаёт пользователь (в том числе пустые);
//! агенты одной группы идут подряд, агенты без группы — после всех групп.

use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// Заголовок группы: имя, индекс первого агента, число агентов, свёрнута ли.
    Header {
        key: String,
        first: usize,
        count: usize,
        collapsed: bool,
    },
    Item(usize),
    /// Поясняющая строка («Без группы», «пусто»).
    Text(String),
}

/// Индекс группы для каждого агента (`None` — без группы или группы нет в списке).
pub fn index_of(groups: &[String], member: Option<&str>) -> Option<usize> {
    member.and_then(|m| groups.iter().position(|g| g == m))
}

/// Новый порядок агентов: блоки групп в порядке списка групп, затем агенты без группы;
/// внутри блока порядок сохраняется. Возвращает индексы старого списка.
pub fn order(group_idx: &[Option<usize>]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..group_idx.len()).collect();
    idx.sort_by_key(|&i| group_idx[i].unwrap_or(usize::MAX)); // сортировка устойчивая
    idx
}

/// Строки списка. `group_idx` — группа каждого агента (в порядке агентов, уже собранных подряд).
pub fn rows(groups: &[String], group_idx: &[Option<usize>], collapsed: &HashSet<String>) -> Vec<Row> {
    let mut out = vec![];
    for (gi, name) in groups.iter().enumerate() {
        let members: Vec<usize> = (0..group_idx.len()).filter(|&i| group_idx[i] == Some(gi)).collect();
        let is_collapsed = collapsed.contains(name);
        out.push(Row::Header {
            key: name.clone(),
            first: members.first().copied().unwrap_or(0),
            count: members.len(),
            collapsed: is_collapsed,
        });
        if is_collapsed {
            continue;
        }
        if members.is_empty() {
            out.push(Row::Text("   пусто".into()));
        }
        out.extend(members.into_iter().map(Row::Item));
    }
    let loose: Vec<usize> = (0..group_idx.len()).filter(|&i| group_idx[i].is_none()).collect();
    if !loose.is_empty() {
        if !groups.is_empty() {
            out.push(Row::Text(" Без группы".into()));
        }
        out.extend(loose.into_iter().map(Row::Item));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn order_blocks_and_loose_last() {
        let idx = vec![None, Some(1), Some(0), None, Some(1)];
        assert_eq!(order(&idx), vec![2, 1, 4, 0, 3]);
        assert_eq!(order(&[]), Vec::<usize>::new());
    }

    #[test]
    fn index_lookup() {
        let groups = g(&["a", "b"]);
        assert_eq!(index_of(&groups, Some("b")), Some(1));
        assert_eq!(index_of(&groups, Some("x")), None);
        assert_eq!(index_of(&groups, None), None);
    }

    #[test]
    fn no_groups_plain_list() {
        let r = rows(&[], &[None, None], &HashSet::new());
        assert_eq!(r, vec![Row::Item(0), Row::Item(1)]);
    }

    #[test]
    fn empty_group_and_loose_label() {
        let groups = g(&["пустая", "a"]);
        let r = rows(&groups, &[Some(1), None], &HashSet::new());
        assert!(matches!(&r[0], Row::Header { key, count: 0, .. } if key == "пустая"));
        assert!(matches!(&r[1], Row::Text(_)));
        assert!(matches!(&r[2], Row::Header { key, first: 0, count: 1, .. } if key == "a"));
        assert_eq!(r[3], Row::Item(0));
        assert_eq!(r[4], Row::Text(" Без группы".into()));
        assert_eq!(r[5], Row::Item(1));
    }

    #[test]
    fn collapsed_hides_members() {
        let groups = g(&["a"]);
        let c: HashSet<String> = ["a".to_string()].into();
        let r = rows(&groups, &[Some(0), Some(0)], &c);
        assert_eq!(r.len(), 1);
        assert!(matches!(&r[0], Row::Header { collapsed: true, count: 2, .. }));
    }
}
