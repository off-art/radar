//! Мышь: выделение и копирование, перетаскивание, клики по окнам и меню.

use super::{in_rect, App, Confirm, Mode, PaneRect, Selection};
use crate::input::{mouse_to_bytes, MouseEv};
use crate::keys::Action;
use crate::menu::{Menu, MenuItem};
use crate::status::Status;
use crate::sync::MutexExt;
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

impl App {
    pub(super) fn pane_at(&self, x: u16, y: u16) -> Option<&PaneRect> {
        self.geo.panes.iter().find(|p| in_rect(&p.outer, x, y))
    }

    /// Передаёт событие мыши агенту, если он сам включил отслеживание мыши. `true` — отправлено.
    pub(super) fn forward_mouse(
        &mut self,
        idx: usize,
        inner: Rect,
        ev: MouseEv,
        mods: KeyModifiers,
        x: u16,
        y: u16,
    ) -> bool {
        if !in_rect(&inner, x, y) {
            return false;
        }
        let Some(s) = self.sessions.get_mut(idx) else {
            return false;
        };
        let (mode, enc, _) = s.mouse_state();
        match mouse_to_bytes(ev, mods, x - inner.x, y - inner.y, mode, enc) {
            Some(b) => {
                s.send_raw(&b);
                true
            }
            None => false,
        }
    }

