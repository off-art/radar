//! Состояние приложения, обработка событий и главный цикл.

use crate::config::{AgentDef, Config};
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
}

pub enum Confirm {
    Close(usize),
    Quit,
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
    Confirm(Confirm),
    Help,
    Menu(Menu),
    Palette(Palette),
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
    /// Кнопка «+ новый» в шапке списка.
    pub new_btn: Rect,
    /// Переключатель уведомлений в строке статуса.
    pub notif_btn: Rect,
}

pub struct App {
    pub cfg: Config,
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub grid: bool,
    pub mode: Mode,
    pub geo: Geometry,
    pub toast: Option<(String, Instant)>,
    pub notifications: bool,
    pub quit: bool,
    pub dirty: bool,
    pub started: Instant,
    pub start_dir: PathBuf,
    /// Какие агенты установлены (None — ещё проверяется). Заполняется фоновыми потоками.
    available: Arc<Mutex<Vec<Option<bool>>>>,
    sidebar_first: usize,
    term_focused: bool,
    focus_supported: bool,
    last_key: Instant,
    next_id: u32,
    tx: Sender<Msg>,
    ctx: SpawnCtx,
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
        let mut app = App {
            cfg,
            sessions: vec![],
            selected: 0,
            grid: false,
            mode: Mode::Normal,
            geo: Geometry::default(),
            toast: None,
            notifications,
            quit: false,
            dirty: true,
            started: Instant::now(),
            start_dir,
            available: Arc::new(Mutex::new(vec![])),
            sidebar_first: 0,
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
        )?;
        self.sessions.push(s);
        self.selected = self.sessions.len() - 1;
        self.dirty = true;
        Ok(())
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
        let id = self.next_id;
        self.next_id += 1;
        match Session::spawn(id, &def, name, cwd, wt, self.pane_size(), self.tx.clone(), &self.ctx) {
            Ok(mut s) => {
                s.muted = muted;
                self.sessions[idx] = s;
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
            s.kill();
        }
    }

    pub fn select(&mut self, idx: usize) {
        if idx < self.sessions.len() {
            self.selected = idx;
            self.sessions[idx].unread = false;
            self.dirty = true;
        }
    }

