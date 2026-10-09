//! Уведомления и звук.
//!
//! macOS: уведомление показывает маленькое приложение-помощник `Radar.app` (создаётся при первом запуске
//! из AppleScript/JXA-апплета), поэтому в уведомлении видна иконка Radar, а в настройках системы оно
//! называется «Radar». Звук проигрывается отдельно (`afplay`) — свои мягкие колокольчики вместо
//! системного «Glass». Если помощник не удалось создать — запасной вариант через `osascript`.
//! Linux (для разработки): `notify-send` и `paplay`/`aplay`.

use crate::config::config_dir;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const ICON: &[u8] = include_bytes!("../assets/icon.icns");

macro_rules! theme {
    ($n:literal) => {
        (
            $n,
            include_bytes!(concat!("../assets/sounds/", $n, "-done.wav")) as &[u8],
            include_bytes!(concat!("../assets/sounds/", $n, "-waiting.wav")) as &[u8],
        )
    };
}

/// Встроенные звуковые темы: (имя, «готово», «ждёт ответа»).
const THEMES: &[(&str, &[u8], &[u8])] = &[
    theme!("bell"),
    theme!("sonar"),
    theme!("retro"),
    theme!("harp"),
    theme!("knock"),
    theme!("thump"),
    theme!("drop"),
];

pub fn theme_names() -> Vec<&'static str> {
    THEMES.iter().map(|t| t.0).collect()
}

pub const DEFAULT_THEME: &str = "bell";

/// JXA-апплет: читает очередь уведомлений из ~/.config/radar/queue и показывает их.
const APPLET: &str = r#"ObjC.import('Foundation');
function run() {
  var app = Application.currentApplication();
  app.includeStandardAdditions = true;
  var fm = $.NSFileManager.defaultManager;
  var dir = ObjC.unwrap($.NSHomeDirectory()) + '/.config/radar/queue';
  var list = fm.contentsOfDirectoryAtPathError(dir, null);
  if (!list) { return; }
  var names = ObjC.deepUnwrap(list).sort();
  for (var i = 0; i < names.length; i++) {
    var path = dir + '/' + names[i];
    var s = $.NSString.stringWithContentsOfFileEncodingError(path, $.NSUTF8StringEncoding, null);
    fm.removeItemAtPathError(path, null);
    if (!s) { continue; }
    try {
      var m = JSON.parse(ObjC.unwrap(s));
      app.displayNotification(m.body, { withTitle: m.title });
    } catch (e) {}
  }
  delay(0.4);
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Done,
    Waiting,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub sound: bool,
    /// Показывать всплывающие уведомления (звук от этого не зависит).
    pub popups: bool,
    pub theme: String,
    pub volume: f32,
    pub sound_done: Option<PathBuf>,
    pub sound_waiting: Option<PathBuf>,
}

fn is_mac() -> bool {
    cfg!(target_os = "macos")
}

fn app_path() -> PathBuf {
    config_dir().join("Radar.app")
}

fn queue_dir() -> PathBuf {
    config_dir().join("queue")
}

fn quiet(cmd: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()
}

// ───────────── звук ─────────────

fn sound_file(kind: Sound, s: &Settings) -> Option<PathBuf> {
    let custom = match kind {
        Sound::Done => &s.sound_done,
        Sound::Waiting => &s.sound_waiting,
    };
    if let Some(p) = custom {
        if p.exists() {
            return Some(p.clone());
        }
    }
    let t = THEMES.iter().find(|t| t.0 == s.theme).unwrap_or(&THEMES[0]);
    let (name, data) = match kind {
        Sound::Done => (format!("{}-done.wav", t.0), t.1),
        Sound::Waiting => (format!("{}-waiting.wav", t.0), t.2),
    };
    let dir = config_dir().join("sounds");
    let path = dir.join(name);
    let ok = std::fs::metadata(&path).map(|m| m.len() == data.len() as u64).unwrap_or(false);
    if !ok {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, data).ok()?;
    }
    Some(path)
}

