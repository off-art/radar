# Settings, color themes and limitations

[← README](../../README.en.md) · [Русский](../ru/config.md)

## Settings window

`Ctrl+b` then `,` (or right click on empty space → "Settings…") opens a window: color theme, notifications, sound and its theme, volume,
popups, list width, restoring agents. Values change with arrows or a click and apply immediately;
everything is stored in `~/.config/radar/state.toml`. The Integrations tab is one `Tab` away (see [Statuses and integrations](integrations.md)).

## Config

`~/.config/radar/config.toml` (an example is created on first run):

```toml
notifications = true
sound = true
popups = true         # popup notifications (sound does not depend on them)
restore = true        # restore the agent list on startup
sidebar_width = 0     # agent list width, 0 = automatic (drag the border with the mouse)
scrollback = 5000     # scrollback depth, lines per agent (≈15 MB per agent at 100 columns)
sound_theme = "bell"  # bell, sonar, retro, harp, knock, thump, drop
volume = 0.6          # notification volume, 0.0–1.0
mouse = true          # false: native terminal selection instead of Radar's

[theme]
name = "radar"        # see "Color themes"; custom colors go in [theme.custom]

[keys]
prefix = "ctrl+b"     # navigation-mode prefix; custom shortcuts go in [keys.direct]

# Custom agents or overrides of built-ins (by name)
[[agent]]
name = "Claude Code"
command = "claude"
args = ["--model", "opus"]
kind = "claude"       # claude: statuses via hooks, generic: heuristics, shell: plain shell
color = "#d97757"

[[agent]]
name = "Aider"
command = "aider"
color = "#22c55e"
```

Another config path: the `RADAR_CONFIG` variable. Anything changed from the UI (theme, sound, volume, list width, restore)
is stored in `state.toml` and wins over `config.toml`; the agent list lives in `sessions.toml`. Command help: `radar --help`.

## Color themes

`radar` (default, your terminal's background), `terminal` (your terminal's palette colors), `catppuccin`, `catppuccin-latte`, `tokyo-night`,
`tokyo-night-day`, `gruvbox`, `dracula`, `nord`, `one-dark`, `solarized-dark`, `solarized-light`. Except for `radar` and `terminal`, a theme paints
the background and remaps the 16 base ANSI colors inside agent windows, so `ls`, `git diff` and other colored output match.

Custom colors on top of the chosen theme (hex, `rgb(r,g,b)` or a color name):

```toml
[theme]
name = "catppuccin"

[theme.custom]
accent = "#a6e3a1"
sidebar_bg = "#181825"
```

Tokens: `accent`, `panel_bg`, `sidebar_bg`, `header_bg`, `active_row_bg`, `text`, `subtext0`, `overlay0`, `overlay1`,
`green`, `yellow`, `red`, `blue`, `teal`, `peach`, `mauve`.

## Limitations

- GigaCode, Codex, Gemini CLI and OpenCode integrations follow their documentation and are not verified on real agents.
- Status heuristics for non-Claude agents can be wrong on unusual wording.
- Automatic light/dark theme switching by system appearance is not supported.
- There is no native Windows build; in WSL2 Radar runs as the Linux version.
- Linux is verified on Debian 13 (aarch64, container); real SberOS (x86_64) has not been tested yet.
- Scrollback accumulated before a window was closed is not kept when reattaching.
- macOS-dependent tests (notifications, `codesign`) run only in CI.
