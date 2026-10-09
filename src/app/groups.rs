//! Группы агентов в списке и перемещение по списку.

use super::{App, GroupForm, Mode};
use crate::keys::Action;
use crate::menu::{Menu, MenuItem};
use crate::session::Session;
use crate::status::Status;
use crate::textfield::TextField;
use anyhow::Result;
use ratatui::layout::Rect;

impl App {
    /// Переставляет агента `from` на место `to` (выбранный агент остаётся выбранным).
    pub(super) fn move_session(&mut self, from: usize, to: usize) {
        let n = self.sessions.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let cur = self.sessions[self.selected.min(n - 1)].id;
        let s = self.sessions.remove(from);
        self.sessions.insert(to, s);
        if let Some(i) = self.sessions.iter().position(|s| s.id == cur) {
            self.selected = i;
        }
        self.sel = None;
        self.press = None;
        self.dirty = true;
    }

    /// Номер группы каждого агента.
    pub(super) fn group_idx(&self) -> Vec<Option<usize>> {
        self.sessions.iter().map(|s| crate::groups::index_of(&self.groups, s.group.as_deref())).collect()
    }

    /// Группы и строки списка в порядке отображения.
    pub fn sidebar_rows(&self) -> Vec<crate::groups::Row> {
        crate::groups::rows(&self.groups, &self.group_idx(), &self.collapsed)
    }

    /// Собирает агентов одной группы подряд (без группы — в конце); выбранным остаётся агент `keep`.
    pub(super) fn normalize_groups(&mut self, keep: u32) {
        // группа, которой нет в списке (например, из старого файла), попадает в конец
        let unknown: Vec<String> =
            self.sessions.iter().filter_map(|s| s.group.clone()).filter(|g| !self.groups.contains(g)).collect();
        for g in unknown {
            if !self.groups.contains(&g) {
                self.groups.push(g);
            }
        }
        let ord = crate::groups::order(&self.group_idx());
        if ord.iter().enumerate().any(|(i, &o)| i != o) {
            let mut old: Vec<Option<Session>> = std::mem::take(&mut self.sessions).into_iter().map(Some).collect();
            self.sessions = ord.into_iter().filter_map(|i| old[i].take()).collect();
        }
        if let Some(i) = self.sessions.iter().position(|s| s.id == keep) {
            self.selected = i;
        }
        self.dirty = true;
    }

    pub(super) fn selected_id(&self) -> u32 {
        self.sessions.get(self.selected).map_or(0, |s| s.id)
    }

    /// Перестановка клавишами: только внутри своей группы.
    pub(super) fn move_in_group(&mut self, to: usize) {
        let from = self.selected;
        match self.sessions.get(to) {
            Some(t) if t.group == self.sessions[from].group => self.move_session(from, to),
            Some(_) => self.toast("Это край группы — перетащите агента мышью или переместите в группу (a)"),
            None => {}
        }
    }

    pub(super) fn open_group_form(&mut self, orig: Option<usize>) {
        let name = orig.and_then(|i| self.groups.get(i)).cloned().unwrap_or_default();
        let checked =
            self.sessions.iter().map(|s| orig.is_some() && s.group.as_deref() == Some(name.as_str())).collect();
        self.mode = Mode::GroupForm(GroupForm {
            orig,
            name: TextField::new(&name),
            ids: self.sessions.iter().map(|s| s.id).collect(),
            checked,
            cursor: self.selected.min(self.sessions.len().saturating_sub(1)),
            focus: 0,
        });
    }

    /// Сохраняет диалог группы. Ошибка (пустое/повторное имя) — текст для тоста.
    pub(super) fn save_group_form(&mut self, f: &GroupForm) -> Result<(), String> {
        let name = f.name.text().trim().to_string();
        if name.is_empty() {
            return Err("Введите имя группы".into());
        }
        if self.groups.iter().enumerate().any(|(i, g)| *g == name && Some(i) != f.orig) {
            return Err(format!("Группа «{name}» уже есть"));
        }
        let keep = self.selected_id();
        match f.orig.and_then(|i| self.groups.get(i).cloned().map(|old| (i, old))) {
            Some((i, old)) => {
                if old != name {
                    for s in &mut self.sessions {
                        if s.group.as_deref() == Some(old.as_str()) {
                            s.group = Some(name.clone());
                        }
                    }
                    if self.collapsed.remove(&old) {
                        self.collapsed.insert(name.clone());
                    }
                    self.groups[i] = name.clone();
                }
            }
            None => self.groups.push(name.clone()),
        }
        for (id, &on) in f.ids.iter().zip(&f.checked) {
            if let Some(s) = self.sessions.iter_mut().find(|s| s.id == *id) {
                if on {
                    s.group = Some(name.clone());
                } else if s.group.as_deref() == Some(name.as_str()) {
                    s.group = None;
                }
            }
        }
        self.collapsed.remove(&name);
        self.normalize_groups(keep);
        let n = self.sessions.iter().filter(|s| s.group.as_deref() == Some(name.as_str())).count();
        self.toast(format!("Группа «{name}»: агентов {n}"));
        Ok(())
    }

