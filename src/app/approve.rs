//! Разрешение запросов агента из списка.

use super::{App, Confirm, Mode};
use crate::status::Status;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    /// «Разрешить» из списка: всегда через диалог с текстом запроса, вслепую ничего не подтверждаем.
    pub fn can_approve(&self, i: usize) -> bool {
        self.approve_state(i).is_ok()
    }

    /// Агенты, которые ждут ответа и которым можно ответить из списка (есть текст запроса).
    pub(super) fn approvable(&self) -> Vec<usize> {
        (0..self.sessions.len()).filter(|&i| self.approve_state(i).is_ok()).collect()
    }

    /// Можно ли разрешить запрос агента `i`; иначе — почему нельзя.
    pub(super) fn approve_state(&self, i: usize) -> Result<String, String> {
        let s = self.sessions.get(i).ok_or("Нет агента")?;
        if s.status != Status::Waiting {
            return Err("Агент ничего не просит: он не в статусе «ждёт ответа»".into());
        }
        let enabled = self.cfg.agents.iter().find(|d| d.name == s.agent).is_some_and(|d| !d.approve.is_empty());
        if !enabled {
            return Err(format!("Для «{}» подтверждение из списка не включено (approve в config.toml)", s.agent));
        }
        if s.note.is_empty() {
            return Err("Текст запроса неизвестен — откройте агента и ответьте в его окне (нужна интеграция)".into());
        }
        Ok(s.note.clone())
    }

    /// `Ctrl+b y`: выбранный агент, если он ждёт, иначе ближайший ждущий из остальных.
    pub(super) fn approve_open(&mut self) {
        let n = self.sessions.len();
        if n == 0 {
            self.toast("Нет агента");
            return;
        }
        if self.approve_state(self.selected).is_ok() {
            return self.approve_open_at(self.selected);
        }
        let waiting = (1..=n).map(|d| (self.selected + d) % n).find(|&i| self.approve_state(i).is_ok());
        match waiting {
            Some(i) => self.approve_open_at(i),
            None => {
                // объясняем причину по выбранному агенту (или по любому ждущему)
                let why = self.approve_state(self.selected).err().unwrap_or_default();
                self.toast(why);
            }
        }
    }

    pub(super) fn approve_open_at(&mut self, i: usize) {
        match self.approve_state(i) {
            Ok(note) => {
                let (_, hi) = crate::session::option_lines(&self.sessions[i].prompt_excerpt());
                self.mode = Mode::Confirm(Confirm::Approve(i, note, hi));
            }
            Err(e) => self.toast(e),
        }
    }

    pub(super) fn approve_run(&mut self, i: usize, note: String, sel: Option<usize>) {
        let Some(s) = self.sessions.get(i) else { return };
        // запрос мог смениться, пока открыт диалог, — тогда не подтверждаем то, чего человек не видел
        if s.status != Status::Waiting || s.note != note {
            self.toast("Запрос изменился — откройте диалог ещё раз");
            return;
        }
        let Some(bytes) = self.cfg.agents.iter().find(|d| d.name == s.agent).map(|d| d.approve.clone()) else {
            return;
        };
        let excerpt = s.prompt_excerpt();
        let (opts, hi) = crate::session::option_lines(&excerpt);
        let (sid, ag, nm) = (s.id, s.agent.clone(), s.name.clone());
        let app_cursor = s.app_cursor();
        let mut answer = "разрешено".to_string();
        // выбор варианта стрелками работает там, где «разрешить» — это Enter на выделенном пункте
        if let (Some(sel), Some(hi), true) = (sel, hi, bytes == b"\r") {
            let code = if sel > hi { KeyCode::Down } else { KeyCode::Up };
            let key = KeyEvent::new(code, KeyModifiers::NONE);
            let arrow = crate::input::key_to_bytes(key, app_cursor).unwrap_or_default();
            for _ in 0..sel.abs_diff(hi) {
                self.sessions[i].send_raw(&arrow);
            }
            if let Some(&line) = opts.get(sel) {
                let text =
                    excerpt[line].trim_start_matches(|c: char| c.is_whitespace() || "›❯>●○→".contains(c)).to_string();
                answer = format!("ответ — {text}");
            }
        }
        self.sessions[i].send_raw(&bytes);
        self.events.push(sid, ag, crate::events::Kind::Approved, format!("{nm}: {answer} · {note}"));
        self.toast(format!("{}: {note}", if answer == "разрешено" { "Разрешено".to_string() } else { answer }));
    }
}
