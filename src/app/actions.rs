//! Действия пользователя (`Action`), меню и палитра команд.

use super::{App, Confirm, Mode, NewForm, Palette};
use crate::keys::Action;
use crate::menu::{Menu, MenuItem, PaletteEntry};
use crate::notify::{self};
use crate::status::Status;
use crate::textfield::TextField;
use ratatui::layout::Rect;

impl App {
    pub(super) fn page(&self) -> usize {
        (self.geo.panes.first().map_or(20, |p| p.inner.height) / 2).max(3) as usize
    }

    pub(super) fn open_new_form(&mut self, agent: Option<usize>) {
        let dir = self.sessions.get(self.selected).map_or_else(|| self.start_dir.clone(), |s| s.cwd.clone());
        let last = self.cfg.agents.len().saturating_sub(1);
        let first_visible = self.visible_agents(false).first().copied().unwrap_or(0);
        let chosen = agent.map_or(first_visible, |i| i.min(last));
        // если выбранного агента нет в системе, показываем всех, чтобы выбор был виден
        let show_all = !self.visible_agents(false).contains(&chosen);
        self.mode = Mode::New(NewForm {
            agent: chosen,
            show_all,
            dir: TextField::new(&crate::paths::short_path(&dir)),
            name: TextField::default(),
            worktree: false,
            field: if agent.is_some() { 1 } else { 0 },
            error: None,
            hints: vec![],
        });
    }

    pub(super) fn selected_agent_index(&self) -> Option<usize> {
        let a = &self.sessions.get(self.selected)?.agent;
        self.cfg.agents.iter().position(|d| &d.name == a)
    }

    /// Выполняет действие над выбранным агентом (или над всем интерфейсом).
    pub fn run_action(&mut self, a: Action) {
        self.dirty = true;
        let has = !self.sessions.is_empty();
        match a {
            Action::NewAgent => self.open_new_form(None),
            Action::NewAgentOf(i) => self.open_new_form(Some(i)),
            Action::NewHere => {
                let kind = self.selected_agent_index();
                self.open_new_form(kind);
            }
            Action::Close if has => self.mode = Mode::Confirm(Confirm::Close(self.selected)),
            Action::Rename if has => {
                let name = self.sessions[self.selected].name.clone();
                self.mode = Mode::Rename(TextField::new(&name));
            }
            Action::Restart if has => self.restart(self.selected),
            Action::ToggleMute if has => {
                let s = &mut self.sessions[self.selected];
                s.muted = !s.muted;
                let t = if s.muted {
                    format!("«{}»: уведомления выключены", s.name)
                } else {
                    format!("«{}»: уведомления включены", s.name)
                };
                self.toast(t);
            }
            Action::ToggleGrid => {
                self.grid = !self.grid;
                self.toast(if self.grid { "Режим: сетка" } else { "Режим: один агент" });
            }
            Action::ToggleNotifications => {
                self.notifications = !self.notifications;
                self.cfg.notifications = self.notifications;
                self.cfg.save_state();
                self.toast(if self.notifications {
                    "Уведомления включены"
                } else {
                    "Уведомления выключены"
                });
            }
            Action::ToggleSound => {
                self.cfg.sound = !self.cfg.sound;
                self.cfg.save_state();
                self.toast(if self.cfg.sound {
                    "Звук уведомлений включён"
                } else {
                    "Звук уведомлений выключен"
                });
            }
            Action::TogglePopups => {
                self.cfg.popups = !self.cfg.popups;
                self.cfg.save_state();
                self.toast(if self.cfg.popups {
                    "Всплывающие уведомления включены"
                } else {
                    "Всплывающие уведомления выключены (звук остаётся)"
                });
            }
            Action::SetTheme(i) => {
                if let Some(name) = notify::theme_names().get(i) {
                    self.cfg.sound_theme = name.to_string();
                    self.cfg.save_state();
                    notify::preview(&self.notify_settings());
                    self.toast(format!("Звук: {name}"));
                    self.open_sound_picker();
                }
            }
            Action::PickSound => self.open_sound_picker(),
            Action::Settings => self.mode = Mode::Settings(0),
            Action::Integrations => self.mode = Mode::Integrations(0),
            Action::Diff => self.open_diff(),
            Action::Approve => self.approve_open(),
            Action::ApproveAt(i) => self.approve_open_at(i),
            Action::CopySel => {
                if let Some(sel) = self.sel.take() {
                    let text = self.selection_text(&sel);
                    let n = text.chars().count();
                    if n > 0 {
                        let ok = crate::clipboard::copy(&text);
                        self.toast(if ok {
                            format!("Скопировано: {n} симв.")
                        } else {
                            "Не удалось скопировать".to_string()
                        });
                    }
                }
            }
            Action::Log => {
                self.events.mark_seen();
                self.mode = Mode::Log(0);
            }
            Action::GitCommit if has => self.git_commit_open(),
            Action::GitPush if has => self.git_confirm(Action::GitPush),
            Action::GitMerge if has => self.git_confirm(Action::GitMerge),
            Action::GitRemoveWorktree if has => self.git_confirm(Action::GitRemoveWorktree),
            Action::MoveUp if has => self.move_in_group(self.selected.wrapping_sub(1)),
            Action::MoveDown if has => self.move_in_group(self.selected + 1),
            Action::NewGroup => self.open_group_form(None),
            Action::Group if has => self.mode = Mode::Menu(self.group_menu()),
            Action::Ungroup if has => self.set_group(None),
            Action::MoveToGroup(i) if has => {
                if let Some(g) = self.groups.get(i).cloned() {
                    self.set_group(Some(g));
                }
            }
            Action::EditGroup(i) if i < self.groups.len() => self.open_group_form(Some(i)),
            Action::DeleteGroup(i) if i < self.groups.len() => self.mode = Mode::Confirm(Confirm::DeleteGroup(i)),
            Action::MoveGroupUp(i) => self.move_group(i, false),
            Action::MoveGroupDown(i) => self.move_group(i, true),
            Action::ToggleGroupAt(i) => {
                if let Some(g) = self.groups.get(i).cloned() {
                    self.toggle_group_key(&g);
                }
            }
            Action::ToggleGroup if has => self.toggle_group(),
            Action::Next => self.select_rel(1),
            Action::Prev => self.select_rel(-1),
            Action::Select(i) => self.select(i),
            Action::NextWaiting if has => self.select_next_attention(),
            Action::ScrollUp if has => {
                let p = self.page();
                self.sessions[self.selected].scroll_by(true, p);
            }
            Action::ScrollDown if has => {
                let p = self.page();
                self.sessions[self.selected].scroll_by(false, p);
            }
            Action::Palette => self.open_palette(),
            Action::Help => self.mode = Mode::Help,
            Action::Quit => self.mode = Mode::Confirm(Confirm::Quit),
            Action::QuitStop => self.mode = Mode::Confirm(Confirm::QuitStop),
            _ => {}
        }
    }

