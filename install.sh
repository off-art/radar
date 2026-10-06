#!/usr/bin/env bash
# Установка Radar на macOS (Apple Silicon и Intel).
#
#   curl -fsSL https://raw.githubusercontent.com/off-art/radar/main/install.sh | bash
#
# Скачивает готовый бинарник из GitHub Releases в ~/.local/bin.
# Если запустить из клонированного репозитория (./install.sh) — соберёт из исходников, нужен Rust.
set -euo pipefail

REPO="${RADAR_REPO:-off-art/radar}"
BIN_DIR="${RADAR_BIN_DIR:-$HOME/.local/bin}"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf '\033[31mОшибка:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || say "Внимание: скрипт рассчитан на macOS, текущая система — $(uname -s)"

mkdir -p "$BIN_DIR"

# Ставим бинарник НОВЫМ файлом: если перезаписать работающий radar поверх (cp), macOS на Apple Silicon
# держит в кэше подпись старого файла и убивает новый процесс («killed»). Поэтому: удалить → положить → подписать.
put_binary() {
  rm -f "$BIN_DIR/radar"
  install -m 755 "$1" "$BIN_DIR/radar"
  if [ "$(uname -s)" = "Darwin" ]; then
    codesign --force --sign - "$BIN_DIR/radar" >/dev/null 2>&1 || true
  fi
}

SRC="${BASH_SOURCE[0]:-}"
HERE=""
[ -n "$SRC" ] && [ -f "$SRC" ] && HERE="$(cd "$(dirname "$SRC")" && pwd)"

if [ -n "$HERE" ] && [ -f "$HERE/Cargo.toml" ]; then
  # 1) Запуск из клонированного репозитория → сборка из исходников
  if ! command -v cargo >/dev/null 2>&1; then
    die "не найден Rust (cargo). Установите его и запустите ./install.sh ещё раз:
    brew install rust
  или
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && source ~/.cargo/env"
  fi
  say "Собираю из исходников (cargo build --release), первый раз ~1-2 минуты…"
  (cd "$HERE" && cargo build --release)
  put_binary "$HERE/target/release/radar"
else
  # 2) Готовый бинарник из релиза (curl | bash)
  case "$(uname -m)" in
    arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
    x86_64)        TARGET="x86_64-apple-darwin" ;;
    *) die "неподдерживаемая архитектура: $(uname -m)" ;;
  esac
  URL="https://github.com/$REPO/releases/latest/download/radar-$TARGET.tar.gz"
  say "Скачиваю $URL"
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  curl -fsSL "$URL" -o "$TMP/radar.tar.gz" || die "не удалось скачать релиз. Проверьте, что в $REPO есть опубликованный релиз."
  tar -xzf "$TMP/radar.tar.gz" -C "$TMP"
  put_binary "$TMP/radar"
fi

# Файл, скачанный браузером, помечается карантином macOS; через curl — нет, но на всякий случай.
xattr -d com.apple.quarantine "$BIN_DIR/radar" 2>/dev/null || true

say "Установлено: $BIN_DIR/radar"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    say "Папки $BIN_DIR нет в PATH. Добавьте в ~/.zshrc:"
    echo "    export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac
echo
echo "Запуск:     radar"
echo "Проверка:   radar doctor   (покажет, какие агенты установлены)"
