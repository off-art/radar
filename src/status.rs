//! Определение состояния агента.
//!
//! Для Claude Code статус приходит точно — через хуки (см. `hook.rs`).
//! Для остальных агентов (Codex, OpenCode, Qwen, Gemini, ...) используется
//! эвристика по содержимому экрана и активности вывода.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Working,
    Waiting,
    Idle,
    Exited,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Starting => "запуск",
            Status::Working => "работает",
            Status::Waiting => "ждёт ответа",
            Status::Idle => "готов",
            Status::Exited => "завершён",
        }
    }
}

/// Тишина в выводе, после которой агент считается закончившим работу.
pub const QUIET: Duration = Duration::from_millis(2000);
/// Тишина, после которой «работает» по хукам считается застрявшим статусом (хук Stop потерялся).
pub const HOOK_STUCK: Duration = Duration::from_secs(6);
/// Сколько экран должен «молчать», чтобы вопрос считался ожидающим ответа.
pub const PROMPT_SETTLE: Duration = Duration::from_millis(700);

/// Фразы (в нижнем регистре), по которым видно, что агент ждёт решения человека.
const WAITING: &[&str] = &[
    "(y/n)",
    "[y/n]",
    "(yes/no)",
    "[yes/no]",
    "do you want to proceed",
    "do you want to make this edit",
    "do you want to create",
    "do you want to allow",
    "do you want to run",
    "would you like to run",
    "would you like to make the following edits",
    "allow once",
    "yes, allow",
    "waiting for approval",
    "waiting for confirmation",
    "requires approval",
    "press enter to continue",
    "press enter to confirm",
    "tab to amend",
    "do you trust the files",
    "trust this folder",
    "❯ 1. yes",
    "> 1. yes",
];

/// Подсказки, которые агенты показывают только пока работают.
const WORKING: &[&str] = &[
    "esc to interrupt",
    "ctrl+c to interrupt",
    "ctrl-c to interrupt",
    "esc to stop",
    "ctrl+c to stop",
    "(esc to cancel",
    "press esc to interrupt",
];

pub fn has_prompt(tail: &str) -> bool {
    let l = tail.to_lowercase();
    WAITING.iter().any(|p| l.contains(p))
}

pub fn has_working_hint(tail: &str) -> bool {
    let l = tail.to_lowercase();
    WORKING.iter().any(|p| l.contains(p))
}

pub struct Signals<'a> {
    /// Нижние строки экрана агента.
    pub tail: &'a str,
    /// Время с последней «настоящей» активности вывода (без эха ввода и ресайзов).
    pub since_activity: Duration,
    /// Пользователь уже отправил агенту хотя бы одну команду.
    pub submitted: bool,
}

/// Эвристика для агентов без хуков.
pub fn decide(prev: Status, s: &Signals) -> Status {
    if prev == Status::Exited {
        return Status::Exited;
    }
    if has_prompt(s.tail) && s.since_activity >= PROMPT_SETTLE {
        return Status::Waiting;
    }
    if has_working_hint(s.tail) {
        return Status::Working;
    }
    if s.since_activity < QUIET {
        // Стартовый вывод агента — это ещё не работа над задачей.
        if prev == Status::Starting && !s.submitted {
            return Status::Starting;
        }
        return Status::Working;
    }
    Status::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(tail: &str, ms: u64, submitted: bool) -> Signals<'_> {
        Signals {
            tail,
            since_activity: Duration::from_millis(ms),
            submitted,
        }
    }

    #[test]
    fn prompt_detected_after_settle() {
        let t = "Do you want to proceed?\n❯ 1. Yes\n  2. No";
        assert_eq!(decide(Status::Working, &sig(t, 900, true)), Status::Waiting);
        // пока вывод ещё идёт — считаем, что агент работает
        assert_eq!(decide(Status::Working, &sig(t, 100, true)), Status::Working);
    }

    #[test]
    fn working_hint_wins_over_silence() {
        let t = "✻ Thinking… (esc to interrupt)";
        assert_eq!(decide(Status::Idle, &sig(t, 5000, true)), Status::Working);
    }

    #[test]
    fn quiet_means_idle() {
        assert_eq!(decide(Status::Working, &sig("> ", 2500, true)), Status::Idle);
    }

    #[test]
    fn startup_output_is_not_work() {
        assert_eq!(decide(Status::Starting, &sig("Welcome", 300, false)), Status::Starting);
        assert_eq!(decide(Status::Starting, &sig("Welcome", 2500, false)), Status::Idle);
    }

    #[test]
    fn startup_trust_dialog_is_waiting() {
        let t = "Do you trust the files in this folder?";
        assert_eq!(decide(Status::Starting, &sig(t, 1000, false)), Status::Waiting);
    }

    #[test]
    fn exited_is_sticky() {
        assert_eq!(decide(Status::Exited, &sig("", 0, true)), Status::Exited);
    }
}
