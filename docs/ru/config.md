# Настройки, цветовые схемы и ограничения

[← README](../../README.md) · [English](../en/config.md)

## Окно настроек

`Ctrl+b`, затем `,` (или ПКМ на пустом месте → «Настройки…») открывает окно: цветовая схема, уведомления, звук и его тема, громкость,
всплывающие окошки, ширина списка, восстановление агентов. Значения меняются стрелками или кликом и сразу применяются;
всё запоминается в `~/.config/radar/state.toml`. Вкладка «Интеграции» — по `Tab` (см. [Статусы и интеграции](integrations.md)).

## Конфиг

`~/.config/radar/config.toml` (пример создаётся при первом запуске):

```toml
notifications = true
sound = true
popups = true         # всплывающие уведомления (звук от них не зависит)
restore = true        # восстанавливать список агентов при запуске
sidebar_width = 0     # ширина списка агентов, 0 — автоматически (можно тянуть мышью за границу)
scrollback = 5000     # глубина прокрутки назад, строк на агента (≈15 МБ на агента при 100 колонках)
sound_theme = "bell"  # bell, sonar, retro, harp, knock, thump, drop
volume = 0.6          # громкость уведомлений, 0.0–1.0
mouse = true          # false — родное выделение терминала вместо выделения Radar

[theme]
name = "radar"        # см. «Цветовые схемы»; свои цвета — [theme.custom]

[keys]
prefix = "ctrl+b"     # префикс режима навигации; свои сочетания — [keys.direct]

# Свои агенты или переопределение встроенных (по имени)
[[agent]]
name = "Claude Code"
command = "claude"
args = ["--model", "opus"]
kind = "claude"       # claude — статусы через хуки, generic — эвристика, shell — обычный shell
color = "#d97757"

[[agent]]
name = "Aider"
command = "aider"
color = "#22c55e"
```

Другой путь к конфигу — переменная `RADAR_CONFIG`. Что меняется из интерфейса (тема, звук, громкость, ширина списка, восстановление),
хранится в `state.toml` и важнее `config.toml`; список агентов — в `sessions.toml`. Справка по командам: `radar --help`.

## Цветовые схемы

`radar` (по умолчанию, фон вашего терминала), `terminal` (цвета палитры вашего терминала), `catppuccin`, `catppuccin-latte`, `tokyo-night`,
`tokyo-night-day`, `gruvbox`, `dracula`, `nord`, `one-dark`, `solarized-dark`, `solarized-light`. Кроме `radar` и `terminal`, схема закрашивает
фон и подменяет 16 базовых цветов ANSI в окнах агентов, так что `ls`, `git diff` и прочий цветной вывод выглядят в тон.

Свои цвета поверх выбранной схемы (hex, `rgb(r,g,b)` или имя цвета):

```toml
[theme]
name = "catppuccin"

[theme.custom]
accent = "#a6e3a1"
sidebar_bg = "#181825"
```

Токены: `accent`, `panel_bg`, `sidebar_bg`, `header_bg`, `active_row_bg`, `text`, `subtext0`, `overlay0`, `overlay1`,
`green`, `yellow`, `red`, `blue`, `teal`, `peach`, `mauve`.

## Ограничения

- Интеграции GigaCode, Codex, Gemini CLI и OpenCode сделаны по документации и не проверены на реальных агентах.
- Эвристика статусов для не-Claude агентов может ошибаться на нестандартных формулировках.
- Автопереключение светлой/тёмной схемы по теме системы не поддерживается.
- Нативного Windows нет; в WSL2 Radar работает как Linux-версия.
- Linux проверен на Debian 13 (aarch64, контейнер); реальная СберОС (x86_64) пока не проверялась.
- Прокрутка истории, накопленная до закрытия окна, при повторном подключении не сохраняется.
- Тесты, зависящие от macOS (уведомления, `codesign`), выполняются только в CI.
