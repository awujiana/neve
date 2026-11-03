"""在 Neve 后面铺几块高饱和纯色窗口，抓屏验证窗口是否真的透出后方内容。

为什么不用 mpv 全屏当背景：mpv 全屏会盖在最上层，Neve 整个被挡住，
抓到的就只有 mpv 的画面。这里用 ctypes 自建几个普通 Win32 窗口（非 topmost），
Neve 置前后就压在它们上面，GDI 抓屏能同时看到两者。

用法：python scripts/probe_transparency.py <neve.exe>
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import threading
import time

from PIL import Image, ImageGrab

TITLE_KEYWORD = "MPV 管理器"
WS_POPUP = 0x80000000
WS_VISIBLE = 0x10000000
WM_QUIT = 0x0012

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
kernel32 = ctypes.windll.kernel32

WNDPROC = ctypes.WINFUNCTYPE(
    ctypes.c_long, ctypes.c_void_p, wt.UINT, ctypes.c_void_p, ctypes.c_void_p
)
DEF_WINDOW_PROC = ctypes.cast(user32.DefWindowProcW, WNDPROC)


class WNDCLASSW(ctypes.Structure):
    _fields_ = [
        ("style", wt.UINT),
        ("lpfnWndProc", WNDPROC),
        ("cbClsExtra", ctypes.c_int),
        ("cbWndExtra", ctypes.c_int),
        ("hInstance", ctypes.c_void_p),
        ("hIcon", ctypes.c_void_p),
        ("hCursor", ctypes.c_void_p),
        ("hbrBackground", ctypes.c_void_p),
        ("lpszMenuName", wt.LPCWSTR),
        ("lpszClassName", wt.LPCWSTR),
    ]


class MSG(ctypes.Structure):
    _fields_ = [
        ("hwnd", wt.HWND),
        ("message", wt.UINT),
        ("wParam", wt.WPARAM),
        ("lParam", wt.LPARAM),
        ("time", wt.DWORD),
        ("pt", wt.POINT),
    ]


def rgb(value: tuple[int, int, int]) -> int:
    r, g, b = value
    return r | (g << 8) | (b << 16)


def make_backdrop_window(index: int, color: tuple[int, int, int], width: int) -> None:
    """创建一个铺满一块屏幕区域的纯色窗口。"""
    class_name = f"NeveProbe{index}"
    gdi32.CreateSolidBrush.restype = ctypes.c_void_p
    kernel32.GetModuleHandleW.restype = ctypes.c_void_p
    user32.CreateWindowExW.restype = ctypes.c_void_p
    user32.CreateWindowExW.argtypes = [
        wt.DWORD,
        wt.LPCWSTR,
        wt.LPCWSTR,
        wt.DWORD,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
    ]
    brush = gdi32.CreateSolidBrush(rgb(color))
    wc = WNDCLASSW()
    wc.lpfnWndProc = DEF_WINDOW_PROC
    wc.hInstance = kernel32.GetModuleHandleW(None)
    wc.hbrBackground = brush
    wc.lpszClassName = class_name
    user32.RegisterClassW(ctypes.byref(wc))
    hwnd = user32.CreateWindowExW(
        0,
        class_name,
        f"probe-{index}",
        WS_POPUP | WS_VISIBLE,
        index * width,
        0,
        width,
        1080,
        None,
        None,
        wc.hInstance,
        None,
    )
    user32.ShowWindow(hwnd, 5)  # SW_SHOW
    user32.UpdateWindow(hwnd)


def pump() -> None:
    """背景窗口靠类刷子绘制，但必须有人派发 WM_PAINT。"""
    message = MSG()
    while user32.GetMessageW(ctypes.byref(message), None, 0, 0) > 0:
        user32.TranslateMessage(ctypes.byref(message))
        user32.DispatchMessageW(ctypes.byref(message))


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


def focus(hwnd: int) -> None:
    user32.ShowWindow(hwnd, 9)
    user32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002)
    user32.SwitchToThisWindow(hwnd, True)
    foreground = user32.GetForegroundWindow()
    foreground_thread = user32.GetWindowThreadProcessId(foreground, None)
    current_thread = kernel32.GetCurrentThreadId()
    user32.AttachThreadInput(current_thread, foreground_thread, True)
    user32.SetForegroundWindow(hwnd)
    user32.AttachThreadInput(current_thread, foreground_thread, False)
    time.sleep(0.6)


def client_rect(hwnd: int) -> tuple[int, int, int, int]:
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    return (
        origin.x,
        origin.y,
        origin.x + rect.right - rect.left,
        origin.y + rect.bottom - rect.top,
    )


def stats(image: Image.Image, label: str) -> None:
    small = image.convert("RGB").resize((200, 140))
    colored = 0
    total = 0
    saturation = 0.0
    for r, g, b in small.getdata():
        total += 1
        high = max(r, g, b)
        low = min(r, g, b)
        value = 0 if high == 0 else (high - low) / high
        saturation += value
        if value > 0.22 and high > 45:
            colored += 1
    print(f"{label}: 彩色像素 {colored * 100 // total}%  平均饱和度 {saturation / total:.2f}")


def main() -> int:
    if len(sys.argv) < 2:
        print("用法: python scripts/probe_transparency.py <neve.exe>")
        return 1

    for index, color in enumerate([(230, 30, 40), (30, 200, 90), (40, 120, 240), (240, 200, 30)]):
        make_backdrop_window(index, color, 490)
    threading.Thread(target=pump, daemon=True).start()
    time.sleep(1.0)

    app = subprocess.Popen([sys.argv[1]])
    time.sleep(4.0)
    hwnd = find_neve()
    if hwnd is None:
        print("未找到 Neve 窗口")
        app.terminate()
        return 1
    focus(hwnd)
    time.sleep(0.8)

    box = client_rect(hwnd)
    shot = ImageGrab.grab(bbox=box)
    shot.save("docs/screenshots/probe-glass.png")
    stats(shot, "Neve 客户区")

    user32.ShowWindow(hwnd, 6)  # SW_MINIMIZE
    time.sleep(1.0)
    bare = ImageGrab.grab(bbox=box)
    bare.save("docs/screenshots/probe-backdrop.png")
    stats(bare, "同一区域（无 Neve）")

    app.terminate()
    print("已保存 docs/screenshots/probe-glass.png / probe-backdrop.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
