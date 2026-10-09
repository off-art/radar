//! Хуки Claude Code: точные статусы без парсинга экрана.
//!
//! Radar запускает `claude --settings <файл>`; в файле — хуки, которые вызывают
//! `radar hook <Событие>`. Эта команда читает JSON из stdin и передаёт его
//! запущенному интерфейсу через unix-сокет.

use crate::config::config_dir;
use crate::session::{shq, Msg};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    base.join(format!("radar-{}.sock", std::process::id()))
}

/// Сервер сокета: принимает события хуков и пересылает их в главный цикл.
pub fn start_server(path: &PathBuf, tx: Sender<Msg>) -> Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut s = String::new();
                if (&stream).take(1 << 20).read_to_string(&mut s).is_err() {
                    return;
                }
                let Ok(v) = serde_json::from_str::<Value>(&s) else {
                    return;
                };
                if v.get("ctl").is_some() {
                    // команда radar ctl: выполняет главный цикл, ответ пишем в тот же сокет
                    let (rtx, rrx) = std::sync::mpsc::channel();
                    let reply = if tx.send(Msg::Ctl { req: v, reply: rtx }).is_ok() {
                        rrx.recv_timeout(Duration::from_secs(5))
                            .unwrap_or_else(|_| json!({"ok": false, "error": "Radar не ответил"}))
                    } else {
                        json!({"ok": false, "error": "Radar закрывается"})
                    };
                    let _ = stream.write_all(reply.to_string().as_bytes());
                    return;
                }
                let session = v.get("session").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                let event = v.get("event").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let payload = v.get("payload").cloned().unwrap_or(Value::Null);
                let _ = tx.send(Msg::Hook { session, event, payload });
            });
        }
    });
    Ok(())
}

/// Клиент: `radar hook <Событие>`. Никогда не падает и ничего не печатает,
/// чтобы не мешать Claude Code.
pub fn run_client(event: &str) {
    let mut input = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut input);
    log_event(event, &input);
    // события идут через процесс-хозяин агента: он переживёт перезапуск окна Radar
    let sock = std::env::var("RADAR_HOST_SOCK").or_else(|_| std::env::var("RADAR_SOCK"));
    let (Ok(sock), Ok(sess)) = (sock, std::env::var("RADAR_SESSION")) else {
        return;
    };
    let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let msg = json!({
        "session": sess.parse::<u32>().unwrap_or(0),
        "event": event,
        "payload": payload,
    });
    if let Ok(mut s) = UnixStream::connect(sock) {
        let _ = s.write_all(msg.to_string().as_bytes());
    }
}

/// Отладка интеграций: `RADAR_HOOK_LOG=1 radar` пишет все события хуков в `~/.config/radar/hooks.log`.
fn log_event(event: &str, input: &str) {
    if std::env::var_os("RADAR_HOOK_LOG").is_none() {
        return;
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let session = std::env::var("RADAR_SESSION").unwrap_or_else(|_| "-".into());
    let body: String = input.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(600).collect();
    let line = format!("{secs} session={session} event={event} payload={body}\n");
    let _ = std::fs::create_dir_all(config_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(config_dir().join("hooks.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Записывает settings-файл с хуками для Claude Code.
pub fn write_claude_settings() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let exe = shq(&exe.to_string_lossy());
    let entry = |ev: &str, matcher: bool| -> Value {
        let hooks = json!([{
            "type": "command",
            "command": format!("{exe} hook {ev}"),
            "timeout": 5
        }]);
        if matcher {
            json!([{ "matcher": "*", "hooks": hooks }])
        } else {
            json!([{ "hooks": hooks }])
        }
    };
    let settings = json!({
        "hooks": {
            "SessionStart": entry("SessionStart", false),
            "UserPromptSubmit": entry("UserPromptSubmit", false),
            "PreToolUse": entry("PreToolUse", true),
            "PostToolUse": entry("PostToolUse", true),
            "Notification": entry("Notification", false),
            "Stop": entry("Stop", false),
        }
    });
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("claude-settings.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&settings)?)?;
    Ok(path)
}
