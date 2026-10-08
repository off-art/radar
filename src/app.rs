//! Состояние приложения, обработка событий и главный цикл.

use crate::config::{AgentDef, Config, Kind};
use crate::input::{key_to_bytes, mouse_to_bytes, MouseEv};
use crate::keys::{nav_action, Action};
use crate::menu::{filter_palette, Menu, MenuItem, PaletteEntry};
use crate::notify::{self, Sound};
use crate::textfield::TextField;
use crate::session::{Attention, Msg, Session, SpawnCtx};
use crate::status::Status;
use crate::ui;
use unicode_width::UnicodeWidthStr;
use anyhow::{anyhow, Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct NewForm {
    /// Индекс в `cfg.agents`.
    pub agent: usize,
    /// Показывать и те агенты, которых нет в системе.
    pub show_all: bool,
    pub dir: TextField,
    pub name: TextField,
    pub worktree: bool,
    pub field: usize,
    pub error: Option<String>,
    /// Подходящие папки после Tab (подсказка под полем «Папка»).
    pub hints: Vec<String>,
}

/// Выделение текста мышью в окне агента (координаты — ячейки экрана агента).
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub idx: usize,
    pub anchor: (u16, u16),
    pub head: (u16, u16),
}

impl Selection {
    /// (начало, конец) в порядке чтения.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }
}

pub enum Confirm {
    Close(usize),
    DeleteGroup(usize),
    Quit,
    QuitStop,
    Push(usize),
    Merge(usize),
    RemoveWorktree(usize),
    /// Разрешить запрос агента: (номер агента, текст запроса на момент диалога).
    /// Третье поле — выбранный в диалоге вариант ответа (по порядку), если на экране агента есть список.
    Approve(usize, String, Option<usize>),
}

/// Диалог группы: имя и галочки у агентов.
pub struct GroupForm {
    /// Номер редактируемой группы (`None` — новая).
    pub orig: Option<usize>,
    pub name: TextField,
    /// Агенты (id) в порядке списка и их галочки.
    pub ids: Vec<u32>,
    pub checked: Vec<bool>,
    pub cursor: usize,
    /// 0 — поле имени, 1 — список агентов.
    pub focus: u8,
}

pub struct Palette {
    pub input: TextField,
    pub entries: Vec<PaletteEntry>,
    pub sel: usize,
}

impl Palette {
    pub fn matches(&self) -> Vec<usize> {
        filter_palette(&self.entries, &self.input.text())
    }
}

pub enum Mode {
    /// Клавиши уходят агенту.
    Normal,
    /// Режим навигации (после префикса): клавиши управляют Radar, пока не нажать Esc.
    Nav(Instant),
    New(NewForm),
    Rename(TextField),
    /// Создание/правка группы.
    GroupForm(GroupForm),
    Confirm(Confirm),
    Help,
    Menu(Menu),
    Palette(Palette),
    /// Настройки; число — выбранная строка.
    Settings(usize),
    /// Интеграции с агентами; число — выбранная строка.
    Integrations(usize),
    /// Просмотр изменений (git diff) агента.
    Diff(Box<crate::diff::View>),
    /// Лента событий; число — выбранная строка (0 — самое новое).
    Log(usize),
    /// Сообщение коммита для выбранного агента.
    Commit(TextField),
}

pub struct IntegrationRow {
    pub name: String,
    pub id: String,
    /// None — для агента интеграции нет.
    pub state: Option<crate::integrations::State>,
    pub found: bool,
    pub path: String,
}

pub struct PaneRect {
    pub idx: usize,
    pub outer: Rect,
    /// Заголовок со статусом (одиночный режим). В сетке заголовок — в рамке.
    pub header: Option<Rect>,
    pub inner: Rect,
}

#[derive(Default)]
pub struct Geometry {
    pub sidebar: Rect,
    pub main: Rect,
    pub status: Rect,
    pub panes: Vec<PaneRect>,
    pub items: Vec<(usize, Rect)>,
    /// Заголовки групп: (ключ, первый агент, число агентов, область).
    pub headers: Vec<(String, usize, usize, Rect)>,
    /// Поясняющие строки списка («Без группы», «пусто»).
    pub texts: Vec<(String, Rect)>,
    /// Кнопка «+ группа».
    pub group_btn: Rect,
    /// Кнопка «+ новый» в шапке списка.
    pub new_btn: Rect,
    /// Переключатель уведомлений в строке статуса.
    pub notif_btn: Rect,
}

pub struct App {
    pub cfg: Config,
    pub theme: crate::theme::Theme,
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub grid: bool,
    pub mode: Mode,
    pub geo: Geometry,
    pub toast: Option<(String, Instant)>,
    pub notifications: bool,
    pub quit: bool,
    /// При выходе остановить и агентов (иначе они продолжают работать в фоне).
    pub stop_on_quit: bool,
    /// Последний записанный список агентов (чтобы не писать файл зря).
    persisted: String,
    pub events: crate::events::Log,
    /// Текущее выделение текста в окне агента.
    pub sel: Option<Selection>,
    /// Нажатая левая кнопка в окне агента: ждём, будет ли это клик (уйдёт агенту) или выделение.
    press: Option<(usize, Rect, u16, u16, KeyModifiers)>,
    /// Агент, которого тянут мышью по списку (перестановка).
    drag_item: Option<usize>,
    /// Идёт перетаскивание границы между списком агентов и окном агента.
    pub divider_drag: bool,
    git: crate::git::Watcher,
    pub dirty: bool,
    pub started: Instant,
    pub start_dir: PathBuf,
    /// Какие агенты установлены (None — ещё проверяется). Заполняется фоновыми потоками.
    available: Arc<Mutex<Vec<Option<bool>>>>,
    sidebar_first: usize,
    /// Свёрнутые группы (по ключу).
    pub collapsed: HashSet<String>,
    /// Группы списка по порядку (в том числе пустые).
    pub groups: Vec<String>,
    term_focused: bool,
    focus_supported: bool,
    last_key: Instant,
    next_id: u32,
    tx: Sender<Msg>,
    ctx: SpawnCtx,
}

