//! Состояние git-репозитория агента: ветка, число изменений, +/− строк.
//!
//! Опрашивается в фоновом потоке (раз в несколько секунд и сразу после завершения задачи),
//! чтобы не тормозить интерфейс. Не-git папки просто не показывают git-информацию.

use crate::session::Msg;
use crate::sync::MutexExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Info {
    /// Имя ветки или `@abc1234` для отсоединённого HEAD.
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    /// Файлов с изменениями (включая новые).
    pub files: u32,
    pub added: u32,
    pub removed: u32,
}

impl Info {
    pub fn dirty(&self) -> bool {
        self.files > 0
    }

    /// Короткая сводка для списка: `⎇ main ●3 +12 −4`.
    pub fn summary(&self) -> String {
        let mut s = format!("⎇ {}", self.branch);
        if self.ahead > 0 {
            s.push_str(&format!(" ↑{}", self.ahead));
        }
        if self.behind > 0 {
            s.push_str(&format!(" ↓{}", self.behind));
        }
        if self.files > 0 {
            s.push_str(&format!(" ●{}", self.files));
        }
        if self.added > 0 || self.removed > 0 {
            s.push_str(&format!(" +{} −{}", self.added, self.removed));
        }
        s
    }
}

/// Результат запуска git.
struct Captured {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// Читает поток до конца в отдельном потоке: большой вывод (diff) иначе заполнит канал, и git зависнет.
fn drain(mut pipe: impl std::io::Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// Запускает git и ждёт не дольше `limit`. Никогда не ждёт ввода: запрос пароля отключён, ssh — в пакетном режиме.
fn capture(dir: &Path, args: &[&str], limit: Duration) -> Result<Captured, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let mut child = cmd.spawn().map_err(|_| "git не найден".to_string())?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("не удалось прочитать вывод git".into());
    };
    let (out, err) = (drain(out), drain(err));
    let deadline = Instant::now() + limit;
    let wait = |rx: &Receiver<Vec<u8>>| rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    let (Ok(stdout), Ok(stderr)) = (wait(&out), wait(&err)) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("git не ответил за {} с", limit.as_secs()));
    };
    let ok = child.wait().is_ok_and(|s| s.success());
    Ok(Captured {
        ok,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Запускает git с таймаутом 3 с; возвращает stdout при успехе.
pub fn run(dir: &Path, args: &[&str]) -> Option<String> {
    run_timeout(dir, args, Duration::from_secs(3))
}

pub fn run_timeout(dir: &Path, args: &[&str], limit: Duration) -> Option<String> {
    capture(dir, args, limit).ok().filter(|c| c.ok).map(|c| c.stdout)
}

/// Выполняет git для действий пользователя (commit, push, merge…): возвращает вывод или текст ошибки.
pub fn exec(dir: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    let c = capture(dir, args, limit)?;
    let (so, se) = (c.stdout.trim(), c.stderr.trim());
    if c.ok {
        Ok(if so.is_empty() { se } else { so }.to_string())
    } else {
        Err(match (se.is_empty(), so.is_empty()) {
            (false, _) => se.to_string(),
            (true, false) => so.to_string(),
            (true, true) => "git завершился с ошибкой".to_string(),
        })
    }
}

/// Разбирает вывод `git status --porcelain=v2 -b`.
fn parse_status(text: &str) -> Info {
    let mut info = Info::default();
    let mut oid = String::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("# branch.head ") {
            info.branch = h.to_string();
        } else if let Some(o) = line.strip_prefix("# branch.oid ") {
            oid = o.to_string();
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    info.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    info.behind = n.parse().unwrap_or(0);
                }
            }
        } else if matches!(line.as_bytes().first(), Some(b'1' | b'2' | b'u' | b'?')) {
            info.files += 1;
        }
    }
    if info.branch == "(detached)" {
        info.branch = format!("@{}", oid.chars().take(7).collect::<String>());
    }
    info
}

/// Суммирует вывод `git diff --numstat` (бинарные файлы — `-`, считаются нулём).
fn parse_numstat(text: &str) -> (u32, u32) {
    let (mut a, mut r) = (0u32, 0u32);
    for line in text.lines() {
        let mut it = line.split('\t');
        a += it.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
        r += it.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    }
    (a, r)
}

/// Строки в новых (неотслеживаемых) текстовых файлах: `git diff` их не считает.
fn untracked_lines(dir: &Path) -> u32 {
    let Some(list) = run(dir, &["ls-files", "-o", "--exclude-standard", "-z"]) else {
        return 0;
    };
    let mut total = 0u32;
    for rel in list.split('\0').filter(|s| !s.is_empty()).take(200) {
        let p = dir.join(rel);
        let Ok(m) = std::fs::metadata(&p) else { continue };
        if !m.is_file() || m.len() > 200_000 {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&p) {
            if !bytes.contains(&0) {
                total += String::from_utf8_lossy(&bytes).lines().count() as u32;
            }
        }
    }
    total
}

