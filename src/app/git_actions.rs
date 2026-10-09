//! Git-действия над папкой агента: коммит, push, слияние, удаление worktree.

use super::{App, Confirm, Mode};
use crate::keys::Action;
use crate::session::Msg;
use crate::textfield::TextField;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    /// Выполняет git-действие в фоне; результат придёт как `Msg::GitDone`.
    pub(super) fn spawn_git(&self, label: &str, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let tx = self.tx.clone();
        let label = label.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::GitDone { label, result: job() });
        });
    }

    pub(super) fn git_commit_open(&mut self) {
        let Some(s) = self.sessions.get(self.selected) else { return };
        match &s.git {
            None => self.toast("Папка агента не git-репозиторий"),
            Some(g) if !g.dirty() => self.toast("Нечего коммитить: изменений нет"),
            Some(_) => {
                // подсказка — последний запрос к агенту
                let hint: String = s.subtitle.lines().next().unwrap_or("").chars().take(72).collect();
                self.mode = Mode::Commit(TextField::new(hint.trim()));
            }
        }
    }

    pub(super) fn key_commit(&mut self, mut t: TextField, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
                let msg = t.text();
                if msg.trim().is_empty() {
                    self.toast("Введите сообщение коммита");
                    self.mode = Mode::Commit(t);
                    return;
                }
                if let Some(s) = self.sessions.get(self.selected) {
                    let dir = s.cwd.clone();
                    self.toast("Коммит…");
                    self.spawn_git("Коммит", move || crate::gitops::commit(&dir, &msg));
                }
            }
            _ => {
                t.handle_key(&k);
                self.mode = Mode::Commit(t);
            }
        }
    }

    /// Подтверждение опасных/внешних git-действий.
    pub(super) fn git_confirm(&mut self, a: Action) {
        let i = self.selected;
        let Some(s) = self.sessions.get(i) else { return };
        if s.git.is_none() {
            self.toast("Папка агента не git-репозиторий");
            return;
        }
        match a {
            Action::GitPush => self.mode = Mode::Confirm(Confirm::Push(i)),
            Action::GitMerge | Action::GitRemoveWorktree if s.worktree.is_none() => {
                self.toast("Это не worktree-агент (worktree включается галочкой при создании агента)");
            }
            Action::GitMerge => self.mode = Mode::Confirm(Confirm::Merge(i)),
            Action::GitRemoveWorktree => self.mode = Mode::Confirm(Confirm::RemoveWorktree(i)),
            _ => {}
        }
    }

    pub(super) fn git_push_run(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        let dir = s.cwd.clone();
        self.toast("Отправка…");
        self.spawn_git("Push", move || crate::gitops::push(&dir));
    }

    pub(super) fn git_merge_run(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        let (Some(wt), Some(g)) = (s.worktree.clone(), s.git.clone()) else { return };
        self.toast("Слияние…");
        self.spawn_git("Слияние", move || crate::gitops::merge_into_main(&wt, &g.branch));
    }

    pub(super) fn git_remove_run(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        let (Some(wt), Some(g)) = (s.worktree.clone(), s.git.clone()) else { return };
        self.close(i); // агент работает внутри worktree — сначала останавливаем
        self.toast("Удаление worktree…");
        self.spawn_git("Worktree", move || crate::gitops::remove_worktree(&wt, &g.branch));
    }
}
