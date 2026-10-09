//! Статусы агентов, уведомления и разбор сообщений фоновых потоков.

use super::{App, Mode};
use crate::notify::{self, Sound};
use crate::session::{Attention, Msg};
use crate::status::Status;
use std::time::Duration;

impl App {
    pub(super) fn is_viewing(&self, idx: usize) -> bool {
        let focused =
            if self.focus_supported { self.term_focused } else { self.last_key.elapsed() < Duration::from_secs(8) };
        focused && self.visible_indices().contains(&idx)
    }

    pub(super) fn handle_attention(&mut self, idx: usize, a: Attention) {
        let viewing = self.is_viewing(idx);
        if a == Attention::Done {
            self.git.poke(); // агент закончил — быстро обновить git-состояние
        }
        let Some(s) = self.sessions.get_mut(idx) else {
            return;
        };
        if !viewing {
            s.unread = true;
        }
        let (title, verb, sound) = match a {
            Attention::NeedsInput => ("Нужно подтверждение", "ждёт ответа", Sound::Waiting),
            Attention::Done => ("Задача выполнена", "закончил работу", Sound::Done),
        };
        let body = format!("{} · {}", s.agent, s.name);
        {
            use crate::events::{fmt_dur, Kind};
            let (kind, text) = match a {
                Attention::NeedsInput if s.note.is_empty() => (Kind::Waiting, "ждёт ответа".to_string()),
                Attention::NeedsInput => (Kind::Waiting, format!("ждёт ответа: {}", s.note)),
                Attention::Done => match s.last_worked {
                    Some(d) => (Kind::Done, format!("закончил за {}", fmt_dur(d.as_secs()))),
                    None => (Kind::Done, "закончил работу".to_string()),
                },
            };
            let (sid, ag, nm) = (s.id, s.agent.clone(), s.name.clone());
            self.events.push(sid, ag, kind, format!("{nm}: {text}"));
        }
        let toast = format!("{} {} — {}", s.agent, s.name, verb);
        let muted = s.muted;
        self.toast(toast);
        if !viewing && self.notifications && !muted {
            notify::notify(sound, title, &body, &self.notify_settings());
        }
    }

    pub fn tick(&mut self) {
        self.git.set_targets(self.sessions.iter().filter(|s| s.is_running()).map(|s| (s.id, s.cwd.clone())).collect());
        for i in 0..self.sessions.len() {
            self.sessions[i].sync_meta();
            let before = self.sessions[i].status;
            if let Some(a) = self.sessions[i].tick() {
                self.handle_attention(i, a);
            }
            if before != self.sessions[i].status {
                self.dirty = true;
            }
        }
        if let Some((_, t)) = &self.toast {
            if t.elapsed() > Duration::from_secs(5) {
                self.toast = None;
                self.dirty = true;
            }
        }
        if let Mode::Nav(since) = self.mode {
            let t = self.cfg.keys.nav_timeout;
            if t > 0 && since.elapsed() > Duration::from_secs(t) {
                self.mode = Mode::Normal;
                self.dirty = true;
            }
        }
        self.persist_sessions();
    }

    pub fn needs_animation(&self) -> bool {
        self.sessions.iter().any(|s| matches!(s.status, Status::Working | Status::Waiting | Status::Starting))
    }

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Output(id) => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
                    s.on_output();
                    self.dirty = true;
                }
            }
            Msg::GitDone { label, result } => {
                let one_line = |s: &str| s.lines().take(2).collect::<Vec<_>>().join(" · ");
                match result {
                    Ok(m) => self.toast(format!("{label}: {}", one_line(&m))),
                    Err(e) => self.toast(format!("{label}: ошибка — {}", one_line(&e))),
                }
                if let Some((t, _)) = self.toast.clone() {
                    self.events.push(0, "", crate::events::Kind::Git, t);
                }
                self.git.poke();
                self.dirty = true;
            }
            Msg::Git(id, info) => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
                    if s.git != info {
                        s.git = info;
                        self.dirty = true;
                    }
                }
            }
            Msg::Exited(id, code) => {
                if let Some(i) = self.sessions.iter().position(|s| s.id == id) {
                    {
                        let s = &self.sessions[i];
                        let (sid, ag, nm) = (s.id, s.agent.clone(), s.name.clone());
                        self.events.push(sid, ag, crate::events::Kind::Exited, format!("{nm}: завершён (код {code})"));
                    }
                    if let Some(a) = self.sessions[i].mark_exited(code) {
                        self.handle_attention(i, a);
                    }
                    if !self.is_viewing(i) {
                        self.sessions[i].unread = true;
                    }
                    self.dirty = true;
                }
            }
            Msg::Ctl { req, reply } => {
                let _ = reply.send(self.handle_ctl(&req));
            }
            Msg::Hook { session, event, payload } => {
                if let Some(i) = self.sessions.iter().position(|s| s.id == session) {
                    if let Some(a) = self.sessions[i].apply_hook(&event, &payload) {
                        self.handle_attention(i, a);
                    }
                    self.dirty = true;
                }
            }
        }
    }
}
