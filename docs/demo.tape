# Сценарий записи демо-GIF для README. Инструмент: VHS (https://github.com/charmbracelet/vhs)
#   brew install vhs        # ставит также ttyd и ffmpeg
#   vhs docs/demo.tape      # создаст docs/demo.gif
#
# Подготовка (один раз): три демо-папки с git-репозиториями и включённые интеграции.
#   mkdir -p /tmp/demo/{api,web,docs} && for d in /tmp/demo/*; do git -C $d init -q; done
#   radar integration install qwen
# Режим навигации включается Ctrl+B и остаётся включённым до Escape,
# поэтому перед набором текста агенту в сценарии стоит Escape.
# Цель: GIF < 3 МБ (тема, размер и частота кадров подобраны под это).

Output docs/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 720
Set Padding 12
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms
Set Framerate 20

# --- Кадр 1: установка ---
Type "curl -fsSL https://raw.githubusercontent.com/off-art/radar/main/install.sh | bash"
Sleep 600ms
Enter
Sleep 3s
Hide
Type "clear"
Enter
Show

# --- Кадр 2: запуск с тремя агентами ---
Type "radar /tmp/demo/api claude /tmp/demo/web codex /tmp/demo/docs qwen"
Enter
Sleep 4s

# --- Кадр 3: задачи трём агентам (статусы «работает») ---
Ctrl+B
Type "1"
Escape
Type "Добавь эндпоинт /health и тест"
Enter
Sleep 1s
Ctrl+B
Type "2"
Escape
Type "Подготовь миграцию страницы входа"
Enter
Sleep 1s
Ctrl+B
Type "3"
Escape
Type "Обнови README"
Enter
Sleep 4s

# --- Кадр 4: один агент ждёт ответа — разрешаем из списка ---
Ctrl+B
Type "w"
Sleep 1s
Type "y"
Sleep 2s
Type "y"
Sleep 2s

# --- Кадр 5: просмотр изменений ---
Type "v"
Sleep 3s
Type "n"
Sleep 2s
Escape
Sleep 500ms

# --- Кадр 6: сетка; закрываем окно — агенты остаются в фоне, возвращаемся ---
Type "g"
Sleep 3s
Type "q"
Sleep 1s
Type "radar"
Enter
Sleep 3s
