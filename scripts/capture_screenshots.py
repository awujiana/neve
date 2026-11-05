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

注意：窗口句柄会在 GPUI 初始化后期被重建，因此每一步都重新枚举（见 `live_window`）。
只查一次会让后续点击发给死句柄，表现为「点了没反应」，且会抓成整屏。

点击是否真的生效由脚本自己验证：点完比对前后两帧，画面没变就换一种注入方式重试
（见 `click`）。合成输入在 GPUI 上偶发丢失，靠肉眼核对会白抓一堆图。
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import sys
import time

from PIL import ImageChops, ImageGrab

# 窗口标题包含该关键字即认为是 Neve 主窗口（见 main.rs 的 WindowOptions.title）。
TITLE_KEYWORD = "MPV 管理器"
OUTPUT_DIR = "docs/screenshots"

# 判定「画面确实变了」的像素占比阈值。切页会整块重绘，0.2% 已经很宽松；
# 设小了会把光标移动造成的微小变化误判成切换成功。
CHANGE_THRESHOLD = 0.002

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


def live_window() -> tuple[int, str]:
    """重新枚举窗口，返回一个**当前有效**的句柄。

    GPUI 在 Windows 上会在初始化后期重建 HWND，进程不变但句柄会换。只在启动时
    查一次的话，后续所有点击都会发给已销毁的句柄——表现是「点了没反应」，而
    `client_rect` 对死句柄返回 0×0，`ImageGrab` 就退化成抓整屏（截到别的窗口）。
    因此每一步都重新枚举，并校验客户区确实有面积。
    """
    for _ in range(20):
        hwnd, title = find_window()
        if hwnd is not None:
            left, top, right, bottom = client_rect(hwnd)
            if right - left > 0 and bottom - top > 0:
                return hwnd, title or ""
        time.sleep(0.3)
    return 0, ""


def keep_on_top(hwnd: int) -> None:
    """整个截图会话期间保持窗口置顶。

    ImageGrab 抓的是屏幕像素，不是窗口内容：只要 Neve 被别的窗口盖住，
    前台抢没抢到都无所谓，截出来一定是别人的界面。置顶是最稳的兜底，
    会话结束（finally）再还原成普通 z 序。
    """
    user32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002)  # HWND_TOPMOST, NOSIZE|NOMOVE


def restore_z_order(hwnd: int) -> None:
    user32.SetWindowPos(hwnd, -2, 0, 0, 0, 0, 0x0001 | 0x0002)  # HWND_NOTOPMOST


def ensure_on_screen(hwnd: int) -> None:
    """把窗口挪回屏幕内，保证整个客户区都能被截到。

    `ImageGrab` 只能抓到屏幕上的像素：窗口一旦有一部分在屏幕外（实测客户区右边界
    跑到 2358，而屏幕只有 1920 宽），超出部分就是黑的，截出来右边一片纯黑。
    """
    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    screen_width = user32.GetSystemMetrics(0)
    screen_height = user32.GetSystemMetrics(1)
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    if width <= 0 or height <= 0:
        return
    x = min(max(0, rect.left), max(0, screen_width - width))
    y = min(max(0, rect.top), max(0, screen_height - height))
    if (x, y) != (rect.left, rect.top):
        # SWP_NOSIZE | SWP_NOZORDER：只挪位置，不动尺寸与层级。
        user32.SetWindowPos(hwnd, 0, x, y, 0, 0, 0x0001 | 0x0004)


def focus(hwnd: int) -> bool:
    """把窗口抢到前台，返回是否成功。

    单靠 `SetForegroundWindow` 在 Windows 上会被前台锁拒绝（调用方不是前台进程时）。
    三招叠加：先敲一次 ALT 让系统认为「最后一次输入来自用户」，再用
    `AttachThreadInput` 挂到前台窗口的输入队列，最后置顶兜底遮挡。
    """
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    keep_on_top(hwnd)
    ensure_on_screen(hwnd)

    kernel32 = ctypes.windll.kernel32
    for _ in range(3):
        # ALT 按下即松开：这是绕开前台锁的常用手法，不产生任何可见副作用。
        user32.keybd_event(0x12, 0, 0, 0)  # VK_MENU
        user32.keybd_event(0x12, 0, 0x0002, 0)  # KEYEVENTF_KEYUP

        foreground = user32.GetForegroundWindow()
        foreground_thread = user32.GetWindowThreadProcessId(foreground, None)
        current_thread = kernel32.GetCurrentThreadId()
        attached = bool(foreground_thread) and foreground_thread != current_thread
        if attached:
            user32.AttachThreadInput(current_thread, foreground_thread, True)
        user32.SetForegroundWindow(hwnd)
        user32.BringWindowToTop(hwnd)
        if attached:
            user32.AttachThreadInput(current_thread, foreground_thread, False)

        time.sleep(0.2)
        if user32.GetForegroundWindow() == hwnd:
            time.sleep(0.15)
            return True

    title = ctypes.create_unicode_buffer(256)
    user32.GetWindowTextW(user32.GetForegroundWindow(), title, 256)
    print(f"[警告] 窗口未能切到前台（当前前台：{title.value}），靠置顶兜底遮挡")
    return False