    /// Палитра, отфильтрованная по темам звука; текущая тема выделена.
    pub(super) fn open_sound_picker(&mut self) {
        self.open_palette();
        if let Mode::Palette(p) = &mut self.mode {
            p.input = TextField::new("Звук:");
            let cur = format!("Звук: {}", self.cfg.sound_theme);
            let m = p.matches();
            p.sel = m.iter().position(|&i| p.entries[i].title.starts_with(&cur)).unwrap_or(0);
        }
    }

    pub(super) fn open_palette(&mut self) {
        let k = &self.cfg.keys;
        let hint = |a: Action, nav: &str| k.direct_label(a).unwrap_or_else(|| nav.to_string());
        let mut e =
            vec![PaletteEntry { title: "Новый агент".into(), hint: "n".into(), action: Action::NewAgent }];
        for i in self.visible_agents(false) {
            let d = &self.cfg.agents[i];
            e.push(PaletteEntry {
                title: format!("Новый: {}", d.name),
                hint: String::new(),
                action: Action::NewAgentOf(i),
            });
        }
        if !self.sessions.is_empty() {
            e.push(PaletteEntry {
                title: "Новый агент того же типа в этой папке".into(),
                hint: "N".into(),
                action: Action::NewHere,
            });
            for (i, s) in self.sessions.iter().enumerate() {
                e.push(PaletteEntry {
                    title: format!("Перейти: {} · {}", s.name, s.agent),
                    hint: if i < 9 { (i + 1).to_string() } else { String::new() },
                    action: Action::Select(i),
                });
            }
            let ents = [
                ("К агенту, который ждёт ответа", Action::NextWaiting, "w"),
                ("Закрыть выбранного агента", Action::Close, "x"),
                ("Переименовать выбранного агента", Action::Rename, "r"),
                ("Новая группа…", Action::NewGroup, "G"),
                ("Переместить агента в группу…", Action::Group, "a"),
                ("Свернуть/развернуть группу", Action::ToggleGroup, "o"),
                ("Перезапустить завершившегося агента", Action::Restart, "R"),
                ("Уведомления выбранного агента: вкл/выкл", Action::ToggleMute, "m"),
                ("Прокрутить историю вверх", Action::ScrollUp, "PgUp"),
                ("Прокрутить историю вниз", Action::ScrollDown, "PgDn"),
            ];
            for (t, a, nav) in ents {
                e.push(PaletteEntry { title: t.into(), hint: hint(a, nav), action: a });
            }
        }
        e.push(PaletteEntry {
            title: "Сетка / один агент".into(), hint: "g".into(), action: Action::ToggleGrid
        });
        e.push(PaletteEntry {
            title: "Все уведомления: вкл/выкл".into(),
            hint: "M".into(),
            action: Action::ToggleNotifications,
        });
        e.push(PaletteEntry {
            title: format!("Звук уведомлений: {}", if self.cfg.sound { "вкл → выключить" } else { "выкл → включить" }),
            hint: String::new(),
            action: Action::ToggleSound,
        });
        e.push(PaletteEntry {
            title: format!(
                "Всплывающие окошки: {}",
                if self.cfg.popups { "вкл → выключить" } else { "выкл → включить" }
            ),
            hint: String::new(),
            action: Action::TogglePopups,
        });
        for (i, t) in notify::theme_names().iter().enumerate() {
            let cur = *t == self.cfg.sound_theme;
            e.push(PaletteEntry {
                title: format!("Звук: {t}{}", if cur { "  ✓ выбран" } else { "" }),
                hint: if i == 0 { "S".into() } else { String::new() },
                action: Action::SetTheme(i),
            });
        }
        e.push(PaletteEntry {
            title: "Настройки: тема оформления, звук, ширина списка".into(),
            hint: ",".into(),
            action: Action::Settings,
        });
        e.push(PaletteEntry {
            title: "Изменения выбранного агента (git diff)".into(),
            hint: "v".into(),
            action: Action::Diff,
        });
        e.push(PaletteEntry { title: "Лента событий".into(), hint: "l".into(), action: Action::Log });
        for i in self.approvable() {
            let s = &self.sessions[i];
            e.push(PaletteEntry {
                title: format!("Разрешить: {} · {} — {}", s.name, s.agent, s.note),
                hint: "y".into(),
                action: Action::ApproveAt(i),
            });
        }
        if self.sessions.get(self.selected).is_some_and(|s| s.git.is_some()) {
            e.push(PaletteEntry {
                title: "Git: закоммитить изменения агента".into(),
                hint: String::new(),
                action: Action::GitCommit,
            });
            e.push(PaletteEntry {
                title: "Git: отправить ветку (push)".into(),
                hint: String::new(),
                action: Action::GitPush,
            });
            if self.sessions[self.selected].worktree.is_some() {
                e.push(PaletteEntry {
                    title: "Git: влить ветку агента в основную".into(),
                    hint: String::new(),
                    action: Action::GitMerge,
                });
                e.push(PaletteEntry {
                    title: "Git: удалить worktree агента".into(),
                    hint: String::new(),
                    action: Action::GitRemoveWorktree,
                });
            }
        }
        e.push(PaletteEntry {
            title: "Интеграции агентов: точные статусы через хуки".into(),
            hint: String::new(),
            action: Action::Integrations,
        });
        e.push(PaletteEntry {
            title: "Помощь и горячие клавиши".into(), hint: "?".into(), action: Action::Help
        });
        e.push(PaletteEntry {
            title: "Выйти из Radar (агенты продолжат работать)".into(),
            hint: "q".into(),
            action: Action::Quit,
        });
        e.push(PaletteEntry {
            title: "Остановить всех агентов и выйти".into(),
            hint: "Q".into(),
            action: Action::QuitStop,
        });
        self.mode = Mode::Palette(Palette { input: TextField::default(), entries: e, sel: 0 });
    }

