//! Клавиатура: режимы, навигация и диалоги.

use super::{form_complete, App, Confirm, GroupForm, Mode, NewForm, Palette};
use crate::input::key_to_bytes;
use crate::keys::nav_action;
use crate::menu::Menu;
use crate::textfield::TextField;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use std::time::Instant;

impl App {
    pub fn on_event(&mut self, ev: Event) {
        if !matches!(&ev, Event::Mouse(m) if m.kind == MouseEventKind::Moved) {
            self.dirty = true;
        }
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                self.last_key = Instant::now();
                if !matches!(self.mode, Mode::Menu(_)) {
                    self.sel = None;
                }
                self.on_key(k);
            }
            Event::Paste(text) => {
                self.last_key = Instant::now();
                match &mut self.mode {
                    Mode::Normal | Mode::Nav(_) => {
                        self.mode = Mode::Normal;
                        if let Some(s) = self.sessions.get_mut(self.selected) {
                            s.paste(&text);
                        }
                    }
                    Mode::New(f) => match f.field {
                        1 => f.dir.insert_str(text.trim()),
                        2 => f.name.insert_str(text.trim()),
                        _ => {}
                    },
                    Mode::Rename(t) => t.insert_str(text.trim()),
                    Mode::GroupForm(f) if f.focus == 0 => f.name.insert_str(text.trim()),
                    Mode::Palette(p) => {
                        p.input.insert_str(text.trim());
                        p.sel = 0;
                    }
                    _ => {}
                }
            }
            Event::Mouse(m) => self.on_mouse(m),
            Event::FocusGained => {
                self.focus_supported = true;
                self.term_focused = true;
                if let Some(s) = self.sessions.get_mut(self.selected) {
                    s.unread = false;
                }
            }
            Event::FocusLost => {
                self.focus_supported = true;
                self.term_focused = false;
            }
            _ => {}
        }
    }

    pub(super) fn on_key(&mut self, k: KeyEvent) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        match mode {
            Mode::Normal => self.key_normal(k),
            Mode::Nav(_) => self.key_nav(k),
            Mode::New(f) => self.key_new(f, k),
            Mode::Rename(t) => self.key_rename(t, k),
            Mode::GroupForm(f) => self.key_group_form(f, k),
            Mode::Confirm(c) => self.key_confirm(c, k),
            Mode::Menu(m) => self.key_menu(m, k),
            Mode::Palette(p) => self.key_palette(p, k),
            Mode::Settings(i) => self.key_settings(i, k),
            Mode::Integrations(i) => self.key_integrations(i, k),
            Mode::Diff(v) => self.key_diff(v, k),
            Mode::Log(i) => self.key_log(i, k),
            Mode::Commit(t) => self.key_commit(t, k),
            Mode::Help => {}
        }
    }

    pub(super) fn key_normal(&mut self, k: KeyEvent) {
        if self.cfg.keys.prefix.matches(&k) {
            self.mode = Mode::Nav(Instant::now());
            return;
        }
        if let Some(a) = self.cfg.keys.direct_action(&k) {
            self.run_action(a);
            return;
        }
        if self.sessions.is_empty() {
            match k.code {
                KeyCode::Char('n') | KeyCode::Enter => self.open_new_form(None),
                KeyCode::Char('q') => self.quit = true,
                KeyCode::Char('?') => self.mode = Mode::Help,
                KeyCode::Char('p') | KeyCode::Char(':') => self.open_palette(),
                _ => {}
            }
            return;
        }
        let Some(s) = self.sessions.get_mut(self.selected) else {
            return;
        };
        s.unread = false;
        let Some(bytes) = key_to_bytes(k, s.app_cursor()) else {
            return;
        };
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let typed = match k.code {
            KeyCode::Char(c) if plain => Some(c),
            _ => None,
        };
        let submit = k.code == KeyCode::Enter && !k.modifiers.contains(KeyModifiers::ALT);
        let backspace = k.code == KeyCode::Backspace;
        s.send_input(&bytes, typed, submit, backspace);
    }

    /// Режим навигации: остаётся включённым, пока не нажат Esc/Enter или действие, открывающее окно.
    pub(super) fn key_nav(&mut self, k: KeyEvent) {
        if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
            return;
        }
        // повторный префикс — отправить его агенту как обычную клавишу
        if self.cfg.keys.prefix.matches(&k) {
            if let Some(s) = self.sessions.get_mut(self.selected) {
                if let Some(b) = key_to_bytes(k, s.app_cursor()) {
                    s.send_raw(&b);
                }
            }
            return;
        }
        if let Some((a, stay)) = nav_action(&k) {
            self.run_action(a);
            if stay && matches!(self.mode, Mode::Normal) {
                self.mode = Mode::Nav(Instant::now());
            }
        }
    }

    pub(super) fn key_menu(&mut self, mut m: Menu, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {}
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                m.step(1);
                self.mode = Mode::Menu(m);
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                m.step(-1);
                self.mode = Mode::Menu(m);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(a) = m.current() {
                    self.run_action(a);
                }
            }
            _ => self.mode = Mode::Menu(m),
        }
    }

    pub(super) fn key_palette(&mut self, mut p: Palette, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let n = p.matches().len();
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                if let Some(&i) = p.matches().get(p.sel) {
                    let a = p.entries[i].action;
                    self.run_action(a);
                    return;
                }
            }
            KeyCode::Down => p.sel = if n == 0 { 0 } else { (p.sel + 1) % n },
            KeyCode::Up => p.sel = if n == 0 { 0 } else { (p.sel + n - 1) % n },
            KeyCode::Char('n' | 'j') if ctrl => p.sel = if n == 0 { 0 } else { (p.sel + 1) % n },
            KeyCode::Char('p' | 'k') if ctrl => p.sel = if n == 0 { 0 } else { (p.sel + n - 1) % n },
            _ => {
                if p.input.handle_key(&k) {
                    p.sel = 0;
                }
            }
        }
        self.mode = Mode::Palette(p);
    }

    pub(super) fn key_confirm(&mut self, c: Confirm, k: KeyEvent) {
        if let Confirm::Approve(i, note, sel) = c {
            let n = self.sessions.get(i).map_or(0, |s| crate::session::option_lines(&s.prompt_excerpt()).0.len());
            let keep =
                |app: &mut Self, sel: Option<usize>| app.mode = Mode::Confirm(Confirm::Approve(i, note.clone(), sel));
            match k.code {
                KeyCode::Down | KeyCode::Char('j') if n > 0 => keep(self, Some((sel.unwrap_or(0) + 1).min(n - 1))),
                KeyCode::Up | KeyCode::Char('k') if n > 0 => keep(self, Some(sel.unwrap_or(0).saturating_sub(1))),
                KeyCode::Char(d @ '1'..='9') if (d as usize - '0' as usize) <= n => {
                    keep(self, Some(d as usize - '1' as usize))
                }
                KeyCode::Enter | KeyCode::Char('y' | 'Y' | 'н' | 'Н') => self.approve_run(i, note, sel),
                _ => {}
            }
            return;
        }
        if matches!(k.code, KeyCode::Char('y' | 'Y' | 'н' | 'Н') | KeyCode::Enter) {
            match c {
                Confirm::Approve(..) => {}
                Confirm::Close(i) => self.close(i),
                Confirm::DeleteGroup(i) => self.delete_group(i),
                Confirm::Quit => self.quit = true,
                Confirm::QuitStop => {
                    self.stop_on_quit = true;
                    self.quit = true;
                }
                Confirm::Push(i) => self.git_push_run(i),
                Confirm::Merge(i) => self.git_merge_run(i),
                Confirm::RemoveWorktree(i) => self.git_remove_run(i),
            }
        }
    }

    pub(super) fn key_rename(&mut self, mut t: TextField, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
                let name = t.text();
                if let Some(sess) = self.sessions.get_mut(self.selected) {
                    if !name.trim().is_empty() {
                        sess.name = name.trim().to_string();
                    }
                }
            }
            _ => {
                t.handle_key(&k);
                self.mode = Mode::Rename(t);
            }
        }
    }

    pub(super) fn key_group_form(&mut self, mut f: GroupForm, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Enter => match self.save_group_form(&f) {
                Ok(()) => return,
                Err(e) => self.toast(e),
            },
            KeyCode::Tab | KeyCode::BackTab => f.focus = 1 - f.focus,
            KeyCode::Down if f.focus == 0 => f.focus = 1,
            KeyCode::Up if f.focus == 1 && f.cursor == 0 => f.focus = 0,
            KeyCode::Up if f.focus == 1 => f.cursor -= 1,
            KeyCode::Down if f.focus == 1 => f.cursor = (f.cursor + 1).min(f.ids.len().saturating_sub(1)),
            KeyCode::Char(' ') if f.focus == 1 => {
                if let Some(c) = f.checked.get_mut(f.cursor) {
                    *c = !*c;
                }
            }
            _ if f.focus == 0 => {
                f.name.handle_key(&k);
            }
            _ => {}
        }
        self.mode = Mode::GroupForm(f);
    }

    pub(super) fn key_new(&mut self, mut f: NewForm, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                let def = self.cfg.agents[f.agent].clone();
                let dir = crate::paths::expand_tilde(f.dir.text().trim());
                let name = Some(f.name.text());
                match self.create_session(def, dir, name, f.worktree) {
                    Ok(()) => return,
                    Err(e) => f.error = Some(e.to_string()),
                }
            }
            KeyCode::Right | KeyCode::End if f.field == 1 && f.dir.cursor() == f.dir.text().chars().count() => {
                // курсор в конце: → принимает серую подсказку
                f.error = None;
                if let Some(s) = crate::complete::suggest(&f.dir.text()) {
                    f.dir = TextField::new(&s);
                    f.hints.clear();
                }
            }
            KeyCode::Tab if f.field == 1 => {
                f.error = None;
                form_complete(&mut f);
            }
            KeyCode::Tab | KeyCode::Down => f.field = (f.field + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => f.field = (f.field + 3) % 4,
            _ => {
                f.error = None;
                f.hints.clear();
                match f.field {
                    0 => {
                        let vis = self.visible_agents(f.show_all);
                        let pos = vis.iter().position(|&i| i == f.agent).unwrap_or(0);
                        match k.code {
                            KeyCode::Left if !vis.is_empty() => f.agent = vis[(pos + vis.len() - 1) % vis.len()],
                            KeyCode::Right if !vis.is_empty() => f.agent = vis[(pos + 1) % vis.len()],
                            KeyCode::Char('a' | 'ф') => {
                                f.show_all = !f.show_all;
                                let vis = self.visible_agents(f.show_all);
                                if !vis.contains(&f.agent) {
                                    f.agent = vis.first().copied().unwrap_or(0);
                                }
                            }
                            KeyCode::Char(c) => {
                                if let Some(d) = c.to_digit(10).map(|d| d as usize) {
                                    if (1..=vis.len()).contains(&d) {
                                        f.agent = vis[d - 1];
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    1 => {
                        f.dir.handle_key(&k);
                    }
                    2 => {
                        f.name.handle_key(&k);
                    }
                    _ => {
                        if matches!(k.code, KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right) {
                            f.worktree = !f.worktree;
                        }
                    }
                }
            }
        }
        self.mode = Mode::New(f);
    }
}
