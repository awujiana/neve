//! Névé —— PCL2 式 MPV 内核播放器管理器
//! 纯 Rust + GPUI 原生渲染，组件层使用 gpui-component（经 GPUI Kit 统一入口）
//!
//! 启动流程：检测播放器 → 构建面板视图模型 → 初始化组件库 → 打开窗口。
//! TODO：检测与扫描移入后台线程，避免阻塞启动（GPUI 支持 spawn/背景任务）。

mod core;
mod ui;

use core::players::detect_players;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{
    App, AppContext as _, Bounds, WindowBounds, WindowOptions, application, init, open_window, px,
    size,
};

fn main() {
    // TODO: 移入后台线程；当前同步执行（目录扫描深度受限，通常 < 1s）
    let players = detect_players();
    for p in &players {
        eprintln!(
            "[detect] {} ({:?}) -> {}",
            p.name,
            p.kind,
            p.exe_path.display()
        );
    }
    let panels = ui::build_panels(&players);

    application().run(move |cx: &mut App| {
        // 组件库初始化必须在任何组件使用之前
        init(cx);
        Theme::global_mut(cx).mode = ThemeMode::Dark;

        let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        // open_window 会自动挂载 Root（组件库主题/弹层宿主）
        if let Err(error) = open_window(options, cx, move |_window, cx| {
            cx.new(|_| ui::PlayersPage::new(players, panels))
        }) {
            eprintln!("无法打开 Névé 主窗口：{error}");
        }
    });
}
