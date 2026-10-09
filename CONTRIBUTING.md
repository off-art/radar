# Разработка / Contributing

Radar — Rust, [Ratatui](https://ratatui.rs), crossterm, portable-pty, vt100. Один бинарник, минимум зависимостей.

```sh
cargo run                 # запуск
cargo test                # тесты
cargo build --release     # релизная сборка → target/release/radar
```

## Перед коммитом

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test   # то же проверяет CI
```

## Структура `src/`

- `main.rs` — точка входа и подкоманды (`host`, `hook`, `ctl`, `doctor`, `integration`, …); `ctl.rs` — клиент `radar ctl`; `groups.rs` — модель групп;
- `app/` — состояние и события окна: `mod.rs` (типы и `App`), `run.rs` (главный цикл), `sessions.rs`, `groups.rs`, `layout.rs`,
  `attention.rs` (статусы, тики, уведомления), `keys.rs`, `mouse.rs`, `actions.rs`, `settings.rs`, `git_actions.rs`, `views.rs`,
  `approve.rs`, `remote.rs` (`radar ctl`);
- `ui/` — отрисовка (ratatui): `sidebar`, `panes`, `statusbar`, `popup`, `form`, `overlays`, `settings`, `diff`;
- `session.rs` — pty и конечный автомат статусов; `status.rs` — эвристика; `host.rs` — фоновый хозяин агента;
- `hook.rs`, `integrations.rs` — хуки и плагины агентов; `events.rs` — журнал событий;
- `git.rs`, `gitops.rs`, `diff.rs` — git (запуск с таймаутом, наблюдатель, worktree, коммит/push, diff);
- `config.rs`, `persist.rs`, `paths.rs` — настройки, сохранённый список агентов, каталоги;
- `theme.rs`, `keys.rs`, `input.rs`, `menu.rs`, `textfield.rs`, `complete.rs`, `notify.rs`, `clipboard.rs`, `update.rs`,
  `sync.rs` (мьютексы без паники), `testutil.rs` (помощники тестов);
- `assets/` — иконка, звуки (`python3 assets/gen_sounds.py` пересоздаёт звуки) и исходники логотипа (`assets/brand/`).

## Как мы работаем над фичей

1. Ветка `feat/<имя>`, одна фича — один PR.
2. Код + юнит-тесты + проверка в tmux на демо-агентах.
3. Фича доступна **и мышью, и клавишей, и из палитры**; новые клавиши — через `Action` и `[keys]`.
4. Обновить README и `docs/` (оба языка), справку `?`, палитру и пример `config.toml`.
5. Релиз: поднять `version` в `Cargo.toml`, обновить скриншоты в `docs/`, тег `vX.Y.Z` (один тег собирает macOS и Linux).

Планы — в [ROADMAP.md](ROADMAP.md).

---

Radar is written in Rust (Ratatui, crossterm, portable-pty, vt100). Run `cargo run` / `cargo test`; before committing run `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`. The list above maps modules to responsibilities.
Every feature must work by mouse, by key and from the command palette, and be documented in both `docs/ru` and `docs/en`.
