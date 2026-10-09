# Запись демо-GIF

`docs/demo.gif` записан скриптом `record.py` на **настоящем Radar**, но с **имитированными агентами** (`agent.sh`):
они печатают «работу», правят файлы в git-репозитории, шлют те же события хуков, что и Claude Code, и один раз просят разрешение.
Так запись воспроизводима и не тратит токены. Версия с настоящими агентами — `../demo.tape` (нужен [VHS](https://github.com/charmbracelet/vhs)).

```sh
cargo build --release
pip install pillow          # нужны tmux, ffmpeg, шрифты DejaVu
python3 -I docs/demo/record.py --work /tmp/rd   # путь короткий: у unix-сокета лимит длины
```

Параметры: `--radar`, `--out`, `--duration`, `--frames-only`, `--keep`. Сценарий — функция `timeline()` в `record.py`.
