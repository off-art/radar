#!/usr/bin/env python3
"""Генерирует логотип Radar (вариант C: вордмарк `radar_`) в виде SVG с контурами вместо шрифта.

Запуск:  python3 assets/brand/build.py        (нужен fonttools: pip install fonttools)
Шрифт:   DejaVu Sans Mono Bold (свободная лицензия, допускает встраивание контуров).
Результат: SVG рядом со скриптом. PNG и .icns собираются отдельно (render.py, make-icons.sh).
"""
import os
import sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

HERE = os.path.dirname(os.path.abspath(__file__))
FONT_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/Library/Fonts/DejaVuSansMono-Bold.ttf",
    os.path.expanduser("~/Library/Fonts/DejaVuSansMono-Bold.ttf"),
]
font_path = next((p for p in FONT_CANDIDATES if os.path.exists(p)), None)
if not font_path:
    sys.exit("Не найден DejaVuSansMono-Bold.ttf: установите шрифт или поправьте FONT_CANDIDATES")

font = TTFont(font_path)
gs = font.getGlyphSet()
cmap = font.getBestCmap()
ADV = gs[cmap[ord("r")]].width  # моноширинный шрифт: ширина ячейки одинакова

ORANGE_DARK_BG = "#f59e0b"
ORANGE_LIGHT_BG = "#d97706"
INK_ON_DARK = "#e6e9ef"
INK_ON_LIGHT = "#1b1e24"


def outline(text, x0=0.0, y0=0.0, scale=1.0):
    """Контур строки как один path (ось Y перевёрнута под SVG)."""
    d = []
    x = x0
    for ch in text:
        pen = SVGPathPen(gs)
        tp = TransformPen(pen, (scale, 0, 0, -scale, x, y0))
        gs[cmap[ord(ch)]].draw(tp)
        d.append(pen.getCommands())
        x += ADV * scale
    return " ".join(d)


def rect_path(x, y, w, h, r):
    """Скруглённый прямоугольник как path (чтобы весь знак был одним типом примитивов)."""
    return (f"M{x + r:.1f} {y:.1f}H{x + w - r:.1f}A{r:.1f} {r:.1f} 0 0 1 {x + w:.1f} {y + r:.1f}"
            f"V{y + h - r:.1f}A{r:.1f} {r:.1f} 0 0 1 {x + w - r:.1f} {y + h:.1f}"
            f"H{x + r:.1f}A{r:.1f} {r:.1f} 0 0 1 {x:.1f} {y + h - r:.1f}"
            f"V{y + r:.1f}A{r:.1f} {r:.1f} 0 0 1 {x + r:.1f} {y:.1f}Z")


# ---------- Вордмарк ----------
WORD = "radar"
CUR_GAP, CUR_W, CUR_H, CUR_DROP = 90, 980, 230, 0     # курсор: отступ, ширина, толщина, сдвиг ниже базовой линии
PAD_X, TOP, BOT = 140, 1640, 330                      # поля и вертикальные границы


def wordmark(ink, cursor, name):
    text_d = outline(WORD)
    cx = ADV * len(WORD) + CUR_GAP
    cursor_d = rect_path(cx, CUR_DROP, CUR_W, CUR_H, 50)
    w = cx + CUR_W + 2 * PAD_X
    h = TOP + BOT
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{-PAD_X} {-TOP} {w:.0f} {h}" '
           f'role="img" aria-label="radar_">\n'
           f'  <title>radar_</title>\n'
           f'  <path fill="{ink}" d="{text_d}"/>\n'
           f'  <path fill="{cursor}" d="{cursor_d}"/>\n</svg>\n')
    open(os.path.join(HERE, name), "w", encoding="utf-8").write(svg)


wordmark(INK_ON_DARK, ORANGE_DARK_BG, "radar-wordmark-on-dark.svg")      # для тёмного фона
wordmark(INK_ON_LIGHT, ORANGE_LIGHT_BG, "radar-wordmark-on-light.svg")   # для светлого фона
wordmark("#000000", "#000000", "radar-wordmark-mono-black.svg")          # одноцветный
wordmark("#ffffff", "#ffffff", "radar-wordmark-mono-white.svg")


# ---------- Знак: монограмма r_ ----------
from fontTools.pens.boundsPen import BoundsPen

_bp = BoundsPen(gs)
gs[cmap[ord("r")]].draw(_bp)
R_XMIN, R_YMIN, R_XMAX, R_YMAX = _bp.bounds       # реальные границы «r» в единицах шрифта


def mark(name, size=1024, margin=0, radius_ratio=0.225, fill=0.58, bg_top="#171d27", bg_bot="#0b0e13",
         ink=INK_ON_DARK, cursor=ORANGE_DARK_BG, with_bg=True):
    """Квадратный знак. margin — прозрачное поле (для иконки macOS 100/1024); fill — доля ширины плашки под r_."""
    box = size - 2 * margin
    r = box * radius_ratio
    r_w = R_XMAX - R_XMIN
    gap = 0.16 * r_w                                  # зазор между «r» и курсором
    cur_w = 0.95 * r_w                                # курсор чуть короче «r» по ширине
    cur_h = 0.24 * r_w                                # толстый, чтобы жил в 16 px
    group_w = r_w + gap + cur_w
    s = box * fill / group_w
    # вертикаль: «r» от базовой линии вверх на R_YMAX, курсор уходит под базовую линию на cur_h
    group_h = R_YMAX * s + cur_h * s * 0.65
    x_left = margin + (box - group_w * s) / 2
    top = margin + (box - group_h) / 2
    base_y = top + R_YMAX * s
    r_d = outline("r", x_left - R_XMIN * s, base_y, s)
    cx = x_left + (r_w + gap) * s
    cur_d = rect_path(cx, base_y - cur_h * s * 0.35, cur_w * s, cur_h * s, cur_h * s * 0.22)
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" role="img" aria-label="radar_">',
             '  <title>radar_</title>']
    if with_bg:
        parts.append(f'  <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">'
                     f'<stop offset="0" stop-color="{bg_top}"/><stop offset="1" stop-color="{bg_bot}"/></linearGradient></defs>')
        parts.append(f'  <path fill="url(#g)" d="{rect_path(margin, margin, box, box, r)}"/>')
    parts.append(f'  <path fill="{ink}" d="{r_d}"/>')
    parts.append(f'  <path fill="{cursor}" d="{cur_d}"/>')
    parts.append('</svg>\n')
    open(os.path.join(HERE, name), "w", encoding="utf-8").write("\n".join(parts))


mark("radar-icon.svg", size=1024, margin=100, fill=0.56)          # иконка приложения Radar.app (поле как в шаблоне macOS)
mark("radar-favicon.svg", size=64, margin=0, radius_ratio=0.22, fill=0.70)   # favicon / аватар: без поля
# копии для сайта: Pages отдаёт только каталог docs/
import shutil
SITE = os.path.join(HERE, "..", "..", "docs", "brand")
if os.path.isdir(os.path.dirname(SITE)):
    os.makedirs(SITE, exist_ok=True)
    for f in ("radar-wordmark-on-dark.svg", "radar-wordmark-on-light.svg", "radar-favicon.svg"):
        shutil.copy(os.path.join(HERE, f), SITE)
print("готово:", ", ".join(sorted(f for f in os.listdir(HERE) if f.endswith(".svg"))))
