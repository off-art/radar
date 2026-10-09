//! `radar ctl`: управление запущенным Radar из командной строки и скриптов.
//!
//! Клиент подключается к unix-сокету Radar (тому же, что принимает хуки), отправляет одну JSON-команду
//! `{"ctl": "<команда>", ...}` и читает JSON-ответ `{"ok": true, ...}` или `{"ok": false, "error": "..."}`.
//! Ожидание (`wait`) делается на стороне клиента опросом `status`, поэтому интерфейс Radar не блокируется.

use crate::status::Status;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const HELP: &str = "\
radar ctl — управление запущенным Radar

Использование:
  radar ctl list [--json]                          агенты, статусы, ветки
  radar ctl new <агент> [папка] [--name ИМЯ] [--worktree]
                                                   запустить агента (в папке или в текущей)
  radar ctl send <агент> <текст…> [--no-enter]     отправить текст (текст «-» читается из stdin)
  radar ctl status <агент> [--json]                состояние агента
  radar ctl read <агент> [--lines N]               последние строки экрана агента
  radar ctl wait <агент> [--timeout СЕК] [--settle СЕК]
                                                   ждать, пока агент закончит работу
  radar ctl close <агент>                          закрыть агента

<агент> — номер (id из list), имя или начало имени. Если запущено несколько окон Radar, берётся
последнее; точнее — `--sock ПУТЬ` или переменная RADAR_SOCK (внутри агента она уже задана).

Коды возврата wait: 0 — готов, 3 — ждёт ответа, 4 — завершён, 124 — время вышло. Остальные команды: 0 — успех, 1 — ошибка.";

/// Ключ статуса для скриптов.
pub fn status_key(s: Status) -> &'static str {
    match s {
        Status::Starting => "starting",
        Status::Working => "working",
        Status::Waiting => "waiting",
        Status::Idle => "idle",
        Status::Exited => "exited",
    }
}

/// Находит агента по ссылке: id, точное имя (без учёта регистра) или единственное совпадение по началу имени.
pub fn resolve(items: &[(u32, String)], target: &str) -> Result<u32, String> {
    let t = target.trim();
    if let Ok(id) = t.parse::<u32>() {
        if items.iter().any(|(i, _)| *i == id) {
            return Ok(id);
        }
    }
    let lt = t.to_lowercase();
    if let Some((id, _)) = items.iter().find(|(_, n)| n.to_lowercase() == lt) {
        return Ok(*id);
    }
    let pref: Vec<_> = items.iter().filter(|(_, n)| n.to_lowercase().starts_with(&lt)).collect();
    match pref.len() {
        0 => Err(format!("агент «{t}» не найден")),
        1 => Ok(pref[0].0),
        _ => Err(format!(
            "«{t}» подходит нескольким агентам: {}",
            pref.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Сокет работающего Radar: из `RADAR_SOCK` или самый свежий живой `radar-*.sock`.
pub fn find_socket(explicit: Option<String>) -> Result<PathBuf> {
    if let Some(p) = explicit.clone() {
        return Ok(PathBuf::from(p));
    }
    // RADAR_SOCK внутри агента мог остаться от закрытого окна — берём, только если там кто-то слушает
    if let Some(p) = std::env::var("RADAR_SOCK").ok().filter(|p| !p.is_empty()) {
        if UnixStream::connect(&p).is_ok() {
            return Ok(PathBuf::from(p));
        }
    }
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with("radar-") && n.ends_with(".sock")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(_, p)| UnixStream::connect(p).is_ok())
        .collect();
    found.sort();
    found.pop().map(|(_, p)| p).ok_or_else(|| anyhow!("запущенный Radar не найден (откройте radar в другом окне)"))
}

/// Одна команда: запрос → ответ.
pub fn request(sock: &PathBuf, req: &Value) -> Result<Value> {
    let mut s = UnixStream::connect(sock).with_context(|| format!("не удалось подключиться к {}", sock.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.write_all(req.to_string().as_bytes())?;
    s.shutdown(Shutdown::Write)?;
    let mut out = String::new();
    s.read_to_string(&mut out).context("Radar не ответил")?;
    let v: Value = serde_json::from_str(&out).context("непонятный ответ Radar")?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v)
    } else {
        bail!("{}", v["error"].as_str().unwrap_or("неизвестная ошибка"))
    }
}

/// Простой разбор: позиционные аргументы + флаги `--имя [значение]`.
pub struct Args {
    pub pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    pub fn parse(raw: &[String], with_value: &[&str]) -> Args {
        let (mut pos, mut flags) = (vec![], vec![]);
        let mut it = raw.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--").filter(|n| !n.is_empty()) {
                if with_value.contains(&name) {
                    flags.push((name.to_string(), it.next().cloned()));
                } else {
                    flags.push((name.to_string(), None));
                }
            } else {
                pos.push(a.clone());
            }
        }
        Args { pos, flags }
    }
    pub fn flag(&self, n: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == n)
    }
    pub fn value(&self, n: &str) -> Option<&str> {
        self.flags.iter().find(|(k, _)| k == n).and_then(|(_, v)| v.as_deref())
    }
}

fn target(a: &Args) -> Result<&str> {
    a.pos.first().map(String::as_str).ok_or_else(|| anyhow!("укажите агента (см. radar ctl list)"))
}

fn line(a: &Value) -> String {
    let g = |k: &str| a[k].as_str().unwrap_or("").to_string();
    let mut s = format!("{:>3}  {:<22} {:<12} {:<9}", a["id"], g("name"), g("agent"), g("status"));
    if !g("branch").is_empty() {
        s.push_str(&format!(" ⎇ {}", g("branch")));
    }
    if !g("note").is_empty() {
        s.push_str(&format!("  [{}]", g("note")));
    }
    s
}

