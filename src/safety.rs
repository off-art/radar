//! Защита от сюрпризов при работе с системой: безопасный режим, трассировка, ограничение частоты
//! и таймауты внешних команд (уведомления, звук).
//!
//! Появилось из-за сообщения о падении графической сессии GNOME на Linux при включённых уведомлениях.
//! Причину воспроизвести не удалось, поэтому здесь собраны меры, которые исключают самые вероятные
//! источники проблем: поток однотипных уведомлений, зависшие внешние команды, непригодный текст и
//! отсутствие следов в момент сбоя.

use crate::paths::config_dir;
use crate::sync::MutexExt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ───────────── безопасный режим ─────────────

static SAFE: AtomicBool = AtomicBool::new(false);

/// Безопасный режим: никаких всплывающих уведомлений и звука.
pub fn set_safe(on: bool) {
    SAFE.store(on, Ordering::Relaxed);
}

pub fn is_safe() -> bool {
    SAFE.load(Ordering::Relaxed)
}

fn env_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Включён ли безопасный режим переменной `RADAR_SAFE`.
pub fn safe_from_env() -> bool {
    env_on("RADAR_SAFE")
}

// ───────────── трассировка ─────────────

const TRACE_MAX_BYTES: u64 = 1_000_000;

/// Включена ли трассировка (`RADAR_TRACE=1`).
pub fn trace_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_on("RADAR_TRACE"))
}

/// Файл трассировки: `~/.config/radar/trace.log`.
pub fn trace_path() -> std::path::PathBuf {
    config_dir().join("trace.log")
}

/// Время UTC `ЧЧ:ММ:СС.ммм` из миллисекунд от начала эпохи.
pub fn utc_clock(epoch_ms: u128) -> String {
    let ms = (epoch_ms % 1000) as u32;
    let secs = (epoch_ms / 1000) as u64;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// Дописывает строку в файл трассировки и сбрасывает её на диск: запись должна пережить сбой сеанса.
pub fn write_trace(path: &Path, msg: &str) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > TRACE_MAX_BYTES) {
        let _ = std::fs::remove_file(path);
    }
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let _ = writeln!(f, "{}Z pid={} {msg}", utc_clock(now), std::process::id());
    let _ = f.sync_data();
}

/// Запись в трассировку; ничего не делает, пока она не включена.
pub fn trace(msg: &str) {
    if trace_enabled() {
        write_trace(&trace_path(), msg);
    }
}

// ───────────── ограничение частоты ─────────────

/// Пропускает не чаще одного события за `min_gap` и отбрасывает повтор того же ключа в течение `dedup`.
pub struct Throttle {
    min_gap: Duration,
    dedup: Duration,
    last: Option<Instant>,
    recent: Vec<(String, Instant)>,
}

impl Throttle {
    pub fn new(min_gap: Duration, dedup: Duration) -> Self {
        Throttle { min_gap, dedup, last: None, recent: Vec::new() }
    }

    pub fn allow(&mut self, key: &str, now: Instant) -> bool {
        let dedup = self.dedup;
        self.recent.retain(|(_, t)| now.saturating_duration_since(*t) < dedup);
        if self.recent.iter().any(|(k, _)| k == key) {
            return false;
        }
        if self.last.is_some_and(|l| now.saturating_duration_since(l) < self.min_gap) {
            return false;
        }
        self.last = Some(now);
        self.recent.push((key.to_string(), now));
        true
    }
}

fn popups() -> &'static Mutex<Throttle> {
    static T: OnceLock<Mutex<Throttle>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Throttle::new(Duration::from_secs(2), Duration::from_secs(15))))
}

fn sounds() -> &'static Mutex<Throttle> {
    static T: OnceLock<Mutex<Throttle>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Throttle::new(Duration::from_millis(500), Duration::ZERO)))
}

/// Можно ли сейчас показать всплывающее уведомление с таким текстом?
pub fn popup_allowed(key: &str) -> bool {
    popups().lock_or_recover().allow(key, Instant::now())
}

