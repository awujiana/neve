"""量测截图里文字与底色的实际对比度，用来代替「看着有点淡」的主观判断。

用法（先抓图）：

    python scripts/probe_contrast.py docs/screenshots/overview-glass.png 230,55,760,100

坐标可给多组；不给坐标时按内置的几个默认区域量（概览页的常见文字位置）。

判定：正文按 WCAG AA ≥ 4.5:1，大字/次要文字 ≥ 3:1。
"""

from __future__ import annotations

import collections
import sys

from PIL import Image


def lin(c: float) -> float:
    c /= 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(p: tuple[int, int, int]) -> float:
    r, g, b = p
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)


def contrast(a: tuple[int, int, int], b: tuple[int, int, int]) -> float:
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def measure(im: Image.Image, box: tuple[int, int, int, int], name: str) -> None:
    crop = im.crop(box)
    counts = collections.Counter(crop.getdata())
    bg = counts.most_common(1)[0][0]
    # 文字笔画的核心像素：出现次数够多才当作字形，避免把单个噪点当文字。
    glyphs = [p for p, c in counts.items() if c >= 3]
    if not glyphs:
        print(f"{name}: 区域内没有找到文字")
        return
    txt = min(glyphs, key=luminance)
    ratio = contrast(bg, txt)
    flag = "OK " if ratio >= 4.5 else ("弱 " if ratio >= 3.0 else "差 ")
    print(f"{flag}{name}: bg={bg} txt={txt} ratio={ratio:.2f}")


DEFAULT_BOXES: list[tuple[str, tuple[int, int, int, int]]] = [
    ("页头标题+说明", (230, 55, 780, 100)),
    ("侧栏菜单项", (0, 120, 200, 400)),
    ("卡片辅助文字", (230, 180, 700, 330)),
    ("底部状态栏", (0, 760, 1180, 800)),
]


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    im = Image.open(sys.argv[1]).convert("RGB")
    print("尺寸", im.size)
    boxes = []
    for i, spec in enumerate(sys.argv[2:], 1):
        # 支持「名称:x1,y1,x2,y2」与裸「x1,y1,x2,y2」两种写法。
        name, _, raw = spec.rpartition(":")
        values = [int(v) for v in raw.split(",")]
        if len(values) != 4 or not raw:
            name, values = "", [int(v) for v in spec.split(",")]
        boxes.append((name or f"区域{i}", tuple(values)))  # type: ignore[arg-type]
    if not boxes:
        boxes = DEFAULT_BOXES
    for name, box in boxes:
        measure(im, box, name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
