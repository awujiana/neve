//! Névé —— PCL2 式 MPV 内核播放器管理器
//! 纯 Rust + GPUI 原生渲染
//!
//! 当前：启动时执行 core::players 播放器检测、core::config 冲突报告（FR-11）、
//! core::scripts uosc 状态（FR-09），并在窗口中分区展示。
//! TODO：检测与扫描移入后台线程，避免阻塞启动（GPUI 支持 spawn/背景任务）。

mod core;

use core::backup::BackupManager;
use core::config::report::{PlayerConfigReport, scan_player_configs};
use core::players::{DetectedPlayer, detect_players};
use core::scripts::list_scripts;
use core::scripts::uosc::{UoscOptionValue, UoscStatus, read_uosc_values, uosc_status};

use gpui::{
    AnyElement, App, Application, Bounds, Context, Render, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

// 深色主题基色（对齐 config/ui-theme.toml 的 dark 段）
const BG: u32 = 0x14181d;
const FG: u32 = 0xe6f0f6;
const FG_DIM: u32 = 0xa8bbc9;
const ACCENT: u32 = 0x83c5ea;
const SUCCESS: u32 = 0x82d2a8;
const WARNING: u32 = 0xf1c66f;

struct PlayersPage {
    players: Vec<DetectedPlayer>,
    reports: Vec<PlayerConfigReport>,
    uosc_panels: Vec<UoscPanel>,
}

/// uosc 面板的视图模型（UI 与 core 数据的解耦层）
struct UoscPanel {
    player_name: String,
    status: UoscStatus,
    script_count: usize,
    /// uosc.conf 可视化目录取值（仅已安装时存在）
    options: Option<Vec<UoscOptionValue>>,
    /// 读取 conf 失败时的警告
    options_warning: Option<String>,
    /// 可撤回的 uosc.conf 备份数量
    undo_available: usize,
}

impl PlayersPage {
    /// 冲突面板（FR-11）：按键冲突与配置值冲突
    fn render_conflicts(&self) -> AnyElement {
        let mut blocks: Vec<AnyElement> = Vec::new();
        for report in &self.reports {
            let mut block = div().flex().flex_col().gap_1();

            block = block.child(div().text_sm().text_color(rgb(FG)).child(format!(
                "▸ {}（{}）",
                report.player_name,
                report.config_dir.display()
            )));

            for warning in &report.warnings {
                block = block.child(
                    div()
                        .text_xs()
                        .text_color(rgb(WARNING))
                        .child(format!("   ⚠ {warning}")),
                );
            }

            for conflict in &report.input_conflicts {
                let effective = &conflict.bindings[conflict.effective_index];
                block = block.child(div().text_xs().text_color(rgb(WARNING)).child(format!(
                    "   ⇄ 按键 {}：{} 处绑定，生效 \"{}\"（{}，行 {}）",
                    conflict.key,
                    conflict.bindings.len(),
                    effective.command,
                    effective.source,
                    effective.line_no
                )));
                for binding in &conflict.bindings {
                    block = block.child(div().text_xs().text_color(rgb(FG_DIM)).child(format!(
                        "      [{}] 行 {}：{}",
                        binding.source, binding.line_no, binding.command
                    )));
                }
            }

            for conflict in &report.value_conflicts {
                let extra = if conflict.same_value {
                    "（值相同，重复定义）"
                } else {
                    ""
                };
                block = block.child(div().text_xs().text_color(rgb(WARNING)).child(format!(
                    "   ⇄ 配置 {}：{} 处定义，生效 \"{}\"{extra}",
                    conflict.key,
                    conflict.values.len(),
                    conflict.effective
                )));
                for value in &conflict.values {
                    block = block.child(div().text_xs().text_color(rgb(FG_DIM)).child(format!(
                        "      [{}] 行 {}：{}",
                        value.source, value.line_no, value.value
                    )));
                }
            }

            if !report.has_findings() {
                block = block.child(
                    div()
                        .text_xs()
                        .text_color(rgb(SUCCESS))
                        .child("   ✓ 无冲突"),
                );
            }

            blocks.push(block.into_any_element());
        }

        let mut section = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_lg().text_color(rgb(ACCENT)).child("冲突面板"));
        if blocks.is_empty() {
            section = section.child(
                div()
                    .text_xs()
                    .text_color(rgb(FG_DIM))
                    .child("未找到可扫描的播放器配置目录"),
            );
        }
        section.children(blocks).into_any_element()
    }

    /// uosc 专项面板（FR-09）
    fn render_uosc(&self) -> AnyElement {
        let mut blocks: Vec<AnyElement> = Vec::new();
        for panel in &self.uosc_panels {
            let mut block = div().flex().flex_col().gap_1();

            let install_line = if panel.status.installed {
                match &panel.status.commit {
                    Some(commit) => format!("uosc：已安装 @ {commit}"),
                    None => "uosc：已安装（非 Git 方式，无法增量更新）".to_string(),
                }
            } else {
                "uosc：未安装".to_string()
            };
            let undo_line = if panel.undo_available > 0 {
                format!("撤回：{} 个备份可用", panel.undo_available)
            } else {
                "撤回：暂无备份".to_string()
            };
            block = block.child(div().text_sm().text_color(rgb(FG)).child(format!(
                "▸ {}　{install_line}　脚本 {} 个　uosc.conf：{}　{undo_line}",
                panel.player_name,
                panel.script_count,
                if panel.status.conf_exists {
                    "存在"
                } else {
                    "缺省"
                }
            )));

            if let Some(warning) = &panel.options_warning {
                block = block.child(
                    div()
                        .text_xs()
                        .text_color(rgb(WARNING))
                        .child(format!("   ⚠ {warning}")),
                );
            }

            if let Some(options) = &panel.options {
                for option in options {
                    let value = option.current.clone().unwrap_or_else(|| "默认".to_string());
                    block = block.child(div().text_xs().text_color(rgb(FG_DIM)).child(format!(
                        "   {}（{}）［{}］= {value}",
                        option.spec.label, option.spec.key, option.spec.category
                    )));
                }
            }

            blocks.push(block.into_any_element());
        }

        let mut section = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_lg().text_color(rgb(ACCENT)).child("uosc"));
        if blocks.is_empty() {
            section = section.child(
                div()
                    .text_xs()
                    .text_color(rgb(FG_DIM))
                    .child("未找到可管理的播放器"),
            );
        }
        section.children(blocks).into_any_element()
    }
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
                            .child(format!("   配置目录：{config}")),
                    )
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .p_4()
            .gap_3()
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
            .child(self.render_conflicts())
            .child(self.render_uosc())
    }
}

