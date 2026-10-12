//! Интеграции с агентами: прямые статусы через хуки вместо разбора экрана.
//!
//! Claude Code настраивается автоматически (`--settings` при каждом запуске).
//! Остальным агентам хуки прописываются в их конфиг по явной команде пользователя
//! (экран «Интеграции» или `radar integration install <агент>`). Правки обратимы:
//! записи Radar помечены именем `radar`, перед первой правкой делается резервная копия.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const MARK: &str = "radar";
const PLUGIN_MARK: &str = "radar-integration";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Работает без установки (Claude Code).
    Builtin,
    Installed,
    NotInstalled,
}

#[derive(Clone, Copy)]
enum Method {
    /// Хуки в JSON-файле (формат Claude Code / Qwen / Gemini / Codex).
    JsonHooks(&'static [&'static str]),
    /// JS-плагин OpenCode.
    OpenCodePlugin,
}

struct Spec {
    id: &'static str,
    method: Method,
}

const QWEN_EVENTS: &[&str] =
    &["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "PermissionRequest", "Stop"];
const CODEX_EVENTS: &[&str] =
    &["SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse", "Stop"];
const GEMINI_EVENTS: &[&str] =
    &["SessionStart", "BeforeAgent", "BeforeTool", "AfterTool", "AfterAgent", "Notification"];

const SPECS: &[Spec] = &[
    Spec { id: "qwen", method: Method::JsonHooks(QWEN_EVENTS) },
    Spec { id: "gigacode", method: Method::JsonHooks(QWEN_EVENTS) },
    Spec { id: "gemini", method: Method::JsonHooks(GEMINI_EVENTS) },
    Spec { id: "codex", method: Method::JsonHooks(CODEX_EVENTS) },
    Spec { id: "opencode", method: Method::OpenCodePlugin },
];

fn spec(id: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

/// Ключ интеграции агента — имя его команды (`claude`, `qwen`, `gemini`…).
pub fn key(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first).to_string()
}

/// Есть ли для агента интеграция (Claude Code — встроенная).
pub fn supported(id: &str) -> bool {
    id == "claude" || spec(id).is_some()
}

/// Идентификаторы агентов, у которых интеграцию можно включать вручную.
pub fn installable() -> Vec<&'static str> {
    SPECS.iter().map(|s| s.id).collect()
}

