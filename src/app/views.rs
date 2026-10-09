//! Просмотр изменений (diff) и лента событий.

use super::{in_rect, App, Mode};
use crate::keys::Action;
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

impl App {
    pub(super) fn open_diff(&mut self) {
        let Some(s) = self.sessions.get(self.selected) else {
            self.toast("Нет агента, у которого можно посмотреть изменения");
            return;
        };
        let (dir, title) = (s.cwd.clone(), format!("{} · {}", s.name, s.agent));
        match crate::diff::load(&dir) {
            Err(e) => self.toast(e),
            Ok(files) if files.is_empty() => self.toast("Изменений нет — рабочая папка совпадает с последним коммитом"),
            Ok(files) => {
                self.mode = Mode::Diff(Box::new(crate::diff::View { title, dir, files, sel: 0, scroll: 0 }));
            }
        }
    }

    pub(super) fn diff_height(&self) -> usize {
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        ui::diff_layout(area).2.height as usize
    }

    pub(super) fn key_diff(&mut self, mut v: Box<crate::diff::View>, k: KeyEvent) {
        let h = self.diff_height();
        let page = (h.saturating_sub(2)).max(1) as isize;
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('v') => return,
            KeyCode::Down | KeyCode::Char('j') => v.scroll_by(1, h),
            KeyCode::Up | KeyCode::Char('k') => v.scroll_by(-1, h),
            KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('d') => v.scroll_by(page, h),
            KeyCode::PageUp | KeyCode::Char('u') => v.scroll_by(-page, h),
            KeyCode::Home | KeyCode::Char('g') => v.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => v.scroll = v.max_scroll(h),
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('n') | KeyCode::Char(']') => v.step_file(1),
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('p') | KeyCode::Char('[') => v.step_file(-1),
            KeyCode::Char('c') => {
                self.run_action(Action::GitCommit);
                return;
            }
            KeyCode::Char('P') => {
                self.run_action(Action::GitPush);
                return;
            }
            KeyCode::Char('r') => match crate::diff::load(&v.dir) {
                Ok(files) if !files.is_empty() => {
                    let sel = v.sel.min(files.len() - 1);
                    v.files = files;
                    v.select(sel);
                    self.toast("Обновлено");
                }
                Ok(_) => {
                    self.toast("Изменений больше нет");
                    return;
                }
                Err(e) => self.toast(e),
            },
            _ => {}
        }
        self.mode = Mode::Diff(v);
    }

    pub(super) fn mouse_diff(&mut self, m: MouseEvent) {
        let Mode::Diff(mut v) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        let (popup, list, body) = ui::diff_layout(area);
        let h = body.height as usize;
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollDown => v.scroll_by(3, h),
            MouseEventKind::ScrollUp => v.scroll_by(-3, h),
            MouseEventKind::Down(MouseButton::Left) => {
                if !in_rect(&popup, x, y) {
                    return; // клик вне окна — закрыть
                }
                if in_rect(&list, x, y) {
                    let first = ui::diff_list_first(&v, list.height as usize);
                    v.select(first + (y - list.y) as usize);
                }
            }
            _ => {}
        }
        self.mode = Mode::Diff(v);
    }

    pub(super) fn key_log(&mut self, mut sel: usize, k: KeyEvent) {
        let n = self.events.items.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('l') => return,
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1).min(n.saturating_sub(1)),
            KeyCode::Up | KeyCode::Char('k') => sel = sel.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => sel = 0,
            KeyCode::End | KeyCode::Char('G') => sel = n.saturating_sub(1),
            KeyCode::Enter => {
                self.log_jump(sel);
                return;
            }
            _ => {}
        }
        self.mode = Mode::Log(sel);
    }

    /// Переходит к агенту, о котором событие (если он ещё в списке).
    pub(super) fn log_jump(&mut self, sel: usize) {
        let n = self.events.items.len();
        let Some(e) = n.checked_sub(1 + sel).and_then(|i| self.events.items.get(i)) else {
            return;
        };
        let sid = e.session;
        match self.sessions.iter().position(|s| s.id == sid) {
            Some(i) if sid != 0 => self.select(i),
            _ => self.toast("Этого агента уже нет в списке"),
        }
    }

    pub(super) fn mouse_log(&mut self, sel: usize, x: u16, y: u16) {
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        let (popup, rows, first) = ui::log_layout(area, self.events.items.len(), sel);
        self.mode = Mode::Log(sel);
        if !in_rect(&popup, x, y) {
            self.mode = Mode::Normal;
        } else if let Some(i) = rows.iter().position(|r| in_rect(r, x, y)) {
            self.mode = Mode::Normal;
            self.log_jump(first + i);
        }
    }
}
