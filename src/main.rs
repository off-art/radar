mod app;
mod clipboard;
mod complete;
mod config;
mod ctl;
mod diff;
mod events;
mod git;
mod gitops;
mod groups;
mod hook;
mod host;
mod input;
mod integrations;
mod keys;
mod menu;
mod notify;
mod paths;
mod persist;
mod session;
mod status;
mod sync;
#[cfg(test)]
mod testutil;
mod textfield;
mod theme;
mod ui;
mod update;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::mpsc;

const HELP: &str = "\
Radar — несколько AI-агентов в одном окне терминала

Использование:
  radar [папка] [агент ...]  открыть интерфейс
  radar stop                 остановить всех агентов, работающих в фоне
  radar doctor               проверить, какие агенты установлены
  radar ctl <команда>        управлять запущенным Radar из скриптов (radar ctl --help)
  radar update [--check]     обновить Radar до последней версии (--check — только проверить)
  radar notify-test          проверить уведомления, иконку и звук
  radar integration [install|uninstall <агент>|all]
                             точные статусы агентов через хуки (или экран «Интеграции»)
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
    println!("Radar {}", env!("CARGO_PKG_VERSION"));
    println!("Конфиг: {}", config::config_path().display());
    println!();
    let bins: Vec<&str> = cfg.agents.iter().map(config::AgentDef::bin).collect();
    for (a, found) in cfg.agents.iter().zip(session::find_binaries(&bins)) {
        match found {
            Some(p) => println!("  ✓ {:<14} {}", a.name, p),
            None => println!("  ✗ {:<14} не найден ({})", a.name, a.bin()),
        }
    }
    println!();
    println!("  Уведомления: {}", notify::status_line());
    Ok(())
}

fn integration_cmd(args: &[String]) -> Result<()> {
    let cfg = config::Config::load();
    let ids: Vec<(String, String)> = cfg
        .agents
        .iter()
        .map(|a| (integrations::key(&a.command), a.name.clone()))
        .filter(|(k, _)| integrations::supported(k))
        .collect();
    let act = args.first().map(String::as_str);
    if matches!(act, Some("install") | Some("uninstall")) {
        let targets = &args[1..];
        let list: Vec<String> = if targets.iter().any(|t| t == "all") {
            integrations::installable().iter().map(std::string::ToString::to_string).collect()
        } else if targets.is_empty() {
            eprintln!("radar integration {} <агент ...|all>", act.unwrap());
            std::process::exit(2);
        } else {
            targets.to_vec()
        };
        for id in list {
            let r = if act == Some("install") {
                integrations::install(&id).map(|p| format!("установлено → {}", p.display()))
            } else {
                integrations::uninstall(&id).map(|_| "удалено".to_string())
            };
            match r {
                Ok(m) => println!("  ✓ {id}: {m}"),
                Err(e) => println!("  ✗ {id}: {e}"),
            }
        }
        println!("Уже запущенным агентам нужен перезапуск.");
        return Ok(());
    }
    println!("Интеграции агентов (прямые статусы вместо разбора экрана):\n");
    for (id, name) in ids {
        let mark = match integrations::state(&id) {
            integrations::State::Builtin => "✓ встроено",
            integrations::State::Installed => "✓ включено",
            integrations::State::NotInstalled => "– выключено",
        };
        println!("  {name:<14} {mark:<12} {}", integrations::describe(&id));
    }
    println!("\nВключить: radar integration install <агент|all>   Выключить: radar integration uninstall <агент|all>");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            hook::run_client(args.get(1).map_or("", String::as_str));
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
        Some("ctl") => match ctl::run(&args[1..]) {
            Ok(0) => return Ok(()),
            Ok(code) => std::process::exit(code),
            Err(e) => {
                eprintln!("radar ctl: {e:#}");
                std::process::exit(1);
            }
        },
        Some("host") => return host::run_host(),
        Some("stop") => {
            let n = host::stop_all();
            if n == 0 {
                println!("Работающих агентов нет.");
            } else {
                println!("Остановлено агентов: {n}.");
            }
            return Ok(());
        }
        Some("doctor") => return doctor(),
        Some("update") => {
            if let Err(e) = update::run_update(args.get(1).map(String::as_str) == Some("--check")) {
                eprintln!("radar update: {e:#}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Some("integration") | Some("integrations") => return integration_cmd(&args[1..]),
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
        let p = paths::expand_tilde(a);
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
    let ctx = session::SpawnCtx { sock: sock.clone(), claude_settings };

    let mut app = app::App::new(cfg, start_dir.clone(), tx, ctx);
    let mut errors = vec![];
    let autostart_empty = autostart.is_empty();
    let background = app.attach_existing();
    for def in autostart {
        if let Err(e) = app.create_session(def, start_dir.clone(), None, false) {
            errors.push(e.to_string());
        }
    }
    if autostart_empty && background == 0 {
        app.restore_sessions();
    }
    if let Some(e) = errors.first() {
        app.toast(format!("Не удалось запустить: {e}"));
    }
    app::run(app, rx, sock)
}
