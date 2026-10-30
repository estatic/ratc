#!/usr/bin/env python3
"""Writes crates/atc-xplane-plugin/assets/follow_me_roof.png, the texture of the follow-me
car's roof object (follow_me_roof.obj). Python standard library only.

Layout (256 x 128 RGBA, image row 0 at the top; OBJ8 t = 1 - row / 128):
  rows   0..63              the sign face: "FOLLOW ME" in black on yellow between yellow/black
                            checker bands (t 0.5..1)
  rows  64..127, x   0..63  a round light sprite, white with a soft alpha edge (LIGHT_CUSTOM)
                 x  64..127 plain yellow (the sign's ends and top)
                 x 128..191 plain black
                 x 192..255 amber (the beacon housing)

Usage: python3 -I tools/make-follow-me-texture.py
"""

import os
import struct
import zlib

W, H = 256, 128
YELLOW = (250, 200, 20, 255)
BLACK = (15, 15, 15, 255)
AMBER = (255, 140, 0, 255)

# 5 x 7 glyphs, one string per row, '#' set.
FONT = {
    "F": ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
    "O": [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
    "L": ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
    "W": ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"],
    "M": ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
    "E": ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
    " ": ["....."] * 7,
}
TEXT = "FOLLOW ME"
SCALE = 4
CHECK = 8  # checker square, pixels


def make():
    px = [[YELLOW] * W for _ in range(H)]

    # Sign face: checker bands at the top and bottom of rows 0..63.
    for y in list(range(0, CHECK)) + list(range(64 - CHECK, 64)):
        for x in range(W):
            if ((x // CHECK) + (y // CHECK)) % 2:
                px[y][x] = BLACK
    # The text, centred in the yellow band between them.
    adv = (5 + 1) * SCALE
    text_w = len(TEXT) * adv - SCALE
    x0 = (W - text_w) // 2
    y0 = (64 - 7 * SCALE) // 2
    for i, ch in enumerate(TEXT):
        for gy, row in enumerate(FONT[ch]):
            for gx, bit in enumerate(row):
                if bit != "#":
                    continue
                for dy in range(SCALE):
                    for dx in range(SCALE):
                        px[y0 + gy * SCALE + dy][x0 + i * adv + gx * SCALE + dx] = BLACK

    # Bottom half: light sprite, then flat colours.
    for y in range(64, 128):
        for x in range(64):
            d = (((x - 31.5) ** 2 + (y - 95.5) ** 2) ** 0.5) / 32.0
            a = max(0.0, 1.0 - d) ** 1.5
            px[y][x] = (255, 255, 255, int(round(255 * a)))
        for x in range(64, 128):
            px[y][x] = YELLOW
        for x in range(128, 192):
            px[y][x] = BLACK
        for x in range(192, 256):
            px[y][x] = AMBER
    return px


def png(px):
    raw = b"".join(b"\x00" + b"".join(struct.pack("4B", *p) for p in row) for row in px)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    ihdr = struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = os.path.join(root, "crates", "atc-xplane-plugin", "assets", "follow_me_roof.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "wb") as f:
        f.write(png(make()))
    print("wrote", out)


if __name__ == "__main__":
    main()