/// Проигрывает звук, не блокируя интерфейс.
pub fn play(kind: Sound, s: &Settings) {
    if !s.sound {
        return;
    }
    let Some(path) = sound_file(kind, s) else { return };
    let vol = s.volume.clamp(0.0, 1.0);
    std::thread::spawn(move || {
        if is_mac() {
            let _ = quiet(Command::new("afplay").arg("-v").arg(format!("{vol:.2}")).arg(&path));
        } else if quiet(Command::new("paplay").arg(&path)).is_err() {
            let _ = quiet(Command::new("aplay").arg("-q").arg(&path));
        }
    });
}

// ───────────── помощник Radar.app ─────────────

fn stamp() -> String {
    format!("{}-{}", env!("CARGO_PKG_VERSION"), ICON.len())
}

fn app_ready() -> bool {
    app_path().join("Contents").exists()
        && std::fs::read_to_string(config_dir().join("notifier.stamp")).map(|s| s == stamp()).unwrap_or(false)
}

fn plist_set(plist: &Path, key: &str, kind: &str, value: &str) {
    let pb = "/usr/libexec/PlistBuddy";
    let target = plist.to_string_lossy().to_string();
    if quiet(Command::new(pb).args(["-c", &format!("Set :{key} {value}"), &target]))
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        let _ = quiet(Command::new(pb).args(["-c", &format!("Add :{key} {kind} {value}"), &target]));
    }
}

