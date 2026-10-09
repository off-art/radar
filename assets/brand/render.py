#!/usr/bin/env python3
"""Растеризует логотип Radar и раскладывает результаты по репозиторию. Нужны: playwright (chromium) и Pillow.

    pip install playwright pillow && playwright install chromium
    python3 assets/brand/build.py && python3 assets/brand/render.py

Результат:
  assets/icon.svg, assets/icon.png (1024), assets/icon.icns   — иконка Radar.app (встраивается в бинарник: src/notify.rs)
  docs/icon.png (256), docs/social-preview.png (1280×640)     — README и GitHub
  docs/brand/favicon-180.png                                  — сайт (GitHub Pages отдаёт только docs/)
"""
import os
import shutil
import struct
from playwright.sync_api import sync_playwright
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.normpath(os.path.join(HERE, ".."))
DOCS = os.path.normpath(os.path.join(HERE, "..", "..", "docs"))
SITE = os.path.join(DOCS, "brand")
os.makedirs(SITE, exist_ok=True)

SOCIAL_HTML = """<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;width:1280px;height:640px;background:#0e1116;color:#e6e9ef;
 font-family:-apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif;overflow:hidden}
.c{width:1280px;height:640px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:28px;
 background:radial-gradient(900px 420px at 50% 38%,#1a2230 0%,#0e1116 70%)}
img{height:150px}
p{margin:0;font-size:38px;color:#aab3c2;letter-spacing:-.01em}
.a{display:flex;gap:14px;margin-top:10px}
.a span{font:600 22px ui-monospace,"SF Mono",Menlo,Consolas,monospace;color:#8b94a5;border:1px solid #2a3240;
 padding:8px 16px;border-radius:999px}
</style><div class="c"><img src="radar-wordmark-on-dark.svg">
<p>Many AI agents in one terminal window.</p>
<div class="a"><span>Claude Code</span><span>Codex</span><span>OpenCode</span><span>Qwen Code</span><span>GigaCode</span><span>Gemini CLI</span></div></div>"""

# типы блоков ICNS с PNG-данными: (тип, размер пикселей)
ICNS_TYPES = [("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic10", 1024),
              ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512)]


def write_icns(path, pngs):
    """pngs: {размер: bytes PNG}. Формат: 'icns' + длина, затем блоки (тип, длина, данные)."""
    body = b""
    for t, px in ICNS_TYPES:
        data = pngs[px]
        body += t.encode() + struct.pack(">I", 8 + len(data)) + data
    open(path, "wb").write(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def svg_png_bytes(page, svg_path, size):
    page.set_viewport_size({"width": size, "height": size})
    page.set_content(f'<body style="margin:0;background:transparent"><img src="file://{svg_path}" '
                     f'width="{size}" height="{size}" style="display:block"></body>')
    page.wait_for_timeout(120)
    return page.screenshot(omit_background=True)


with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page()
    icon_svg = os.path.join(HERE, "radar-icon.svg")
    favicon_svg = os.path.join(HERE, "radar-favicon.svg")

    # иконка: каждый размер рисуем из SVG заново (чётче, чем сжимать 1024)
    pngs = {px: svg_png_bytes(pg, icon_svg, px) for px in sorted({px for _, px in ICNS_TYPES})}
    open(os.path.join(ASSETS, "icon.png"), "wb").write(pngs[1024])
    open(os.path.join(DOCS, "icon.png"), "wb").write(svg_png_bytes(pg, icon_svg, 256))
    shutil.copy(icon_svg, os.path.join(ASSETS, "icon.svg"))
    write_icns(os.path.join(ASSETS, "icon.icns"), pngs)

    open(os.path.join(SITE, "favicon-180.png"), "wb").write(svg_png_bytes(pg, favicon_svg, 180))

    # social preview 1280×640 (страница лежит рядом с SVG, чтобы подтянулся вордмарк)
    tmp = os.path.join(HERE, "_social.html")
    open(tmp, "w", encoding="utf-8").write(SOCIAL_HTML)
    pg.set_viewport_size({"width": 1280, "height": 640})
    pg.goto("file://" + tmp)
    pg.wait_for_timeout(200)
    pg.screenshot(path=os.path.join(DOCS, "social-preview.png"))
    os.remove(tmp)
    b.close()

# проверка: Pillow читает собранный .icns
im = Image.open(os.path.join(ASSETS, "icon.icns"))
print("icns ok:", im.format, im.size, "блоков:", len(ICNS_TYPES))
print("готово")
