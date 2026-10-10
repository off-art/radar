# Установка

[← README](../../README.md) · [English](../en/install.md)

Не нужны ни Rust, ни права администратора. Поддерживаются macOS (Apple Silicon и Intel) и Linux (`x86_64`, `aarch64`).

## Способ 1 — одной командой (рекомендуется)

```sh
curl -fsSL https://raw.githubusercontent.com/off-art/radar/main/install.sh | bash
```

Скрипт скачивает готовый бинарник под ваш процессор в `~/.local/bin` и подсказывает, что добавить в `PATH`.
Файл, скачанный через `curl`, карантином macOS не помечается.

## Способ 2 — Homebrew

```sh
brew install off-art/radar/radar
brew upgrade off-art/radar/radar   # обновление
```

Работает на macOS и в Linuxbrew. `radar update` для такой установки подскажет команду `brew upgrade off-art/radar/radar`.

## Способ 3 — архив вручную

1. На странице [Releases](https://github.com/off-art/radar/releases/latest) скачайте архив: `radar-aarch64-apple-darwin.tar.gz`
   (Apple Silicon: M1/M2/M3…) или `radar-x86_64-apple-darwin.tar.gz` (Intel). Процессор: `uname -m` (`arm64` — Apple Silicon, `x86_64` — Intel).
2. Распакуйте и положите в `~/.local/bin`:
   ```sh
   cd ~/Downloads
   tar xzf radar-aarch64-apple-darwin.tar.gz        # имя — как у скачанного файла
   mkdir -p ~/.local/bin && mv radar ~/.local/bin/
   xattr -d com.apple.quarantine ~/.local/bin/radar # снимает карантин браузера
   ```
3. Если `~/.local/bin` нет в `PATH`:
   ```sh
   echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
   ```

Без `xattr` macOS пишет, что не может проверить разработчика: тогда *Системные настройки → Конфиденциальность и безопасность → «Всё равно открыть»*.
Такой же архив можно переслать коллеге в мессенджере.

## Способ 4 — из исходников

Нужен Rust. Без brew он ставится так:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh     # затем перезапустите терминал
git clone https://github.com/off-art/radar.git
cd radar
./install.sh                                                        # соберёт и положит в ~/.local/bin
```

Первая сборка — около двух минут. Альтернатива: `cargo install --path .` (ставит в `~/.cargo/bin`).

## Linux (Debian, СберОС и др.)

Готовые сборки есть для `x86_64` и `aarch64` (glibc 2.35+: Debian 12/13, Ubuntu 22.04+). Проверено на Debian 13 (aarch64, контейнер):
установка, `radar update`, фоновые сессии, `radar stop`. Установка и обновление те же, что выше.

Если GitHub с вашей сети недоступен — скачайте `radar-x86_64-unknown-linux-gnu.tar.gz` (или `aarch64`) на странице Releases и:

```sh
tar xzf radar-x86_64-unknown-linux-gnu.tar.gz
mkdir -p ~/.local/bin && rm -f ~/.local/bin/radar && mv radar ~/.local/bin/
```

Для уведомлений и звука нужны `libnotify-bin` (`notify-send`) и `pulseaudio-utils` (`paplay`) или `alsa-utils` (`aplay`):
`sudo apt install libnotify-bin pulseaudio-utils`. Копирование мышью идёт в системный буфер через `wl-copy` (Wayland) или
`xclip`/`xsel` (X11), если они установлены, иначе через OSC 52 (терминал должен его поддерживать).
Из исходников: `sudo apt install build-essential curl git`, затем Rust и `./install.sh`.

## После установки

```sh
radar doctor        # какие агенты найдены
radar notify-test   # проверка уведомлений: иконка, звуки, тестовое уведомление
radar               # запуск
```

При первом уведомлении macOS спросит разрешение для «Radar» — нажмите «Разрешить» (или включите в *Системные настройки → Уведомления → Radar*).

## Обновление

Настройки (`~/.config/radar/`), сохранённые агенты и интеграции при обновлении остаются. Сначала закройте все окна Radar.

```sh
radar update            # скачает последнюю версию и заменит себя
radar update --check    # только проверить, есть ли новая
```

Если у вас версия 0.3.0 или старше, команды `radar update` ещё нет: один раз обновитесь повторной командой установки (способ 1), дальше хватит `radar update`.

Вручную (архив): сначала **удалите** старый бинарник, потом положите новый:

```sh
rm ~/.local/bin/radar && mv radar ~/.local/bin/ && xattr -d com.apple.quarantine ~/.local/bin/radar
radar --version
```

> На Apple Silicon не копируйте новый бинарник поверх старого командой `cp` — macOS убьёт процесс (`killed`).
> `install.sh` делает это правильно (удаляет, кладёт новый файл, подписывает).

## Удаление

```sh
radar stop                          # остановить агентов, работающих в фоне
radar integration uninstall all     # если включали интеграции: убирает хуки Radar из конфигов агентов
rm ~/.local/bin/radar
rm -rf ~/.config/radar ~/.radar     # настройки, список агентов, помощник уведомлений, git-worktree (необязательно)
```