/// 构建 uosc 面板视图模型（含脚本清单与可视化目录取值）
fn build_uosc_panels(players: &[DetectedPlayer]) -> Vec<UoscPanel> {
    let mut panels = Vec::new();
    for player in players {
        let Some(config_dir) = &player.config_dir else {
            continue;
        };
        let status = uosc_status(config_dir);
        let script_count = list_scripts(config_dir).len();
        let (options, options_warning) = if status.installed {
            match read_uosc_values(config_dir) {
                Ok(values) => (Some(values), None),
                Err(e) => (None, Some(e)),
            }
        } else {
            (None, None)
        };
        let undo_available = BackupManager::with_default_root().list("uosc.conf").len();
        panels.push(UoscPanel {
            player_name: player.name.clone(),
            status,
            script_count,
            options,
            options_warning,
            undo_available,
        });
    }
    panels
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

    let reports = scan_player_configs(&players);
    for report in &reports {
        for conflict in &report.input_conflicts {
            let effective = &conflict.bindings[conflict.effective_index];
            eprintln!(
                "[config] 按键冲突 {}：{} 处绑定，生效 \"{}\"（{}，行 {}）",
                conflict.key,
                conflict.bindings.len(),
                effective.command,
                effective.source,
                effective.line_no
            );
        }
        for conflict in &report.value_conflicts {
            eprintln!(
                "[config] 配置冲突 {}：{} 处定义，生效 \"{}\"",
                conflict.key,
                conflict.values.len(),
                conflict.effective
            );
        }
    }

    let uosc_panels = build_uosc_panels(&players);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.0), px(640.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| PlayersPage {
                    players,
                    reports,
                    uosc_panels,
                })
            },
        ) {
            eprintln!("无法打开 Névé 主窗口：{error}");
        }
    });
}