/// Информация о репозитории в папке; `None`, если это не git-репозиторий.
pub fn query(dir: &Path) -> Option<Info> {
    let status = run(dir, &["status", "--porcelain=v2", "-b"])?;
    let mut info = parse_status(&status);
    if info.branch.is_empty() {
        return None;
    }
    if info.files > 0 {
        // без коммитов HEAD нет — тогда считаем по индексу/рабочему дереву
        let num = run(dir, &["diff", "HEAD", "--numstat"]).or_else(|| run(dir, &["diff", "--numstat"]));
        if let Some(n) = num {
            (info.added, info.removed) = parse_numstat(&n);
        }
        info.added += untracked_lines(dir);
    }
    Some(info)
}

/// Плановое обновление git-состояния, даже если никто не просил.
const REFRESH: Duration = Duration::from_secs(4);
/// Не чаще одного опроса за это время: серия сигналов схлопывается в один опрос.
const MIN_GAP: Duration = Duration::from_millis(300);

/// Фоновый опрос git для списка агентов. Поток ждёт на канале и просыпается по сигналу
/// (`poke`, смена списка) или раз в `REFRESH`; когда `Watcher` уничтожен, поток завершается.
pub struct Watcher {
    targets: Arc<Mutex<Vec<(u32, PathBuf)>>>,
    wake: Sender<()>,
}

impl Watcher {
    pub fn start(tx: Sender<Msg>) -> Watcher {
        let targets = Arc::new(Mutex::new(Vec::new()));
        let (wake, signals) = mpsc::channel::<()>();
        let shared = Arc::clone(&targets);
        std::thread::spawn(move || loop {
            let list: Vec<(u32, PathBuf)> = shared.lock_or_recover().clone();
            let mut seen: Vec<(PathBuf, Option<Info>)> = vec![];
            for (id, dir) in list {
                let info = match seen.iter().find(|(d, _)| *d == dir) {
                    Some((_, i)) => i.clone(),
                    None => {
                        let i = query(&dir);
                        seen.push((dir, i.clone()));
                        i
                    }
                };
                if tx.send(Msg::Git(id, info)).is_err() {
                    return; // интерфейс закрыт
                }
            }
            std::thread::sleep(MIN_GAP);
            match signals.recv_timeout(REFRESH) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => while signals.try_recv().is_ok() {},
                Err(RecvTimeoutError::Disconnected) => return, // Watcher уничтожен
            }
        });
        Watcher { targets, wake }
    }

    pub fn set_targets(&self, list: Vec<(u32, PathBuf)>) {
        let mut t = self.targets.lock_or_recover();
        if *t != list {
            *t = list;
            self.poke();
        }
    }

    /// Обновить немедленно (например, агент закончил работу).
    pub fn poke(&self) {
        let _ = self.wake.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{git, repo, TempDir};

    #[test]
    fn parses_status() {
        let t = "# branch.oid abc1234567\n# branch.head feat/x\n# branch.upstream origin/feat/x\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 a b src/a.rs\n? new.txt\n";
        let i = parse_status(t);
        assert_eq!((i.branch.as_str(), i.ahead, i.behind, i.files), ("feat/x", 2, 1, 2));
        assert_eq!(i.summary(), "⎇ feat/x ↑2 ↓1 ●2");
        let d = parse_status("# branch.oid deadbeefcafe\n# branch.head (detached)\n");
        assert_eq!(d.branch, "@deadbee");
    }

    #[test]
    fn parses_numstat() {
        assert_eq!(parse_numstat("3\t1\ta.rs\n-\t-\timg.png\n10\t0\tb.rs\n"), (13, 1));
    }

    #[test]
    fn real_repository() {
        let d = repo("git");
        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        git(&d, &["add", "."]);
        git(&d, &["commit", "-m", "first"]);

        let clean = query(&d).unwrap();
        assert_eq!((clean.branch.as_str(), clean.files, clean.added), ("main", 0, 0));

        std::fs::write(d.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(d.join("b.txt"), "new\n").unwrap();
        let dirty = query(&d).unwrap();
        assert_eq!(dirty.files, 2);
        assert!(dirty.added >= 2 && dirty.removed >= 1, "{dirty:?}");
        assert!(dirty.dirty());
    }

    #[test]
    fn watcher_reports_and_stops() {
        let d = repo("watch");
        let (tx, rx) = mpsc::channel();
        let w = Watcher::start(tx);
        w.set_targets(vec![(7, d.to_path_buf())]);
        let Msg::Git(id, info) = rx.recv_timeout(Duration::from_secs(5)).expect("сообщение о git") else {
            panic!("ожидалось Msg::Git");
        };
        assert_eq!(id, 7);
        assert_eq!(info.expect("репозиторий").branch, "main");
        // после уничтожения Watcher поток завершается и отпускает Sender
        drop(w);
        loop {
            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(_) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("поток опроса не завершился"),
            }
        }
    }

    #[test]
    fn not_a_repository() {
        let d = TempDir::new("nogit");
        // /tmp может лежать внутри репозитория — поэтому проверяем только отсутствие паники
        let _ = query(&d);
    }
}
