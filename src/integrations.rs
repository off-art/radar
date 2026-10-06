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

const QWEN_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "PermissionRequest",
    "Stop",
];
const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
];
const GEMINI_EVENTS: &[&str] = &[
    "SessionStart",
    "BeforeAgent",
    "BeforeTool",
    "AfterTool",
    "AfterAgent",
    "Notification",
];

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

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Файл, который правит интеграция.
pub fn target_path(id: &str, home: &Path) -> Option<PathBuf> {
    Some(match id {
        "qwen" => home.join(".qwen/settings.json"),
        // GigaCode — форк Qwen Code; в документации путь не указан, по умолчанию ~/.gigacode
        "gigacode" => home.join(".gigacode/settings.json"),
        "gemini" => home.join(".gemini/settings.json"),
        "codex" => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))
            .join("hooks.json"),
        "opencode" => home.join(".config/opencode/plugins/radar.js"),
        _ => return None,
    })
}

/// Короткое пояснение, что именно будет изменено.
pub fn describe(id: &str) -> String {
    match id {
        "claude" => "встроено: хуки подключаются при каждом запуске".into(),
        _ => match target_path(id, &home()) {
            Some(p) => p.to_string_lossy().replacen(&*home().to_string_lossy(), "~", 1),
            None => "не поддерживается".into(),
        },
    }
}

pub fn state(id: &str) -> State {
    state_in(id, &home())
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
        Method::JsonHooks(_) => serde_json::from_str::<Value>(&text)
            .map(|v| has_radar_hooks(&v))
            .unwrap_or(false),
    };
    if on { State::Installed } else { State::NotInstalled }
}

pub fn install(id: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("не удалось определить путь к radar")?;
    install_in(id, &home(), &exe.to_string_lossy())
}

pub fn uninstall(id: &str) -> Result<()> {
    uninstall_in(id, &home())
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
            if root.as_object().map(|o| o.is_empty()).unwrap_or(false) && !backup_path(&path).exists() {
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
        Ok(t) => serde_json::from_str(&t)
            .with_context(|| format!("{}: не удалось разобрать JSON (комментарии не поддерживаются) — файл не тронут", path.display())),
        Err(_) => Ok(Value::Object(Map::new())),
    }
}

fn is_radar_group(g: &Value) -> bool {
    g.get("hooks")
        .and_then(|h| h.as_array())
        .map(|h| h.iter().any(|x| x.get("name").and_then(|n| n.as_str()) == Some(MARK)))
        .unwrap_or(false)
}

fn has_radar_hooks(root: &Value) -> bool {
    root.get("hooks")
        .and_then(|h| h.as_object())
        .map(|h| h.values().any(|v| v.as_array().map(|a| a.iter().any(is_radar_group)).unwrap_or(false)))
        .unwrap_or(false)
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
    hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
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

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("radar-int-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
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
        assert!(v["hooks"]["Stop"][1]["hooks"][0]["command"].as_str().unwrap().starts_with("'/opt/my radar' hook Stop"));
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