/// Файл, который правит интеграция.
pub fn target_path(id: &str, home: &Path) -> Option<PathBuf> {
    Some(match id {
        "qwen" => home.join(".qwen/settings.json"),
        // GigaCode — форк Qwen Code; в документации путь не указан, по умолчанию ~/.gigacode
        "gigacode" => home.join(".gigacode/settings.json"),
        "gemini" => home.join(".gemini/settings.json"),
        "codex" => std::env::var_os("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from).join("hooks.json"),
        "opencode" => home.join(".config/opencode/plugins/radar.js"),
        _ => return None,
    })
}

/// Короткое пояснение, что именно будет изменено.
pub fn describe(id: &str) -> String {
    match id {
        "claude" => "встроено: хуки подключаются при каждом запуске".into(),
        _ => match target_path(id, &crate::paths::home()) {
            Some(p) => {
                let mut text = p.to_string_lossy().replacen(&*crate::paths::home().to_string_lossy(), "~", 1);
                if let Some(exe) = stale_exe(id) {
                    text.push_str(&format!(" · ⚠ {exe} не существует — radar integration install {id}"));
                }
                text
            }
            None => "не поддерживается".into(),
        },
    }
}

/// Путь к radar, записанный в установленной интеграции, если такого файла уже нет
/// (после переезда на brew, переустановки или удаления бинарника хуки молча перестают работать).
pub fn stale_exe(id: &str) -> Option<String> {
    stale_exe_in(id, &crate::paths::home())
}

fn stale_exe_in(id: &str, home: &Path) -> Option<String> {
    let sp = spec(id)?;
    let text = std::fs::read_to_string(target_path(id, home)?).ok()?;
    let exe = match sp.method {
        Method::OpenCodePlugin => {
            let rest = text.lines().find_map(|l| l.strip_prefix("const RADAR = "))?;
            serde_json::from_str::<String>(rest.trim()).ok()?
        }
        Method::JsonHooks(_) => {
            let v: Value = serde_json::from_str(&text).ok()?;
            let cmd = v
                .get("hooks")?
                .as_object()?
                .values()
                .filter_map(|l| l.as_array())
                .flatten()
                .filter(|g| is_radar_group(g))
                .find_map(|g| {
                    g["hooks"].as_array()?.iter().find_map(|h| h.get("command")?.as_str().map(String::from))
                })?;
            first_word(&cmd)
        }
    };
    (!Path::new(&exe).exists()).then_some(exe)
}

/// Первое слово команды; путь в одинарных кавычках (как пишет `shq`) читается целиком.
fn first_word(cmd: &str) -> String {
    match cmd.strip_prefix('\'') {
        Some(rest) => rest.split('\'').next().unwrap_or("").to_string(),
        None => cmd.split_whitespace().next().unwrap_or("").to_string(),
    }
}

pub fn state(id: &str) -> State {
    state_in(id, &crate::paths::home())
}

fn state_in(id: &str, home: &Path) -> State {
    if id == "claude" {
        return State::Builtin;
    }
    let (Some(sp), Some(path)) = (spec(id), target_path(id, home)) else {
        return State::NotInstalled;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return State::NotInstalled;
    };
    let on = match sp.method {
        Method::OpenCodePlugin => text.contains(PLUGIN_MARK),
        Method::JsonHooks(_) => serde_json::from_str::<Value>(&text).is_ok_and(|v| has_radar_hooks(&v)),
    };
    if on {
        State::Installed
    } else {
        State::NotInstalled
    }
}

pub fn install(id: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("не удалось определить путь к radar")?;
    let path = install_in(id, &crate::paths::home(), &exe.to_string_lossy())?;
    set_opt_out(&crate::paths::config_dir(), id, false);
    Ok(path)
}

pub fn uninstall(id: &str) -> Result<()> {
    uninstall_in(id, &crate::paths::home())?;
    // выключили сами — автоматика больше не включает
    set_opt_out(&crate::paths::config_dir(), id, true);
    Ok(())
}

/// Результат автоматического включения интеграции при запуске агента.
#[derive(Debug, PartialEq, Eq)]
pub enum Auto {
    /// Ничего не менялось (уже включена, выключена пользователем или интеграции нет).
    Skipped,
    Enabled,
    /// Хуки вели на несуществующий radar — переписаны.
    Repaired,
    Failed(String),
}

/// Включает интеграцию агента сама: пользователю не нужно знать про `radar integration install`.
/// Не трогает то, что пользователь выключил явно (`radar integration uninstall`).
pub fn ensure(id: &str) -> Auto {
    let Ok(exe) = std::env::current_exe() else { return Auto::Skipped };
    ensure_in(id, &crate::paths::home(), &crate::paths::config_dir(), &exe.to_string_lossy())
}

fn ensure_in(id: &str, home: &Path, cfg_dir: &Path, exe: &str) -> Auto {
    if id == "claude" || spec(id).is_none() || opted_out(cfg_dir, id) {
        return Auto::Skipped;
    }
    let repaired = match state_in(id, home) {
        State::NotInstalled => false,
        State::Installed if stale_exe_in(id, home).is_some() => true,
        _ => return Auto::Skipped,
    };
    match install_in(id, home, exe) {
        Ok(_) if repaired => Auto::Repaired,
        Ok(_) => Auto::Enabled,
        Err(e) => Auto::Failed(e.to_string()),
    }
}

/// Файл со списком интеграций, выключенных пользователем вручную.
fn opt_out_file(cfg_dir: &Path) -> PathBuf {
    cfg_dir.join("integrations-off")
}

fn opted_out(cfg_dir: &Path, id: &str) -> bool {
    std::fs::read_to_string(opt_out_file(cfg_dir)).is_ok_and(|t| t.lines().any(|l| l.trim() == id))
}

fn set_opt_out(cfg_dir: &Path, id: &str, off: bool) {
    let text = std::fs::read_to_string(opt_out_file(cfg_dir)).unwrap_or_default();
    let mut ids: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && *l != id).collect();
    if off {
        ids.push(id);
    }
    let _ = std::fs::create_dir_all(cfg_dir);
    let _ = std::fs::write(opt_out_file(cfg_dir), ids.join("\n") + if ids.is_empty() { "" } else { "\n" });
}

fn install_in(id: &str, home: &Path, exe: &str) -> Result<PathBuf> {
    if id == "claude" {
        bail!("для Claude Code интеграция уже встроена");
    }
    let sp = spec(id).ok_or_else(|| anyhow!("для «{id}» интеграции нет"))?;
    let path = target_path(id, home).ok_or_else(|| anyhow!("для «{id}» интеграции нет"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match sp.method {
        Method::OpenCodePlugin => {
            backup_once(&path)?;
            std::fs::write(&path, opencode_plugin(exe))?;
        }
        Method::JsonHooks(events) => {
            let mut root = read_json(&path)?;
            backup_once(&path)?;
            add_hooks(&mut root, events, exe)?;
            std::fs::write(&path, serde_json::to_vec_pretty(&root)?)?;
        }
    }
    Ok(path)
}

fn uninstall_in(id: &str, home: &Path) -> Result<()> {
    let sp = spec(id).ok_or_else(|| anyhow!("для «{id}» интеграции нет"))?;
    let path = target_path(id, home).ok_or_else(|| anyhow!("для «{id}» интеграции нет"))?;
    if !path.exists() {
        return Ok(());
    }
    match sp.method {
        Method::OpenCodePlugin => {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains(PLUGIN_MARK) {
                std::fs::remove_file(&path)?;
            }
        }
        Method::JsonHooks(_) => {
            let mut root = read_json(&path)?;
            remove_hooks(&mut root);
            // файл создан нами и теперь пуст — убираем его целиком
            if root.as_object().is_some_and(serde_json::Map::is_empty) && !backup_path(&path).exists() {
                std::fs::remove_file(&path)?;
            } else {
                std::fs::write(&path, serde_json::to_vec_pretty(&root)?)?;
            }
        }
    }
    Ok(())
}

fn backup_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".radar-backup");
    PathBuf::from(s)
}

