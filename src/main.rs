//! Névé —— PCL2 式 MPV 内核播放器管理器
//! 纯 Rust + GPUI 原生渲染，组件层使用 gpui-component（经 GPUI Kit 统一入口）
//!
//! 启动流程：读取外观设置 → 检测播放器 → 构建面板视图模型 → 初始化组件库 →
//! 投影外观令牌 → 打开窗口。
//! TODO：检测与扫描移入后台线程，避免阻塞启动（GPUI 支持 spawn/背景任务）。

mod core;
mod ui;

use core::appearance::Appearance;
use core::players::detect_players;

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
    // 外观设置持久化在数据目录，读不到就用默认值。
    let appearance = Appearance::load();
    let background = ui::window_background(appearance.material);

    application().run(move |cx: &mut App| {
        // 组件库初始化必须在任何组件使用之前
        init(cx);
        // 皮肤预览要把 uosc 的图标画成真字形，得先把它的 Material Icons 字体装进字体库。
        // 字体随播放器整合包放在 `fonts/` 下，取第一个能找到的即可。
        let icon_font = players
            .iter()
            .filter_map(|player| player.config_dir.as_deref())
            .find_map(core::skins::find_icon_font);
        match icon_font.as_deref() {
            Some(path) if ui::mpv_preview::load_icon_font(cx, path) => {
                eprintln!("[fonts] 已加载 uosc 图标字体：{}", path.display());
            }
            Some(path) => eprintln!("[fonts] 图标字体加载失败：{}", path.display()),
            None => {
                eprintln!("[fonts] 未找到 MaterialIconsRound-Regular.otf，皮肤预览将退回几何图标")
            }
        }
        // 把外观设置投影到语义色槽位（液态玻璃不区分明暗主题）
        ui::apply_theme(cx, appearance.clone());

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
            window_background: background,
            ..Default::default()
        };
        // open_window 会自动挂载 Root（组件库主题/弹层宿主）
        if let Err(error) = open_window(options, cx, move |_window, cx| {
            cx.new(|cx| ui::AppShell::new(players, panels, appearance, cx))
        }) {
            eprintln!("无法打开 Névé 主窗口：{error}");
        }
    });
}
