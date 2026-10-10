# Windows: план и аудит

Правило: изменения для Windows не должны менять поведение macOS и Linux. Платформенный код — под `cfg(unix)` / `cfg(windows)`,
unix-ветки остаются как есть. Пробные релизы — теги вида `v0.7.0-win.N` (не попадают в `latest`, `install.sh`, brew, `radar update`).

## Где сейчас unix-код

| Файл | Что | Идея для Windows |
|---|---|---|
| `host.rs`, `session.rs`, `hook.rs`, `ctl.rs` | `UnixListener` / `UnixStream` (фоновые сессии, хуки, `radar ctl`) | модуль `ipc`: на Windows AF_UNIX (крейт `uds_windows`), код выше не меняется |
| `host.rs` | `libc::recv(MSG_PEEK)` для `PeekByte` | на Windows — `peek` через `uds_windows`/WinSock |
| `host.rs`, `session.rs` | `libc::setsid()`, `CommandExt::pre_exec` | на Windows `creation_flags(DETACHED_PROCESS \| CREATE_NO_WINDOW)` |
| `session.rs` | список шеллов `/bin/zsh`, `/bin/bash`… | `pwsh` / `powershell` / `cmd` |
| `events.rs` | `libc::localtime_r` | локальное время через `GetLocalTime`/`chrono`-free вариант |
| `hook.rs` | `PermissionsExt` (chmod 0600 сокета) | на Windows права каталога профиля |
| `notify.rs` | `notify-send`, `paplay` | PowerShell toast / `[System.Media.SoundPlayer]` |
| `update.rs` | `tar`, `rename`-замена бинарника, `/usr/` | `.zip`, замена через переименование `.old` |
| `paths.rs` | `~/.config/radar` | `%APPDATA%\radar` |

## Этапы

1. CI: задача `windows` (`cargo check`, не блокирует). Список ошибок компиляции = точный объём работы.
2. Модуль `ipc` и поддержка `cfg(windows)` в местах таблицы выше, пока `cargo check` на Windows не станет зелёным.
3. Запуск на ноутбуке: TUI, ConPTY (`portable-pty`), агент в PowerShell.
4. Хуки Claude Code и интеграции (`radar hook`), пути конфигов агентов на Windows.
5. Уведомления и звук.
6. Релиз: задача `build-windows` (MSVC, `.zip`), `install.ps1`, `radar update` для Windows, winget/scoop — позже.