    pub(super) fn delete_group(&mut self, i: usize) {
        let Some(name) = self.groups.get(i).cloned() else { return };
        let keep = self.selected_id();
        for s in &mut self.sessions {
            if s.group.as_deref() == Some(name.as_str()) {
                s.group = None;
            }
        }
        self.groups.remove(i);
        self.collapsed.remove(&name);
        self.normalize_groups(keep);
        self.toast(format!("Группа «{name}» удалена, агенты остались в списке"));
    }

    pub(super) fn set_group(&mut self, group: Option<String>) {
        let keep = self.selected_id();
        if let Some(s) = self.sessions.get_mut(self.selected) {
            s.group = group.clone();
        }
        if let Some(g) = &group {
            self.collapsed.remove(g);
        }
        self.normalize_groups(keep);
    }

    pub(super) fn move_group(&mut self, i: usize, down: bool) {
        let j = if down { i + 1 } else { i.wrapping_sub(1) };
        if i < self.groups.len() && j < self.groups.len() {
            let keep = self.selected_id();
            self.groups.swap(i, j);
            self.normalize_groups(keep);
        }
    }

    /// Свернуть/развернуть группу выбранного агента.
    pub(super) fn toggle_group(&mut self) {
        match self.sessions.get(self.selected).and_then(|s| s.group.clone()) {
            Some(g) => self.toggle_group_key(&g),
            None => self.toast("Агент не в группе — переместить: a, создать группу: G"),
        }
    }

    pub(super) fn toggle_group_key(&mut self, key: &str) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_string());
        }
        self.dirty = true;
    }

    /// Меню «в какую группу переместить» выбранного агента.
    pub(super) fn group_menu(&self) -> Menu {
        let cur = self.sessions.get(self.selected).and_then(|s| s.group.clone());
        let mut items = vec![];
        for (i, g) in self.groups.iter().enumerate() {
            let mut it = MenuItem::new(&format!("В группу «{g}»"), Action::MoveToGroup(i), "");
            it.enabled = cur.as_deref() != Some(g.as_str());
            items.push(it);
        }
        if cur.is_some() {
            items.push(MenuItem::new("Убрать из группы", Action::Ungroup, ""));
        }
        if !items.is_empty() {
            items.push(MenuItem::sep());
        }
        items.push(MenuItem::new("Новая группа…", Action::NewGroup, "G"));
        let (x, y) = self
            .geo
            .items
            .iter()
            .find(|(i, _)| *i == self.selected)
            .map_or((self.geo.sidebar.x + 2, self.geo.sidebar.y + 3), |(_, r)| (r.x + 2, r.y + 1));
        Menu::new(x, y, items, Rect::new(0, 0, self.term_size().0, self.term_size().1))
    }

    pub(super) fn select_rel(&mut self, delta: isize) {
        // идём только по видимым агентам (без спрятанных в свёрнутых группах)
        let vis: Vec<usize> = self
            .sidebar_rows()
            .into_iter()
            .filter_map(|r| if let crate::groups::Row::Item(i) = r { Some(i) } else { None })
            .collect();
        let n = vis.len() as isize;
        if n == 0 {
            return;
        }
        let next = match vis.iter().position(|&i| i == self.selected) {
            Some(p) => vis[(p as isize + delta).rem_euclid(n) as usize],
            None if delta > 0 => vis.iter().copied().find(|&i| i > self.selected).unwrap_or(vis[0]),
            None => vis.iter().rev().copied().find(|&i| i < self.selected).unwrap_or(vis[vis.len() - 1]),
        };
        self.select(next);
    }

    pub(super) fn select_next_attention(&mut self) {
        let n = self.sessions.len();
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.sessions[i].status == Status::Waiting {
                self.select(i);
                return;
            }
        }
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.sessions[i].unread {
                self.select(i);
                return;
            }
        }
        self.toast("Нет агентов, которым нужен ответ");
    }
}
