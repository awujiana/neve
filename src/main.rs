//! Névé —— PCL2 式 MPV 内核播放器管理器
//! 纯 Rust + GPUI 原生渲染
//!
//! 当前：启动时执行 core::players 播放器检测 + core::config 配置冲突诊断（FR-11），
//! 并在窗口中列出结果。
//! TODO：检测与冲突扫描移入后台线程，避免阻塞启动（GPUI 支持 spawn/背景任务）。

mod core;

use core::config::conf::ConfDoc;
use core::config::conflicts::{detect_input_conflicts, detect_value_conflicts};
use core::config::input::InputDoc;
use core::players::{DetectedPlayer, detect_players};

use gpui::{
    App, Application, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
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
                let portable_label = if p.is_portable() { " (便携版)" } else { "" };
                div()
                    .py_1p5()
                    .text_sm()
                    .text_color(rgb(FG))
                    .child(format!("▸ {}{}", p.name, portable_label))
                    .child(div().text_xs().text_color(rgb(FG_DIM)).child(format!(
                        "   {:?} | {} | {}",
                        p.kind,
                        version,
                        p.exe_path.display()
                    )))
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
        eprintln!(
            "[detect] {} ({:?}) -> {}",
            p.name,
            p.kind,
            p.exe_path.display()
        );
    }
    log_config_diagnostics(&players);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(860.0), px(560.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| PlayersPage { players }),
        ) {
            eprintln!("无法打开 Névé 主窗口：{error}");
        }
    });
}

/// 启动诊断：扫描各播放器配置目录的 input.conf / mpv.conf，
/// 输出按键冲突与配置值冲突（FR-11 核心链路的最小接入）。
/// TODO: 结果改为结构化返回，由 UI 冲突面板展示。
fn log_config_diagnostics(players: &[DetectedPlayer]) {
    for player in players {
        let Some(config_dir) = &player.config_dir else {
            continue;
        };

        let input_path = config_dir.join("input.conf");
        if let Ok(text) = std::fs::read_to_string(&input_path) {
            let doc = InputDoc::parse(&text);
            if doc.render() != text {
                eprintln!(
                    "[config] 警告：input.conf 渲染与原文存在差异（{}）",
                    input_path.display()
                );
            }
            for conflict in detect_input_conflicts(&[("input.conf", &doc)]) {
                let effective = &conflict.bindings[conflict.effective_index];
                eprintln!(
                    "[config] 按键冲突 {}：{} 处绑定，生效命令 \"{}\"（行 {}）",
                    conflict.key,
                    conflict.bindings.len(),
                    effective.command,
                    effective.line_no
                );
            }
        }

        let mpv_conf_path = config_dir.join("mpv.conf");
        if let Ok(text) = std::fs::read_to_string(&mpv_conf_path) {
            let doc = ConfDoc::parse(&text);
            if doc.render() != text {
                eprintln!(
                    "[config] 警告：mpv.conf 渲染与原文存在差异（{}）",
                    mpv_conf_path.display()
                );
            }
            for conflict in detect_value_conflicts(&[("mpv.conf", &doc)]) {
                eprintln!(
                    "[config] 配置冲突 {}：{} 处定义，生效值 \"{}\"",
                    conflict.key,
                    conflict.values.len(),
                    conflict.effective
                );
            }
        }
    }
}