WM_MOUSEMOVE = 0x0200
WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP = 0x0202


def grab(hwnd: int):
    """抓客户区。句柄失效时 bbox 会退化成 0×0，直接报错而不是截整屏。"""
    bbox = client_rect(hwnd)
    if bbox[2] <= bbox[0] or bbox[3] <= bbox[1]:
        raise RuntimeError("客户区为空，窗口可能已销毁")
    return ImageGrab.grab(bbox=bbox)


def changed_ratio(before, after) -> float:
    """两帧之间亮度差超过 8 的像素占比。"""
    if before.size != after.size:
        return 1.0
    difference = ImageChops.difference(before.convert("L"), after.convert("L"))
    histogram = difference.histogram()
    total = before.size[0] * before.size[1]
    return sum(histogram[8:]) / total if total else 0.0


def press_by_message(hwnd: int, x: int, y: int) -> None:
    """投递式点击：直接给窗口塞鼠标消息。

    GPUI 的命中测试用的是它自己缓存的鼠标位置，只调 `SetCursorPos` 时窗口不一定
    收到移动消息，点击就会落在上一次的坐标上——表现为「反复点同一个旧按钮」。
    `PostMessage` 的 lParam 里带上目标坐标，命中测试才可靠。
    """
    origin_x, origin_y, _, _ = client_rect(hwnd)
    user32.SetCursorPos(origin_x + x, origin_y + y)
    time.sleep(0.15)

    position = (y << 16) | (x & 0xFFFF)
    user32.PostMessageW(hwnd, WM_MOUSEMOVE, 0, position)
    time.sleep(0.15)
    user32.PostMessageW(hwnd, WM_LBUTTONDOWN, 1, position)
    time.sleep(0.08)
    user32.PostMessageW(hwnd, WM_LBUTTONUP, 0, position)


def press_by_input(hwnd: int, x: int, y: int) -> None:
    """系统级注入：按绝对坐标移动光标再按下抬起。

    和 `press_by_message` 是两条不同的路径，一条失灵时另一条常常还能用，
    因此 `click` 会交替尝试。
    """
    origin_x, origin_y, _, _ = client_rect(hwnd)
    screen_w = user32.GetSystemMetrics(0)
    screen_h = user32.GetSystemMetrics(1)
    normalized_x = int((origin_x + x) * 65535 / max(1, screen_w - 1))
    normalized_y = int((origin_y + y) * 65535 / max(1, screen_h - 1))

    user32.mouse_event(0x8001, normalized_x, normalized_y, 0, 0)  # MOVE|ABSOLUTE
    time.sleep(0.2)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.08)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP


def click(hwnd: int, x: int, y: int, settle: float = 3.0, attempts: int = 3):
    """点击 (x, y)，并**确认画面真的变了**，返回点击后的那一帧。

    两个坑：

    1. **重绘有 1~3 秒延迟**。切页会重新扫描全部播放器的脚本 / 滤镜 / 配置，
       立刻抓屏拿到的是上一页。所以先等 `settle` 秒再比对。
    2. **合成输入偶发丢失**。丢一次就白抓一张图，而且很难事后发现。因此每轮
       点完都比对前后两帧，没变化就换另一种注入方式重试。

    重试只发生在「画面完全没变」时，所以对开关类控件是安全的——开关第一次点
    一定改变画面，不会触发第二轮把它点回去。
    """
    before = grab(hwnd)
    after = before
    strategies = (press_by_message, press_by_input)
    for attempt in range(attempts):
        strategies[attempt % len(strategies)](hwnd, x, y)
        time.sleep(settle)
        after = grab(hwnd)
        if changed_ratio(before, after) > CHANGE_THRESHOLD:
            return after
        print(f"  [重试] 点击 ({x}, {y}) 未改变画面，第 {attempt + 1} 次")
    print(f"  [警告] 点击 ({x}, {y}) 始终没有生效")
    return after


def main() -> int:
    hwnd, title = live_window()
    if not hwnd:
        print("未找到 Neve 窗口，请先启动应用（cargo run）")
        return 1
    print("窗口：", title)

    try:
        for spec in sys.argv[1:]:
            name, _, position = spec.partition(":")
            # 每一步都重新取句柄：窗口可能在两步之间被重建。
            hwnd, _ = live_window()
            if not hwnd:
                print(f"[跳过] {name}：窗口已消失")
                continue
            focus(hwnd)
            try:
                if position:
                    x, y = (int(value) for value in position.split(","))
                    # click 返回它自己验证过的那一帧，省一次抓屏也避免再漂一帧。
                    frame = click(hwnd, x, y)
                else:
                    frame = grab(hwnd)
            except RuntimeError as error:
                print(f"[跳过] {name}：{error}")
                continue
            frame.save(f"{OUTPUT_DIR}/{name}.png")
            print("已保存", name)
    finally:
        if hwnd:
            restore_z_order(hwnd)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
