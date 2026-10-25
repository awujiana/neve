//! Névé —— PCL2 式 MPV 播放器与脚本管理器
//! 纯 Rust + GPUI 原生渲染
//!
//! 当前为骨架：验证 GPUI 窗口能正常启动。
//! 后续模块规划见 README.md。

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Render, SharedString,
    Window, WindowBounds, WindowOptions,
};

struct HelloNeve {
    text: SharedString,
}

impl Render for HelloNeve {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(0x14181d)) // 深色底：冰山夜色
            .justify_center()
            .items_center()
            .text_xl()
            .text_color(rgb(0xd8e4ec))
            .child(format!("Névé — {}", &self.text))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(520.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| HelloNeve {
                    text: "冰山之源".into(),
                })
            },
        )
        .unwrap();
    });
}
