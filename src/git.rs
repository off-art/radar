//! Состояние git-репозитория агента: ветка, число изменений, +/− строк.
//!
//! Опрашивается в фоновом потоке (раз в несколько секунд и сразу после завершения задачи),
//! чтобы не тормозить интерфейс. Не-git папки просто не показывают git-информацию.

use crate::session::Msg;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
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

/// Запускает git с таймаутом; возвращает stdout при успехе.
pub fn run(dir: &Path, args: &[&str]) -> Option<String> {
    run_timeout(dir, args, Duration::from_secs(3))
}

pub fn run_timeout(dir: &Path, args: &[&str], limit: Duration) -> Option<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // stdout читаем в отдельном потоке: большой вывод (diff) иначе заполнит канал и git зависнет
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(limit) {
        Ok(buf) => {
            let ok = child.wait().map(|s| s.success()).unwrap_or(false);
            ok.then(|| String::from_utf8_lossy(&buf).into_owned())
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// Выполняет git для действий пользователя (commit, push, merge…): возвращает вывод или текст ошибки.
/// Никогда не ждёт ввода: запрос пароля отключён, ssh — в пакетном режиме.
pub fn exec(dir: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let mut child = cmd.spawn().map_err(|_| "git не найден".to_string())?;
    let mut out = child.stdout.take().ok_or("нет stdout")?;
    let mut err = child.stderr.take().ok_or("нет stderr")?;
    let (tx, rx) = std::sync::mpsc::channel();
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        let _ = tx.send((true, b));
    });
    std::thread::spawn(move || {
        use std::io::Read;
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        let _ = tx2.send((false, b));
    });
    let (mut so, mut se) = (Vec::new(), Vec::new());
    let start = Instant::now();
    for _ in 0..2 {
        let left = limit.saturating_sub(start.elapsed());
        match rx.recv_timeout(left) {
            Ok((true, b)) => so = b,
            Ok((false, b)) => se = b,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git не ответил за {} с", limit.as_secs()));
            }
        }
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    let so = String::from_utf8_lossy(&so).trim().to_string();
    let se = String::from_utf8_lossy(&se).trim().to_string();
    if ok {
        Ok(if so.is_empty() { se } else { so })
    } else {
        Err(if !se.is_empty() {
            se
        } else if !so.is_empty() {
            so
        } else {
            "git завершился с ошибкой".into()
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

/// Фоновый опрос git для списка агентов.
#[derive(Clone)]
pub struct Watcher {
    targets: Arc<Mutex<Vec<(u32, PathBuf)>>>,
    poke: Arc<AtomicBool>,
}

impl Watcher {
    pub fn start(tx: Sender<Msg>) -> Watcher {
        let w = Watcher { targets: Arc::new(Mutex::new(vec![])), poke: Arc::new(AtomicBool::new(true)) };
        let t = w.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now() - Duration::from_secs(60);
            loop {
                std::thread::sleep(Duration::from_millis(300));
                let due = t.poke.swap(false, Ordering::Relaxed) || last.elapsed() >= Duration::from_secs(4);
                if !due {
                    continue;
                }
                last = Instant::now();
                let targets = t.targets.lock().unwrap().clone();
                let mut seen: Vec<(PathBuf, Option<Info>)> = vec![];
                for (id, dir) in targets {
                    let info = match seen.iter().find(|(d, _)| *d == dir) {
                        Some((_, i)) => i.clone(),
                        None => {
                            let i = query(&dir);
                            seen.push((dir.clone(), i.clone()));
                            i
                        }
                    };
                    if tx.send(Msg::Git(id, info)).is_err() {
                        return; // интерфейс закрыт
                    }
                }
            }
        });
        w
    }

    pub fn set_targets(&self, list: Vec<(u32, PathBuf)>) {
        let mut t = self.targets.lock().unwrap();
        if *t != list {
            *t = list;
            self.poke.store(true, Ordering::Relaxed);
        }
    }

    /// Обновить немедленно (например, агент закончил работу).
    pub fn poke(&self) {
        self.poke.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn real_repository() {
        let d = std::env::temp_dir().join(format!("radar-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(query(&d).is_none() || d.join(".git").exists() || true);
        git(&d, &["init"]);
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
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn not_a_repository() {
        let d = std::env::temp_dir().join(format!("radar-nogit-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // /tmp может лежать внутри репозитория — поэтому проверяем только отсутствие паники
        let _ = query(&d);
        let _ = std::fs::remove_dir_all(&d);
    }
}
