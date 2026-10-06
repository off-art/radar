mod app;
mod config;
mod hook;
mod input;
mod keys;
mod menu;
mod notify;
mod session;
mod status;
mod textfield;
mod ui;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::mpsc;

const HELP: &str = "\
Radar — несколько AI-агентов в одном окне терминала

Использование:
  radar [папка] [агент ...]  открыть интерфейс
  radar doctor               проверить, какие агенты установлены
  radar notify-test          проверить уведомления, иконку и звук
  radar --help | --version

Примеры:
  radar                      пустой список, агентов добавлять клавишей n
  radar ~/work/api claude    открыть и сразу запустить Claude Code в папке
  radar claude claude codex  три агента в текущей папке
  radar ~/work/api gigacode  GigaCode CLI в папке проекта

Управление: правая кнопка мыши — меню; Ctrl+b включает режим навигации (j/k — выбор,
n — новый, x — закрыть, p — палитра команд, ? — помощь, Esc — выйти из режима).
Конфиг: ~/.config/radar/config.toml";

fn doctor() -> Result<()> {
    let cfg = config::Config::load();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    println!("Radar {}", env!("CARGO_PKG_VERSION"));
    println!("Конфиг: {}", config::config_path().display());
    println!();
    for a in &cfg.agents {
        let bin = a.command.split_whitespace().next().unwrap_or("");
        let found = if bin == "$SHELL" {
            Some(shell.clone())
        } else {
            std::process::Command::new(&shell)
                .args(["-l", "-i", "-c", &format!("command -v {}", session::shq(bin))])
                .env("SHELL_SESSIONS_DISABLE", "1")
                .env_remove("TERM_SESSION_ID")
                .stderr(std::process::Stdio::null())
                .output()
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
        };
        match found {
            Some(p) => println!("  ✓ {:<14} {}", a.name, p),
            None => println!("  ✗ {:<14} не найден ({})", a.name, bin),
        }
    }
    println!();
    println!("  Уведомления: {}", notify::status_line());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            hook::run_client(args.get(1).map(String::as_str).unwrap_or(""));
            return Ok(());
        }
        Some("-h") | Some("--help") | Some("help") => {
            println!("{HELP}");
            return Ok(());
        }
        Some("-V") | Some("--version") => {
            println!("radar {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("doctor") => return doctor(),
        Some("notify-test") => {
            let cfg = config::Config::load();
            notify::self_test(&notify::Settings {
                sound: cfg.sound,
                popups: cfg.popups,
                theme: cfg.sound_theme.clone(),
                volume: cfg.volume,
                sound_done: cfg.sound_done,
                sound_waiting: cfg.sound_waiting,
            });
            return Ok(());
        }
        _ => {}
    }

    config::write_example_if_missing();
    let cfg = config::Config::load();

    // позиционные аргументы: папка и/или идентификаторы агентов для автозапуска
    let mut start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut autostart = vec![];
    for a in &args {
        let p = app::expand_tilde(a);
        if p.is_dir() {
            start_dir = p.canonicalize().unwrap_or(p);
        } else if let Some(def) = cfg.find(a) {
            autostart.push(def.clone());
        } else {
            eprintln!("radar: неизвестный аргумент «{a}» (см. radar --help)");
            std::process::exit(2);
        }
    }

    let (tx, rx) = mpsc::channel();
    let sock = hook::socket_path();
    hook::start_server(&sock, tx.clone())?;
    let claude_settings = hook::write_claude_settings().ok();
    let ctx = session::SpawnCtx {
        sock: sock.clone(),
        claude_settings,
    };

    let mut app = app::App::new(cfg, start_dir.clone(), tx, ctx);
    let mut errors = vec![];
    for def in autostart {
        if let Err(e) = app.create_session(def, start_dir.clone(), None, false) {
            errors.push(e.to_string());
        }
    }
    if let Some(e) = errors.first() {
        app.toast(format!("Не удалось запустить: {e}"));
    }
    app::run(app, rx, sock)
}
