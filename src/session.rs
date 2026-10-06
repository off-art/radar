//! Сессия агента: pty + эмулятор терминала + конечный автомат статусов.

use crate::config::{AgentDef, Kind};
use crate::status::{self, Signals, Status};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use ratatui::style::Color;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Сообщения фоновых потоков главному циклу.
pub enum Msg {
    Output(u32),
    Exited(u32, i32),
    Hook {
        session: u32,
        event: String,
        payload: serde_json::Value,
    },
}

/// Событие, о котором стоит уведомить человека.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attention {
    NeedsInput,
    Done,
}

pub struct SpawnCtx {
    pub sock: PathBuf,
    pub claude_settings: Option<PathBuf>,
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

pub struct Session {
    pub id: u32,
    pub name: String,
    pub agent: String,
    pub color: Color,
    pub kind: Kind,
    pub cwd: PathBuf,
    pub worktree: Option<PathBuf>,
    pub parser: Arc<Mutex<vt100::Parser>>,
    pub status: Status,
    pub status_since: Instant,
    pub work_started: Option<Instant>,
    pub subtitle: String,
    /// Пояснение, пока агент ждёт ответа (например, какое разрешение просит Claude).
    pub note: String,
    pub exit_code: Option<i32>,
    pub scroll: usize,
    pub unread: bool,
    /// Уведомления и звук для этого агента выключены.
    pub muted: bool,
    pub size: (u16, u16),
    pub hooks_seen: bool,
    submitted: bool,
    typed: String,
    last_activity: Instant,
    ignore_until: Instant,
    writer: Writer,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

pub fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Отвечает на запросы терминалу (DA, DSR), на которые агенты иногда ждут ответ.
fn answer_queries(data: &[u8], writer: &Writer, parser: &Arc<Mutex<vt100::Parser>>) {
    let mut reply: Vec<u8> = Vec::new();
    if contains(data, b"\x1b[c") || contains(data, b"\x1b[0c") {
        reply.extend_from_slice(b"\x1b[?62;c");
    }
    if contains(data, b"\x1b[>c") || contains(data, b"\x1b[>0c") {
        reply.extend_from_slice(b"\x1b[>0;0;0c");
    }
    if contains(data, b"\x1b[5n") {
        reply.extend_from_slice(b"\x1b[0n");
    }
    if contains(data, b"\x1b[6n") {
        let (r, c) = parser.lock().unwrap().screen().cursor_position();
        reply.extend_from_slice(format!("\x1b[{};{}R", r + 1, c + 1).as_bytes());
    }
    if !reply.is_empty() {
        if let Ok(mut w) = writer.lock() {
            let _ = w.write_all(&reply);
            let _ = w.flush();
        }
    }
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        id: u32,
        def: &AgentDef,
        name: String,
        cwd: PathBuf,
        worktree: Option<PathBuf>,
        size: (u16, u16),
        tx: Sender<Msg>,
        ctx: &SpawnCtx,
    ) -> Result<Session> {
        let (rows, cols) = size;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("не удалось открыть pty")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = CommandBuilder::new(&shell);
        if def.kind == Kind::Shell {
            cmd.arg("-l");
        } else {
            let mut line = format!("exec {}", def.command);
            for a in &def.args {
                line.push(' ');
                line.push_str(&shq(a));
            }
            if def.kind == Kind::Claude {
                if let Some(s) = &ctx.claude_settings {
                    line.push_str(" --settings ");
                    line.push_str(&shq(&s.to_string_lossy()));
                }
            }
            // login + interactive: подхватываем PATH из .zprofile/.zshrc (nvm, brew и т.п.)
            cmd.args(["-l", "-i", "-c", &line]);
        }
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("RADAR_SESSION", id.to_string());
        cmd.env("RADAR_SOCK", &ctx.sock);
        cmd.env_remove("CLAUDECODE");
        // Не наследуем переменные терминала-родителя: иначе zsh в Apple Terminal печатает
        // «Restored session…», а агенты могут принять Radar за iTerm/VS Code.
        for k in ["TERM_PROGRAM_VERSION", "TERM_SESSION_ID", "ITERM_SESSION_ID", "WARP_SESSION_ID"] {
            cmd.env_remove(k);
        }
        cmd.env("TERM_PROGRAM", "radar");
        cmd.env("SHELL_SESSIONS_DISABLE", "1");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("не удалось запустить {shell}"))?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let killer = child.clone_killer();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 5000)));

        {
            let parser = parser.clone();
            let writer = writer.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            parser.lock().unwrap().process(&buf[..n]);
                            answer_queries(&buf[..n], &writer, &parser);
                            if tx.send(Msg::Output(id)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
                let _ = tx.send(Msg::Exited(id, code));
            });
        }

        let now = Instant::now();
        Ok(Session {
            id,
            name,
            agent: def.name.clone(),
            color: def.color,
            kind: def.kind,
            cwd,
            worktree,
            parser,
            status: Status::Starting,
            status_since: now,
            work_started: None,
            subtitle: String::new(),
            note: String::new(),
            exit_code: None,
            scroll: 0,
            unread: false,
            muted: false,
            size,
            hooks_seen: false,
            submitted: false,
            typed: String::new(),
            last_activity: now,
            ignore_until: now,
            writer,
            master: pair.master,
            killer,
        })
    }

    pub fn is_running(&self) -> bool {
        self.status != Status::Exited
    }

    pub fn app_cursor(&self) -> bool {
        self.parser.lock().unwrap().screen().application_cursor()
    }

    pub fn bracketed_paste(&self) -> bool {
        self.parser.lock().unwrap().screen().bracketed_paste()
    }

    /// Режимы мыши, которые включил сам агент, и находится ли он на альтернативном экране.
    pub fn mouse_state(&self) -> (vt100::MouseProtocolMode, vt100::MouseProtocolEncoding, bool) {
        let p = self.parser.lock().unwrap();
        let sc = p.screen();
        (sc.mouse_protocol_mode(), sc.mouse_protocol_encoding(), sc.alternate_screen())
    }

    /// Сколько строк истории можно прокрутить.
    pub fn max_scrollback(&self) -> usize {
        let mut p = self.parser.lock().unwrap();
        p.screen_mut().set_scrollback(usize::MAX);
        let n = p.screen().scrollback();
        p.screen_mut().set_scrollback(0);
        n
    }

    /// Прокрутка истории (вверх — к старому), с ограничением реальной длиной истории.
    pub fn scroll_by(&mut self, up: bool, lines: usize) {
        self.scroll = if up {
            (self.scroll + lines).min(self.max_scrollback())
        } else {
            self.scroll.saturating_sub(lines)
        };
    }

    /// Сырые байты агенту (события мыши и т. п.), без побочных эффектов ввода.
    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.ignore_until = Instant::now() + Duration::from_millis(350);
        self.write_raw(bytes);
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        if !self.is_running() {
            return;
        }
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Ввод пользователя (клавиши) — уходит агенту.
    pub fn send_input(&mut self, bytes: &[u8], typed: Option<char>, submit: bool, backspace: bool) {
        let now = Instant::now();
        self.scroll = 0;
        // эхо набранных символов не считаем активностью агента
        self.ignore_until = now + Duration::from_millis(350);
        if let Some(c) = typed {
            self.typed.push(c);
        }
        if backspace {
            self.typed.pop();
        }
        if submit {
            self.submitted = true;
            if self.kind != Kind::Shell {
                self.last_activity = now;
            }
            // подпись берём из хука (Claude) либо из набранного текста; ответы на вопросы («y») — не задача
            let t = self.typed.trim();
            if !t.is_empty() && !self.hooks_seen && self.status != Status::Waiting {
                self.subtitle = t.chars().take(160).collect();
            }
            self.typed.clear();
            self.unread = false;
        }
        self.write_raw(bytes);
    }

    pub fn paste(&mut self, text: &str) {
        self.scroll = 0;
        self.ignore_until = Instant::now() + Duration::from_millis(350);
        self.typed.push_str(&text.replace('\n', " "));
        if self.bracketed_paste() {
            let mut v = b"\x1b[200~".to_vec();
            v.extend_from_slice(text.as_bytes());
            v.extend_from_slice(b"\x1b[201~");
            self.write_raw(&v);
        } else {
            let t = text.replace('\n', "\r");
            self.write_raw(t.as_bytes());
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 || (rows, cols) == self.size {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.lock().unwrap().screen_mut().set_size(rows, cols);
        self.ignore_until = Instant::now() + Duration::from_millis(1500);
    }

    /// Вызывается, когда агент что-то вывел.
    pub fn on_output(&mut self) {
        let now = Instant::now();
        if now >= self.ignore_until {
            self.last_activity = now;
        }
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }

    pub fn mark_exited(&mut self, code: i32) -> Option<Attention> {
        self.exit_code = Some(code);
        if code == 127 {
            self.subtitle = "команда не найдена: проверьте, что агент установлен".to_string();
        }
        let a = self.set_status(Status::Exited);
        self.work_started = None;
        a
    }

    /// Нижние строки экрана агента (для эвристики).
    fn tail_text(&self) -> String {
        let p = self.parser.lock().unwrap();
        let screen = p.screen();
        let (_, cols) = screen.size();
        let rows: Vec<String> = screen
            .rows(0, cols)
            .filter(|r| !r.trim().is_empty())
            .collect();
        let start = rows.len().saturating_sub(16);
        rows[start..].join("\n")
    }

    /// Периодическая проверка статуса для агентов без хуков.
    pub fn tick(&mut self) -> Option<Attention> {
        if self.status == Status::Exited || self.hooks_seen {
            return None;
        }
        let tail = self.tail_text();
        let sig = Signals {
            tail: &tail,
            since_activity: self.last_activity.elapsed(),
            submitted: self.submitted,
        };
        let next = status::decide(self.status, &sig);
        self.set_status(next)
    }

    /// Событие от хука Claude Code.
    pub fn apply_hook(&mut self, event: &str, payload: &serde_json::Value) -> Option<Attention> {
        if self.status == Status::Exited {
            return None;
        }
        self.hooks_seen = true;
        let str_of = |k: &str| payload.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match event {
            "SessionStart" => self.set_status(Status::Idle),
            "UserPromptSubmit" => {
                let p = str_of("prompt");
                if !p.trim().is_empty() {
                    self.subtitle = p.trim().chars().take(160).collect();
                }
                self.submitted = true;
                self.unread = false;
                self.set_status(Status::Working)
            }
            "PreToolUse" | "PostToolUse" => self.set_status(Status::Working),
            "Notification" => match str_of("notification_type").as_str() {
                "idle_prompt" => self.set_status(Status::Idle),
                "auth_success" => None,
                _ => {
                    self.note = str_of("message").chars().take(160).collect();
                    self.set_status(Status::Waiting)
                }
            },
            "Stop" => self.set_status(Status::Idle),
            _ => None,
        }
    }

    /// Меняет статус и сообщает, нужно ли привлечь внимание человека.
    pub fn set_status(&mut self, new: Status) -> Option<Attention> {
        if new == self.status {
            return None;
        }
        let prev = self.status;
        let now = Instant::now();
        self.status = new;
        self.status_since = now;
        if new != Status::Waiting {
            self.note.clear();
        }
        match new {
            Status::Working => {
                if self.work_started.is_none() {
                    self.work_started = Some(now);
                }
                None
            }
            Status::Waiting => Some(Attention::NeedsInput),
            Status::Idle => {
                let worked = self.work_started.take().map(|t| t.elapsed());
                match (prev, worked) {
                    (Status::Working | Status::Waiting, Some(d)) if d >= Duration::from_secs(5) => {
                        Some(Attention::Done)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