    pub(super) fn session_menu(&self, idx: usize, from_sidebar: bool, x: u16, y: u16) -> Menu {
        let s = &self.sessions[idx];
        let mut items = vec![];
        if from_sidebar {
            items.push(MenuItem::new("Открыть", Action::Select(idx), ""));
            items.push(MenuItem::sep());
        }
        if self.sel.is_some_and(|sl| sl.idx == idx) {
            items.push(MenuItem::new("Копировать", Action::CopySel, ""));
            items.push(MenuItem::sep());
        }
        items.push(MenuItem::new("Переименовать…", Action::Rename, "r"));
        items.push(MenuItem::new("В группу…", Action::Group, "a"));
        if s.group.is_some() {
            items.push(MenuItem::new("Свернуть/развернуть группу", Action::ToggleGroup, "o"));
        }
        if self.sessions.len() > 1 {
            let mut up = MenuItem::new("Выше в списке", Action::MoveUp, "K");
            up.enabled = idx > 0;
            let mut down = MenuItem::new("Ниже в списке", Action::MoveDown, "J");
            down.enabled = idx + 1 < self.sessions.len();
            items.push(up);
            items.push(down);
        }
        let mut restart = MenuItem::new("Перезапустить", Action::Restart, "R");
        if s.is_running() {
            restart = restart.disabled();
        }
        items.push(restart);
        items.push(MenuItem::new(
            if s.muted {
                "Включить уведомления агента"
            } else {
                "Выключить уведомления агента"
            },
            Action::ToggleMute,
            "m",
        ));
        if s.status == Status::Waiting {
            items.push(MenuItem::new("Разрешить запрос…", Action::Approve, "y"));
        }
        items.push(MenuItem::new("Изменения (git diff)…", Action::Diff, "v"));
        items.push(MenuItem::new("Лента событий…", Action::Log, "l"));
        if s.git.is_some() {
            items.push(MenuItem::new("Закоммитить…", Action::GitCommit, ""));
            items.push(MenuItem::new("Отправить (push)…", Action::GitPush, ""));
            if s.worktree.is_some() {
                items.push(MenuItem::new("Влить ветку в основную…", Action::GitMerge, ""));
                items.push(MenuItem::new("Удалить worktree…", Action::GitRemoveWorktree, ""));
            }
        }
        items.push(MenuItem::sep());
        items.push(MenuItem::new("Новый агент в этой папке…", Action::NewHere, "N"));
        if !from_sidebar {
            items.push(MenuItem::new(if self.grid { "Один агент" } else { "Сетка" }, Action::ToggleGrid, "g"));
        }
        items.push(MenuItem::sep());
        items.push(MenuItem::new("Закрыть агента", Action::Close, "x"));
        Menu::new(x, y, items, Rect::new(0, 0, self.term_size().0, self.term_size().1))
    }

