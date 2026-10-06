#!/usr/bin/env python3
"""Генерирует звуки уведомлений Radar (без внешних зависимостей).

Для каждой темы — пара: <тема>-done.wav («готово», восходящее/разрешающее) и
<тема>-waiting.wav («нужен ваш ответ», настойчивее). Темы намеренно разные по тембру.
Запуск: python3 assets/gen_sounds.py
"""
import math, struct, wave, os, random

SR = 22050
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sounds")
TAU = 2 * math.pi
random.seed(7)


def mix_into(mix, samples, start):
    off = int(SR * start)
    for i, v in enumerate(samples):
        if off + i < len(mix):
            mix[off + i] += v


def lowpass(x, a):
    y, out = 0.0, []
    for v in x:
        y += a * (v - y)
        out.append(y)
    return out


def sine(f, dur, decay, attack=0.004, harm=()):
    out = []
    for i in range(int(SR * dur)):
        t = i / SR
        env = min(1, t / attack) * math.exp(-t / decay)
        v = math.sin(TAU * f * t)
        for k, (mult, amp, dk) in enumerate(harm):
            v += amp * math.sin(TAU * f * mult * t) * math.exp(-t / dk)
        out.append(env * v)
    return out


def bell(f):
    return sine(f, 0.9, 0.17, 0.008, [(2, .3, .09), (3, .1, .05)])


def drop(f):
    out, ph = [], 0.0
    for i in range(int(SR * 0.5)):
        t = i / SR
        ph += TAU * f * (1 + 1.6 * math.exp(-t / 0.035)) / SR
        out.append(min(1, t / 0.002) * math.exp(-t / 0.09) * math.sin(ph))
    return out


def square(f, dur, duty=0.25):
    out = []
    for i in range(int(SR * dur)):
        t = i / SR
        env = min(1, t / 0.003) * math.exp(-t / (dur * 0.7))
        out.append(env * (1.0 if (f * t) % 1.0 < duty else -1.0))
    return lowpass(out, 0.45)


def harp(f):
    return sine(f, 0.8, 0.22, 0.002, [(2, .35, .12), (3, .15, .06), (4, .06, .04)])


def knock(f):
    out = []
    for i in range(int(SR * 0.12)):
        t = i / SR
        env = min(1, t / 0.0008) * math.exp(-t / 0.018)
        out.append(env * (math.sin(TAU * f * t) + 0.5 * math.sin(TAU * f * 2.3 * t) + 0.25 * random.uniform(-1, 1)))
    return lowpass(out, 0.5)


def thump(f):
    out, ph = [], 0.0
    for i in range(int(SR * 0.5)):
        t = i / SR
        ph += TAU * (f * 0.45 + f * 0.55 * math.exp(-t / 0.04)) / SR
        out.append(min(1, t / 0.002) * math.exp(-t / 0.13) * math.sin(ph))
    return out


def swoosh(f0, f1, dur=0.7):
    """Шумовой «вздох»: полосовой фильтр плавно меняет частоту, громкость нарастает и спадает."""
    out, low, band = [], 0.0, 0.0
    n = int(SR * dur)
    for i in range(n):
        p = i / n
        f = f0 + (f1 - f0) * p
        a = 2 * math.sin(math.pi * min(f, SR / 6) / SR)
        x = random.uniform(-1, 1)
        low += a * band
        high = x - low - 0.35 * band
        band += a * high
        env = math.sin(math.pi * p) ** 1.5
        out.append(env * band)
    return out


def ping(f, echoes=((0.42, .38), (0.84, .16))):
    base = sine(f, 0.5, 0.16, 0.006, [(2, .12, .1)])
    mix = [0.0] * int(SR * (0.5 + echoes[-1][0]))
    mix_into(mix, base, 0)
    for t, a in echoes:
        mix_into(mix, [a * v for v in lowpass(base, 0.35)], t)
    return mix


def render(parts, total, peak=0.5):
    n = int(SR * total)
    mix = [0.0] * n
    for start, samples, amp in parts:
        mix_into(mix, [amp * v for v in samples], start)
    m = max(abs(x) for x in mix) or 1.0
    fade = int(SR * 0.06)
    x = [v / m for v in mix]
    rms = math.sqrt(sum(v * v for v in x) / n) or 1.0
    gain = min(0.095 / rms, 0.7)  # одинаковая «громкость на слух», без клиппинга
    return [v * gain * ((n - i) / fade if i > n - fade else 1) for i, v in enumerate(x)]


C5, E5, G5, A5, B5, C6, D6, E6 = 523.25, 659.25, 783.99, 880.0, 987.77, 1046.5, 1174.66, 1318.5

THEMES = {
    # светлый колокольчик
    "bell": (
        render([(0, bell(G5), .85), (.15, bell(D6), 1)], 1.1),
        render([(0, bell(B5), .9), (.20, bell(G5), 1)], 1.1),
    ),
    # сонар: одинокий пинг с эхом (в тему «Радара»)
    "sonar": (
        render([(0, ping(1500), 1)], 1.4),
        render([(0, ping(900, ((0.38, .4), (0.76, .2))), 1), (0.32, ping(900, ((0.38, .3), (0.76, .12))), .8)], 1.5),
    ),
    # ретро-приставка: квадратные арпеджио
    "retro": (
        render([(i * 0.07, square(f, 0.11), 1) for i, f in enumerate([C5, E5, G5, C6, E6])], 0.6),
        render([(i * 0.11, square(f, 0.13), 1) for i, f in enumerate([G5, D6, G5, D6, G5, D6])], 0.85),
    ),
    # арфа: быстрое глиссандо вверх / три ноты вниз
    "harp": (
        render([(i * 0.06, harp(f), 1) for i, f in enumerate([C5, E5, G5, C6, E6, G5 * 2])], 1.3),
        render([(i * 0.17, harp(f), 1) for i, f in enumerate([G5 * 1.0 * 1.5, C6, G5])], 1.3),
    ),
    # деревянный стук: тук-тук / тук-тук-тук
    "knock": (
        render([(0, knock(700), 1), (0.13, knock(1050), 1)], 0.5),
        render([(0, knock(650), 1), (0.16, knock(650), 1), (0.32, knock(650), 1)], 0.65),
    ),
    # низкий мягкий удар: один / «сердцебиение» дважды
    "thump": (
        render([(0, thump(110), 1), (0.18, thump(165), .7)], 0.8),
        render([(0, thump(95), 1), (0.24, thump(95), .85), (0.62, thump(95), 1), (0.86, thump(95), .85)], 1.3),
    ),
    # капля воды
    "drop": (
        render([(0, drop(700), .8), (.16, drop(1000), 1)], 0.8),
        render([(0, drop(900), .9), (.20, drop(600), 1)], 0.8),
    ),
}


def save(name, samples):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, name)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, x)) * 32767)) for x in samples))
    print(name, f"{len(samples)/SR:.2f}s {os.path.getsize(path)//1024}K")


for name, (done, waiting) in THEMES.items():
    save(f"{name}-done.wav", done)
    save(f"{name}-waiting.wav", waiting)