/// Копия исходного файла перед первой правкой (повторно не перезаписывается).
fn backup_once(p: &Path) -> Result<()> {
    let b = backup_path(p);
    if p.exists() && !b.exists() {
        let text = std::fs::read_to_string(p).unwrap_or_default();
        if !text.contains(PLUGIN_MARK) && !text.contains(&format!("\"name\": \"{MARK}\"")) {
            std::fs::copy(p, b)?;
        }
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(t) => serde_json::from_str(&t).with_context(|| {
            format!("{}: не удалось разобрать JSON (комментарии не поддерживаются) — файл не тронут", path.display())
        }),
        Err(_) => Ok(Value::Object(Map::new())),
    }
}

fn is_radar_group(g: &Value) -> bool {
    g.get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|h| h.iter().any(|x| x.get("name").and_then(|n| n.as_str()) == Some(MARK)))
}

fn has_radar_hooks(root: &Value) -> bool {
    root.get("hooks")
        .and_then(|h| h.as_object())
        .is_some_and(|h| h.values().any(|v| v.as_array().is_some_and(|a| a.iter().any(is_radar_group))))
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn add_hooks(root: &mut Value, events: &[&str], exe: &str) -> Result<()> {
    remove_hooks(root);
    let obj = root.as_object_mut().ok_or_else(|| anyhow!("корень конфигурации — не объект JSON"))?;
    let hooks = obj.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut().ok_or_else(|| anyhow!("поле hooks — не объект"))?;
    for ev in events {
        let list = hooks.entry(ev.to_string()).or_insert_with(|| json!([]));
        let list = list.as_array_mut().ok_or_else(|| anyhow!("hooks.{ev} — не массив"))?;
        list.push(json!({
            "hooks": [{ "type": "command", "command": format!("{} hook {ev}", shq(exe)), "name": MARK }]
        }));
    }
    Ok(())
}

fn remove_hooks(root: &mut Value) {
    let Some(obj) = root.as_object_mut() else { return };
    let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut()) else { return };
    for list in hooks.values_mut() {
        if let Some(a) = list.as_array_mut() {
            a.retain(|g| !is_radar_group(g));
        }
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        obj.remove("hooks");
    }
}

