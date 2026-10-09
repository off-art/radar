# Statuses, integrations and notifications

[← README](../../README.en.md) · [Русский](../ru/integrations.md)

## How statuses are detected

- **Claude Code: exact, via hooks.** Radar starts `claude --settings ~/.config/radar/claude-settings.json`.
  Hooks (`UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop`…) call `radar hook <event>`, which reports to the UI over a
  unix socket. Your global Claude Code settings and hooks are untouched. Hooks are separate processes and sometimes arrive out of order
  or get lost, so there is a safety net: if hooks say "working" but the screen has been silent for 6 seconds with no `esc to interrupt`
  hint, Radar treats the agent as finished (and sends a notification).
- **Other agents: heuristics** (screen content and output activity):
  - a hint like `esc to interrupt` or ongoing output → `working`;
  - a question on screen (`(y/n)`, `Do you want to proceed?`, `Allow once`, …) and output went quiet → `waiting`;
  - output silent for more than 2 seconds → `ready`.

  If your agent phrases prompts differently, add the phrase to `src/status.rs` (the `WAITING` and `WORKING` lists); it is a couple of lines.

## Integrations

![Integrations](../integrations.png)

For other agents exact statuses can be enabled via hooks. **Settings → Integrations tab** (`,` in navigation mode, then `Tab`;
or right click → "Agent integrations…"): `✓` enabled, `–` disabled, and the right side shows whether the agent is installed.
`Enter`/`Space`/click toggles. The same from the terminal:

```sh
radar integration                      # list and state
radar integration install qwen         # enable (or all)
radar integration uninstall qwen       # disable
```

| Agent | What is edited | Verified |
|---|---|---|
| Claude Code | nothing: built in, always on | yes |
| Qwen Code | `~/.qwen/settings.json` (hooks) | yes, Qwen Code 0.25: working → waiting for permission (the request is visible in the list) → ready |
| GigaCode | `~/.gigacode/settings.json` (hooks; a Qwen Code fork) | no, per documentation |
| Gemini CLI | `~/.gemini/settings.json` (hooks) | no, per documentation |
| Codex | `~/.codex/hooks.json` (hooks) | no, per documentation |
| OpenCode | plugin `~/.config/opencode/plugins/radar.js` | no, per documentation |

Changes are reversible: Radar's entries are tagged with the name `radar`, the rest of the config is untouched, and a `*.radar-backup`
is saved next to the file before the first edit. If the file has comments or does not parse as JSON, Radar leaves it alone.
Outside Radar these hooks do nothing. Already running agents must be restarted. Without an integration, heuristics apply.

**Debugging.** `RADAR_HOOK_LOG=1 radar` writes every hook event (agent, event, payload) to `~/.config/radar/hooks.log`;
`tail -f` shows what the agent really sends. If statuses are inaccurate after enabling an integration, attach this log to an issue.

## Notifications

Notifications come from the **Radar** app with the radar icon: on first run Radar creates a tiny helper
`~/.config/radar/Radar.app` itself (it needs only the stock `osacompile`/`codesign` from macOS). If that fails, it falls back to `osascript`.
Sound uses custom soft chimes: one for "done", a slightly lower one for "needs confirmation".

- Sound and popups are independent: `popups = false` keeps sound only, `sound = false` keeps popups only.
- Seven sound themes: `bell`, `sonar`, `retro`, `harp`, `knock`, `thump`, `drop`. Pick one with preview via `Ctrl+b`, `S`
  (or right click on empty space → "Choose sound…"). The choice is stored in `~/.config/radar/state.toml` and overrides `config.toml`.
- Volume is `volume`, the default theme is `sound_theme`, custom files are `sound_done` / `sound_waiting`.
- No notification is shown if the terminal window is focused and you are looking at that very agent.
- Notifications can be muted per agent: right click → "Mute agent notifications" (a `⊘` icon appears in the list).

Check with **`radar notify-test`**: it creates the helper, plays both sounds and shows a test notification.