/// Точка входа: `radar ctl …`. Возвращает код выхода.
pub fn run(raw: &[String]) -> Result<i32> {
    let Some(cmd) = raw.first().map(String::as_str) else {
        println!("{HELP}");
        return Ok(0);
    };
    if matches!(cmd, "-h" | "--help" | "help") {
        println!("{HELP}");
        return Ok(0);
    }
    let a = Args::parse(&raw[1..], &["name", "lines", "timeout", "settle", "sock"]);
    let sock = find_socket(a.value("sock").map(String::from))?;
    match cmd {
        "list" | "ls" => {
            let v = request(&sock, &json!({"ctl": "list"}))?;
            if a.flag("json") {
                println!("{}", v["sessions"]);
            } else if let Some(arr) = v["sessions"].as_array().filter(|x| !x.is_empty()) {
                arr.iter().for_each(|s| println!("{}", line(s)));
            } else {
                println!("агентов нет");
            }
        }
        "new" => {
            let agent = a.pos.first().ok_or_else(|| anyhow!("укажите агента: radar ctl new claude [папка]"))?;
            let dir = match a.pos.get(1) {
                Some(d) => {
                    std::fs::canonicalize(crate::app::expand_tilde(d)).with_context(|| format!("нет папки {d}"))?
                }
                None => std::env::current_dir()?,
            };
            let v = request(
                &sock,
                &json!({"ctl": "new", "agent": agent, "dir": dir, "name": a.value("name"), "worktree": a.flag("worktree")}),
            )?;
            println!("{}", v["session"]["id"]);
        }
        "send" => {
            let t = target(&a)?;
            let mut text = a.pos[1..].join(" ");
            if text == "-" {
                text.clear();
                std::io::stdin().read_to_string(&mut text)?;
                text = text.trim_end().to_string();
            }
            if text.is_empty() {
                bail!("нечего отправлять: radar ctl send <агент> <текст>");
            }
            request(&sock, &json!({"ctl": "send", "target": t, "text": text, "enter": !a.flag("no-enter")}))?;
        }
        "status" => {
            let v = request(&sock, &json!({"ctl": "status", "target": target(&a)?}))?;
            if a.flag("json") {
                println!("{}", v["session"]);
            } else {
                println!("{}", line(&v["session"]));
            }
        }
        "read" => {
            let n = a.value("lines").and_then(|x| x.parse::<u32>().ok()).unwrap_or(40);
            let v = request(&sock, &json!({"ctl": "read", "target": target(&a)?, "lines": n}))?;
            println!("{}", v["text"].as_str().unwrap_or(""));
        }
        "wait" => return wait(&sock, target(&a)?, &a),
        "close" => {
            request(&sock, &json!({"ctl": "close", "target": target(&a)?}))?;
        }
        _ => bail!("неизвестная команда «{cmd}» (см. radar ctl --help)"),
    }
    Ok(0)
}

/// Ждёт, пока агент не перестанет работать (idle / waiting / exited) и продержится так `settle` секунд —
/// иначе сразу после `send` можно увидеть ещё старое состояние «готов».
fn wait(sock: &PathBuf, t: &str, a: &Args) -> Result<i32> {
    let timeout = a.value("timeout").and_then(|x| x.parse::<f64>().ok());
    let settle = Duration::from_secs_f64(a.value("settle").and_then(|x| x.parse().ok()).unwrap_or(1.5));
    let start = Instant::now();
    let mut calm_since: Option<Instant> = None;
    loop {
        let v = request(sock, &json!({"ctl": "status", "target": t}))?;
        let st = v["session"]["status"].as_str().unwrap_or("").to_string();
        let calm = matches!(st.as_str(), "idle" | "waiting" | "exited");
        if calm {
            let since = *calm_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= settle || st == "exited" {
                println!("{st}");
                return Ok(match st.as_str() {
                    "waiting" => 3,
                    "exited" => 4,
                    _ => 0,
                });
            }
        } else {
            calm_since = None;
        }
        if timeout.map_or(false, |t| start.elapsed().as_secs_f64() >= t) {
            println!("{st}");
            return Ok(124);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<(u32, String)> {
        vec![(1, "api".into()), (2, "api #2".into()), (7, "Docs".into())]
    }

    #[test]
    fn resolve_by_id_name_prefix() {
        assert_eq!(resolve(&items(), "7"), Ok(7));
        assert_eq!(resolve(&items(), "docs"), Ok(7));
        assert_eq!(resolve(&items(), "api"), Ok(1)); // точное имя важнее префикса
        assert_eq!(resolve(&items(), "do"), Ok(7));
        assert!(resolve(&items(), "api #").is_ok());
        assert!(resolve(&items(), "zzz").is_err());
        assert!(resolve(&[(1, "web a".into()), (2, "web b".into())], "web").unwrap_err().contains("нескольким"));
    }

    #[test]
    fn args_parse() {
        let raw: Vec<String> =
            ["claude", "/tmp", "--name", "x y", "--worktree"].iter().map(|s| s.to_string()).collect();
        let a = Args::parse(&raw, &["name"]);
        assert_eq!(a.pos, vec!["claude", "/tmp"]);
        assert_eq!(a.value("name"), Some("x y"));
        assert!(a.flag("worktree"));
    }

    #[test]
    fn status_keys() {
        assert_eq!(status_key(Status::Waiting), "waiting");
    }
}