    fn select_rel(&mut self, delta: isize) {
        let n = self.sessions.len() as isize;
        if n == 0 {
            return;
        }
        let i = (self.selected as isize + delta).rem_euclid(n) as usize;
        self.select(i);
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
        let sw = if area.width < 60 { 0 } else { (area.width / 4).clamp(26, 36) };
        let sidebar = Rect::new(body.x, body.y, sw, body.height);
        let main = Rect::new(body.x + sw, body.y, body.width - sw, body.height);

        // элементы списка: 3 строки на агента, 3 строки шапка
        let cap = (body.height.saturating_sub(3) / 3).max(1) as usize;
        if self.selected < self.sidebar_first {
            self.sidebar_first = self.selected;
        } else if self.selected >= self.sidebar_first + cap {
            self.sidebar_first = self.selected + 1 - cap;
        }
        if self.sidebar_first + cap > self.sessions.len() {
            self.sidebar_first = self.sessions.len().saturating_sub(cap);
        }
        let mut items = vec![];
        if sw > 0 {
            for (row, idx) in (self.sidebar_first..self.sessions.len()).take(cap).enumerate() {
                items.push((idx, Rect::new(sidebar.x, sidebar.y + 3 + row as u16 * 3, sw - 1, 2)));
            }
        }
        let new_btn = if sw > 12 {
            Rect::new(sidebar.x + sw - 1 - 9, sidebar.y, 9, 1)
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
        self.geo = Geometry { sidebar, main, status, panes, items, new_btn, notif_btn };
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
        let toast = format!("{} {} — {}", s.agent, s.name, verb);
        let muted = s.muted;
        self.toast(toast);
        if !viewing && self.notifications && !muted {
            notify::notify(sound, title, &body, &self.notify_settings());
        }
    }

    pub fn tick(&mut self) {
        for i in 0..self.sessions.len() {
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
            Msg::Exited(id, code) => {
                if let Some(i) = self.sessions.iter().position(|s| s.id == id) {
                    if let Some(a) = self.sessions[i].mark_exited(code) {
                        self.handle_attention(i, a);
                    }
                    if !self.is_viewing(i) {
                        self.sessions[i].unread = true;
                    }
                    self.dirty = true;
                }
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
        e.push(PaletteEntry { title: "Помощь и горячие клавиши".into(), hint: "?".into(), action: Action::Help });
        e.push(PaletteEntry { title: "Выйти из Radar".into(), hint: "q".into(), action: Action::Quit });
        self.mode = Mode::Palette(Palette { input: TextField::default(), entries: e, sel: 0 });
    }

    fn session_menu(&self, idx: usize, from_sidebar: bool, x: u16, y: u16) -> Menu {
        let s = &self.sessions[idx];
        let mut items = vec![];
        if from_sidebar {
            items.push(MenuItem::new("Открыть", Action::Select(idx), ""));
            items.push(MenuItem::sep());
        }
        items.push(MenuItem::new("Переименовать…", Action::Rename, "r"));
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
        items.extend([
            MenuItem::new("Новый агент…", Action::NewAgent, "n"),
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
            MenuItem::new("Помощь", Action::Help, "?"),
            MenuItem::new("Выйти", Action::Quit, "q"),
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
            Mode::Confirm(c) => self.key_confirm(c, k),
            Mode::Menu(m) => self.key_menu(m, k),
            Mode::Palette(p) => self.key_palette(p, k),
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

    fn key_confirm(&mut self, c: Confirm, k: KeyEvent) {
        if matches!(k.code, KeyCode::Char('y' | 'Y' | 'н' | 'Н') | KeyCode::Enter) {
            match c {
                Confirm::Close(i) => self.close(i),
                Confirm::Quit => self.quit = true,
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
            KeyCode::Tab | KeyCode::Down => f.field = (f.field + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => f.field = (f.field + 3) % 4,
            _ => {
                f.error = None;
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
            MouseEventKind::Down(MouseButton::Left) => {
                self.mode = Mode::Normal;
                if in_rect(&self.geo.new_btn, x, y) {
                    self.open_new_form(None);
                } else if in_rect(&self.geo.notif_btn, x, y) {
                    self.run_action(Action::ToggleNotifications);
                } else if let Some(&(idx, _)) = self.geo.items.iter().find(|(_, r)| in_rect(r, x, y)) {
                    self.select(idx);
                } else if let Some((idx, inner)) = pane {
                    if idx == self.selected {
                        self.forward_mouse(idx, inner, MouseEv::Down(0), mods, x, y);
                    } else {
                        self.select(idx);
                    }
                }
            }
            MouseEventKind::Down(b @ MouseButton::Middle) | MouseEventKind::Up(b @ (MouseButton::Left | MouseButton::Middle)) => {
                if let Some((idx, inner)) = pane.filter(|(i, _)| *i == self.selected) {
                    let n = if b == MouseButton::Left { 0 } else { 1 };
                    let ev = if matches!(m.kind, MouseEventKind::Down(_)) { MouseEv::Down(n) } else { MouseEv::Up(n) };
                    self.forward_mouse(idx, inner, ev, mods, x, y);
                }
            }
            MouseEventKind::Drag(b @ (MouseButton::Left | MouseButton::Middle)) => {
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

    /// Колесо: агенту с включённой мышью — событие; полноэкранным программам (less, vim, htop) —
    /// стрелки; обычному выводу — прокрутка истории Radar.
    fn wheel(&mut self, idx: usize, inner: Rect, up: bool, mods: KeyModifiers, x: u16, y: u16) {
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

    fn context_menu(&mut self, x: u16, y: u16) {
        let item = self.geo.items.iter().find(|(_, r)| in_rect(r, x, y)).map(|(i, _)| *i);
        let pane = self.pane_at(x, y).map(|p| p.idx);
        let menu = if let Some(idx) = item {
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
