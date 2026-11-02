//! Névé —— PCL2 式 MPV 内核播放器管理器
//! 纯 Rust + GPUI 原生渲染，组件层使用 gpui-component（经 GPUI Kit 统一入口）
//!
//! 启动流程：检测播放器 → 构建面板视图模型 → 初始化组件库 → 投影设计令牌 →
//! 打开窗口。
//! TODO：检测与扫描移入后台线程，避免阻塞启动（GPUI 支持 spawn/背景任务）。

mod core;
mod ui;

use core::players::detect_players;

use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{
    App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, application, init,
    open_window, px, size,
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

    // 启动时默认深色；界面可在顶栏切换，后续接入 settings.json 的 theme_mode。
    let initial_mode = ThemeMode::Dark;
    let dark = initial_mode.is_dark();

    application().run(move |cx: &mut App| {
        // 组件库初始化必须在任何组件使用之前
        init(cx);
        // 把 config/ui-theme.toml 的设计令牌投影到语义色槽位
        ui::apply_theme(cx, initial_mode);

        let bounds = Bounds::centered(None, size(px(1180.0), px(800.0)), cx);
        // 隐藏系统标题栏，改由界面自绘，避免深色应用顶部出现白色系统条。
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(880.0), px(560.0))),
            titlebar: Some(TitlebarOptions {
                title: Some("Névé — MPV 管理器".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            app_owns_titlebar_drag: true,
            ..Default::default()
        };
        // open_window 会自动挂载 Root（组件库主题/弹层宿主）
        if let Err(error) = open_window(options, cx, move |_window, cx| {
            cx.new(|_| ui::AppShell::new(players, panels, dark))
        }) {
            eprintln!("无法打开 Névé 主窗口：{error}");
        }
    });
}
