//! Жизненный цикл сессий: создание, подключение к фоновым агентам, восстановление, сохранение, закрытие.

use super::App;
use crate::config::AgentDef;
use crate::session::Session;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

impl App {
    pub(super) fn pane_size(&self) -> (u16, u16) {
        let m = self.geo.main;
        if m.width > 4 && m.height > 4 {
            (m.height.saturating_sub(1), m.width)
        } else {
            (30, 100)
        }
    }

    pub fn create_session(&mut self, def: AgentDef, dir: PathBuf, name: Option<String>, worktree: bool) -> Result<()> {
        self.create_session_with(def, dir, name, worktree, None)
    }

    pub(super) fn create_session_with(
        &mut self,
        def: AgentDef,
        dir: PathBuf,
        name: Option<String>,
        worktree: bool,
        resume: Option<&str>,
    ) -> Result<()> {
        if !dir.is_dir() {
            return Err(anyhow!("папка не найдена: {}", dir.display()));
        }
        let (cwd, wt) = if worktree {
            let p = crate::gitops::add_worktree(&dir, &def.id).map_err(|e| anyhow!(e))?;
            (p.clone(), Some(p))
        } else {
            (dir, None)
        };
        let base = name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| cwd.file_name().map_or_else(|| def.name.clone(), |s| s.to_string_lossy().to_string()));
        // одинаковые имена (несколько агентов в одной папке) различаем номером
        let mut name = base.clone();
        let mut n = 2;
        while self.sessions.iter().any(|s| s.name == name) {
            name = format!("{base} #{n}");
            n += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        let s = Session::spawn(id, &def, name, cwd, wt, self.pane_size(), self.tx.clone(), &self.ctx, resume)?;
        let (sid, agent_name, sname) = (s.id, s.agent.clone(), s.name.clone());
        self.sessions.push(s);
        self.events.push(sid, agent_name, crate::events::Kind::Started, format!("{sname}: запущен"));
        self.selected = self.sessions.len() - 1;
        self.normalize_groups(sid);
        self.dirty = true;
        Ok(())
    }

    /// Группы, свёрнутость и выбранный агент из сохранённого списка.
    pub(super) fn apply_saved_layout(&mut self, saved: &crate::persist::File) {
        self.groups = saved.groups.clone();
        self.collapsed = saved.collapsed.iter().cloned().collect();
        if !self.sessions.is_empty() {
            self.selected = saved.selected.min(self.sessions.len() - 1);
            let id = self.sessions[self.selected].id;
            self.normalize_groups(id);
        }
    }

    /// Подключается к агентам, которые продолжали работать в фоне после закрытия Radar.
    /// Возвращает, сколько фоновых агентов найдено (включая занятых другим окном).
    pub fn attach_existing(&mut self) -> usize {
        let socks = crate::host::live_sockets();
        let (mut busy, mut unknown) = (0, 0);
        let mut found: Vec<Session> = vec![];
        for sock in &socks {
            match crate::host::attach(sock) {
                Ok(Some((meta, stream))) => {
                    let Some(def) = self.cfg.agents.iter().find(|a| a.name == meta.agent).cloned() else {
                        unknown += 1;
                        continue; // агента убрали из конфига — не трогаем
                    };
                    if let Ok(s) = Session::from_host(&def, meta, stream, self.tx.clone()) {
                        found.push(s);
                    }
                }
                Ok(None) => busy += 1,
                Err(_) => {}
            }
        }
        // порядок — как в прошлый раз (сохранённый список), новые агенты в конец
        let saved = crate::persist::load();
        found.sort_by_key(|s| saved.sessions.iter().position(|sv| sv.name == s.name).unwrap_or(usize::MAX));
        for mut s in found {
            self.next_id = self.next_id.max(s.id + 1);
            s.group = saved.sessions.iter().find(|sv| sv.name == s.name).and_then(|sv| sv.group.clone());
            self.sessions.push(s);
        }
        self.apply_saved_layout(&saved);
        if busy > 0 {
            self.toast(format!("Агентов в другом окне Radar: {busy} — они здесь не показаны"));
        } else if unknown > 0 {
            self.toast(format!(
                "Фоновых агентов не из этого конфига: {unknown} — они продолжают работать (radar stop — остановить)"
            ));
        }
        self.dirty = true;
        socks.len()
    }