    pub(super) fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);

        // открытое меню: наведение, выбор пункта, закрытие кликом мимо
        if matches!(self.mode, Mode::Menu(_)) {
            let Mode::Menu(mut menu) = std::mem::replace(&mut self.mode, Mode::Normal) else {
                return;
            };
            match m.kind {
                MouseEventKind::Moved => {
                    if let Some(i) = menu.item_at(x, y) {
                        if menu.sel != i {
                            menu.sel = i;
                            self.dirty = true;
                        }
                    }
                    self.mode = Mode::Menu(menu);
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(i) = menu.item_at(x, y) {
                        if let Some(a) = menu.items[i].action {
                            self.run_action(a);
                        }
                    } else if menu.contains(x, y) {
                        self.mode = Mode::Menu(menu); // клик по рамке/разделителю
                    }
                }
                MouseEventKind::Down(MouseButton::Right) if !menu.contains(x, y) => {
                    self.context_menu(x, y); // правый клик в другом месте — новое меню
                }
                MouseEventKind::Down(_) => {}
                _ => self.mode = Mode::Menu(menu),
            }
            return;
        }
        if let Mode::Settings(sel) = self.mode {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                self.mouse_settings(sel, x, y);
            }
            return;
        }
        if matches!(self.mode, Mode::Diff(_)) {
            self.mouse_diff(m);
            return;
        }
        if let Mode::Log(sel) = self.mode {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                self.mouse_log(sel, x, y);
            }
            return;
        }
        if let Mode::Integrations(sel) = self.mode {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                self.mouse_integrations(sel, x, y);
            }
            return;
        }
        if matches!(self.mode, Mode::New(_)) {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                self.mouse_new_form(x, y);
            }
            return;
        }
        if matches!(
            self.mode,
            Mode::Confirm(_) | Mode::Rename(_) | Mode::Commit(_) | Mode::GroupForm(_) | Mode::Palette(_)
        ) {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                self.mouse_modal(x, y);
            }
            return;
        }
        if matches!(self.mode, Mode::Help) {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.mode = Mode::Normal;
            }
            return;
        }
        if !matches!(self.mode, Mode::Normal | Mode::Nav(_)) {
            return;
        }

        let mods = m.modifiers;
        let pane = self.pane_at(x, y).map(|p| (p.idx, p.inner));
        match m.kind {
            // правая кнопка (и Ctrl+клик, как в macOS) — контекстное меню Radar
            MouseEventKind::Down(MouseButton::Right) => self.context_menu(x, y),
            MouseEventKind::Down(MouseButton::Left) if mods.contains(KeyModifiers::CONTROL) => self.context_menu(x, y),
            // граница списка и окна агента: тянем мышью (как в Herdr)
            MouseEventKind::Down(MouseButton::Left) if self.on_divider(x, y) => {
                self.mode = Mode::Normal;
                self.divider_drag = true;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.divider_drag => {
                let total = self.geo.sidebar.width + self.geo.main.width;
                let max = total.saturating_sub(40).max(20);
                self.cfg.sidebar_width = (x + 1).clamp(20, max);
                self.dirty = true;
            }
            MouseEventKind::Up(MouseButton::Left) if self.divider_drag => {
                self.divider_drag = false;
                self.cfg.save_state();
                self.dirty = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.mode = Mode::Normal;
                if in_rect(&self.geo.new_btn, x, y) {
                    self.open_new_form(None);
                } else if in_rect(&self.geo.group_btn, x, y) {
                    self.open_group_form(None);
                } else if in_rect(&self.geo.notif_btn, x, y) {
                    self.run_action(Action::ToggleNotifications);
                } else if let Some(key) = self.geo.headers.iter().find(|h| in_rect(&h.3, x, y)).map(|h| h.0.clone()) {
                    self.toggle_group_key(&key);
                } else if let Some(&(idx, r)) = self.geo.items.iter().find(|(_, r)| in_rect(r, x, y)) {
                    self.select(idx);
                    self.drag_item = Some(idx);
                    // клик по строке статуса «ждёт ответа» — сразу диалог разрешения
                    if y == r.y + 1 && self.sessions[idx].status == Status::Waiting {
                        self.approve_open_at(idx);
                    }
                } else if let Some(idx) = self
                    .geo
                    .panes
                    .iter()
                    .find(|p| p.header.is_some_and(|h| in_rect(&h, x, y)))
                    .map(|p| p.idx)
                    .filter(|&i| self.sessions[i].status == Status::Waiting)
                {
                    self.select(idx);
                    self.approve_open_at(idx);
                } else if let Some((idx, inner)) = pane {
                    self.sel = None;
                    if idx == self.selected && !self.cfg.mouse_select {
                        self.forward_mouse(idx, inner, MouseEv::Down(0), mods, x, y);
                    } else if idx == self.selected {
                        // клик или начало выделения — решится по движению мыши
                        self.press = Some((idx, inner, x, y, mods));
                    } else {
                        self.select(idx);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) if !self.cfg.mouse_select => {
                if let Some((idx, inner)) = pane.filter(|(i, _)| *i == self.selected) {
                    self.forward_mouse(idx, inner, MouseEv::Up(0), mods, x, y);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if !self.cfg.mouse_select => {
                if let Some((idx, inner)) = pane.filter(|(i, _)| *i == self.selected) {
                    self.forward_mouse(idx, inner, MouseEv::Drag(0), mods, x, y);
                }
            }
            MouseEventKind::Up(MouseButton::Left) if self.drag_item.is_some() => self.drag_item = None,
            MouseEventKind::Drag(MouseButton::Left) if self.drag_item.is_some() => {
                let Some(from) = self.drag_item.filter(|&f| f < self.sessions.len()) else { return };
                let id = self.sessions[from].id;
                // на заголовок группы — агент переходит в неё; на агента — в его группу (и на его место)
                let over_header = self.geo.headers.iter().find(|h| in_rect(&h.3, x, y)).map(|h| h.0.clone());
                let over_item = self.geo.items.iter().find(|(_, r)| in_rect(r, x, y)).map(|(i, _)| *i);
                if let Some(g) = over_header {
                    if self.sessions[from].group.as_ref() != Some(&g) {
                        self.sessions[from].group = Some(g);
                        self.normalize_groups(id);
                        self.drag_item = self.sessions.iter().position(|s| s.id == id);
                    }
                } else if let Some(to) = over_item.filter(|&t| t != from) {
                    self.sessions[from].group = self.sessions[to].group.clone();
                    self.move_session(from, to);
                    self.normalize_groups(id);
                    self.drag_item = self.sessions.iter().position(|s| s.id == id);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.mouse_up_left(x, y),
            MouseEventKind::Drag(MouseButton::Left) => self.mouse_drag_left(x, y),
            MouseEventKind::Down(b @ MouseButton::Middle) | MouseEventKind::Up(b @ MouseButton::Middle) => {
                if let Some((idx, inner)) = pane.filter(|(i, _)| *i == self.selected) {
                    let n = if b == MouseButton::Left { 0 } else { 1 };
                    let ev = if matches!(m.kind, MouseEventKind::Down(_)) { MouseEv::Down(n) } else { MouseEv::Up(n) };
                    self.forward_mouse(idx, inner, ev, mods, x, y);
                }
            }
            MouseEventKind::Drag(b @ MouseButton::Middle) => {
                if let Some((idx, inner)) = pane.filter(|(i, _)| *i == self.selected) {
                    let n = if b == MouseButton::Left { 0 } else { 1 };
                    self.forward_mouse(idx, inner, MouseEv::Drag(n), mods, x, y);
                }
            }
            MouseEventKind::Moved => {
                if let Some((idx, inner)) = pane.filter(|(i, _)| *i == self.selected) {
                    self.forward_mouse(idx, inner, MouseEv::Move, mods, x, y);
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                if let Some((idx, inner)) = pane {
                    self.wheel(idx, inner, up, mods, x, y);
                }
            }
            _ => {}
        }
    }

    /// Ячейка экрана агента под курсором мыши (с прижатием к границам окна).
    pub(super) fn pane_cell(inner: Rect, x: u16, y: u16) -> (u16, u16) {
        let r = y.clamp(inner.y, inner.bottom().saturating_sub(1)) - inner.y;
        let c = x.clamp(inner.x, inner.right().saturating_sub(1)) - inner.x;
        (r, c)
    }

    /// Протянули мышь с нажатой левой кнопкой: начинаем или продолжаем выделение текста.
    pub(super) fn mouse_drag_left(&mut self, x: u16, y: u16) {
        let Some((idx, inner, px, py, _)) = self.press else { return };
        if self.sel.is_none() && (x, y) == (px, py) {
            return;
        }
        let head = Self::pane_cell(inner, x, y);
        let anchor = self.sel.map_or_else(|| Self::pane_cell(inner, px, py), |s| s.anchor);
        self.sel = Some(Selection { idx, anchor, head });
        self.dirty = true;
    }

    /// Отпустили левую кнопку: после выделения — копируем, иначе это был обычный клик агенту.
    pub(super) fn mouse_up_left(&mut self, x: u16, y: u16) {
        let Some((idx, inner, px, py, mods)) = self.press.take() else { return };
        match self.sel {
            Some(sel) => {
                let text = self.selection_text(&sel);
                if text.is_empty() {
                    self.sel = None;
                } else {
                    let n = text.chars().count();
                    let ok = crate::clipboard::copy(&text);
                    self.toast(if ok {
                        format!("Скопировано: {n} симв.")
                    } else {
                        "Не удалось скопировать".to_string()
                    });
                }
            }
            None => {
                self.forward_mouse(idx, inner, MouseEv::Down(0), mods, px, py);
                self.forward_mouse(idx, inner, MouseEv::Up(0), mods, x, y);
            }
        }
        self.dirty = true;
    }

    /// Текст выделения с экрана агента (с учётом прокрутки истории).
    pub(super) fn selection_text(&self, sel: &Selection) -> String {
        let Some(s) = self.sessions.get(sel.idx) else { return String::new() };
        let ((r0, c0), (r1, c1)) = sel.ordered();
        let mut p = s.parser.lock_or_recover();
        p.screen_mut().set_scrollback(s.scroll);
        let (_, cols) = p.screen().size();
        let text = p.screen().contents_between(r0, c0.min(cols), r1, (c1 + 1).min(cols));
        p.screen_mut().set_scrollback(0);
        text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end().to_string()
    }

    /// Колесо: агенту с включённой мышью — событие; полноэкранным программам (less, vim, htop) —
    /// стрелки; обычному выводу — прокрутка истории Radar.
    pub(super) fn wheel(&mut self, idx: usize, inner: Rect, up: bool, mods: KeyModifiers, x: u16, y: u16) {
        self.sel = None; // содержимое сдвинется — выделение потеряло бы смысл
        let Some(s) = self.sessions.get_mut(idx) else {
            return;
        };
        let (mode, _, alt) = s.mouse_state();
        let ev = if up { MouseEv::WheelUp } else { MouseEv::WheelDown };
        if mode != vt100::MouseProtocolMode::None && s.scroll == 0 {
            self.forward_mouse(idx, inner, ev, mods, x, y);
        } else if alt && s.scroll == 0 {
            let app = s.app_cursor();
            let key = match (up, app) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            s.send_raw(key.repeat(3).as_bytes());
        } else {
            s.scroll_by(up, 3);
        }
    }

    /// Клик в форме нового агента: выбор агента, переход между полями, галочка worktree.
    /// Клики в небольших окнах: кнопки, строки списков, клик мимо окна — отмена.
    pub(super) fn mouse_modal(&mut self, x: u16, y: u16) {
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let hit_in = |rs: &[Rect], n: usize| rs.get(n).is_some_and(|r| in_rect(r, x, y));
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Confirm(c) => {
                let hit = ui::confirm_hit(self, &c, area);
                if hit_in(&hit.buttons, 0) {
                    self.key_confirm(c, enter);
                } else if let Some(&(o, _)) = hit.rows.iter().find(|(_, r)| in_rect(r, x, y)) {
                    if let Confirm::Approve(i, note, _) = c {
                        self.mode = Mode::Confirm(Confirm::Approve(i, note, Some(o)));
                    }
                } else if in_rect(&hit.popup, x, y) && !hit_in(&hit.buttons, 1) {
                    self.mode = Mode::Confirm(c);
                }
            }
            Mode::Rename(t) => {
                let hit = ui::input_hit(&self.theme, area);
                if hit_in(&hit.buttons, 0) {
                    self.key_rename(t, enter);
                } else if in_rect(&hit.popup, x, y) && !hit_in(&hit.buttons, 1) {
                    self.mode = Mode::Rename(t);
                }
            }
            Mode::Commit(t) => {
                let hit = ui::input_hit(&self.theme, area);
                if hit_in(&hit.buttons, 0) {
                    self.key_commit(t, enter);
                } else if in_rect(&hit.popup, x, y) && !hit_in(&hit.buttons, 1) {
                    self.mode = Mode::Commit(t);
                }
            }
            Mode::GroupForm(mut f) => {
                let hit = ui::group_hit(&self.theme, &f, area);
                if hit_in(&hit.buttons, 0) {
                    self.key_group_form(f, enter);
                    return;
                }
                if hit_in(&hit.buttons, 1) || !in_rect(&hit.popup, x, y) {
                    return;
                }
                if let Some(&(n, _)) = hit.rows.iter().find(|(_, r)| in_rect(r, x, y)) {
                    f.focus = 1;
                    f.cursor = n;
                    if let Some(c) = f.checked.get_mut(n) {
                        *c = !*c;
                    }
                } else if in_rect(&hit.field, x, y) {
                    f.focus = 0;
                }
                self.mode = Mode::GroupForm(f);
            }
            Mode::Palette(mut p) => {
                let hit = ui::palette_hit(&self.theme, &p, area);
                if let Some(&(pos, _)) = hit.rows.iter().find(|(_, r)| in_rect(r, x, y)) {
                    p.sel = pos;
                    self.key_palette(p, enter);
                } else if in_rect(&hit.popup, x, y) {
                    self.mode = Mode::Palette(p);
                }
            }
            other => self.mode = other,
        }
        self.dirty = true;
    }

    pub(super) fn mouse_new_form(&mut self, x: u16, y: u16) {
        let Mode::New(mut f) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        let lay = ui::form_layout(area, self, &f);
        if !in_rect(&lay.popup, x, y) {
            return; // клик мимо окна — отмена
        }
        if let Some(&(i, _)) = lay.chips.iter().find(|(_, r)| in_rect(r, x, y)) {
            f.field = 0;
            if i == usize::MAX {
                f.show_all = !f.show_all;
                let vis = self.visible_agents(f.show_all);
                if !vis.contains(&f.agent) {
                    f.agent = vis.first().copied().unwrap_or(0);
                }
            } else {
                f.agent = i;
            }
        } else if in_rect(&lay.dir, x, y) {
            f.field = 1;
        } else if in_rect(&lay.name, x, y) {
            f.field = 2;
        } else if in_rect(&lay.worktree, x, y) {
            f.field = 3;
            f.worktree = !f.worktree;
        }
        self.mode = Mode::New(f);
    }

    /// Курсор на границе между списком агентов и окном агента.
    pub(super) fn on_divider(&self, x: u16, y: u16) -> bool {
        let sb = self.geo.sidebar;
        sb.width > 0 && x + 1 == sb.right() && y >= sb.y && y < sb.bottom()
    }

    pub(super) fn context_menu(&mut self, x: u16, y: u16) {
        let item = self.geo.items.iter().find(|(_, r)| in_rect(r, x, y)).map(|(i, _)| *i);
        let pane = self.pane_at(x, y).map(|p| p.idx);
        let header = self.geo.headers.iter().find(|h| in_rect(&h.3, x, y)).map(|h| h.0.clone());
        let menu = if let Some(gi) = header.and_then(|k| self.groups.iter().position(|g| *g == k)) {
            let mut up = MenuItem::new("Выше", Action::MoveGroupUp(gi), "");
            up.enabled = gi > 0;
            let mut down = MenuItem::new("Ниже", Action::MoveGroupDown(gi), "");
            down.enabled = gi + 1 < self.groups.len();
            let items = vec![
                MenuItem::new("Свернуть/развернуть", Action::ToggleGroupAt(gi), "o"),
                MenuItem::new("Переименовать и состав…", Action::EditGroup(gi), ""),
                up,
                down,
                MenuItem::sep(),
                MenuItem::new("Удалить группу…", Action::DeleteGroup(gi), ""),
            ];
            Menu::new(x, y, items, Rect::new(0, 0, self.term_size().0, self.term_size().1))
        } else if let Some(idx) = item {
            self.select(idx);
            self.session_menu(idx, true, x, y)
        } else if let Some(idx) = pane {
            self.select(idx);
            self.session_menu(idx, false, x, y)
        } else {
            self.general_menu(x, y)
        };
        self.mode = Mode::Menu(menu);
        self.dirty = true;
    }
}
