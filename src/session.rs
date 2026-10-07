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
    /// Состояние git для агента (None — не репозиторий).
    Git(u32, Option<crate::git::Info>),
    /// Результат действия с git, выполненного в фоне (commit, push, merge…).
    GitDone {
        label: String,
        result: Result<String, String>,
    },
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
    /// Сколько длилась последняя завершённая работа (для ленты событий).
    pub last_worked: Option<Duration>,
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
    pub git: Option<crate::git::Info>,
    /// Идентификатор диалога Claude Code (для восстановления после перезапуска Radar).
    pub resume_id: Option<String>,
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

/// Ищет команду так, как её найдёт запускаемый агент: через login+interactive shell
/// (подхватывает PATH из .zprofile/.zshrc — nvm, brew и т. п.). Возвращает полный путь.
pub fn find_binary(bin: &str) -> Option<String> {
    if bin == "$SHELL" {
        return Some(std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()));
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(shell);
    // Интерактивный шелл без своей сессии захватывает терминал Radar (tcsetpgrp) — и Radar
    // получает SIGTTOU («suspended (tty output)»). setsid отрезает его от управляющего терминала.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.args(["-l", "-i", "-c", &format!("command -v {}", shq(bin))])
        .env("SHELL_SESSIONS_DISABLE", "1")
        .env_remove("TERM_SESSION_ID")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        // шелл может напечатать приветствие — путь всегда в последней строке
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .rfind(|l| l.starts_with('/'))
                .map(str::to_string)
        })
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
        resume: Option<&str>,
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
                if let Some(r) = resume {
                    line.push_str(" --resume ");
                    line.push_str(&shq(r));
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
            last_worked: None,
            subtitle: String::new(),
            note: String::new(),
            exit_code: None,
            scroll: 0,
            unread: false,
            muted: false,
            size,
            hooks_seen: false,
            git: None,
            resume_id: resume.map(String::from),
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

    /// Вопрос агента и варианты ответа так, как они показаны на его экране (для диалога разрешения).
    pub fn prompt_excerpt(&self) -> Vec<String> {
        let p = self.parser.lock().unwrap();
        let screen = p.screen();
        let (_, cols) = screen.size();
        let rows: Vec<String> = screen.rows(0, cols).collect();
        excerpt(&rows)
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
        if self.status == Status::Exited {
            return None;
        }
        if self.hooks_seen {
            // Хуки — отдельные процессы и могут прийти не по порядку или потеряться (последний
            // PostToolUse после Stop). Если «работа» по хукам, а экран давно молчит и подсказки
            // «esc to interrupt» нет — агент на самом деле закончил.
            if self.status == Status::Working && self.last_activity.elapsed() >= status::HOOK_STUCK {
                let tail = self.tail_text();
                if !status::has_working_hint(&tail) {
                    return self.set_status(Status::Idle);
                }
            }
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
        // события Gemini CLI приводим к общим именам
        let event = match event {
            "BeforeAgent" => "UserPromptSubmit",
            "BeforeTool" => "PreToolUse",
            "AfterTool" => "PostToolUse",
            "AfterAgent" => "Stop",
            e => e,
        };
        match event {
            "PermissionRequest" => {
                // что именно просит агент: «run_shell_command: touch zz.txt»
                let input = payload.get("tool_input");
                let detail = ["command", "file_path", "path", "url", "description"]
                    .iter()
                    .find_map(|k| input.and_then(|i| i.get(*k)).and_then(|v| v.as_str()))
                    .unwrap_or("");
                let tool = str_of("tool_name");
                let m = str_of("message");
                let text = match (tool.is_empty(), detail.is_empty()) {
                    (false, false) => format!("{tool}: {detail}"),
                    (false, true) => tool,
                    _ => m,
                };
                if !text.is_empty() {
                    self.note = text.replace('\n', " ").chars().take(160).collect();
                }
                self.set_status(Status::Waiting)
            }
            "SessionStart" => self.set_status(Status::Idle),
            "UserPromptSubmit" => {
                let p = str_of("prompt");
                if !p.trim().is_empty() {
                    self.subtitle = p.trim().chars().take(160).collect();
                }
                self.submitted = true;
                if self.kind == Kind::Claude {
                    let id = str_of("session_id");
                    if !id.is_empty() {
                        self.resume_id = Some(id);
                    }
                }
                self.unread = false;
                self.set_status(Status::Working)
            }
            "PreToolUse" | "PostToolUse" => self.set_status(Status::Working),
            "Notification" => match str_of("notification_type").as_str() {
                "idle_prompt" => self.set_status(Status::Idle),
                "auth_success" => None,
                _ => {
                    // более точное описание из PermissionRequest не затираем общим сообщением
                    if !(self.status == Status::Waiting && !self.note.is_empty()) {
                        self.note = str_of("message").chars().take(160).collect();
                    }
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
                self.last_worked = worked;
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

fn is_frame_char(c: char) -> bool {
    c.is_whitespace() || ('\u{2500}'..='\u{257F}').contains(&c)
}

/// Из строк экрана выбирает вопрос с вариантами: вокруг последнего списка «1. …».
/// Рамки и пустые строки отбрасываются; если списка нет — берутся последние строки.
/// Номер варианта в начале строки («› 2. …» → 2) и отмечена ли строка маркером выбора.
fn option_number(line: &str) -> Option<(usize, bool)> {
    const MARKS: &str = "›❯>●○→";
    let marked = line.trim_start().starts_with(|c| MARKS.contains(c));
    let t = line.trim_start_matches(|c: char| c.is_whitespace() || MARKS.contains(c));
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[digits.len()..];
    if digits.is_empty() || !(rest.starts_with('.') || rest.starts_with(')')) {
        return None;
    }
    Some((digits.parse().ok()?, marked))
}

/// Строки выборки, которые являются вариантами ответа (подряд идущая нумерация с 1),
/// и номер (по порядку) варианта, который агент сейчас выделил.
pub fn option_lines(excerpt: &[String]) -> (Vec<usize>, Option<usize>) {
    let mut idx = vec![];
    let mut hi = None;
    for (k, l) in excerpt.iter().enumerate() {
        if let Some((n, marked)) = option_number(l) {
            if n == idx.len() + 1 {
                if marked {
                    hi = Some(idx.len());
                }
                idx.push(k);
            }
        }
    }
    (idx, hi)
}

pub fn excerpt(rows: &[String]) -> Vec<String> {
    let lines: Vec<String> = rows
        .iter()
        .map(|r| r.trim_matches(is_frame_char).to_string())
        .filter(|r| !r.is_empty())
        .collect();
    let first_option = |s: &String| {
        let t = s.trim_start_matches(|c: char| c.is_whitespace() || "›❯>●○→".contains(c));
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits == "1" && t[1..].starts_with(|c| c == '.' || c == ')')
    };
    let (start, end) = match lines.iter().rposition(first_option) {
        Some(i) => {
            // вверх — до эха пользовательского запроса («> …») или ответа агента («● …»)
            let mut start = i;
            while start > 0 && i - start < 7 {
                let prev = &lines[start - 1];
                if prev.starts_with('>') || prev.starts_with('●') {
                    break;
                }
                start -= 1;
            }
            (start, (i + 9).min(lines.len()))
        }
        None => (lines.len().saturating_sub(8), lines.len()),
    };
    lines[start..end].to_vec()
}

#[cfg(test)]
mod excerpt_tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn picks_question_around_options() {
        let rows = v(&[
            "старый вывод агента",
            "│ ? Shell touch zz.txt │",
            "",
            "  Allow execution of: 'touch'?",
            "  › 1. Yes, allow once",
            "    2. Always allow",
            "    4. No, suggest changes (esc)",
            "──────────────",
            "radar-test · git:(main)",
        ]);
        let e = excerpt(&rows);
        assert!(e.iter().any(|l| l.contains("Allow execution")));
        assert!(e.iter().any(|l| l.starts_with("› 1.")));
        assert!(!e.iter().any(|l| l.chars().all(|c| c == '─')));
    }

    #[test]
    fn finds_options_and_highlight() {
        let e = v(&["Allow?", "  1. Yes", "› 2. Always", "  3. No", "⠏ Waiting"]);
        let (idx, hi) = option_lines(&e);
        assert_eq!(idx, vec![1, 2, 3]);
        assert_eq!(hi, Some(1));
        let (idx, hi) = option_lines(&v(&["просто текст", "1 штука"]));
        assert!(idx.is_empty() && hi.is_none());
    }

    #[test]
    fn stops_at_user_prompt_echo() {
        let rows = v(&["шум", "> создай файл", "? Shell touch zz.txt", "Allow execution?", "› 1. Yes", "2. No"]);
        let e = excerpt(&rows);
        assert_eq!(e[0], "? Shell touch zz.txt");
        assert_eq!(e.len(), 4);
    }

    #[test]
    fn falls_back_to_tail() {
        let rows: Vec<String> = (0..30).map(|i| format!("строка {i}")).collect();
        let e = excerpt(&rows);
        assert_eq!(e.len(), 8);
        assert_eq!(e.last().unwrap(), "строка 29");
    }
}
