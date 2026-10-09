//! Окно настроек и интеграции агентов.

use super::{in_rect, App, IntegrationRow, Mode};
use crate::config::Kind;
use crate::notify::{self};
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

impl App {
    /// Строки экрана настроек: (название, значение).
    pub fn settings_rows(&self) -> Vec<(String, String)> {
        let onoff = |b: bool| if b { "вкл" } else { "выкл" }.to_string();
        vec![
            ("Цветовая схема".into(), self.cfg.theme_name.clone()),
            ("Уведомления".into(), onoff(self.notifications)),
            ("Звук".into(), onoff(self.cfg.sound)),
            ("Звуковая тема".into(), self.cfg.sound_theme.clone()),
            ("Громкость".into(), format!("{}%", (self.cfg.volume * 100.0).round() as u32)),
            ("Всплывающие окошки".into(), onoff(self.cfg.popups)),
            (
                "Ширина списка".into(),
                if self.cfg.sidebar_width == 0 { "авто".into() } else { self.cfg.sidebar_width.to_string() },
            ),
            ("Восстанавливать агентов".into(), onoff(self.cfg.restore)),
            ("Выделение мышью (копирование)".into(), onoff(self.cfg.mouse_select)),
        ]
    }

    /// Меняет настройку: dir = +1 / -1 (для переключателей направление не важно).
    pub(super) fn settings_change(&mut self, row: usize, dir: i32) {
        fn cycle<T: PartialEq + Clone>(list: &[T], cur: &T, dir: i32) -> T {
            let n = list.len() as i32;
            let i = list.iter().position(|x| x == cur).unwrap_or(0) as i32;
            list[((i + dir).rem_euclid(n)) as usize].clone()
        }
        match row {
            0 => {
                let names: Vec<String> = crate::theme::names().iter().map(std::string::ToString::to_string).collect();
                self.cfg.theme_name = cycle(&names, &self.cfg.theme_name, dir);
                self.theme = self.cfg.theme();
            }
            1 => {
                self.notifications = !self.notifications;
                self.cfg.notifications = self.notifications;
            }
            2 => self.cfg.sound = !self.cfg.sound,
            3 => {
                let names: Vec<String> = notify::theme_names().iter().map(std::string::ToString::to_string).collect();
                self.cfg.sound_theme = cycle(&names, &self.cfg.sound_theme, dir);
                notify::preview(&self.notify_settings());
            }
            4 => {
                self.cfg.volume = (self.cfg.volume + 0.1 * dir as f32).clamp(0.0, 1.0);
                self.cfg.volume = (self.cfg.volume * 10.0).round() / 10.0;
                notify::preview(&self.notify_settings());
            }
            5 => self.cfg.popups = !self.cfg.popups,
            6 => {
                self.cfg.sidebar_width = cycle(&[0u16, 26, 32, 40, 48], &self.cfg.sidebar_width, dir);
            }
            7 => self.cfg.restore = !self.cfg.restore,
            8 => {
                self.cfg.mouse_select = !self.cfg.mouse_select;
                self.sel = None;
                self.press = None;
            }
            _ => {}
        }
        self.cfg.save_state();
        self.dirty = true;
    }

    pub(super) fn key_settings(&mut self, mut sel: usize, k: KeyEvent) {
        let n = self.settings_rows().len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => return,
            KeyCode::Tab | KeyCode::BackTab => {
                self.mode = Mode::Integrations(0);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1) % n,
            KeyCode::Up | KeyCode::Char('k') => sel = (sel + n - 1) % n,
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => self.settings_change(sel, 1),
            KeyCode::Left | KeyCode::Char('h') => self.settings_change(sel, -1),
            _ => {}
        }
        self.mode = Mode::Settings(sel);
    }

    pub(super) fn mouse_settings(&mut self, sel: usize, x: u16, y: u16) {
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        let (popup, rows) = ui::settings_layout(area, self.settings_rows().len());
        self.mode = Mode::Settings(sel);
        if !in_rect(&popup, x, y) {
            self.mode = Mode::Normal;
            return;
        }
        if in_rect(&ui::tab_rects(popup)[1], x, y) {
            self.mode = Mode::Integrations(0);
            return;
        }
        if let Some(i) = rows.iter().position(|r| in_rect(r, x, y)) {
            // левая половина строки — назад, правая — вперёд
            let mid = rows[i].x + rows[i].width / 2;
            self.mode = Mode::Settings(i);
            self.settings_change(i, if x < mid && !matches!(i, 1 | 2 | 5 | 7) { -1 } else { 1 });
        }
    }

    /// Строки раздела «Интеграции»: все агенты, кроме обычного шелла.
    pub fn integration_rows(&self) -> Vec<IntegrationRow> {
        use crate::integrations as ig;
        self.cfg
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.kind != Kind::Shell)
            .map(|(i, a)| {
                let id = ig::key(&a.command);
                IntegrationRow {
                    name: a.name.clone(),
                    state: ig::supported(&id).then(|| ig::state(&id)),
                    found: self.agent_available(i),
                    path: if ig::supported(&id) {
                        ig::describe(&id)
                    } else {
                        "интеграции нет: статусы определяются по экрану".into()
                    },
                    id,
                }
            })
            .collect()
    }

    pub(super) fn integration_toggle(&mut self, row: usize) {
        use crate::integrations as ig;
        let rows = self.integration_rows();
        let Some(r) = rows.get(row) else { return };
        match r.state {
            None => self.toast(format!("Для «{}» интеграции пока нет", r.name)),
            Some(ig::State::Builtin) => self.toast(format!("{}: интеграция встроена и всегда включена", r.name)),
            Some(ig::State::Installed) => match ig::uninstall(&r.id) {
                Ok(()) => self.toast(format!("{}: интеграция выключена", r.name)),
                Err(e) => self.toast(format!("Ошибка: {e}")),
            },
            Some(ig::State::NotInstalled) => match ig::install(&r.id) {
                Ok(_) => self.toast(format!("{}: интеграция включена — перезапустите агента", r.name)),
                Err(e) => self.toast(format!("Ошибка: {e}")),
            },
        }
        self.dirty = true;
    }

    pub(super) fn key_integrations(&mut self, mut sel: usize, k: KeyEvent) {
        let n = self.integration_rows().len().max(1);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => return,
            KeyCode::Tab | KeyCode::BackTab => {
                self.mode = Mode::Settings(0);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1) % n,
            KeyCode::Up | KeyCode::Char('k') => sel = (sel + n - 1) % n,
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Left => self.integration_toggle(sel),
            _ => {}
        }
        self.mode = Mode::Integrations(sel);
    }

    pub(super) fn mouse_integrations(&mut self, sel: usize, x: u16, y: u16) {
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        let (popup, rects) = ui::integrations_layout(area, self.integration_rows().len());
        self.mode = Mode::Integrations(sel);
        if !in_rect(&popup, x, y) {
            self.mode = Mode::Normal;
        } else if in_rect(&ui::tab_rects(popup)[0], x, y) {
            self.mode = Mode::Settings(0);
        } else if let Some(i) = rects.iter().position(|r| in_rect(r, x, y)) {
            self.mode = Mode::Integrations(i);
            self.integration_toggle(i);
        }
    }
}