    pub(super) fn general_menu(&self, x: u16, y: u16) -> Menu {
        let mut items = vec![];
        for i in self.visible_agents(false) {
            items.push(MenuItem::new(&format!("Новый: {}", self.cfg.agents[i].name), Action::NewAgentOf(i), ""));
        }
        for i in self.approvable() {
            let s = &self.sessions[i];
            items.push(MenuItem::new(&format!("Разрешить: {} — {}", s.name, s.note), Action::ApproveAt(i), "y"));
        }
        items.extend([
            MenuItem::new("Новый агент…", Action::NewAgent, "n"),
            MenuItem::new("Новая группа…", Action::NewGroup, "G"),
            MenuItem::new("Палитра команд", Action::Palette, "p"),
            MenuItem::sep(),
            MenuItem::new(if self.grid { "Один агент" } else { "Сетка" }, Action::ToggleGrid, "g"),
            MenuItem::new(
                if self.notifications {
                    "Выключить все уведомления"
                } else {
                    "Включить все уведомления"
                },
                Action::ToggleNotifications,
                "M",
            ),
            MenuItem::new(if self.cfg.sound { "Выключить звук" } else { "Включить звук" }, Action::ToggleSound, ""),
            MenuItem::new(
                if self.cfg.popups {
                    "Выключить всплывающие окошки"
                } else {
                    "Включить всплывающие окошки"
                },
                Action::TogglePopups,
                "",
            ),
            MenuItem::new("Выбрать звук…", Action::PickSound, "S"),
            MenuItem::sep(),
            MenuItem::new("Настройки…", Action::Settings, ","),
            MenuItem::new("Интеграции агентов…", Action::Integrations, ""),
            MenuItem::new("Помощь", Action::Help, "?"),
            MenuItem::new("Выйти (агенты продолжат работать)", Action::Quit, "q"),
            MenuItem::new("Остановить всех агентов и выйти", Action::QuitStop, "Q"),
        ]);
        Menu::new(x, y, items, Rect::new(0, 0, self.term_size().0, self.term_size().1))
    }
}