/// Tab в поле «Папка»: дописывает путь до однозначного места; если вариантов несколько —
/// только показывает их под полем (ничего не выбирает за человека).
fn form_complete(f: &mut NewForm) {
    let c = crate::complete::complete(&f.dir.text());
    f.dir = TextField::new(&c.text);
    f.hints = c.matches;
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    PathBuf::from(p)
}

pub fn short_path(p: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = p.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    p.display().to_string()
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("git не найден")?;
    if !out.status.success() {
        return Err(anyhow!("{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Создаёт отдельный git worktree, чтобы агенты не мешали друг другу в одном репозитории.
fn make_worktree(dir: &Path, agent_id: &str) -> Result<PathBuf> {
    let top = PathBuf::from(
        run_git(dir, &["rev-parse", "--show-toplevel"]).map_err(|_| anyhow!("папка не в git-репозитории"))?,
    );
    let repo = top
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() % 100_000)
        .unwrap_or(0);
    let slug = format!("{agent_id}-{stamp}");
    let base = dirs::home_dir()
        .unwrap_or_default()
        .join(".radar")
        .join("worktrees");
    std::fs::create_dir_all(&base)?;
    let path = base.join(format!("{repo}-{slug}"));
    run_git(
        &top,
        &[
            "worktree",
            "add",
            "-b",
            &format!("radar/{slug}"),
            &path.to_string_lossy(),
        ],
    )?;
    Ok(path)
}

impl App {
    pub fn new(cfg: Config, start_dir: PathBuf, tx: Sender<Msg>, ctx: SpawnCtx) -> App {
        let notifications = cfg.notifications;
        let theme = cfg.theme();
        let mut app = App {
            theme,
            cfg,
            sessions: vec![],
            selected: 0,
            grid: false,
            mode: Mode::Normal,
            geo: Geometry::default(),
            toast: None,
            notifications,
            quit: false,
            stop_on_quit: false,
            persisted: String::new(),
            events: Default::default(),
            sel: None,
            press: None,
            drag_item: None,
            divider_drag: false,
            git: crate::git::Watcher::start(tx.clone()),
            dirty: true,
            started: Instant::now(),
            start_dir,
            available: Arc::new(Mutex::new(vec![])),
            sidebar_first: 0,
            collapsed: HashSet::new(),
            groups: vec![],
            term_focused: true,
            focus_supported: false,
            last_key: Instant::now(),
            next_id: 1,
            tx,
            ctx,
        };
        app.detect_agents();
        if let Some(w) = app.cfg.warnings.first().cloned() {
            app.toast(w);
        }
        app
    }

    /// Проверяет в фоне, какие агенты установлены (параллельно, чтобы не тормозить запуск).
    fn detect_agents(&mut self) {
        let n = self.cfg.agents.len();
        *self.available.lock().unwrap() = vec![None; n];
        for (i, def) in self.cfg.agents.iter().enumerate() {
            let bin = def.command.split_whitespace().next().unwrap_or("").to_string();
            let shared = self.available.clone();
            std::thread::spawn(move || {
                let ok = crate::session::find_binary(&bin).is_some();
                if let Some(slot) = shared.lock().unwrap().get_mut(i) {
                    *slot = Some(ok);
                }
            });
        }
    }

    /// Установлен ли агент (пока проверка не закончилась — считаем, что да).
    pub fn agent_available(&self, i: usize) -> bool {
        self.available.lock().unwrap().get(i).copied().flatten().unwrap_or(true)
    }

    /// Агенты для выбора: только установленные, либо все.
    pub fn visible_agents(&self, show_all: bool) -> Vec<usize> {
        let all: Vec<usize> = (0..self.cfg.agents.len()).collect();
        if show_all {
            return all;
        }
        let ok: Vec<usize> = all.iter().copied().filter(|&i| self.agent_available(i)).collect();
        if ok.is_empty() {
            all
        } else {
            ok
        }
    }

    pub fn anim_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
        self.dirty = true;
    }

    pub fn notif_label(&self) -> String {
        format!("уведомления: {} ", if self.notifications { "вкл" } else { "выкл" })
    }

    pub fn prefix_label(&self) -> String {
        self.cfg.keys.prefix.label()
    }

    fn notify_settings(&self) -> notify::Settings {
        notify::Settings {
            sound: self.cfg.sound,
            popups: self.cfg.popups,
            theme: self.cfg.sound_theme.clone(),
            volume: self.cfg.volume,
            sound_done: self.cfg.sound_done.clone(),
            sound_waiting: self.cfg.sound_waiting.clone(),
        }
    }

    // ───────────── сессии ─────────────

    fn pane_size(&self) -> (u16, u16) {
        let m = self.geo.main;
        if m.width > 4 && m.height > 4 {
            (m.height.saturating_sub(1), m.width)
        } else {
            (30, 100)
        }
    }

    pub fn create_session(
        &mut self,
        def: AgentDef,
        dir: PathBuf,
        name: Option<String>,
        worktree: bool,
    ) -> Result<()> {
        self.create_session_with(def, dir, name, worktree, None)
    }

    fn create_session_with(
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
            let p = make_worktree(&dir, &def.id)?;
            (p.clone(), Some(p))
        } else {
            (dir, None)
        };
        let base = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
            cwd.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| def.name.clone())
        });
        // одинаковые имена (несколько агентов в одной папке) различаем номером
        let mut name = base.clone();
        let mut n = 2;
        while self.sessions.iter().any(|s| s.name == name) {
            name = format!("{base} #{n}");
            n += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        let s = Session::spawn(
            id,
            &def,
            name,
            cwd,
            wt,
            self.pane_size(),
            self.tx.clone(),
            &self.ctx,
            resume,
        )?;
        let (sid, agent_name, sname) = (s.id, s.agent.clone(), s.name.clone());
        self.sessions.push(s);
        self.events.push(sid, agent_name, crate::events::Kind::Started, format!("{sname}: запущен"));
        self.selected = self.sessions.len() - 1;
        self.normalize_groups(sid);
        self.dirty = true;
        Ok(())
    }

    fn ctl_info(&self, i: usize) -> serde_json::Value {
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
    fn handle_ctl(&mut self, req: &serde_json::Value) -> serde_json::Value {
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
                    found.push(Session::from_host(&def, meta, stream, self.tx.clone()));
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
        self.groups = saved.groups.clone();
        self.collapsed = saved.collapsed.iter().cloned().collect();
        if !self.sessions.is_empty() {
            self.selected = saved.selected.min(self.sessions.len() - 1);
            let id = self.sessions[self.selected].id;
            self.normalize_groups(id);
        }
        if busy > 0 {
            self.toast(format!("Агентов в другом окне Radar: {busy} — они здесь не показаны"));
        } else if unknown > 0 {
            self.toast(format!("Фоновых агентов не из этого конфига: {unknown} — они продолжают работать (radar stop — остановить)"));
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
        self.groups = saved.groups.clone();
        self.collapsed = saved.collapsed.iter().cloned().collect();
        if !self.sessions.is_empty() {
            self.selected = saved.selected.min(self.sessions.len() - 1);
            let id = self.sessions[self.selected].id;
            self.normalize_groups(id);
        }
        if failed > 0 {
            self.toast(format!("Не удалось восстановить агентов: {failed} (папка удалена или агент убран из конфига)"));
        }
        self.persisted = crate::persist::to_text(&self.snapshot());
    }

    fn snapshot(&self) -> crate::persist::File {
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
    pub fn persist_sessions(&mut self) {
        if !self.cfg.restore {
            return;
        }
        let text = crate::persist::to_text(&self.snapshot());
        if text != self.persisted {
            crate::persist::save_text(&text);
            self.persisted = text;
        }
    }

    fn restart(&mut self, idx: usize) {
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

    fn close(&mut self, idx: usize) {
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

    /// Переставляет агента `from` на место `to` (выбранный агент остаётся выбранным).
    fn move_session(&mut self, from: usize, to: usize) {
        let n = self.sessions.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let cur = self.sessions[self.selected.min(n - 1)].id;
        let s = self.sessions.remove(from);
        self.sessions.insert(to, s);
        if let Some(i) = self.sessions.iter().position(|s| s.id == cur) {
            self.selected = i;
        }
        self.sel = None;
        self.press = None;
        self.dirty = true;
    }

    /// Номер группы каждого агента.
    fn group_idx(&self) -> Vec<Option<usize>> {
        self.sessions.iter().map(|s| crate::groups::index_of(&self.groups, s.group.as_deref())).collect()
    }

    /// Группы и строки списка в порядке отображения.
    pub fn sidebar_rows(&self) -> Vec<crate::groups::Row> {
        crate::groups::rows(&self.groups, &self.group_idx(), &self.collapsed)
    }

    /// Собирает агентов одной группы подряд (без группы — в конце); выбранным остаётся агент `keep`.
    fn normalize_groups(&mut self, keep: u32) {
        // группа, которой нет в списке (например, из старого файла), попадает в конец
        let unknown: Vec<String> = self
            .sessions
            .iter()
            .filter_map(|s| s.group.clone())
            .filter(|g| !self.groups.contains(g))
            .collect();
        for g in unknown {
            if !self.groups.contains(&g) {
                self.groups.push(g);
            }
        }
        let ord = crate::groups::order(&self.group_idx());
        if ord.iter().enumerate().any(|(i, &o)| i != o) {
            let mut old: Vec<Option<Session>> = std::mem::take(&mut self.sessions).into_iter().map(Some).collect();
            self.sessions = ord.into_iter().filter_map(|i| old[i].take()).collect();
        }
        if let Some(i) = self.sessions.iter().position(|s| s.id == keep) {
            self.selected = i;
        }
        self.dirty = true;
    }

    fn selected_id(&self) -> u32 {
        self.sessions.get(self.selected).map_or(0, |s| s.id)
    }

    /// Перестановка клавишами: только внутри своей группы.
    fn move_in_group(&mut self, to: usize) {
        let from = self.selected;
        match self.sessions.get(to) {
            Some(t) if t.group == self.sessions[from].group => self.move_session(from, to),
            Some(_) => self.toast("Это край группы — перетащите агента мышью или переместите в группу (a)"),
            None => {}
        }
    }

    fn open_group_form(&mut self, orig: Option<usize>) {
        let name = orig.and_then(|i| self.groups.get(i)).cloned().unwrap_or_default();
        let checked = self.sessions.iter().map(|s| orig.is_some() && s.group.as_deref() == Some(name.as_str())).collect();
        self.mode = Mode::GroupForm(GroupForm {
            orig,
            name: TextField::new(&name),
            ids: self.sessions.iter().map(|s| s.id).collect(),
            checked,
            cursor: self.selected.min(self.sessions.len().saturating_sub(1)),
            focus: 0,
        });
    }

    /// Сохраняет диалог группы. Ошибка (пустое/повторное имя) — текст для тоста.
    fn save_group_form(&mut self, f: &GroupForm) -> Result<(), String> {
        let name = f.name.text().trim().to_string();
        if name.is_empty() {
            return Err("Введите имя группы".into());
        }
        if self.groups.iter().enumerate().any(|(i, g)| *g == name && Some(i) != f.orig) {
            return Err(format!("Группа «{name}» уже есть"));
        }
        let keep = self.selected_id();
        match f.orig.and_then(|i| self.groups.get(i).cloned().map(|old| (i, old))) {
            Some((i, old)) => {
                if old != name {
                    for s in &mut self.sessions {
                        if s.group.as_deref() == Some(old.as_str()) {
                            s.group = Some(name.clone());
                        }
                    }
                    if self.collapsed.remove(&old) {
                        self.collapsed.insert(name.clone());
                    }
                    self.groups[i] = name.clone();
                }
            }
            None => self.groups.push(name.clone()),
        }
        for (id, &on) in f.ids.iter().zip(&f.checked) {
            if let Some(s) = self.sessions.iter_mut().find(|s| s.id == *id) {
                if on {
                    s.group = Some(name.clone());
                } else if s.group.as_deref() == Some(name.as_str()) {
                    s.group = None;
                }
            }
        }
        self.collapsed.remove(&name);
        self.normalize_groups(keep);
        let n = self.sessions.iter().filter(|s| s.group.as_deref() == Some(name.as_str())).count();
        self.toast(format!("Группа «{name}»: агентов {n}"));
        Ok(())
    }

    fn delete_group(&mut self, i: usize) {
        let Some(name) = self.groups.get(i).cloned() else { return };
        let keep = self.selected_id();
        for s in &mut self.sessions {
            if s.group.as_deref() == Some(name.as_str()) {
                s.group = None;
            }
        }
        self.groups.remove(i);
        self.collapsed.remove(&name);
        self.normalize_groups(keep);
        self.toast(format!("Группа «{name}» удалена, агенты остались в списке"));
    }

    fn set_group(&mut self, group: Option<String>) {
        let keep = self.selected_id();
        if let Some(s) = self.sessions.get_mut(self.selected) {
            s.group = group.clone();
        }
        if let Some(g) = &group {
            self.collapsed.remove(g);
        }
        self.normalize_groups(keep);
    }

    fn move_group(&mut self, i: usize, down: bool) {
        let j = if down { i + 1 } else { i.wrapping_sub(1) };
        if i < self.groups.len() && j < self.groups.len() {
            let keep = self.selected_id();
            self.groups.swap(i, j);
            self.normalize_groups(keep);
        }
    }

    /// Свернуть/развернуть группу выбранного агента.
    fn toggle_group(&mut self) {
        match self.sessions.get(self.selected).and_then(|s| s.group.clone()) {
            Some(g) => self.toggle_group_key(&g),
            None => self.toast("Агент не в группе — переместить: a, создать группу: G"),
        }
    }

    fn toggle_group_key(&mut self, key: &str) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_string());
        }
        self.dirty = true;
    }

    /// Меню «в какую группу переместить» выбранного агента.
    fn group_menu(&self) -> Menu {
        let cur = self.sessions.get(self.selected).and_then(|s| s.group.clone());
        let mut items = vec![];
        for (i, g) in self.groups.iter().enumerate() {
            let mut it = MenuItem::new(&format!("В группу «{g}»"), Action::MoveToGroup(i), "");
            it.enabled = cur.as_deref() != Some(g.as_str());
            items.push(it);
        }
        if cur.is_some() {
            items.push(MenuItem::new("Убрать из группы", Action::Ungroup, ""));
        }
        if !items.is_empty() {
            items.push(MenuItem::sep());
        }
        items.push(MenuItem::new("Новая группа…", Action::NewGroup, "G"));
        let (x, y) = self
            .geo
            .items
            .iter()
            .find(|(i, _)| *i == self.selected)
            .map_or((self.geo.sidebar.x + 2, self.geo.sidebar.y + 3), |(_, r)| (r.x + 2, r.y + 1));
        Menu::new(x, y, items, Rect::new(0, 0, self.term_size().0, self.term_size().1))
    }

    fn select_rel(&mut self, delta: isize) {
        // идём только по видимым агентам (без спрятанных в свёрнутых группах)
        let vis: Vec<usize> = self
            .sidebar_rows()
            .into_iter()
            .filter_map(|r| if let crate::groups::Row::Item(i) = r { Some(i) } else { None })
            .collect();
        let n = vis.len() as isize;
        if n == 0 {
            return;
        }
        let next = match vis.iter().position(|&i| i == self.selected) {
            Some(p) => vis[(p as isize + delta).rem_euclid(n) as usize],
            None if delta > 0 => vis.iter().copied().find(|&i| i > self.selected).unwrap_or(vis[0]),
            None => vis.iter().rev().copied().find(|&i| i < self.selected).unwrap_or(vis[vis.len() - 1]),
        };
        self.select(next);
    }

    fn select_next_attention(&mut self) {
        let n = self.sessions.len();
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.sessions[i].status == Status::Waiting {
                self.select(i);
                return;
            }
        }
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.sessions[i].unread {
                self.select(i);
                return;
            }
        }
        self.toast("Нет агентов, которым нужен ответ");
    }

    // ───────────── раскладка ─────────────

    pub fn visible_indices(&self) -> Vec<usize> {
        let n = self.sessions.len();
        if n == 0 {
            return vec![];
        }
        if !self.grid {
            return vec![self.selected];
        }
        let per_page = 9;
        let page = self.selected / per_page;
        (page * per_page..((page + 1) * per_page).min(n)).collect()
    }

    pub fn compute_layout(&mut self, area: Rect) {
        let status = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
        let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
        let sw = if area.width < 60 {
            0
        } else if self.cfg.sidebar_width > 0 {
            self.cfg.sidebar_width.min(area.width / 2)
        } else {
            (area.width / 4).clamp(26, 36)
        };
        let sidebar = Rect::new(body.x, body.y, sw, body.height);
        let main = Rect::new(body.x + sw, body.y, body.width - sw, body.height);

        // строки списка: заголовок группы — 1 строка, агент — 3 строки + пустая; 3 строки шапка
        let rows = self.sidebar_rows();
        let rh = |r: &crate::groups::Row| if matches!(r, crate::groups::Row::Item(_)) { 4u16 } else { 1 };
        let avail = body.height.saturating_sub(3);
        let sel_row = rows
            .iter()
            .position(|r| match r {
                crate::groups::Row::Item(i) => *i == self.selected,
                crate::groups::Row::Header { first, count, collapsed, .. } => {
                    *collapsed && (*first..*first + *count).contains(&self.selected)
                }
                crate::groups::Row::Text(_) => false,
            })
            .unwrap_or(0);
        if rows.is_empty() {
            self.sidebar_first = 0;
        } else {
            self.sidebar_first = self.sidebar_first.min(sel_row);
            while self.sidebar_first < sel_row && rows[self.sidebar_first..=sel_row].iter().map(rh).sum::<u16>() > avail {
                self.sidebar_first += 1;
            }
        }
        let mut items = vec![];
        let mut headers = vec![];
        let mut texts = vec![];
        if sw > 0 {
            let mut y = sidebar.y + 3;
            let bottom = sidebar.y + body.height;
            for r in rows.iter().skip(self.sidebar_first) {
                match r {
                    crate::groups::Row::Header { key, first, count, .. } => {
                        if y + 1 > bottom {
                            break;
                        }
                        headers.push((key.clone(), *first, *count, Rect::new(sidebar.x, y, sw - 1, 1)));
                        y += 1;
                    }
                    crate::groups::Row::Text(t) => {
                        if y + 1 > bottom {
                            break;
                        }
                        texts.push((t.clone(), Rect::new(sidebar.x, y, sw - 1, 1)));
                        y += 1;
                    }
                    crate::groups::Row::Item(idx) => {
                        if y + 3 > bottom {
                            break;
                        }
                        items.push((*idx, Rect::new(sidebar.x, y, sw - 1, 3)));
                        y += 4;
                    }
                }
            }
        }
        let new_btn = if sw > 12 {
            Rect::new(sidebar.x + sw - 1 - 9, sidebar.y, 9, 1)
        } else {
            Rect::default()
        };
        let group_btn = if sw >= 14 {
            Rect::new(sidebar.x + 1, sidebar.y + 2, 10, 1)
        } else {
            Rect::default()
        };
        let nw = self.notif_label().width() as u16;
        let notif_btn = if status.width > nw + 20 {
            Rect::new(status.right() - nw, status.y, nw, 1)
        } else {
            Rect::default()
        };

        // панели
        let vis = self.visible_indices();
        let mut panes = vec![];
        if !self.grid {
            if let Some(&idx) = vis.first() {
                let header = Rect::new(main.x, main.y, main.width, 1);
                let inner = Rect::new(main.x, main.y + 1, main.width, main.height.saturating_sub(1));
                panes.push(PaneRect { idx, outer: main, header: Some(header), inner });
            }
        } else if !vis.is_empty() {
            let n = vis.len();
            let (cols, rows) = match n {
                1 => (1, 1),
                2 => (2, 1),
                3 | 4 => (2, 2),
                5 | 6 => (3, 2),
                _ => (3, 3),
            };
            let ch = main.height / rows as u16;
            for (k, &idx) in vis.iter().enumerate() {
                let (c, r) = ((k % cols) as u16, (k / cols) as u16);
                // в неполном последнем ряду панели растягиваются на всю ширину
                let in_row = (n - r as usize * cols).min(cols) as u16;
                let cw = main.width / in_row;
                let w = if c == in_row - 1 { main.width - cw * c } else { cw };
                let h = if r as usize == rows - 1 { main.height - ch * r } else { ch };
                let outer = Rect::new(main.x + c * cw, main.y + r * ch, w, h);
                let inner = Rect::new(
                    outer.x + 1,
                    outer.y + 1,
                    outer.width.saturating_sub(2),
                    outer.height.saturating_sub(2),
                );
                panes.push(PaneRect { idx, outer, header: None, inner });
            }
        }
        for p in &panes {
            if let Some(s) = self.sessions.get_mut(p.idx) {
                s.resize(p.inner.height, p.inner.width);
            }
        }
        // прокрутка не может уйти дальше реальной истории
        for s in self.sessions.iter_mut().filter(|s| s.scroll > 0) {
            let max = s.max_scrollback();
            s.scroll = s.scroll.min(max);
        }
        self.geo = Geometry { sidebar, main, status, panes, items, headers, texts, group_btn, new_btn, notif_btn };
    }

    // ───────────── статусы и уведомления ─────────────

    fn is_viewing(&self, idx: usize) -> bool {
        let focused = if self.focus_supported {
            self.term_focused
        } else {
            self.last_key.elapsed() < Duration::from_secs(8)
        };
        focused && self.visible_indices().contains(&idx)
    }

    fn handle_attention(&mut self, idx: usize, a: Attention) {
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
        self.git.set_targets(
            self.sessions.iter().filter(|s| s.is_running()).map(|s| (s.id, s.cwd.clone())).collect(),
        );
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
    }

    pub fn needs_animation(&self) -> bool {
        self.sessions
            .iter()
            .any(|s| matches!(s.status, Status::Working | Status::Waiting | Status::Starting))
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

    // ───────────── действия ─────────────

    fn page(&self) -> usize {
        (self.geo.panes.first().map(|p| p.inner.height).unwrap_or(20) / 2).max(3) as usize
    }

    fn open_new_form(&mut self, agent: Option<usize>) {
        let dir = self
            .sessions
            .get(self.selected)
            .map(|s| s.cwd.clone())
            .unwrap_or_else(|| self.start_dir.clone());
        let last = self.cfg.agents.len().saturating_sub(1);
        let first_visible = self.visible_agents(false).first().copied().unwrap_or(0);
        let chosen = agent.map(|i| i.min(last)).unwrap_or(first_visible);
        // если выбранного агента нет в системе, показываем всех, чтобы выбор был виден
        let show_all = !self.visible_agents(false).contains(&chosen);
        self.mode = Mode::New(NewForm {
            agent: chosen,
            show_all,
            dir: TextField::new(&short_path(&dir)),
            name: TextField::default(),
            worktree: false,
            field: if agent.is_some() { 1 } else { 0 },
            error: None,
            hints: vec![],
        });
    }

    fn selected_agent_index(&self) -> Option<usize> {
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
                self.toast(if self.notifications { "Уведомления включены" } else { "Уведомления выключены" });
            }
            Action::ToggleSound => {
                self.cfg.sound = !self.cfg.sound;
                self.cfg.save_state();
                self.toast(if self.cfg.sound { "Звук уведомлений включён" } else { "Звук уведомлений выключен" });
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
                        self.toast(if ok { format!("Скопировано: {n} симв.") } else { "Не удалось скопировать".to_string() });
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
    fn open_sound_picker(&mut self) {
        self.open_palette();
        if let Mode::Palette(p) = &mut self.mode {
            p.input = TextField::new("Звук:");
            let cur = format!("Звук: {}", self.cfg.sound_theme);
            let m = p.matches();
            p.sel = m.iter().position(|&i| p.entries[i].title.starts_with(&cur)).unwrap_or(0);
        }
    }

    fn open_palette(&mut self) {
        let k = &self.cfg.keys;
        let hint = |a: Action, nav: &str| k.direct_label(a).unwrap_or_else(|| nav.to_string());
        let mut e = vec![PaletteEntry { title: "Новый агент".into(), hint: "n".into(), action: Action::NewAgent }];
        for i in self.visible_agents(false) {
            let d = &self.cfg.agents[i];
            e.push(PaletteEntry { title: format!("Новый: {}", d.name), hint: String::new(), action: Action::NewAgentOf(i) });
        }
        if !self.sessions.is_empty() {
            e.push(PaletteEntry { title: "Новый агент того же типа в этой папке".into(), hint: "N".into(), action: Action::NewHere });
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
        e.push(PaletteEntry { title: "Сетка / один агент".into(), hint: "g".into(), action: Action::ToggleGrid });
        e.push(PaletteEntry { title: "Все уведомления: вкл/выкл".into(), hint: "M".into(), action: Action::ToggleNotifications });
        e.push(PaletteEntry {
            title: format!("Звук уведомлений: {}", if self.cfg.sound { "вкл → выключить" } else { "выкл → включить" }),
            hint: String::new(),
            action: Action::ToggleSound,
        });
        e.push(PaletteEntry {
            title: format!("Всплывающие окошки: {}", if self.cfg.popups { "вкл → выключить" } else { "выкл → включить" }),
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
        e.push(PaletteEntry { title: "Настройки: тема оформления, звук, ширина списка".into(), hint: ",".into(), action: Action::Settings });
        e.push(PaletteEntry { title: "Изменения выбранного агента (git diff)".into(), hint: "v".into(), action: Action::Diff });
        e.push(PaletteEntry { title: "Лента событий".into(), hint: "l".into(), action: Action::Log });
        for i in self.approvable() {
            let s = &self.sessions[i];
            e.push(PaletteEntry {
                title: format!("Разрешить: {} · {} — {}", s.name, s.agent, s.note),
                hint: "y".into(),
                action: Action::ApproveAt(i),
            });
        }
        if self.sessions.get(self.selected).map(|s| s.git.is_some()).unwrap_or(false) {
            e.push(PaletteEntry { title: "Git: закоммитить изменения агента".into(), hint: String::new(), action: Action::GitCommit });
            e.push(PaletteEntry { title: "Git: отправить ветку (push)".into(), hint: String::new(), action: Action::GitPush });
            if self.sessions[self.selected].worktree.is_some() {
                e.push(PaletteEntry { title: "Git: влить ветку агента в основную".into(), hint: String::new(), action: Action::GitMerge });
                e.push(PaletteEntry { title: "Git: удалить worktree агента".into(), hint: String::new(), action: Action::GitRemoveWorktree });
            }
        }
        e.push(PaletteEntry { title: "Интеграции агентов: точные статусы через хуки".into(), hint: String::new(), action: Action::Integrations });
        e.push(PaletteEntry { title: "Помощь и горячие клавиши".into(), hint: "?".into(), action: Action::Help });
        e.push(PaletteEntry { title: "Выйти из Radar (агенты продолжат работать)".into(), hint: "q".into(), action: Action::Quit });
        e.push(PaletteEntry { title: "Остановить всех агентов и выйти".into(), hint: "Q".into(), action: Action::QuitStop });
        self.mode = Mode::Palette(Palette { input: TextField::default(), entries: e, sel: 0 });
    }

    fn session_menu(&self, idx: usize, from_sidebar: bool, x: u16, y: u16) -> Menu {
        let s = &self.sessions[idx];
        let mut items = vec![];
        if from_sidebar {
            items.push(MenuItem::new("Открыть", Action::Select(idx), ""));
            items.push(MenuItem::sep());
        }
        if self.sel.map_or(false, |sl| sl.idx == idx) {
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
            if s.muted { "Включить уведомления агента" } else { "Выключить уведомления агента" },
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

    fn general_menu(&self, x: u16, y: u16) -> Menu {
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
                if self.notifications { "Выключить все уведомления" } else { "Включить все уведомления" },
                Action::ToggleNotifications,
                "M",
            ),
            MenuItem::new(if self.cfg.sound { "Выключить звук" } else { "Включить звук" }, Action::ToggleSound, ""),
            MenuItem::new(
                if self.cfg.popups { "Выключить всплывающие окошки" } else { "Включить всплывающие окошки" },
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

    fn term_size(&self) -> (u16, u16) {
        let s = self.geo.status;
        (s.right().max(1), s.bottom().max(1))
    }

    // ───────────── ввод ─────────────

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

    fn on_key(&mut self, k: KeyEvent) {
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

    fn key_normal(&mut self, k: KeyEvent) {
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
    fn key_nav(&mut self, k: KeyEvent) {
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

    fn key_menu(&mut self, mut m: Menu, k: KeyEvent) {
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

    // ───────────── настройки ─────────────

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
    fn settings_change(&mut self, row: usize, dir: i32) {
        fn cycle<T: PartialEq + Clone>(list: &[T], cur: &T, dir: i32) -> T {
            let n = list.len() as i32;
            let i = list.iter().position(|x| x == cur).unwrap_or(0) as i32;
            list[((i + dir).rem_euclid(n)) as usize].clone()
        }
        match row {
            0 => {
                let names: Vec<String> = crate::theme::names().iter().map(|s| s.to_string()).collect();
                self.cfg.theme_name = cycle(&names, &self.cfg.theme_name, dir);
                self.theme = self.cfg.theme();
            }
            1 => {
                self.notifications = !self.notifications;
                self.cfg.notifications = self.notifications;
            }
            2 => self.cfg.sound = !self.cfg.sound,
            3 => {
                let names: Vec<String> = notify::theme_names().iter().map(|s| s.to_string()).collect();
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

    fn key_settings(&mut self, mut sel: usize, k: KeyEvent) {
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

    fn mouse_settings(&mut self, sel: usize, x: u16, y: u16) {
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

    fn integration_toggle(&mut self, row: usize) {
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

    /// Выполняет git-действие в фоне; результат придёт как `Msg::GitDone`.
    fn spawn_git(&self, label: &str, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let tx = self.tx.clone();
        let label = label.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::GitDone { label, result: job() });
        });
    }

    fn git_commit_open(&mut self) {
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

    fn key_commit(&mut self, mut t: TextField, k: KeyEvent) {
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
    fn git_confirm(&mut self, a: Action) {
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

    fn git_push_run(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        let dir = s.cwd.clone();
        self.toast("Отправка…");
        self.spawn_git("Push", move || crate::gitops::push(&dir));
    }

    fn git_merge_run(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        let (Some(wt), Some(g)) = (s.worktree.clone(), s.git.clone()) else { return };
        self.toast("Слияние…");
        self.spawn_git("Слияние", move || crate::gitops::merge_into_main(&wt, &g.branch));
    }

    fn git_remove_run(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        let (Some(wt), Some(g)) = (s.worktree.clone(), s.git.clone()) else { return };
        self.close(i); // агент работает внутри worktree — сначала останавливаем
        self.toast("Удаление worktree…");
        self.spawn_git("Worktree", move || crate::gitops::remove_worktree(&wt, &g.branch));
    }

    fn open_diff(&mut self) {
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

    fn diff_height(&self) -> usize {
        let area = Rect::new(0, 0, self.term_size().0, self.term_size().1);
        ui::diff_layout(area).2.height as usize
    }

    fn key_diff(&mut self, mut v: Box<crate::diff::View>, k: KeyEvent) {
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

    fn mouse_diff(&mut self, m: MouseEvent) {
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

    fn key_integrations(&mut self, mut sel: usize, k: KeyEvent) {
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

    fn mouse_integrations(&mut self, sel: usize, x: u16, y: u16) {
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

    fn key_log(&mut self, mut sel: usize, k: KeyEvent) {
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
    fn log_jump(&mut self, sel: usize) {
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

    fn mouse_log(&mut self, sel: usize, x: u16, y: u16) {
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

    fn key_palette(&mut self, mut p: Palette, k: KeyEvent) {
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

    /// «Разрешить» из списка: всегда через диалог с текстом запроса, вслепую ничего не подтверждаем.
    pub fn can_approve(&self, i: usize) -> bool {
        self.approve_state(i).is_ok()
    }

    /// Агенты, которые ждут ответа и которым можно ответить из списка (есть текст запроса).
    fn approvable(&self) -> Vec<usize> {
        (0..self.sessions.len()).filter(|&i| self.approve_state(i).is_ok()).collect()
    }

    /// Можно ли разрешить запрос агента `i`; иначе — почему нельзя.
    fn approve_state(&self, i: usize) -> Result<String, String> {
        let s = self.sessions.get(i).ok_or("Нет агента")?;
        if s.status != Status::Waiting {
            return Err("Агент ничего не просит: он не в статусе «ждёт ответа»".into());
        }
        let enabled = self.cfg.agents.iter().find(|d| d.name == s.agent).map(|d| !d.approve.is_empty()).unwrap_or(false);
        if !enabled {
            return Err(format!("Для «{}» подтверждение из списка не включено (approve в config.toml)", s.agent));
        }
        if s.note.is_empty() {
            return Err("Текст запроса неизвестен — откройте агента и ответьте в его окне (нужна интеграция)".into());
        }
        Ok(s.note.clone())
    }

    /// `Ctrl+b y`: выбранный агент, если он ждёт, иначе ближайший ждущий из остальных.
    fn approve_open(&mut self) {
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

    fn approve_open_at(&mut self, i: usize) {
        match self.approve_state(i) {
            Ok(note) => {
                let (_, hi) = crate::session::option_lines(&self.sessions[i].prompt_excerpt());
                self.mode = Mode::Confirm(Confirm::Approve(i, note, hi));
            }
            Err(e) => self.toast(e),
        }
    }

    fn approve_run(&mut self, i: usize, note: String, sel: Option<usize>) {
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
                let text = excerpt[line].trim_start_matches(|c: char| c.is_whitespace() || "›❯>●○→".contains(c)).to_string();
                answer = format!("ответ — {text}");
            }
        }
        self.sessions[i].send_raw(&bytes);
        self.events.push(sid, ag, crate::events::Kind::Approved, format!("{nm}: {answer} · {note}"));
        self.toast(format!("{}: {note}", if answer == "разрешено" { "Разрешено".to_string() } else { answer }));
    }

    fn key_confirm(&mut self, c: Confirm, k: KeyEvent) {
        if let Confirm::Approve(i, note, sel) = c {
            let n = self
                .sessions
                .get(i)
                .map(|s| crate::session::option_lines(&s.prompt_excerpt()).0.len())
                .unwrap_or(0);
            let keep = |app: &mut Self, sel: Option<usize>| app.mode = Mode::Confirm(Confirm::Approve(i, note.clone(), sel));
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

    fn key_rename(&mut self, mut t: TextField, k: KeyEvent) {
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

    fn key_group_form(&mut self, mut f: GroupForm, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                match self.save_group_form(&f) {
                    Ok(()) => return,
                    Err(e) => self.toast(e),
                }
            }
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

    fn key_new(&mut self, mut f: NewForm, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                let def = self.cfg.agents[f.agent].clone();
                let dir = expand_tilde(f.dir.text().trim());
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

    // ───────────── мышь ─────────────

    fn pane_at(&self, x: u16, y: u16) -> Option<&PaneRect> {
        self.geo.panes.iter().find(|p| in_rect(&p.outer, x, y))
    }

    /// Передаёт событие мыши агенту, если он сам включил отслеживание мыши. `true` — отправлено.
    fn forward_mouse(&mut self, idx: usize, inner: Rect, ev: MouseEv, mods: KeyModifiers, x: u16, y: u16) -> bool {
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

    fn on_mouse(&mut self, m: MouseEvent) {
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
            MouseEventKind::Down(MouseButton::Left) if mods.contains(KeyModifiers::CONTROL) => {
                self.context_menu(x, y)
            }
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
                    .find(|p| p.header.map_or(false, |h| in_rect(&h, x, y)))
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
    fn pane_cell(inner: Rect, x: u16, y: u16) -> (u16, u16) {
        let r = y.clamp(inner.y, inner.bottom().saturating_sub(1)) - inner.y;
        let c = x.clamp(inner.x, inner.right().saturating_sub(1)) - inner.x;
        (r, c)
    }

    /// Протянули мышь с нажатой левой кнопкой: начинаем или продолжаем выделение текста.
    fn mouse_drag_left(&mut self, x: u16, y: u16) {
        let Some((idx, inner, px, py, _)) = self.press else { return };
        if self.sel.is_none() && (x, y) == (px, py) {
            return;
        }
        let head = Self::pane_cell(inner, x, y);
        let anchor = self.sel.map(|s| s.anchor).unwrap_or_else(|| Self::pane_cell(inner, px, py));
        self.sel = Some(Selection { idx, anchor, head });
        self.dirty = true;
    }

    /// Отпустили левую кнопку: после выделения — копируем, иначе это был обычный клик агенту.
    fn mouse_up_left(&mut self, x: u16, y: u16) {
        let Some((idx, inner, px, py, mods)) = self.press.take() else { return };
        match self.sel {
            Some(sel) => {
                let text = self.selection_text(&sel);
                if text.is_empty() {
                    self.sel = None;
                } else {
                    let n = text.chars().count();
                    let ok = crate::clipboard::copy(&text);
                    self.toast(if ok { format!("Скопировано: {n} симв.") } else { "Не удалось скопировать".to_string() });
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
    fn selection_text(&self, sel: &Selection) -> String {
        let Some(s) = self.sessions.get(sel.idx) else { return String::new() };
        let ((r0, c0), (r1, c1)) = sel.ordered();
        let mut p = s.parser.lock().unwrap();
        p.screen_mut().set_scrollback(s.scroll);
        let (_, cols) = p.screen().size();
        let text = p.screen().contents_between(r0, c0.min(cols), r1, (c1 + 1).min(cols));
        p.screen_mut().set_scrollback(0);
        text.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n").trim_end().to_string()
    }

    /// Колесо: агенту с включённой мышью — событие; полноэкранным программам (less, vim, htop) —
    /// стрелки; обычному выводу — прокрутка истории Radar.
    fn wheel(&mut self, idx: usize, inner: Rect, up: bool, mods: KeyModifiers, x: u16, y: u16) {
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
    fn mouse_new_form(&mut self, x: u16, y: u16) {
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
    fn on_divider(&self, x: u16, y: u16) -> bool {
        let sb = self.geo.sidebar;
        sb.width > 0 && x + 1 == sb.right() && y >= sb.y && y < sb.bottom()
    }

    fn context_menu(&mut self, x: u16, y: u16) {
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

fn in_rect(r: &Rect, x: u16, y: u16) -> bool {
    r.width > 0 && x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

/// Главный цикл.
pub fn run(mut app: App, rx: Receiver<Msg>, sock: PathBuf) -> Result<()> {
    let mut terminal = ratatui::init();
    if app.cfg.notifications {
        notify::prepare();
    }
    let mut out = std::io::stdout();
    let _ = execute!(out, EnableBracketedPaste, EnableFocusChange);
    if app.cfg.mouse {
        let _ = execute!(out, EnableMouseCapture);
    }

    let result = (|| -> Result<()> {
        let mut last_tick = Instant::now();
        let mut last_draw = Instant::now() - Duration::from_secs(1);
        loop {
            if event::poll(Duration::from_millis(16))? {
                loop {
                    app.on_event(event::read()?);
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            while let Ok(msg) = rx.try_recv() {
                app.on_msg(msg);
            }
            if last_tick.elapsed() >= Duration::from_millis(250) {
                app.tick();
                last_tick = Instant::now();
            }
            if app.quit {
                break;
            }
            app.persist_sessions();
            let since = last_draw.elapsed();
            let due = (app.dirty && since >= Duration::from_millis(16))
                || (app.needs_animation() && since >= Duration::from_millis(100))
                || since >= Duration::from_secs(1);
            if due {
                let size = terminal.size()?;
                app.compute_layout(Rect::new(0, 0, size.width, size.height));
                terminal.draw(|f| ui::draw(f, &app))?;
                app.dirty = false;
                last_draw = Instant::now();
            }
        }
        Ok(())
    })();

    app.shutdown();
    let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
    ratatui::restore();
    let _ = std::fs::remove_file(sock);
    result
}