fn opencode_plugin(exe: &str) -> String {
    format!(
        r#"// {PLUGIN_MARK}: плагин Radar для OpenCode — передаёт статусы агента в интерфейс Radar.
// Управление: `radar integration install|uninstall opencode`. Вне Radar ничего не делает.
import {{ spawnSync }} from "node:child_process"

const RADAR = {exe}

export const RadarPlugin = async () => {{
  if (!process.env.RADAR_SOCK || !process.env.RADAR_SESSION) return {{}}
  const send = (event, payload = {{}}) => {{
    try {{
      spawnSync(RADAR, ["hook", event], {{ input: JSON.stringify(payload), timeout: 3000 }})
    }} catch (_) {{}}
  }}
  return {{
    event: async ({{ event }}) => {{
      const p = event.properties || {{}}
      switch (event.type) {{
        case "session.status":
          if (p.status && p.status.type === "idle") send("Stop")
          else if (p.status && p.status.type === "busy") send("PreToolUse")
          break
        case "session.idle":
          send("Stop")
          break
        case "permission.asked":
          send("PermissionRequest", {{ message: p.title || "Требуется разрешение" }})
          break
        case "permission.replied":
          send("PostToolUse")
          break
      }}
    }},
  }}
}}
"#,
        exe = serde_json::to_string(exe).unwrap()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn tmp(name: &str) -> TempDir {
        TempDir::new(&format!("int-{name}"))
    }

    #[test]
    fn ensure_enables_repairs_and_respects_opt_out() {
        let h = tmp("auto");
        let cfg = h.join("cfg");
        let real = h.join("radar");
        std::fs::write(&real, "").unwrap();
        let exe = real.to_string_lossy().into_owned();
        // выключена → включаем сами
        assert_eq!(ensure_in("gigacode", &h, &cfg, &exe), Auto::Enabled);
        assert_eq!(state_in("gigacode", &h), State::Installed);
        // уже включена и путь живой → не трогаем
        assert_eq!(ensure_in("gigacode", &h, &cfg, &exe), Auto::Skipped);
        // путь умер → чиним
        install_in("gigacode", &h, "/нет/radar").unwrap();
        assert_eq!(ensure_in("gigacode", &h, &cfg, &exe), Auto::Repaired);
        assert_eq!(stale_exe_in("gigacode", &h), None);
        // выключено руками → автоматика не возвращает
        uninstall_in("gigacode", &h).unwrap();
        set_opt_out(&cfg, "gigacode", true);
        assert_eq!(ensure_in("gigacode", &h, &cfg, &exe), Auto::Skipped);
        assert_eq!(state_in("gigacode", &h), State::NotInstalled);
        // явная установка снимает запрет
        set_opt_out(&cfg, "gigacode", false);
        assert_eq!(ensure_in("gigacode", &h, &cfg, &exe), Auto::Enabled);
        // Claude встроен, неизвестные агенты пропускаются
        assert_eq!(ensure_in("claude", &h, &cfg, &exe), Auto::Skipped);
        assert_eq!(ensure_in("unknown", &h, &cfg, &exe), Auto::Skipped);
    }

    #[test]
    fn ensure_does_not_touch_broken_config() {
        let h = tmp("auto-bad");
        let p = h.join(".qwen/settings.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ // комментарий\n}").unwrap();
        assert!(matches!(ensure_in("qwen", &h, &h.join("cfg"), "/x/radar"), Auto::Failed(_)));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ // комментарий\n}");
    }

    #[test]
    fn stale_exe_is_reported_for_missing_binary() {
        let h = tmp("stale");
        let real = h.join("radar bin");
        std::fs::write(&real, "").unwrap();
        install_in("qwen", &h, &real.to_string_lossy()).unwrap();
        assert_eq!(stale_exe_in("qwen", &h), None);
        std::fs::remove_file(&real).unwrap();
        assert_eq!(stale_exe_in("qwen", &h), Some(real.to_string_lossy().into_owned()));
        // плагин OpenCode проверяется так же
        install_in("opencode", &h, "/нет/такого/radar").unwrap();
        assert_eq!(stale_exe_in("opencode", &h).as_deref(), Some("/нет/такого/radar"));
        // не установлено — нечего проверять
        assert_eq!(stale_exe_in("gemini", &h), None);
    }

    #[test]
    fn json_hooks_roundtrip_preserves_user_config() {
        let h = tmp("json");
        let p = h.join(".qwen/settings.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let orig = json!({"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo hi"}]}]}});
        std::fs::write(&p, serde_json::to_vec_pretty(&orig).unwrap()).unwrap();

        assert_eq!(state_in("qwen", &h), State::NotInstalled);
        install_in("qwen", &h, "/opt/my radar").unwrap();
        assert_eq!(state_in("qwen", &h), State::Installed);
        // повторная установка не плодит дубли
        install_in("qwen", &h, "/opt/my radar").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(v["theme"], "dark");
        assert!(v["hooks"]["Stop"][1]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("'/opt/my radar' hook Stop"));
        assert!(backup_path(&p).exists());

        uninstall_in("qwen", &h).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v, orig);
        assert_eq!(state_in("qwen", &h), State::NotInstalled);
    }

    #[test]
    fn fresh_file_is_removed_on_uninstall() {
        let h = tmp("fresh");
        install_in("gemini", &h, "/bin/radar").unwrap();
        let p = target_path("gemini", &h).unwrap();
        assert!(p.exists());
        uninstall_in("gemini", &h).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn broken_json_is_not_touched() {
        let h = tmp("broken");
        let p = h.join(".gemini/settings.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ // comment\n}").unwrap();
        assert!(install_in("gemini", &h, "/bin/radar").is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ // comment\n}");
    }

    #[test]
    fn opencode_plugin_install_uninstall() {
        let h = tmp("oc");
        install_in("opencode", &h, "/bin/radar").unwrap();
        assert_eq!(state_in("opencode", &h), State::Installed);
        let t = std::fs::read_to_string(target_path("opencode", &h).unwrap()).unwrap();
        assert!(t.contains("const RADAR = \"/bin/radar\""));
        uninstall_in("opencode", &h).unwrap();
        assert_eq!(state_in("opencode", &h), State::NotInstalled);
    }

    #[test]
    fn claude_is_builtin() {
        assert_eq!(state_in("claude", Path::new("/nonexistent")), State::Builtin);
        assert!(install_in("claude", Path::new("/nonexistent"), "x").is_err());
    }
}
