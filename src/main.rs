//! Névé —— PCL2 式 MPV 内核播放器管理器
//! 纯 Rust + GPUI 原生渲染
//!
//! 当前：启动时执行 core::players 播放器检测，并在窗口中列出结果。
//! TODO：检测移入后台线程，避免阻塞启动（GPUI 支持 spawn/背景任务）。

mod core;

use core::players::{detect_players, DetectedPlayer};

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Render, Window,
    WindowBounds, WindowOptions,
};

const BG: u32 = 0x14181d;
const FG: u32 = 0xd8e4ec;
const FG_DIM: u32 = 0x7a8a99;
const ACCENT: u32 = 0x6db3f2;

struct PlayersPage {
    players: Vec<DetectedPlayer>,
}

impl Render for PlayersPage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let items: Vec<_> = self
            .players
            .iter()
            .map(|p| {
                let version = p.version.clone().unwrap_or_else(|| "版本未探测".into());
                let config = p
                    .config_dir
                    .as_ref()
                    .map(|c| c.display().to_string())
                    .unwrap_or_else(|| "未找到配置目录".into());
                div()
                    .py_1p5()
                    .text_sm()
                    .text_color(rgb(FG))
                    .child(format!("▸ {}", p.name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(FG_DIM))
                            .child(format!("   {:?} | {} | {}", p.kind, version, p.exe_path.display())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(FG_DIM))
                            .child(format!("   配置目录：{}", config)),
                    )
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .p_4()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(ACCENT))
                    .child("Névé — 播放器检测"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(FG_DIM))
                    .child(format!("共检测到 {} 个 MPV 内核播放器", self.players.len())),
            )
            .children(items)
    }
}

fn main() {
    // TODO: 移入后台线程；当前骨架阶段同步执行（目录扫描深度受限，通常 < 1s）
    let players = detect_players();
    for p in &players {
        eprintln!("[detect] {} ({:?}) -> {}", p.name, p.kind, p.exe_path.display());
    }

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(860.0), px(560.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| PlayersPage { players }),
        )
        .unwrap();
    });
}
