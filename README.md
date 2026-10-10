<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/radar-wordmark-on-dark.svg">
  <img src="assets/brand/radar-wordmark-on-light.svg" alt="radar_" height="64">
</picture>

**Много AI-агентов — в одном окне терминала.**
Видно, кто работает, кто закончил и кому нужен ваш ответ.

[![Release](https://img.shields.io/github/v/release/off-art/radar?color=orange)](https://github.com/off-art/radar/releases/latest)
[![CI](https://github.com/off-art/radar/actions/workflows/ci.yml/badge.svg)](https://github.com/off-art/radar/actions/workflows/ci.yml)
![Platforms](https://img.shields.io/badge/macOS%20%C2%B7%20Linux-lightgrey)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Русский · [English](README.en.md)

![Radar: три агента, запрос разрешения, diff, сетка](docs/demo.gif)

</div>

Claude Code, Codex, OpenCode, Qwen Code, GigaCode, Gemini CLI и обычный shell живут в одном окне:
слева список со статусами, справа живой терминал выбранного агента. Один бинарник на Rust, без зависимостей.

## Почему Radar

- **Статусы в реальном времени** — `работает` · `ждёт ответа` · `готов`. Для Claude Code и Qwen Code точно, через хуки.
- **Уведомления**, когда агент закончил или ждёт вас (macOS и Linux), со звуком.
- **Git на виду** — ветка и `+12 −3` у каждого агента, просмотр diff, commit / push / merge без выхода из Radar.
- **Агенты живут в фоне** — закрыли окно, задачи продолжаются; `radar` подключается обратно.
- **Ответ на запрос разрешения из списка** — `Ctrl+b y`, не заходя в агента, и всегда с текстом запроса.
- **Управление из скриптов** — `radar ctl new / send / wait / read`.

И ещё: сетка до 9 агентов, группы, изоляция через git worktree, лента событий, 12 цветовых схем, мышь и палитра команд.

## Установка

```sh
curl -fsSL https://raw.githubusercontent.com/off-art/radar/main/install.sh | bash
```

Или через Homebrew (macOS и Linux): `brew install off-art/radar/radar`.

Ни Rust, ни прав администратора не нужно. Обновление: `radar update` (для установки через brew — `brew upgrade off-art/radar/radar`).
Ручная установка, Linux, удаление — в [docs/ru/install.md](docs/ru/install.md).

## Быстрый старт

```sh
radar                        # открыть интерфейс, агентов добавлять клавишей n
radar ~/work/api claude      # сразу запустить Claude Code в папке
radar doctor                 # какие агенты найдены
```

Нажмите **`Ctrl+b`** — включится режим навигации (внизу жёлтая плашка). Выход из него — `Esc`.

| Клавиша | Действие |
|---|---|
| `j` / `k` | следующий / предыдущий агент |
| `n` | новый агент |
| `w` | перейти к агенту, который ждёт ответа |
| `y` | разрешить запрос агента |
| `v` | изменения агента (`git diff`) |
| `g` | сетка ⇄ один агент |
| `p` | палитра команд |
| `?` | справка |

Мышь тоже работает: клик выбирает агента, ПКМ открывает меню, выделение текста копируется в буфер.
Полная таблица — в [docs/ru/usage.md](docs/ru/usage.md).

## Документация

| Раздел | О чём |
|---|---|
| [Установка](docs/ru/install.md) | все способы, Linux, обновление, удаление |
| [Работа в интерфейсе](docs/ru/usage.md) | клавиши, мышь, форма агента, группы, копирование |
| [Git](docs/ru/git.md) | ветка в списке, diff, commit/push/merge, разрешение запросов, лента событий |
| [Автоматизация](docs/ru/automation.md) | `radar ctl`, фоновые сессии |
| [Статусы и уведомления](docs/ru/integrations.md) | хуки, интеграции агентов, звук |
| [Настройки](docs/ru/config.md) | `config.toml`, цветовые схемы, ограничения |
| [Для разработчиков](CONTRIBUTING.md) | структура кода, сборка, тесты |

## Разработка

```sh
cargo run && cargo test
```

Подробности — в [CONTRIBUTING.md](CONTRIBUTING.md). Планы — в [ROADMAP.md](ROADMAP.md).

## Лицензия

MIT