/// Создаёт (или обновляет) Radar.app. Возвращает ошибку с пояснением.
pub fn build_helper() -> Result<(), String> {
    if !is_mac() {
        return Err("только macOS".into());
    }
    let dir = config_dir();
    std::fs::create_dir_all(queue_dir()).map_err(|e| e.to_string())?;
    let app = app_path();
    let _ = std::fs::remove_dir_all(&app);
    let src = dir.join("notifier.js");
    std::fs::write(&src, APPLET).map_err(|e| e.to_string())?;
    let out = Command::new("osacompile")
        .args(["-l", "JavaScript", "-o"])
        .arg(&app)
        .arg(&src)
        .output()
        .map_err(|e| format!("osacompile: {e}"))?;
    if !out.status.success() {
        return Err(format!("osacompile: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let res = app.join("Contents").join("Resources");
    std::fs::create_dir_all(&res).map_err(|e| e.to_string())?;
    std::fs::write(res.join("applet.icns"), ICON).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(res.join("Assets.car"));
    let plist = app.join("Contents").join("Info.plist");
    let pb = "/usr/libexec/PlistBuddy";
    let _ = quiet(Command::new(pb).args(["-c", "Delete :CFBundleIconName", &plist.to_string_lossy()]));
    plist_set(&plist, "CFBundleIdentifier", "string", "dev.radar.notifier");
    plist_set(&plist, "CFBundleName", "string", "Radar");
    plist_set(&plist, "CFBundleDisplayName", "string", "Radar");
    plist_set(&plist, "CFBundleIconFile", "string", "applet");
    plist_set(&plist, "LSUIElement", "bool", "true");
    let _ = quiet(Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app));
    let _ = quiet(Command::new("touch").arg(&app));
    let _ = quiet(
        Command::new(
            "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
        )
        .arg("-f")
        .arg(&app),
    );
    std::fs::write(dir.join("notifier.stamp"), stamp()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Готовит помощник в фоне при запуске.
pub fn prepare() {
    if is_mac() && !app_ready() {
        std::thread::spawn(|| {
            let _ = build_helper();
        });
    }
}

fn json_escape(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' | '\r' => o.push(' '),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn send_via_helper(title: &str, body: &str) -> bool {
    if !app_ready() {
        return false;
    }
    let q = queue_dir();
    if std::fs::create_dir_all(&q).is_err() {
        return false;
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let file = q.join(format!("{stamp:032}.json"));
    let json = format!("{{\"title\":{},\"body\":{}}}", json_escape(title), json_escape(body));
    if std::fs::write(&file, json).is_err() {
        return false;
    }
    let ok = quiet(Command::new("open").args(["-g", "-j", "-a"]).arg(app_path())).map(|s| s.success()).unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_file(file);
    }
    ok
}

fn send_via_osascript(title: &str, body: &str) {
    // Текст передаётся аргументами, а не вставляется в скрипт — нет проблем с кавычками.
    // Префикс «» » нужен, чтобы osascript не принял текст за флаг.
    let _ = quiet(
        Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 1 of argv) with title (item 2 of argv)",
                "-e",
                "end run",
            ])
            .arg(format!("» {body}"))
            .arg(format!("» {title}")),
    );
}

fn send(title: &str, body: &str) {
    if is_mac() {
        if !send_via_helper(title, body) {
            send_via_osascript(title, body);
        }
    } else {
        let _ = quiet(Command::new("notify-send").arg(title).arg(body));
    }
}

/// Уведомление + мягкий звук (каждое можно выключить отдельно). Не блокирует интерфейс.
pub fn notify(kind: Sound, title: &str, body: &str, s: &Settings) {
    play(kind, s);
    if s.popups {
        let (t, b) = (title.to_string(), body.to_string());
        std::thread::spawn(move || send(&t, &b));
    }
}

/// Прослушивание темы: «готово», затем «ждёт ответа».
pub fn preview(s: &Settings) {
    let mut s = s.clone();
    s.sound = true;
    std::thread::spawn(move || {
        play(Sound::Done, &s);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        play(Sound::Waiting, &s);
    });
}

/// Для `radar doctor`: в каком состоянии уведомления.
pub fn status_line() -> String {
    if !is_mac() {
        let has = |c: &str| {
            Command::new("sh")
                .args(["-c", &format!("command -v {c}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_or(false, |s| s.success())
        };
        let n = if has("notify-send") {
            "notify-send ок"
        } else {
            "notify-send не найден (sudo apt install libnotify-bin)"
        };
        let a = if has("paplay") || has("aplay") {
            "звук ок"
        } else {
            "звук: нет paplay/aplay (sudo apt install pulseaudio-utils)"
        };
        return format!("{n}; {a}");
    }
    if app_ready() {
        format!("Radar.app готов ({})", app_path().display())
    } else {
        "Radar.app ещё не создан — выполните `radar notify-test`".into()
    }
}

/// `radar notify-test`: пошаговая проверка уведомлений.
pub fn self_test(s: &Settings) {
    println!("Проверка уведомлений Radar");
    if is_mac() {
        print!("  создаю помощник Radar.app … ");
        match build_helper() {
            Ok(()) => println!("ок ({})", app_path().display()),
            Err(e) => println!("не удалось: {e}\n  (будет использован запасной вариант через osascript)"),
        }
    }
    println!("  звук «готово» …");
    play(Sound::Done, s);
    std::thread::sleep(std::time::Duration::from_millis(1400));
    println!("  звук «ждёт ответа» …");
    play(Sound::Waiting, s);
    std::thread::sleep(std::time::Duration::from_millis(400));
    if s.popups {
        send("Задача выполнена", "Claude Code · radar");
    } else {
        println!("  (всплывающие уведомления выключены в настройках — popups = false)");
    }
    std::thread::sleep(std::time::Duration::from_millis(1500));
    println!();
    if is_mac() {
        println!("Если уведомление не появилось: Системные настройки → Уведомления → «Radar» → разрешить.");
        println!("(при первом показе macOS может спросить разрешение — нажмите «Разрешить»)");
    } else {
        println!("Если уведомление не появилось: установите libnotify-bin (sudo apt install libnotify-bin)");
        println!("и проверьте, что в окружении есть сеанс рабочего стола (DBUS_SESSION_BUS_ADDRESS).");
        println!("Нет звука: sudo apt install pulseaudio-utils (paplay) или alsa-utils (aplay).");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_escaped() {
        assert_eq!(json_escape("a\"b\\c\nd"), "\"a\\\"b\\\\c d\"");
    }

    #[test]
    fn assets_are_embedded() {
        assert!(ICON.starts_with(b"icns"));
        assert_eq!(THEMES.len(), 7);
        for t in THEMES {
            assert!(t.1.starts_with(b"RIFF") && t.2.starts_with(b"RIFF"), "{}", t.0);
        }
    }
}
