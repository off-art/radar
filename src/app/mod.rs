//! Состояние приложения, обработка событий и главный цикл.

use crate::config::Config;
use crate::menu::{filter_palette, Menu, PaletteEntry};
use crate::notify;
use crate::session::{Msg, Session, SpawnCtx};
use crate::sync::MutexExt;
use crate::textfield::TextField;
use crossterm::event::KeyModifiers;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
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

mod actions;
mod approve;
mod attention;
mod git_actions;
mod groups;
mod keys;
mod layout;
mod mouse;
mod remote;
mod run;
mod sessions;
mod settings;
mod views;

pub use run::run;

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
        *self.available.lock_or_recover() = vec![None; self.cfg.agents.len()];
        let bins: Vec<String> = self.cfg.agents.iter().map(|d| d.bin().to_string()).collect();
        let shared = self.available.clone();
        std::thread::spawn(move || {
            let refs: Vec<&str> = bins.iter().map(String::as_str).collect();
            let found = crate::session::find_binaries(&refs);
            for (slot, f) in shared.lock_or_recover().iter_mut().zip(found) {
                *slot = Some(f.is_some());
            }
        });
    }

    /// Установлен ли агент (пока проверка не закончилась — считаем, что да).
    pub fn agent_available(&self, i: usize) -> bool {
        self.available.lock_or_recover().get(i).copied().flatten().unwrap_or(true)
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

    fn term_size(&self) -> (u16, u16) {
        let s = self.geo.status;
        (s.right().max(1), s.bottom().max(1))
    }
}

fn in_rect(r: &Rect, x: u16, y: u16) -> bool {
    r.width > 0 && x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}
