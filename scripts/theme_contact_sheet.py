#!/usr/bin/env python3
"""把主题截图拼成两张联络表，便于一次性视觉检查。"""
from PIL import Image
import os

SHOTS = "/tmp/shots"

def sheet(names, out, cols=4, w=320, h=200):
    imgs = []
    for n in names:
        p = os.path.join(SHOTS, n + ".png")
        im = Image.open(p).convert("RGB")
        im.thumbnail((w, h))
        imgs.append(im)
    rows = (len(imgs) + cols - 1) // cols
    pad = 6
    W = cols * (w + pad) + pad
    H = rows * (h + pad) + pad
    canvas = Image.new("RGB", (W, H), (24, 24, 24))
    for i, im in enumerate(imgs):
        x = pad + (i % cols) * (w + pad)
        y = pad + (i // cols) * (h + pad)
        canvas.paste(im, (x, y))
    canvas.save(os.path.join(SHOTS, out), quality=88)
    print(out, canvas.size)

sheet(["glass-home", "liquid", "aurora", "bento", "m3", "fluent",
       "command", "swiss", "editorial", "luxury", "wabi", "ink"], "sheet1.png")
sheet(["clay", "neu", "brutal", "y2k", "terminal", "cyber",
       "vaporwave", "oled", "pixel"], "sheet2.png")
