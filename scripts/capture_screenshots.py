"""抓取 Neve 窗口客户区截图，用于更新 README 展示图。

依赖：`pillow`（`pip install pillow`）。仅 Windows 有效，靠 `user32` 枚举窗口。

用法（先手动启动 `cargo run`）：

    # 直接截当前界面
    python scripts/capture_screenshots.py overview-dark

    # 先点击 (x, y) 再截图，坐标是客户区坐标，可串联多步
    python scripts/capture_screenshots.py "players-dark:48,85" "players-light:955,12"

输出固定写到 `docs/screenshots/<名称>.png`。

坐标怎么定：截图后按像素量一下按钮中心，或用脚本里 `scan_row` 的思路
扫一行像素找控件边界。客户区原点在标题栏左上角。
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import sys
import time

from PIL import ImageGrab

# 窗口标题包含该关键字即认为是 Neve 主窗口（见 main.rs 的 WindowOptions.title）。
TITLE_KEYWORD = "MPV 管理器"
OUTPUT_DIR = "docs/screenshots"

user32 = ctypes.windll.user32


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", wt.LONG),
        ("top", wt.LONG),
        ("right", wt.LONG),
        ("bottom", wt.LONG),
    ]


def find_window() -> tuple[int | None, str | None]:
    """返回第一个标题匹配且可见的顶层窗口。"""
    found: list[tuple[int, str]] = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def callback(hwnd, _lparam):
        length = user32.GetWindowTextLengthW(hwnd)
        if length:
            buffer = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, buffer, length + 1)
            if TITLE_KEYWORD in buffer.value and user32.IsWindowVisible(hwnd):
                found.append((hwnd, buffer.value))
        return True

    user32.EnumWindows(callback, 0)
    return found[0] if found else (None, None)


def client_rect(hwnd: int) -> tuple[int, int, int, int]:
    """客户区在屏幕上的 (left, top, right, bottom)。

    必须走 ClientToScreen：直接抓窗口矩形会把系统边框和桌面背景一起截进来。
    """
    rect = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    return (
        origin.x,
        origin.y,
        origin.x + rect.right - rect.left,
        origin.y + rect.bottom - rect.top,
    )


def focus(hwnd: int) -> None:
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.25)


def click(hwnd: int, x: int, y: int) -> None:
    origin_x, origin_y, _, _ = client_rect(hwnd)
    user32.SetCursorPos(origin_x + x, origin_y + y)
    time.sleep(0.08)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP
    # 点击后要等一帧渲染，否则截到的是点击前的画面。
    time.sleep(1.0)


def main() -> int:
    hwnd, title = find_window()
    if hwnd is None:
        print("未找到 Neve 窗口，请先启动应用（cargo run）")
        return 1
    print("窗口：", title)

    for spec in sys.argv[1:]:
        name, _, position = spec.partition(":")
        focus(hwnd)
        if position:
            x, y = (int(value) for value in position.split(","))
            click(hwnd, x, y)
        ImageGrab.grab(bbox=client_rect(hwnd)).save(f"{OUTPUT_DIR}/{name}.png")
        print("已保存", name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
