//! Команды `radar ctl` от внешних процессов.

use super::App;
use std::path::PathBuf;

impl App {
    pub(super) fn ctl_info(&self, i: usize) -> serde_json::Value {
        let s = &self.sessions[i];
        serde_json::json!({
            "id": s.id,
            "name": s.name,
            "agent": s.agent,
            "status": crate::ctl::status_key(s.status),
            "note": s.note,
            "cwd": s.cwd,
            "branch": s.git.as_ref().map(|g| g.branch.clone()).unwrap_or_default(),
            "worktree": s.worktree.is_some(),
            "selected": i == self.selected,
        })
    }

    /// Команда `radar ctl` от внешнего процесса. Выполняется в главном цикле.
    pub(super) fn handle_ctl(&mut self, req: &serde_json::Value) -> serde_json::Value {
        use serde_json::json;
        let err = |m: String| json!({"ok": false, "error": m});
        let cmd = req["ctl"].as_str().unwrap_or("");
        if cmd == "list" {
            let all: Vec<_> = (0..self.sessions.len()).map(|i| self.ctl_info(i)).collect();
            return json!({"ok": true, "sessions": all});
        }
        if cmd == "new" {
            let name_of = req["agent"].as_str().unwrap_or("");
            let Some(def) = self.cfg.find(name_of).cloned() else {
                return err(format!("неизвестный агент «{name_of}» (см. radar doctor)"));
            };
            let dir = PathBuf::from(req["dir"].as_str().unwrap_or("."));
            let name = req["name"].as_str().map(String::from);
            let prev = (!self.sessions.is_empty()).then_some(self.selected);
            return match self.create_session(def, dir, name, req["worktree"].as_bool().unwrap_or(false)) {
                Ok(()) => {
                    if let Some(p) = prev {
                        self.selected = p; // запуск из скрипта не отнимает фокус у человека
                    }
                    json!({"ok": true, "session": self.ctl_info(self.sessions.len() - 1)})
                }
                Err(e) => err(e.to_string()),
            };
        }
        let items: Vec<(u32, String)> = self.sessions.iter().map(|s| (s.id, s.name.clone())).collect();
        let id = match crate::ctl::resolve(&items, req["target"].as_str().unwrap_or("")) {
            Ok(id) => id,
            Err(e) => return err(e),
        };
        let Some(i) = self.sessions.iter().position(|s| s.id == id) else {
            return err("агент не найден".into());
        };
        self.dirty = true;
        match cmd {
            "status" => json!({"ok": true, "session": self.ctl_info(i)}),
            "read" => {
                let n = req["lines"].as_u64().unwrap_or(40) as usize;
                json!({"ok": true, "text": self.sessions[i].screen_text(n)})
            }
            "send" => {
                if !self.sessions[i].is_running() {
                    return err("агент уже завершён".into());
                }
                let text = req["text"].as_str().unwrap_or("");
                self.sessions[i].paste(text);
                if req["enter"].as_bool().unwrap_or(true) {
                    self.sessions[i].send_input(b"\r", None, true, false);
                }
                json!({"ok": true})
            }
            "close" => {
                self.close(i);
                json!({"ok": true})
            }
            other => err(format!("неизвестная команда «{other}»")),
        }
    }
}
