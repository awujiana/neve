"""改变「背景不透明度」后抓 PrintWindow，用亮度变化验证底色层真的带 alpha。

GDI 屏幕截图抓不到 DirectComposition 窗口，所以改用 PrintWindow：它渲染窗口
自身的内容（不含桌面）。若底色层带 alpha，PrintWindow 会把窗口内容合成到黑底上，
于是「背景不透明度」越低，整幅图就越暗；若某一层仍是实色，亮度就不会随滑杆变化。

用法：python scripts/probe_opacity.py <neve.exe> [不透明度取值...]
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image

TITLE_KEYWORD = "MPV 管理器"
CONF = Path.home() / "AppData" / "Roaming" / "neve" / "appearance.conf"

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

PW_RENDERFULLCONTENT = 0x00000002
DIB_RGB_COLORS = 0
BI_RGB = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD),
        ("biWidth", wt.LONG),
        ("biHeight", wt.LONG),
        ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", wt.LONG),
        ("biYPelsPerMeter", wt.LONG),
        ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


def find_neve() -> int | None:
    found: list[int] = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def callback(hwnd, _lparam):
        length = user32.GetWindowTextLengthW(hwnd)
        if length:
            buffer = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, buffer, length + 1)
            if TITLE_KEYWORD in buffer.value and user32.IsWindowVisible(hwnd):
                found.append(hwnd)
        return True

    user32.EnumWindows(callback, 0)
    return found[0] if found else None


def capture(hwnd: int) -> Image.Image:
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    hdc = user32.GetDC(hwnd)
    mem = gdi32.CreateCompatibleDC(hdc)
    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = -height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = BI_RGB
    bits = ctypes.c_void_p()
    bitmap = gdi32.CreateDIBSection(
        hdc, ctypes.byref(info), DIB_RGB_COLORS, ctypes.byref(bits), None, 0
    )
    gdi32.SelectObject(mem, bitmap)
    user32.PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT)
    buffer = ctypes.string_at(bits, width * height * 4)
    image = Image.frombuffer("RGBA", (width, height), buffer, "raw", "BGRA", 0, 1).copy()
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(hwnd, hdc)
    return image


def set_opacity(value: str) -> None:
    text = CONF.read_text(encoding="utf-8")
    lines = [
        f"background_opacity = {value}" if line.startswith("background_opacity") else line
        for line in text.splitlines()
    ]
    CONF.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    if len(sys.argv) < 2:
        print("用法: python scripts/probe_opacity.py <neve.exe> [取值...]")
        return 1
    exe = sys.argv[1]
    values = sys.argv[2:] or ["0", "45", "100"]
    original = CONF.read_text(encoding="utf-8")

    try:
        for value in values:
            set_opacity(value)
            app = subprocess.Popen([exe])
            time.sleep(4.0)
            hwnd = find_neve()
            if hwnd is None:
                print(f"不透明度 {value}: 未找到窗口")
                app.terminate()
                continue
            image = capture(hwnd).convert("RGB")
            grey = image.convert("L")
            pixels = list(grey.getdata())
            mean = sum(pixels) / len(pixels)
            print(f"不透明度 {value:>3}: 平均亮度 {mean:6.2f}")
            app.terminate()
            time.sleep(1.0)
    finally:
        CONF.write_text(original, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
