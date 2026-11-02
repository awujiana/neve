//! ui —— Névé 主界面（GPUI + gpui-component）
//!
//! 视图模型与交互动作在此聚合；core 层只提供纯逻辑与文件操作。

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::{AnyElement, Context, Render, SharedString, Window, div, prelude::*, rgb};

use crate::core::backup::BackupManager;
use crate::core::config::report::PlayerConfigReport;
use crate::core::players::{DetectedPlayer, detect_players};
use crate::core::scripts::uosc::{
    UoscOptionValue, UoscStatus, install_uosc, read_uosc_values, uosc_status, update_uosc,
};
use crate::core::scripts::{
    DependencyIssue, ScriptEntry, disable_script, enable_script, list_scripts, scan_dependencies,
};
use crate::core::sync::{ConflictPolicy, SyncKind, apply_sync, plan_script_sync};

// 深色主题基色（对齐 config/ui-theme.toml 的 dark 段）
const FG: u32 = 0xe6f0f6;
const FG_DIM: u32 = 0xa8bbc9;
const ACCENT: u32 = 0x83c5ea;
const SUCCESS: u32 = 0x82d2a8;
const WARNING: u32 = 0xf1c66f;

/// 单个播放器的面板视图模型
#[derive(Clone)]
pub struct PlayerPanel {
    pub name: String,
    pub kind_label: String,
    pub exe_path: String,
    pub version: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub config_dir_label: String,
    pub portable: bool,
    pub uosc: UoscStatus,
    pub scripts: Vec<ScriptEntry>,
    pub deps: Vec<DependencyIssue>,
    pub report: PlayerConfigReport,
    pub options: Option<Vec<UoscOptionValue>>,
    pub options_warning: Option<String>,
    pub undo_available: usize,
}

