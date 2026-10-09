#!/usr/bin/env python3
"""Записывает демо-GIF интерфейса Radar без внешних видеорекордеров: tmux + рендер кадров в Pillow + ffmpeg.

Запускается настоящий Radar (релизный бинарник), агенты — имитация docs/demo/agent.sh (они правят файлы в демо-репозиториях,
так что ветки, счётчики +/−, diff и статусы в Radar настоящие). Подробности — docs/demo/README.md.

    cargo build --release
    python3 docs/demo/record.py                # → docs/demo.gif
    python3 docs/demo/record.py --frames-only  # только PNG-кадры в рабочей папке (для отладки)

Нужны: tmux, ffmpeg, Pillow, fontTools (для проверки глифов), шрифты DejaVu Sans Mono / DejaVu Sans.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
AGENT_SH = os.path.join(ROOT, "docs", "demo", "agent.sh")
COLS, ROWS = 116, 32
CW, CH = 9, 19                      # размер ячейки в пикселях
PAD = 14
FPS = 8
FONT_DIRS = ["/usr/share/fonts/truetype/dejavu", "/Library/Fonts", os.path.expanduser("~/Library/Fonts")]

BG = (14, 17, 22)
FG = (205, 214, 244)
ANSI16 = [(69, 71, 90), (243, 139, 168), (166, 227, 161), (249, 226, 175), (137, 180, 250), (245, 194, 231), (148, 226, 213), (186, 194, 222),
          (88, 91, 112), (243, 139, 168), (166, 227, 161), (249, 226, 175), (137, 180, 250), (245, 194, 231), (148, 226, 213), (166, 173, 200)]


def sh(*a, **kw):
    return subprocess.run(a, check=kw.pop("check", True), capture_output=True, text=True, **kw)


# ---------- окружение ----------
def make_repo(path, branch, files, remote_ahead=False):
    os.makedirs(path, exist_ok=True)
    env = {**os.environ, "GIT_AUTHOR_NAME": "demo", "GIT_AUTHOR_EMAIL": "demo@example.com",
           "GIT_COMMITTER_NAME": "demo", "GIT_COMMITTER_EMAIL": "demo@example.com"}
    sh("git", "init", "-q", "-b", "main", cwd=path)
    for rel, text in files.items():
        os.makedirs(os.path.dirname(os.path.join(path, rel)) or path, exist_ok=True)
        open(os.path.join(path, rel), "w", encoding="utf-8").write(text)
    sh("git", "add", "-A", cwd=path)
    sh("git", "commit", "-q", "-m", "init", cwd=path, env=env)
    sh("git", "checkout", "-q", "-b", branch, cwd=path)


def setup(work, radar_bin):
    home = os.path.join(work, "home")
    cfgdir = os.path.join(home, ".config", "radar")
    os.makedirs(cfgdir, exist_ok=True)
    proj = os.path.join(work, "proj")
    make_repo(os.path.join(proj, "api"), "feat/health", {
        "src/server.ts": "import express from 'express';\nexport const app = express();\n// TODO: health\napp.get('/', (_req, res) => res.send('ok'));\n",
        "package.json": '{ "name": "api" }\n'})
    make_repo(os.path.join(proj, "web"), "feat/login", {
        "src/Login.tsx": "import { useState } from 'react';\nexport function Login() {\n  const [error, setError] = useState<string | null>(null);\n  return <form />;\n}\n",
        "package.json": '{ "name": "web" }\n'})
    make_repo(os.path.join(proj, "docs"), "docs/readme", {"README.md": "# Docs\n\nПроект документации.\n"})
    open(os.path.join(cfgdir, "config.toml"), "w", encoding="utf-8").write(f'''notifications = false
sound = false
popups = false
restore = false
sidebar_width = 34

[theme]
name = "catppuccin"

[[agent]]
name = "Demo"
command = "bash"
args = ["{AGENT_SH}"]
kind = "generic"
color = "#d97757"
approve = "enter"
''')
    env = {**os.environ, "HOME": home, "XDG_CONFIG_HOME": os.path.join(home, ".config"), "TERM": "xterm-256color",
           "COLORTERM": "truecolor", "RADAR_DEMO_BIN": radar_bin, "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "SHELL": "/bin/bash"}
    env.pop("RADAR_SOCK", None)
    return home, proj, env


# ---------- tmux ----------
class Tmux:
    def __init__(self, env, work):
        self.env = env
        self.conf = os.path.join(work, "tmux.conf")
        open(self.conf, "w").write("set -g prefix None\nunbind C-b\nset -g status off\nset -g escape-time 0\n"
                                   "set -g default-terminal tmux-256color\nset -ga terminal-features ',*:RGB'\n")
        self.sock = os.path.join(work, "tmux.sock")

    def run(self, *a, check=True):
        return subprocess.run(["tmux", "-S", self.sock, "-f", self.conf, *a], env=self.env, check=check, capture_output=True, text=True)

    def start(self, cmd):
        self.run("new-session", "-d", "-s", "demo", "-x", str(COLS), "-y", str(ROWS), cmd)

    def keys(self, *k):
        self.run("send-keys", "-t", "demo", *k)

    def capture(self):
        return self.run("capture-pane", "-t", "demo", "-p", "-e", "-N").stdout

    def kill(self):
        self.run("kill-server", check=False)


# ---------- ANSI → сетка ячеек ----------
def color256(n):
    if n < 16:
        return ANSI16[n]
    if n < 232:
        n -= 16
        lv = [0, 95, 135, 175, 215, 255]
        return (lv[n // 36], lv[(n // 6) % 6], lv[n % 6])
    g = 8 + (n - 232) * 10
    return (g, g, g)


SGR_RE = re.compile(r"\x1b\[([0-9;:]*)m")


def parse_line(line):
    cells, fg, bg, bold, dim, rev, ul = [], None, None, False, False, False, False
    i = 0
    while i < len(line):
        m = SGR_RE.match(line, i)
        if m:
            p = [int(x) if x else 0 for x in re.split(r"[;:]", m.group(1))] or [0]
            j = 0
            while j < len(p):
                c = p[j]
                if c == 0: fg = bg = None; bold = dim = rev = ul = False
                elif c == 1: bold = True
                elif c == 2: dim = True
                elif c == 22: bold = dim = False
                elif c == 4: ul = True
                elif c == 24: ul = False
                elif c == 7: rev = True
                elif c == 27: rev = False
                elif 30 <= c <= 37: fg = ANSI16[c - 30]
                elif 90 <= c <= 97: fg = ANSI16[c - 90 + 8]
                elif c == 39: fg = None
                elif 40 <= c <= 47: bg = ANSI16[c - 40]
                elif 100 <= c <= 107: bg = ANSI16[c - 100 + 8]
                elif c == 49: bg = None
                elif c in (38, 48):
                    if j + 1 < len(p) and p[j + 1] == 5 and j + 2 < len(p):
                        col = color256(p[j + 2]); j += 2
                    elif j + 1 < len(p) and p[j + 1] == 2 and j + 4 < len(p):
                        col = (p[j + 2], p[j + 3], p[j + 4]); j += 4
                    else:
                        col = None
                    if col is not None:
                        if c == 38: fg = col
                        else: bg = col
                j += 1
            i = m.end()
            continue
        if line[i] == "\x1b":          # прочие управляющие последовательности пропускаем
            m2 = re.match(r"\x1b\[[0-9;?]*[A-Za-ln-z]", line[i:])
            i += m2.end() if m2 else 1
            continue
        cells.append((line[i], fg, bg, bold, dim, rev, ul))
        i += 1
    return cells


class Fonts:
    def __init__(self):
        def find(name):
            for d in FONT_DIRS:
                p = os.path.join(d, name)
                if os.path.exists(p):
                    return p
            raise SystemExit(f"нет шрифта {name}")
        size = 15
        self.regular = [find("DejaVuSansMono.ttf"), find("DejaVuSans.ttf")]
        self.bold = [find("DejaVuSansMono-Bold.ttf"), find("DejaVuSans-Bold.ttf")]
        self.cm = {p: set(TTFont(p).getBestCmap()) for p in set(self.regular + self.bold)}
        self.cache = {}
        self.size = size

    def get(self, ch, bold):
        paths = self.bold if bold else self.regular
        for p in paths:
            if ord(ch) in self.cm[p]:
                break
        else:
            p = paths[0]
        key = (p, self.size)
        if key not in self.cache:
            self.cache[key] = ImageFont.truetype(p, self.size)
        return self.cache[key]


def render(text, fonts, scale=1):
    lines = text.split("\n")[:ROWS]
    W, H = COLS * CW + 2 * PAD, ROWS * CH + 2 * PAD + 30
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    # «окно»: заголовок с тремя точками
    for k, c in enumerate([(255, 95, 87), (254, 188, 46), (40, 200, 64)]):
        d.ellipse((PAD + k * 20, 11, PAD + k * 20 + 11, 22), fill=c)
    d.text((W // 2 - 20, 8), "radar", fill=(139, 148, 165), font=fonts.get("r", False))
    oy = 30 + PAD
    for y in range(ROWS):
        cells = parse_line(lines[y]) if y < len(lines) else []
        for x, (ch, fg, bg, bold, dim, rev, ul) in enumerate(cells[:COLS]):
            f, b = fg or FG, bg or BG
            if rev:
                f, b = (bg or BG), (fg or FG)
            if dim:
                f = tuple(int(f[i] * 0.62 + b[i] * 0.38) for i in range(3))
            px, py = PAD + x * CW, oy + y * CH
            if b != BG:
                d.rectangle((px, py, px + CW - 1, py + CH - 1), fill=b)
            if ch != " ":
                d.text((px, py + 1), ch, fill=f, font=fonts.get(ch, bold))
            if ul:
                d.line((px, py + CH - 2, px + CW - 1, py + CH - 2), fill=f)
    return img


# ---------- сценарий ----------
def timeline(radar_bin, proj, env, tm):
    """(t, действие). Действие — callable. Время в секундах от старта записи."""
    ctl = lambda *a: subprocess.run([radar_bin, "ctl", *a], env=env, capture_output=True, text=True)
    new = lambda name: ctl("new", "demo", os.path.join(proj, name), "--name", name)
    send = lambda name, text: ctl("send", name, text)

    # режим навигации после действия: `w`, `g` оставляют его включённым, остальные — выключают
    # (экран для проверки не годится: тосты закрывают полосу НАВИГАЦИЯ)
    stay = {"w", "g"}
    state = {"on": False}

    def nav(key):
        if not state["on"]:
            tm.keys("C-b")
            time.sleep(0.15)
        tm.keys(key)
        state["on"] = key in stay

    def leave_nav():
        if state["on"]:
            tm.keys("Escape")
            state["on"] = False

    return [
        (1.2, lambda: new("api")),
        (1.8, lambda: new("web")),
        (2.4, lambda: new("docs")),
        (4.0, lambda: send("api", "Добавь эндпоинт /health и тест")),
        (4.4, lambda: send("web", "Добавь состояние загрузки в форму входа")),
        (4.8, lambda: send("docs", "Обнови README")),
        (10.5, lambda: nav("w")),                 # к агенту, который ждёт ответа
        (11.8, lambda: nav("y")),                 # диалог разрешения запроса
        (14.2, lambda: tm.keys("y")),             # подтверждаем
        (17.5, lambda: nav("v")),                 # изменения агента (git diff)
        (20.5, lambda: tm.keys("j")),
        (21.8, lambda: tm.keys("Escape")),        # закрыть diff
        (22.4, lambda: nav("g")),                 # сетка
        (25.6, lambda: nav("g")),                 # обратно к одному агенту
        (26.3, lambda: nav("l")),                 # лента событий
        (29.3, lambda: tm.keys("Escape")),
        (30.0, leave_nav),
    ]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--radar", default=os.path.join(ROOT, "target", "release", "radar"))
    ap.add_argument("--out", default=os.path.join(ROOT, "docs", "demo.gif"))
    ap.add_argument("--work", default=None, help="рабочая папка (по умолчанию временная)")
    ap.add_argument("--duration", type=float, default=31.0)
    ap.add_argument("--frames-only", action="store_true")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    for tool in ("tmux", "ffmpeg"):
        if not shutil.which(tool):
            sys.exit(f"нужен {tool}")
    if not os.path.exists(a.radar):
        sys.exit(f"нет бинарника {a.radar}: cargo build --release")
    work = a.work or tempfile.mkdtemp(prefix="radar-demo-")
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    home, proj, env = setup(work, a.radar)
    tm = Tmux(env, work)
    frames = os.path.join(work, "frames")
    os.makedirs(frames)
    fonts = Fonts()
    events = timeline(a.radar, proj, env, tm)
    try:
        tm.start(f"{a.radar}")
        time.sleep(0.8)
        t0 = time.monotonic()
        n, ei, raw = 0, 0, []
        while True:
            t = time.monotonic() - t0
            if t > a.duration:
                break
            while ei < len(events) and events[ei][0] <= t:
                events[ei][1]()
                ei += 1
            raw.append(tm.capture())
            n += 1
            nxt = t0 + n / FPS
            time.sleep(max(0.0, nxt - time.monotonic()))
        for i, txt in enumerate(raw):
            render(txt, fonts).save(os.path.join(frames, f"f{i:04d}.png"))
    finally:
        subprocess.run([a.radar, "stop"], env=env, capture_output=True)
        tm.kill()
    print(f"кадров: {len(raw)} (≈{len(raw) / FPS:.1f} с), папка {frames}")
    if a.frames_only:
        return
    pal = os.path.join(work, "pal.png")
    inp = ["-framerate", str(FPS), "-i", os.path.join(frames, "f%04d.png")]
    sh("ffmpeg", "-y", "-loglevel", "error", *inp, "-vf", "palettegen=max_colors=96:stats_mode=diff", pal)
    sh("ffmpeg", "-y", "-loglevel", "error", *inp, "-i", pal, "-lavfi",
       "paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle", "-loop", "0", a.out)
    print(f"готово: {a.out} ({os.path.getsize(a.out) / 1e6:.2f} МБ)")
    if not a.keep and not a.work:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