    /// Поднимает агентов, сохранённых при прошлом закрытии Radar.
    pub fn restore_sessions(&mut self) {
        if !self.cfg.restore {
            return;
        }
        let saved = crate::persist::load();
        let mut failed = 0;
        for sv in &saved.sessions {
            let Some(def) = self.cfg.agents.iter().find(|a| a.name == sv.agent).cloned() else {
                failed += 1;
                continue;
            };
            let dir = PathBuf::from(&sv.dir);
            match self.create_session_with(def, dir, Some(sv.name.clone()), false, sv.resume.as_deref()) {
                Ok(()) => {
                    if let Some(s) = self.sessions.last_mut() {
                        s.muted = sv.muted;
                        s.worktree = sv.worktree.as_ref().map(PathBuf::from);
                        s.group = sv.group.clone();
                    }
                }
                Err(_) => failed += 1,
            }
        }
        self.apply_saved_layout(&saved);
        if failed > 0 {
            self.toast(format!("Не удалось восстановить агентов: {failed} (папка удалена или агент убран из конфига)"));
        }
        self.persisted = crate::persist::to_text(&self.snapshot());
    }

    pub(super) fn snapshot(&self) -> crate::persist::File {
        let mut sessions = vec![];
        let mut selected = 0;
        for (i, s) in self.sessions.iter().enumerate() {
            if !s.is_running() {
                continue; // агент завершился сам — не возвращаем
            }
            if i == self.selected {
                selected = sessions.len();
            }
            sessions.push(crate::persist::Saved {
                agent: s.agent.clone(),
                dir: s.cwd.to_string_lossy().to_string(),
                name: s.name.clone(),
                worktree: s.worktree.as_ref().map(|p| p.to_string_lossy().to_string()),
                muted: s.muted,
                resume: s.resume_id.clone(),
                group: s.group.clone(),
            });
        }
        let mut collapsed: Vec<String> = self.collapsed.iter().cloned().collect();
        collapsed.sort();
        crate::persist::File { selected, sessions, groups: self.groups.clone(), collapsed }
    }

    /// Записывает список агентов, если он изменился (вызывается из главного цикла).
    pub(super) fn persist_sessions(&mut self) {
        if !self.cfg.restore {
            return;
        }
        let text = crate::persist::to_text(&self.snapshot());
        if text != self.persisted {
            crate::persist::save_text(&text);
            self.persisted = text;
        }
    }

    pub(super) fn restart(&mut self, idx: usize) {
        let Some(old) = self.sessions.get(idx) else {
            return;
        };
        if old.is_running() {
            self.toast("Агент ещё работает — сначала закройте его");
            return;
        }
        let Some(def) = self.cfg.agents.iter().find(|a| a.name == old.agent).cloned() else {
            return;
        };
        let (cwd, name, wt, muted) = (old.cwd.clone(), old.name.clone(), old.worktree.clone(), old.muted);
        let resume = old.resume_id.clone();
        let id = self.next_id;
        self.next_id += 1;
        match Session::spawn(id, &def, name, cwd, wt, self.pane_size(), self.tx.clone(), &self.ctx, resume.as_deref()) {
            Ok(mut s) => {
                s.muted = muted;
                let mut old = std::mem::replace(&mut self.sessions[idx], s);
                old.kill(); // освобождает хозяина завершившегося агента
            }
            Err(e) => self.toast(format!("Не удалось перезапустить: {e}")),
        }
        self.dirty = true;
    }

    pub(super) fn close(&mut self, idx: usize) {
        if idx >= self.sessions.len() {
            return;
        }
        let mut s = self.sessions.remove(idx);
        s.kill();
        if self.selected >= self.sessions.len() {
            self.selected = self.sessions.len().saturating_sub(1);
        }
        self.dirty = true;
    }

    pub fn shutdown(&mut self) {
        self.persist_sessions(); // последнее состояние до остановки агентов
        for s in &mut self.sessions {
            if self.stop_on_quit {
                s.kill();
            } else {
                s.sync_meta();
                s.detach();
            }
        }
    }

    pub fn select(&mut self, idx: usize) {
        if idx < self.sessions.len() {
            self.selected = idx;
            self.sessions[idx].unread = false;
            // выбранный агент не должен прятаться в свёрнутой группе
            if let Some(g) = self.sessions[idx].group.clone() {
                self.collapsed.remove(&g);
            }
            self.dirty = true;
        }
    }
}