/// Можно ли сейчас проиграть звук?
pub fn sound_allowed() -> bool {
    sounds().lock_or_recover().allow("sound", Instant::now())
}

// ───────────── текст уведомления ─────────────

/// Убирает управляющие символы и лишние пробелы, обрезает до `max` символов.
pub fn clean_text(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= max {
        return joined;
    }
    let mut cut: String = joined.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Экранирует символы разметки: уведомления GNOME разбирают текст как Pango-разметку.
pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// ───────────── внешние команды ─────────────

/// Запускает команду без ввода-вывода и ждёт не дольше `timeout`; зависшую команду убивает.
/// `Ok(None)` — команда не уложилась во время.
pub fn run_quiet(cmd: &mut Command, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn clock_formats_utc() {
        assert_eq!(utc_clock(0), "00:00:00.000");
        assert_eq!(utc_clock(3_723_456), "01:02:03.456");
        assert_eq!(utc_clock(86_400_000 + 1_000), "00:00:01.000");
    }

    #[test]
    fn throttle_gap_and_dedup() {
        let t0 = Instant::now();
        let mut t = Throttle::new(Duration::from_secs(2), Duration::from_secs(15));
        assert!(t.allow("a", t0));
        assert!(!t.allow("b", t0 + Duration::from_secs(1)), "слишком скоро после предыдущего");
        assert!(t.allow("b", t0 + Duration::from_secs(3)));
        assert!(!t.allow("a", t0 + Duration::from_secs(6)), "повтор того же текста");
        assert!(t.allow("a", t0 + Duration::from_secs(20)), "окно повтора прошло");
    }

    #[test]
    fn throttle_without_dedup_only_spaces_events() {
        let t0 = Instant::now();
        let mut t = Throttle::new(Duration::from_millis(500), Duration::ZERO);
        assert!(t.allow("x", t0));
        assert!(!t.allow("x", t0 + Duration::from_millis(100)));
        assert!(t.allow("x", t0 + Duration::from_millis(600)));
    }

    #[test]
    fn text_is_cleaned_and_truncated() {
        assert_eq!(clean_text("  Claude \n\t Code\x1b[31m · api\u{7}  ", 80), "Claude Code [31m · api");
        assert_eq!(clean_text("абвгде", 4), "абв…");
        assert_eq!(clean_text("abc", 3), "abc");
        assert_eq!(clean_text("", 5), "");
    }

    #[test]
    fn markup_is_escaped() {
        assert_eq!(escape_markup("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(escape_markup("-x"), "-x");
    }

    #[cfg(unix)]
    #[test]
    fn run_quiet_waits_and_kills() {
        let ok = run_quiet(&mut Command::new("true"), Duration::from_secs(5)).unwrap();
        assert!(ok.is_some_and(|s| s.success()));
        let start = Instant::now();
        let hung = run_quiet(Command::new("sleep").arg("30"), Duration::from_millis(200)).unwrap();
        assert!(hung.is_none(), "зависшая команда должна быть убита по таймауту");
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(run_quiet(&mut Command::new("radar-no-such-command"), Duration::from_secs(1)).is_err());
    }

    #[test]
    fn trace_appends_and_rotates() {
        let dir = TempDir::new("trace");
        let p = dir.join("trace.log");
        write_trace(&p, "первая");
        write_trace(&p, "вторая");
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("первая") && text.contains("вторая") && text.contains("pid="));
        std::fs::write(&p, vec![b'x'; (TRACE_MAX_BYTES + 10) as usize]).unwrap();
        write_trace(&p, "после");
        assert_eq!(std::fs::read_to_string(&p).unwrap().lines().count(), 1, "большой файл начинается заново");
    }

    #[test]
    fn safe_flag_roundtrip() {
        set_safe(true);
        assert!(is_safe());
        set_safe(false);
        assert!(!is_safe());
    }
}