/// 由检测结果构建全部面板
pub fn build_panels(players: &[DetectedPlayer]) -> Vec<PlayerPanel> {
    let reports = crate::core::config::report::scan_player_configs(players);
    let mut panels = Vec::new();
    for player in players {
        let config_dir = player.config_dir.clone();
        let report = reports
            .iter()
            .find(|r| r.player_name == player.name)
            .cloned()
            .unwrap_or_default();
        let (uosc, scripts, deps, options, options_warning) = match &config_dir {
            Some(dir) => {
                let status = uosc_status(dir);
                let scripts = list_scripts(dir);
                let deps = scan_dependencies(dir);
                let (options, warning) = if status.installed {
                    match read_uosc_values(dir) {
                        Ok(values) => (Some(values), None),
                        Err(e) => (None, Some(e)),
                    }
                } else {
                    (None, None)
                };
                (status, scripts, deps, options, warning)
            }
            None => (
                UoscStatus {
                    installed: false,
                    install_dir: PathBuf::new(),
                    commit: None,
                    conf_exists: false,
                },
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
        };
        let undo_available = BackupManager::with_default_root().list("uosc.conf").len();
        panels.push(PlayerPanel {
            name: player.name.clone(),
            kind_label: format!("{:?}", player.kind),
            exe_path: player.exe_path.display().to_string(),
            version: player.version.clone(),
            config_dir_label: config_dir
                .as_ref()
                .map(|c| c.display().to_string())
                .unwrap_or_else(|| "未找到配置目录".to_string()),
            portable: player.is_portable(),
            config_dir,
            uosc,
            scripts,
            deps,
            report,
            options,
            options_warning,
            undo_available,
        });
    }
    panels
}

/// 主页面实体
pub struct PlayersPage {
    players: Vec<DetectedPlayer>,
    panels: Vec<PlayerPanel>,
    policy: ConflictPolicy,
    status: Option<String>,
}

impl PlayersPage {
    pub fn new(players: Vec<DetectedPlayer>, panels: Vec<PlayerPanel>) -> Self {
        Self {
            players,
            panels,
            policy: ConflictPolicy::Skip,
            status: None,
        }
    }

    /// 重新扫描全部播放器并刷新面板
    fn refresh(&mut self) {
        self.players = detect_players();
        self.panels = build_panels(&self.players);
    }

    fn config_dir_of(&self, idx: usize) -> Option<PathBuf> {
        self.panels.get(idx).and_then(|p| p.config_dir.clone())
    }

    fn install(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.config_dir_of(idx) else {
            return;
        };
        let name = self.panels[idx].name.clone();
        self.status = Some(match install_uosc(&dir) {
            Ok(msg) => format!("{name}：{msg}"),
            Err(e) => format!("{name} 安装失败：{e}"),
        });
        self.refresh();
        cx.notify();
    }

    fn update(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.config_dir_of(idx) else {
            return;
        };
        let name = self.panels[idx].name.clone();
        self.status = Some(match update_uosc(&dir) {
            Ok(msg) => format!("{name}：{msg}"),
            Err(e) => format!("{name} 更新失败：{e}"),
        });
        self.refresh();
        cx.notify();
    }

    fn undo_uosc_conf(&mut self, idx: usize, cx: &mut Context<Self>) {
        let name = self.panels[idx].name.clone();
        let backups = BackupManager::with_default_root();
        self.status = Some(match backups.restore_latest("uosc.conf") {
            Ok(record) => format!(
                "{name}：已撤回 uosc.conf（{}）",
                record.original_path.display()
            ),
            Err(e) => format!("{name} 撤回失败：{e}"),
        });
        self.refresh();
        cx.notify();
    }

    fn toggle_script(&mut self, idx: usize, script_idx: usize, cx: &mut Context<Self>) {
        let Some(script) = self.panels[idx].scripts.get(script_idx).cloned() else {
            return;
        };
        let action = if script.enabled {
            disable_script(&script).map(|_| "已禁用")
        } else {
            enable_script(&script).map(|_| "已启用")
        };
        self.status = Some(match action {
            Ok(verb) => format!("{} {}", script.name, verb),
            Err(e) => e,
        });
        self.refresh();
        cx.notify();
    }

    fn sync_script(
        &mut self,
        src_idx: usize,
        script_name: &str,
        dst_idx: usize,
        cx: &mut Context<Self>,
    ) {
        let (Some(src_dir), Some(dst_dir)) =
            (self.config_dir_of(src_idx), self.config_dir_of(dst_idx))
        else {
            return;
        };
        let dst_name = self.panels[dst_idx].name.clone();
        let plan = plan_script_sync(&src_dir, &dst_dir, script_name);
        self.status = Some(match plan {
            Ok(plan) => {
                let planned_new = plan.count(SyncKind::New);
                let planned_conflict = plan.count(SyncKind::Conflict);
                let backups = BackupManager::with_default_root();
                match apply_sync(&plan, self.policy, &backups) {
                    Ok(report) => format!(
                        "{script_name} → {dst_name}（计划 新增{planned_new}/冲突{planned_conflict}）：写入 {}，跳过 {}，双份 {}，备份 {}",
                        report.written,
                        report.skipped,
                        report.kept_both,
                        report.backups.len()
                    ),
                    Err(e) => format!("同步失败：{e}"),
                }
            }
            Err(e) => format!("无法生成同步计划：{e}"),
        });
        self.refresh();
        cx.notify();
    }

    fn cycle_policy(&mut self, cx: &mut Context<Self>) {
        self.policy = match self.policy {
            ConflictPolicy::Skip => ConflictPolicy::Overwrite,
            ConflictPolicy::Overwrite => ConflictPolicy::KeepBoth,
            ConflictPolicy::KeepBoth => ConflictPolicy::Skip,
        };
        cx.notify();
    }

    fn policy_label(&self) -> &'static str {
        match self.policy {
            ConflictPolicy::Skip => "冲突：跳过",
            ConflictPolicy::Overwrite => "冲突：覆盖",
            ConflictPolicy::KeepBoth => "冲突：保留双份",
        }
    }

    fn render_player_card(&self, idx: usize, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.panels[idx].clone();
        let mut card = div().flex().flex_col().gap_1().py_2();

        let portable_label = if panel.portable {
            "（便携版）"
        } else {
            ""
        };
        card = card.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(FG))
                        .child(format!("▸ {}{portable_label}", panel.name)),
                )
                .child(div().text_xs().text_color(rgb(FG_DIM)).child(format!(
                            "{} | {} | {}",
                            panel.kind_label,
                            panel
                                .version
                                .clone()
                                .unwrap_or_else(|| "版本未探测".to_string()),
                            panel.exe_path
                        ))),
        );

        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(FG_DIM))
                .child(format!("配置目录：{}", panel.config_dir_label)),
        );

        // uosc 操作行
        let mut actions = div().flex().flex_row().items_center().gap_2();
        if panel.config_dir.is_some() {
            if panel.uosc.installed {
                let commit = panel
                    .uosc
                    .commit
                    .clone()
                    .unwrap_or_else(|| "非 Git".to_string());
                actions = actions.child(
                    div()
                        .text_xs()
                        .text_color(rgb(SUCCESS))
                        .child(format!("uosc 已安装 @ {commit}")),
                );
                actions = actions.child(
                    Button::new(SharedString::from(format!("update-{idx}")))
                        .label("更新 uosc")
                        .compact()
                        .on_click(cx.listener(move |this, _, _, cx| this.update(idx, cx))),
                );
                actions = actions.child(
                    Button::new(SharedString::from(format!("undo-{idx}")))
                        .label(format!("撤回 uosc.conf（{}）", panel.undo_available))
                        .compact()
                        .on_click(cx.listener(move |this, _, _, cx| this.undo_uosc_conf(idx, cx))),
                );
            } else {
                actions =
                    actions.child(div().text_xs().text_color(rgb(FG_DIM)).child("uosc 未安装"));
                actions = actions.child(
                    Button::new(SharedString::from(format!("install-{idx}")))
                        .label("安装 uosc")
                        .primary()
                        .compact()
                        .on_click(cx.listener(move |this, _, _, cx| this.install(idx, cx))),
                );
            }
        }
        card = card.child(actions);

        // 依赖警告
        for issue in &panel.deps {
            card = card.child(div().text_xs().text_color(rgb(WARNING)).child(format!(
                "⚠ {} 缺少依赖：{}",
                issue.script,
                issue.missing.join("、")
            )));
        }

        // 脚本列表（含启停开关）
        if panel.scripts.is_empty() {
            card = card.child(div().text_xs().text_color(rgb(FG_DIM)).child("脚本：无"));
        } else {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(rgb(FG_DIM))
                    .child(format!("脚本（{}）", panel.scripts.len())),
            );
            for (script_idx, script) in panel.scripts.iter().enumerate() {
                let opt = if script.has_script_opt { " ·conf" } else { "" };
                let row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(FG))
                            .child(format!("{}{opt}", script.name)),
                    )
                    .child(
                        Switch::new(SharedString::from(format!("sw-{idx}-{script_idx}")))
                            .checked(script.enabled)
                            .on_click(cx.listener(move |this, _checked, _, cx| {
                                this.toggle_script(idx, script_idx, cx)
                            })),
                    );
                card = card.child(row);
            }
        }

        // 冲突
        for conflict in &panel.report.input_conflicts {
            let effective = &conflict.bindings[conflict.effective_index];
            card = card.child(div().text_xs().text_color(rgb(WARNING)).child(format!(
                "⇄ 按键 {}：{} 处绑定，生效 \"{}\"（{}，行 {}）",
                conflict.key,
                conflict.bindings.len(),
                effective.command,
                effective.source,
                effective.line_no
            )));
        }
        for conflict in &panel.report.value_conflicts {
            card = card.child(div().text_xs().text_color(rgb(WARNING)).child(format!(
                "⇄ 配置 {}：{} 处定义，生效 \"{}\"",
                conflict.key,
                conflict.values.len(),
                conflict.effective
            )));
        }
        for warning in &panel.report.warnings {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(rgb(WARNING))
                    .child(format!("⚠ {warning}")),
            );
        }
        if panel.config_dir.is_some() && !panel.report.has_findings() {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(rgb(SUCCESS))
                    .child("✓ 配置无冲突"),
            );
        }

        // uosc.conf 可视化目录取值
        if let Some(options) = &panel.options {
            for option in options {
                let value = option.current.clone().unwrap_or_else(|| "默认".to_string());
                card = card.child(div().text_xs().text_color(rgb(FG_DIM)).child(format!(
                    "   {}（{}）［{}］= {value}",
                    option.spec.label, option.spec.key, option.spec.category
                )));
            }
        }
        if let Some(warning) = &panel.options_warning {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(rgb(WARNING))
                    .child(format!("⚠ {warning}")),
            );
        }

        // 跨包同步：每个脚本 → 其他播放器
        if panel.config_dir.is_some() && !panel.scripts.is_empty() {
            for script in &panel.scripts {
                let mut sync_row = div().flex().flex_row().items_center().gap_2().child(
                    div()
                        .text_xs()
                        .text_color(rgb(FG_DIM))
                        .child(format!("同步 {} →", script.name)),
                );
                for (dst_idx, dst) in self.panels.iter().enumerate() {
                    if dst_idx == idx || dst.config_dir.is_none() {
                        continue;
                    }
                    let script_name = script.name.clone();
                    sync_row = sync_row.child(
                        Button::new(SharedString::from(format!(
                            "sync-{idx}-{dst_idx}-{}",
                            script.name
                        )))
                        .label(dst.name.clone())
                        .compact()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.sync_script(idx, &script_name, dst_idx, cx)
                        })),
                    );
                }
                card = card.child(sync_row);
            }
        }

        card.into_any_element()
    }
}

impl Render for PlayersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div().flex().flex_col().gap_3();

        content = content.child(
            div()
                .text_lg()
                .text_color(rgb(ACCENT))
                .child("Névé — 播放器管理器"),
        );

        // 顶栏：策略切换 + 最近操作结果
        let mut top = div().flex().flex_row().items_center().gap_3();
        top = top.child(
            div()
                .text_xs()
                .text_color(rgb(FG_DIM))
                .child(format!("共 {} 个 MPV 内核播放器", self.panels.len())),
        );
        top = top.child(
            Button::new("policy")
                .label(self.policy_label())
                .compact()
                .on_click(cx.listener(|this, _, _, cx| this.cycle_policy(cx))),
        );
        if let Some(status) = &self.status {
            top = top.child(
                div()
                    .text_xs()
                    .text_color(rgb(SUCCESS))
                    .child(status.clone()),
            );
        }
        content = content.child(top);

        for idx in 0..self.panels.len() {
            content = content.child(self.render_player_card(idx, cx));
        }

        if self.panels.is_empty() {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(rgb(FG_DIM))
                    .child("未检测到 MPV 内核播放器"),
            );
        }

        content
    }
}
