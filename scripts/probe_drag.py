"""实测三件事：自绘标题栏能不能拖动窗口、设置页滑杆能不能手动拖、精度是不是 1%。

为什么要有这个脚本：这三条都是「看起来该能用」但极容易悄悄坏掉的路径——
窗口拖动要靠 WM_NCHITTEST 命中测试把拖动区报成 HTCAPTION，滑杆要拿到渲染后的
bounds 才能把指针位置换算成值。三者都只能靠真实指针事件验证。

用法（会自己拉起 Neve，跑完自动关掉）：

    python scripts/probe_drag.py

输出：窗口拖动前后的窗口矩形、三次滑杆拖动后 `appearance.conf` 的取值。
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageGrab

TITLE_KEYWORD = "MPV 管理器"
LEFTDOWN = 0x0002
LEFTUP = 0x0004
# 侧栏「设置」的行位置（客户区坐标，会随导航项增减变化，必要时先截图量一次）。
SETTINGS_ROW = (60, 287)
CONF = Path.home() / "AppData/Roaming/neve/appearance.conf"

user32 = ctypes.windll.user32


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
    kernel32 = ctypes.windll.kernel32
    foreground_thread = user32.GetWindowThreadProcessId(user32.GetForegroundWindow(), None)
    current_thread = kernel32.GetCurrentThreadId()
    user32.AttachThreadInput(current_thread, foreground_thread, True)
    user32.SetForegroundWindow(hwnd)
    user32.AttachThreadInput(current_thread, foreground_thread, False)
    user32.SetWindowPos(hwnd, -2, 0, 0, 0, 0, 0x0001 | 0x0002)
    time.sleep(0.5)
    # 焦点没抢下来时后面所有鼠标事件都会打在别的窗口上，直接暴露出来。
    if user32.GetForegroundWindow() != hwnd:
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.3)
        print(f"   [focus] 前台窗口 {user32.GetForegroundWindow()}, 期望 {hwnd}")


def focus_until(hwnd: int, tries: int = 5) -> None:
    """启动头几秒前台窗口常被终端/编辑器抢走，抢到为止。"""
    for attempt in range(tries):
        focus(hwnd)
        if user32.GetForegroundWindow() == hwnd:
            return
        time.sleep(0.6)
    print(f"   [focus] 重试 {tries} 次仍未抢到前台，鼠标事件可能丢失")


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


def window_rect(hwnd: int) -> tuple[int, int, int, int]:
    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    return (rect.left, rect.top, rect.right, rect.bottom)


def drag(origin_x: int, origin_y: int, x: int, y: int, dx: int, dy: int) -> None:
    """在客户区 (x, y) 按下，分 12 步移到 (x+dx, y+dy) 再松开。"""
    # 先微抖一次：标题栏拖动依赖 WM_NCHITTEST 命中测试，而 gpui 的命中测试
    # 结果只在上一次 WM_MOUSEMOVE 里更新。光标若本来就在原地，不会重新派发。
    user32.SetCursorPos(origin_x + x + 2, origin_y + y + 2)
    time.sleep(0.08)
    user32.SetCursorPos(origin_x + x, origin_y + y)
    time.sleep(0.15)
    user32.mouse_event(LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.1)
    for step in range(1, 13):
        user32.SetCursorPos(
            origin_x + x + dx * step // 12, origin_y + y + dy * step // 12
        )
        time.sleep(0.03)
    time.sleep(0.1)
    user32.mouse_event(LEFTUP, 0, 0, 0, 0)
    time.sleep(0.4)


def find_tracks(image: Image.Image) -> list[tuple[int, int, int]]:
    """找滑杆轨道：宽度落在 150..220px 的水平实色带（TRACK 常量是 180）。

    背景基准取整行亮度的**中位数**而不是众数：渐变背景的浅色像素有好几种
    近似色，众数可能只有一百来票，反而被 180px 宽的轨道色反客为主，
    结果把「轨道」当成背景、把整片背景当成轨道。中位数不受这种拆分影响。
    """
    rgb = image.convert("RGB")
    width, height = rgb.size
    hits: list[tuple[int, int, int]] = []
    for y in range(40, height - 20):
        lumas = [sum(rgb.getpixel((x, y))) / 3.0 for x in range(width)]
        baseline = sorted(lumas)[width // 2]
        start: int | None = None
        for x, luma in enumerate(lumas):
            on_track = abs(luma - baseline) >= 12.0
            if on_track:
                if start is None:
                    start = x
            elif start is not None:
                # 滑杆在右侧内容区，且右侧要留出 padding，排除贴边的假阳性
                # （右边界贴到 width-41 的通常是文字/按钮背景，不是轨道）。
                if 150 <= x - start <= 220 and start > 500 and x < width - 60:
                    hits.append((y, start, x))
                start = None
    # 同一条轨道会连续出现多行（轨道高约 6px），按 y 聚成一段取中行；
    # 单行命中多半是文字或图标，直接丢掉。
    tracks: list[tuple[int, int, int]] = []
    group: list[tuple[int, int, int]] = []
    for hit in hits:
        if group and hit[0] - group[-1][0] > 3:
            if len(group) >= 3:
                tracks.append(group[len(group) // 2])
            group = []
        group.append(hit)
    if len(group) >= 3:
        tracks.append(group[len(group) // 2])
    return tracks


def read_conf() -> dict[str, int]:
    if not CONF.exists():
        return {}
    values: dict[str, int] = {}
    for line in CONF.read_text(encoding="utf-8").splitlines():
        key, _, tail = line.partition("=")
        key, tail = key.strip(), tail.strip()
        if key in ("background_opacity", "lightness") and tail.isdigit():
            values[key] = int(tail)
    return values


def knob_x(track: tuple[int, int, int], percent: float) -> int:
    """把百分比换算出屏幕上的像素横坐标（轨道宽 180px ⇒ 1% = 1.8px）。"""
    _y, left, right = track
    return left + round((right - left) * percent / 100.0)


def drag_between(
    origin_x: int, origin_y: int, track: tuple[int, int, int], start: float, end: float
) -> None:
    """按住滑块从 start% 拖到 end%。"""
    y = track[0]
    drag(origin_x, origin_y, knob_x(track, start), y, knob_x(track, end) - knob_x(track, start), 0)


def main() -> int:
    # 先把外观写成已知值，这样滑块起始位置是确定的，才能算出按下点。
    CONF.parent.mkdir(parents=True, exist_ok=True)
    CONF.write_text("background_opacity = 35\nlightness = 86\n", encoding="utf-8")

    app = subprocess.Popen([sys.argv[1] if len(sys.argv) > 1 else "target/debug/neve.exe"])
    try:
        time.sleep(8.0)
        hwnd = find_neve()
        if hwnd is None:
            print("未找到 Neve 窗口")
            return 1
        focus_until(hwnd)

        # 1) 窗口拖动：标题栏中间的拖动区（图标之后、右侧控件之前）。
        # 先摆到确定位置，避免上一轮的残留位置干扰位移判定。
        user32.SetWindowPos(hwnd, 0, 200, 150, 0, 0, 0x0001 | 0x0002)
        time.sleep(0.4)
        origin_x, origin_y, _, _ = client_rect(hwnd)
        before = window_rect(hwnd)
        drag(origin_x, origin_y, 700, 18, 120, 90)
        after = window_rect(hwnd)
        moved = (after[0] - before[0], after[1] - before[1])
        print(f"1) 窗口拖动: {before} -> {after}  位移 {moved}")
        print("   ", "OK" if moved == (120, 90) else "失败：窗口没有按要求移动")

        # 2) 切到设置页，自动定位两条滑杆轨道。
        focus_until(hwnd)
        origin_x, origin_y, _, _ = client_rect(hwnd)
        user32.SetCursorPos(origin_x + SETTINGS_ROW[0], origin_y + SETTINGS_ROW[1])
        time.sleep(0.08)
        user32.mouse_event(LEFTDOWN, 0, 0, 0, 0)
        user32.mouse_event(LEFTUP, 0, 0, 0, 0)
        time.sleep(1.2)

        box = client_rect(hwnd)
        shot = ImageGrab.grab(bbox=box)
        shot.save("_probe-sliders.png")
        tracks = find_tracks(shot)
        print(f"2) 找到 {len(tracks)} 条轨道: {tracks}")
        if len(tracks) < 2:
            print("   滑杆定位失败，先看 _probe-sliders.png")
            return 1
        # 页面顺序：先「背景不透明度」后「明暗」。
        opacity_track, lightness_track = tracks[0], tracks[1]

        failures = 0

        # 3) 手动拖到 50%：35% -> 50%
        focus_until(hwnd)
        origin_x, origin_y, _, _ = client_rect(hwnd)
        drag_between(origin_x, origin_y, opacity_track, 35, 50)
        time.sleep(0.8)
        got = read_conf().get("background_opacity")
        ok = got == 50
        failures += 0 if ok else 1
        print(f"3) 拖到 50%: background_opacity={got}  {'OK' if ok else '失败'}")

        # 4) 1% 精度：拖到 57%（不是 5 的倍数）。若 step 被改成 5 或 10，
        #    结果一定落在 55/60 这类整档上；能落在 56..59 才说明是逐 1% 的。
        #    允许 ±1px 的像素取整误差，因此判据是「非 5 倍数且接近 57」。
        focus_until(hwnd)
        origin_x, origin_y, _, _ = client_rect(hwnd)
        drag_between(origin_x, origin_y, opacity_track, 50, 57)
        time.sleep(0.8)
        got = read_conf().get("background_opacity")
        ok = got is not None and 55 < got < 60 and got % 5 != 0
        failures += 0 if ok else 1
        print(f"4) 1% 精度 (拖到 57%): background_opacity={got}  {'OK' if ok else '失败'}")

        # 5) 第二条轨道（明暗）也要能拖：86% -> 40%
        focus_until(hwnd)
        origin_x, origin_y, _, _ = client_rect(hwnd)
        drag_between(origin_x, origin_y, lightness_track, 86, 40)
        time.sleep(0.8)
        got = read_conf().get("lightness")
        ok = got == 40
        failures += 0 if ok else 1
        print(f"5) 明暗拖到 40%: lightness={got}  {'OK' if ok else '失败'}")

        print()
        print("全部通过" if failures == 0 else f"{failures} 项失败")
        return 0 if failures == 0 else 1
    finally:
        app.terminate()


if __name__ == "__main__":
    raise SystemExit(main())
