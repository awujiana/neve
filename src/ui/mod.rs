//! ui —— Névé 主界面（GPUI + gpui-component）
//!
//! 布局依据 `docs/UI设计规范.md` §3：顶栏 + 常驻侧栏 + 内容区 + 详情区。
//! 颜色一律取自 `ui::theme` 投影后的语义令牌（`cx.theme().colors.*`），
//! 不在页面里硬编码 RGB，从而保证昼夜主题、对比度与后续皮肤能力一致。
//!
//! 视图模型与交互动作在此聚合；core 层只提供纯逻辑与文件操作。

mod theme;

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, InteractiveElementExt as _, Sizable as _, Size, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, BoxShadow, Context, Div, ElementId, Hsla, IntoElement, MouseButton,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Window, div, linear_color_stop,
    linear_gradient, point, prelude::*, px,
};

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

pub use theme::apply as apply_theme;

// ---------------------------------------------------------------------------
// 视图模型
// ---------------------------------------------------------------------------

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

impl PlayerPanel {
    /// 该播放器待处理的事项数：依赖缺失 + 配置冲突 + 警告。
    fn issue_count(&self) -> usize {
        let dep = self.deps.len();
        let conflict = self.report.input_conflicts.len() + self.report.value_conflicts.len();
        let warn = self.report.warnings.len() + usize::from(self.options_warning.is_some());
        dep + conflict + warn
    }
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
            kind_label: player.kind.display_name(),
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

// ---------------------------------------------------------------------------
// 操作反馈
// ---------------------------------------------------------------------------

/// 反馈语义。依据规范 §5：成功/失败提示必须包含可理解结果，且状态不单靠颜色表达。
#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

impl StatusKind {
    fn glyph(self) -> &'static str {
        match self {
            StatusKind::Info => "•",
            StatusKind::Success => "✓",
            StatusKind::Warning => "!",
            StatusKind::Error => "×",
        }
    }

    fn color(self, cx: &App) -> Hsla {
        let c = &cx.theme().colors;
        match self {
            StatusKind::Info => c.muted_foreground,
            StatusKind::Success => c.success,
            StatusKind::Warning => c.warning,
            StatusKind::Error => c.danger,
        }
    }
}

#[derive(Clone)]
struct StatusMessage {
    kind: StatusKind,
    text: String,
}

impl StatusMessage {
    fn success(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Success,
            text: text.into(),
        }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Warning,
            text: text.into(),
        }
    }

    fn info(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Info,
            text: text.into(),
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Error,
            text: text.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Players,
    Scripts,
    Skins,
    Configs,
    Backup,
    Settings,
}

impl Page {
    const ALL: [Page; 7] = [
        Page::Overview,
        Page::Players,
        Page::Scripts,
        Page::Skins,
        Page::Configs,
        Page::Backup,
        Page::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Overview => "概览",
            Page::Players => "播放器",
            Page::Scripts => "脚本",
            Page::Skins => "皮肤",
            Page::Configs => "配置",
            Page::Backup => "备份与同步",
            Page::Settings => "设置",
        }
    }

    /// 内容区页标题下的一句说明（规范 §3：页标题与一句说明）。
    fn description(self) -> &'static str {
        match self {
            Page::Overview => "检测结果、待处理事项与最近一次操作的汇总。",
            Page::Players => "所有使用 MPV 内核的播放器，及其配置目录与可管理内容。",
            Page::Scripts => "已安装脚本、依赖关系与跨包同步。",
            Page::Skins => "uosc 主题与 OSC 皮肤的预览和应用。",
            Page::Configs => "mpv.conf、input.conf 与脚本配置的冲突清单。",
            Page::Backup => "写入前的备份批次与恢复入口。",
            Page::Settings => "外观、语言、音效、托盘、数据位置与系统集成。",
        }
    }

    /// 尚未接入的页面给出版本归属，避免把「未实现」伪装成「空列表」。
    fn milestone(self) -> Option<&'static str> {
        match self {
            Page::Overview | Page::Players => None,
            Page::Scripts => Some(
                "启停开关、依赖扫描与跨包同步已可用，见「播放器」页。独立的脚本库视图随 M3 接入。",
            ),
            Page::Skins => Some("uosc 主题与 OSC 皮肤管理对应 FR-10，随 M3 接入。"),
            Page::Configs => {
                Some("冲突检测已接入「播放器」页的配置检查区；独立配置编辑器随 M3 接入。")
            }
            Page::Backup => {
                Some("备份记录已落盘（backups/<操作>/<时间戳>/），浏览与批量恢复随 M4 接入。")
            }
            Page::Settings => Some("外观定制（FR-04）与系统集成（FR-06）随 M4 接入。"),
        }
    }
}

// ---------------------------------------------------------------------------
// 通用渲染构件
// ---------------------------------------------------------------------------

/// 卡片容器：液态玻璃材质。
///
/// 依据规范 §4.1，玻璃由四层构成，GPUI 无背景模糊，故用以下组合实现：
/// 1. 半透明填充（`Glass::surface`）—— 透出底层渐变，形成材质感；
/// 2. 1px 亮边（`Glass::highlight`）—— 边缘折射高光；
/// 3. 内侧 inset 阴影 —— 顶部内侧一条细亮线，这是液态玻璃最关键的读感；
/// 4. 向下的柔和投影 —— 把卡片从背景层抬起。
fn card(cx: &App) -> Div {
    let g = theme::glass(cx);
    v_flex()
        .gap_3()
        .p_4()
        .rounded(px(14.))
        .bg(g.surface)
        .border_1()
        .border_color(g.highlight)
        .shadow(vec![
            BoxShadow {
                color: g.shadow,
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(-8.),
                inset: false,
            },
            BoxShadow {
                color: g.highlight,
                offset: point(px(0.), px(1.5)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: true,
            },
        ])
}

/// 背景柔光：一个透明圆，只留它的高斯模糊投影。
///
/// GPUI 既没有背景模糊，也没有径向渐变，能给出「无分层平滑衰减」的原语只有阴影模糊。
/// 所以这里画一个形状可见、填充透明的圆，把 `blur_radius` 放大到接近自身尺寸，
/// 投影便摊成一片光晕：边缘由高斯核自然收敛，不存在任何硬边。
///
/// 早期版本用多层同心圆逐层叠 alpha 近似径向渐变，深色底上会出现肉眼可见的环带，
/// 无论怎么加密层数都只能把环推细而不能消除，故改为本方案。
/// 背景柔光：多层同心圆近似径向渐变。
///
/// GPUI 没有背景模糊，也没有径向渐变，能画出来的只有圆和线性渐变。
/// 试过两条路都不可行：单层圆留硬边；用大半径阴影模糊（`blur_radius` ≈ 260px）
/// 会因采样精度不足出现多边形棱边与网格接缝。所以只能老老实实叠同心圆。
///
/// 关键在于**衰减曲线**：按 `(1-u)^2.2` 分配各层 alpha（u 为归一化半径，外圈为 1），
/// 使每层的 alpha 增量在**外圈趋近于零**——那正是肉眼最容易看出环带的位置；
/// 增量集中在内圈，那里表面已被下层铺满、局部对比度极低，看不出台阶。
/// 层数取 64，配合该曲线可把外圈的步进压到 0.4% 以下（1/255 ≈ 0.39%）。
fn glow(size: Pixels, color: Hsla, center_top: Pixels, center_left: Pixels) -> Vec<Div> {
    const LAYERS: usize = 64;
    const FALLOFF: f32 = 1.9;
    const OUTER_SCALE: f32 = 0.75;

    let mut layers = Vec::with_capacity(LAYERS);
    let mut previous = 0.0_f32;
    for index in 0..LAYERS {
        // index 0 是最外圈（u = 1），逐层收到最内圈（u = 0）。
        let u = 1.0 - index as f32 / (LAYERS - 1) as f32;
        // 该半径处期望的累计不透明度。
        let target = color.a * (1.0 - u).powf(FALLOFF);
        // 已知下层已累计 previous，反解本层需要补的 alpha。
        let alpha = 1.0 - (1.0 - target) / (1.0 - previous);
        previous = target;

        let layer_size = size * (1.0 + OUTER_SCALE * u);
        layers.push(
            div()
                .absolute()
                .top(center_top + (size - layer_size) / 2.0)
                .left(center_left + (size - layer_size) / 2.0)
                .w(layer_size)
                .h(layer_size)
                .rounded_full()
                .bg(Hsla {
                    h: color.h,
                    s: color.s,
                    l: color.l,
                    a: alpha,
                }),
        );
    }
    layers
}

/// 分区小标题。
fn section_title(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().colors.muted_foreground)
        .child(text.into())
}

/// 分区之间的分隔线（同时提供留白）。
fn section_divider(cx: &App) -> Div {
    div()
        .pt_3()
        .border_t_1()
        .border_color(cx.theme().colors.border)
}

/// 小徽标。文字与底色配对，保证任意主题下对比度可读。
fn badge(label: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    h_flex()
        .h(px(18.))
        .px_2()
        .items_center()
        .rounded(px(4.))
        .bg(bg)
        .text_color(fg)
        .text_xs()
        .child(label.into())
}

/// 窗口控制按钮的动作。
#[derive(Clone, Copy)]
enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
}

/// 自绘窗口控制按钮。
///
/// 不使用组件库 `TitleBar` 自带的控制按钮，是因为它依赖 SVG 图标资源；
/// 资源缺失时按钮只剩空白可点区域，用户会看不到关闭入口。
/// 这里用字形绘制，任何环境下都可见。
fn window_control(
    id: &'static str,
    glyph: &'static str,
    fg: Hsla,
    hover_bg: Hsla,
    hover_fg: Hsla,
    action: WindowAction,
) -> AnyElement {
    div()
        .id(ElementId::Name(SharedString::from(id)))
        .w(px(38.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(fg)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
        .on_click(move |_, window, _| match action {
            WindowAction::Minimize => window.minimize_window(),
            WindowAction::ToggleMaximize => window.zoom_window(),
            WindowAction::Close => window.remove_window(),
        })
        .child(glyph)
        .into_any_element()
}

/// 空状态：图标位 + 标题 + 说明 + 可选下一步动作。
fn empty_state(
    title: &'static str,
    hint: impl Into<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> Div {
    let mut body = v_flex().items_center().gap_2().py_8().px_4();
    body = body.child(
        div()
            .text_color(cx.theme().colors.muted_foreground)
            .text_lg()
            .child("◍"),
    );
    body = body.child(
        div()
            .text_sm()
            .text_color(cx.theme().colors.foreground)
            .child(title),
    );
    body = body.child(
        div()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(hint.into()),
    );
    if let Some(action) = action {
        body = body.child(div().pt_2().child(action));
    }
    body
}

/// 统计卡：数值 + 标签 + 一句上下文。
fn stat_card(value: impl Into<SharedString>, label: &'static str, hint: String, cx: &App) -> Div {
    // 统计卡沿用卡片玻璃，但数值用强调色，把「数据」与「正文」分开层级。
    card(cx)
        .flex_1()
        .min_w_0()
        .gap_1()
        .p_3()
        .child(
            div()
                .text_xl()
                .text_color(cx.theme().colors.primary)
                .child(value.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.foreground)
                .child(label),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(hint),
        )
}

// ---------------------------------------------------------------------------
// 主页面实体
// ---------------------------------------------------------------------------

pub struct AppShell {
    players: Vec<DetectedPlayer>,
    panels: Vec<PlayerPanel>,
    policy: ConflictPolicy,
    status: Option<StatusMessage>,
    page: Page,
    dark: bool,
    /// 已展开完整脚本列表的播放器下标（默认只显示前几条）。
    expanded_scripts: Vec<usize>,
}

impl AppShell {
    pub fn new(players: Vec<DetectedPlayer>, panels: Vec<PlayerPanel>, dark: bool) -> Self {
        Self {
            players,
            panels,
            policy: ConflictPolicy::Skip,
            status: None,
            page: Page::Overview,
            dark,
            expanded_scripts: Vec::new(),
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

    fn player_name(&self, idx: usize) -> String {
        self.panels
            .get(idx)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.refresh();
        // 未检测到播放器时不用「成功」语气，避免把空结果包装成正常状态。
        self.status = Some(if self.panels.is_empty() {
            StatusMessage::warning("未检测到 MPV 内核播放器。安装播放器或整合包后重新扫描。")
        } else {
            StatusMessage::success(format!(
                "已重新扫描，发现 {} 个 MPV 内核播放器",
                self.panels.len()
            ))
        });
        cx.notify();
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.dark = !self.dark;
        let mode = if self.dark {
            gpui_kit::component::theme::ThemeMode::Dark
        } else {
            gpui_kit::component::theme::ThemeMode::Light
        };
        theme::apply(cx, mode);
        cx.notify();
    }

    fn install(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.config_dir_of(idx) else {
            return;
        };
        let name = self.player_name(idx);
        self.status = Some(match install_uosc(&dir) {
            Ok(msg) => StatusMessage::success(format!("{name}：{msg}")),
            Err(e) => StatusMessage::error(format!("{name} 安装失败：{e}")),
        });
        self.refresh();
        cx.notify();
    }

    fn update(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.config_dir_of(idx) else {
            return;
        };
        let name = self.player_name(idx);
        self.status = Some(match update_uosc(&dir) {
            Ok(msg) => StatusMessage::success(format!("{name}：{msg}")),
            Err(e) => StatusMessage::error(format!("{name} 更新失败：{e}")),
        });
        self.refresh();
        cx.notify();
    }

    fn undo_uosc_conf(&mut self, idx: usize, cx: &mut Context<Self>) {
        let name = self.player_name(idx);
        let backups = BackupManager::with_default_root();
        self.status = Some(match backups.restore_latest("uosc.conf") {
            Ok(record) => StatusMessage::success(format!(
                "{name}：已撤回 uosc.conf（{}）",
                record.original_path.display()
            )),
            Err(e) => StatusMessage::error(format!("{name} 撤回失败：{e}")),
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
            Ok(verb) => StatusMessage::success(format!("{} {verb}", script.name)),
            Err(e) => StatusMessage::error(e),
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
        let dst_name = self.player_name(dst_idx);
        let plan = plan_script_sync(&src_dir, &dst_dir, script_name);
        self.status = Some(match plan {
            Ok(plan) => {
                let planned_new = plan.count(SyncKind::New);
                let planned_conflict = plan.count(SyncKind::Conflict);
                let backups = BackupManager::with_default_root();
                match apply_sync(&plan, self.policy, &backups) {
                    Ok(report) => {
                        let text = format!(
                            "{script_name} → {dst_name}（计划 新增{planned_new}/冲突{planned_conflict}）：写入 {}，跳过 {}，双份 {}，备份 {}",
                            report.written,
                            report.skipped,
                            report.kept_both,
                            report.backups.len()
                        );
                        if report.skipped > 0 {
                            StatusMessage::warning(text)
                        } else {
                            StatusMessage::success(text)
                        }
                    }
                    Err(e) => StatusMessage::error(format!("同步失败：{e}")),
                }
            }
            Err(e) => StatusMessage::error(format!("无法生成同步计划：{e}")),
        });
        self.refresh();
        cx.notify();
    }

    fn toggle_scripts_expanded(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(pos) = self.expanded_scripts.iter().position(|v| *v == idx) {
            self.expanded_scripts.remove(pos);
        } else {
            self.expanded_scripts.push(idx);
        }
        cx.notify();
    }

    fn cycle_policy(&mut self, cx: &mut Context<Self>) {
        self.policy = match self.policy {
            ConflictPolicy::Skip => ConflictPolicy::Overwrite,
            ConflictPolicy::Overwrite => ConflictPolicy::KeepBoth,
            ConflictPolicy::KeepBoth => ConflictPolicy::Skip,
        };
        self.status = Some(StatusMessage::info(format!(
            "{}（下次同步生效）",
            self.policy_label()
        )));
        cx.notify();
    }

    fn policy_label(&self) -> &'static str {
        match self.policy {
            ConflictPolicy::Skip => "冲突策略：跳过",
            ConflictPolicy::Overwrite => "冲突策略：覆盖",
            ConflictPolicy::KeepBoth => "冲突策略：保留双份",
        }
    }

    // -----------------------------------------------------------------------
    // 标题栏
    // -----------------------------------------------------------------------

    /// 自绘标题栏（窗口选项里隐藏了系统标题栏）。
    ///
    /// 用自绘而非系统标题栏，是为了让标题栏底色跟随昼夜主题——
    /// 系统标题栏在深色应用上仍是白色，构成明显的视觉断层。
    /// 左侧区域承担窗口拖动与双击最大化，右侧是最小化/最大化/关闭。
    fn render_title_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let g = theme::glass(cx);
        let theme_label = if self.dark { "浅色" } else { "深色" };
        let fg = t.colors.foreground;
        let muted = t.colors.muted_foreground;
        let hover_bg = t.colors.accent;
        let danger_bg = t.colors.danger;
        let danger_fg = t.colors.primary_foreground;
        let maximize_glyph = if window.is_maximized() { "❐" } else { "▢" };

        h_flex()
            .flex_none()
            .h(px(34.))
            .pl_3()
            .items_center()
            .gap_2()
            // 常驻容器用更实的玻璃，与卡片区分层级（规范 §4.1 第 2 层）。
            .bg(g.surface_strong)
            .border_b_1()
            .border_color(g.highlight)
            // 拖动区：占据标题栏左侧剩余空间，与真实标题栏行为一致。
            .child(
                h_flex()
                    .id("titlebar-drag")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap_2()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                    .on_double_click(|_, window, _| window.zoom_window())
                    .child(div().text_sm().text_color(fg).child("Névé"))
                    .child(div().text_xs().text_color(muted).child("粒雪 · MPV 管理器")),
            )
            .child(
                Button::new("toggle-theme")
                    .label(theme_label)
                    .with_size(Size::Small)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))),
            )
            .child(
                Button::new("rescan")
                    .label("重新扫描")
                    .primary()
                    .with_size(Size::Small)
                    .on_click(cx.listener(|this, _, _, cx| this.rescan(cx))),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h_full()
                    .items_center()
                    .child(window_control(
                        "win-min",
                        "─",
                        muted,
                        hover_bg,
                        fg,
                        WindowAction::Minimize,
                    ))
                    .child(window_control(
                        "win-max",
                        maximize_glyph,
                        muted,
                        hover_bg,
                        fg,
                        WindowAction::ToggleMaximize,
                    ))
                    .child(window_control(
                        "win-close",
                        "✕",
                        muted,
                        danger_bg,
                        danger_fg,
                        WindowAction::Close,
                    )),
            )
    }

    // -----------------------------------------------------------------------
    // 侧栏
    // -----------------------------------------------------------------------

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let mut nav = v_flex().gap_1();
        for page in Page::ALL {
            let selected = page == self.page;
            let fg = if selected {
                t.colors.sidebar_accent_foreground
            } else {
                t.colors.sidebar_foreground
            };
            let mut item = h_flex()
                .id(ElementId::Name(SharedString::from(format!(
                    "nav-{}",
                    page.label()
                ))))
                .h(px(30.))
                .px_2()
                .gap_2()
                .items_center()
                .rounded(t.radius)
                .text_sm()
                .text_color(fg)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
                .child(div().text_color(fg).child(page.label()));
            if selected {
                // 选中态同时用背景、左侧标记与文字强调表达，不只靠颜色（规范 §3）。
                item = item
                    .bg(t.colors.sidebar_accent)
                    .border_l_2()
                    .border_color(t.colors.primary)
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(t.colors.primary).child("•"));
            } else {
                // 悬停底色弱于选中底色，避免两者混淆。
                let hover = t.colors.sidebar_accent.opacity(0.45);
                item = item.hover(move |style| style.bg(hover));
            }
            nav = nav.child(item);
        }

        let g = theme::glass(cx);
        v_flex()
            .flex_none()
            .w(px(184.))
            .h_full()
            .py_3()
            .px_2()
            .gap_2()
            .bg(g.surface_strong)
            .border_r_1()
            .border_color(g.highlight)
            .child(nav)
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap_1()
                    .px_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.colors.muted_foreground)
                            .child(format!("{} 个播放器", self.panels.len())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.colors.muted_foreground)
                            .child(env!("CARGO_PKG_VERSION")),
                    ),
            )
    }

    // -----------------------------------------------------------------------
    // 内容区：页头
    // -----------------------------------------------------------------------

    fn render_page_header(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let mut head = v_flex().gap_1().px_6().pt_5().pb_4();

        let mut title_row = h_flex().items_center().gap_3().child(
            div()
                .text_lg()
                .text_color(t.colors.foreground)
                .child(self.page.label()),
        );
        title_row = title_row.child(
            div()
                .text_xs()
                .text_color(t.colors.muted_foreground)
                .child(self.page.description()),
        );
        head = head.child(title_row);

        // 工具行：仅在播放器相关页面出现，避免无关页面堆叠控件。
        if matches!(self.page, Page::Overview | Page::Players) {
            let mut tools = h_flex().items_center().gap_2().pt_2();
            tools = tools.child(
                Button::new("policy")
                    .label(self.policy_label())
                    .with_size(Size::Small)
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_policy(cx))),
            );
            if let Some(status) = &self.status {
                let color = status.kind.color(cx);
                tools = tools.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(div().text_xs().text_color(color).child(status.kind.glyph()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(color)
                                .child(status.text.clone()),
                        ),
                );
            }
            head = head.child(tools);
        }

        head.border_b_1().border_color(theme::glass(cx).highlight)
    }

    // -----------------------------------------------------------------------
    // 概览页
    // -----------------------------------------------------------------------

    fn render_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let installed = self.panels.iter().filter(|p| p.uosc.installed).count();
        let scripts: usize = self.panels.iter().map(|p| p.scripts.len()).sum();
        let issues: usize = self.panels.iter().map(PlayerPanel::issue_count).sum();

        let mut body = v_flex().gap_4();

        body = body.child(
            h_flex()
                .gap_3()
                .child(stat_card(
                    self.panels.len().to_string(),
                    "检测到的播放器",
                    "使用 MPV 内核".to_string(),
                    cx,
                ))
                .child(stat_card(
                    installed.to_string(),
                    "已安装 uosc",
                    format!("共 {} 个可管理", self.panels.len()),
                    cx,
                ))
                .child(stat_card(
                    scripts.to_string(),
                    "已安装脚本",
                    "含目录型脚本".to_string(),
                    cx,
                ))
                .child(stat_card(
                    issues.to_string(),
                    "待处理事项",
                    "依赖缺失与配置冲突".to_string(),
                    cx,
                )),
        );

        // 待处理事项清单：把「需要处理」变成可点击入口。
        let mut findings = card(cx);
        findings = findings.child(section_title("待处理事项", cx));
        if self.panels.is_empty() {
            findings = findings.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("未检测到播放器，先安装 mpv、mpv.net 或整合包后重新扫描。"),
            );
        } else {
            let mut rows = 0_usize;
            for (idx, panel) in self.panels.iter().enumerate() {
                if panel.issue_count() == 0 && panel.uosc.installed {
                    continue;
                }
                rows += 1;
                let hint = if panel.config_dir.is_none() {
                    "未找到配置目录".to_string()
                } else if !panel.uosc.installed {
                    "uosc 未安装".to_string()
                } else {
                    format!("{} 项问题", panel.issue_count())
                };
                findings = findings.child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w(px(160.))
                                .flex_none()
                                .text_xs()
                                .text_color(cx.theme().colors.foreground)
                                .child(panel.name.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().colors.warning)
                                .child(hint),
                        )
                        .child(
                            Button::new(SharedString::from(format!("go-{idx}")))
                                .label("查看")
                                .with_size(Size::XSmall)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.page = Page::Players;
                                    cx.notify();
                                })),
                        ),
                );
            }
            if rows == 0 {
                findings = findings.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.success)
                        .child("✓ 全部播放器状态正常"),
                );
            }
        }
        body = body.child(findings);

        // 最近操作
        let mut recent = card(cx);
        recent = recent.child(section_title("最近操作", cx));
        recent = recent.child(match &self.status {
            Some(status) => h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(status.kind.color(cx))
                        .child(status.kind.glyph()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().colors.foreground)
                        .child(status.text.clone()),
                ),
            None => div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child("本次会话还没有执行写入操作。所有写入前会自动备份。"),
        });
        body = body.child(recent);

        body.into_any_element()
    }

    // -----------------------------------------------------------------------
    // 播放器页
    // -----------------------------------------------------------------------

    fn render_players(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.panels.is_empty() {
            let action = Button::new("rescan-empty")
                .label("重新扫描")
                .primary()
                .with_size(Size::Small)
                .on_click(cx.listener(|this, _, _, cx| this.rescan(cx)))
                .into_any_element();
            return empty_state(
                "未检测到 MPV 内核播放器",
                "已扫描常见安装路径、注册表卸载项与 PATH。安装播放器或整合包后重新扫描。",
                Some(action),
                cx,
            )
            .into_any_element();
        }

        let mut body = v_flex().gap_3();
        for idx in 0..self.panels.len() {
            body = body.child(self.render_player_card(idx, cx));
        }
        body.into_any_element()
    }

    fn render_player_card(&self, idx: usize, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.panels[idx].clone();
        let t = cx.theme();
        let mut body = card(cx);

        // --- 头部：名称 + 类型 + 便携 + 版本 | 主要操作 ---
        let mut title_row = h_flex().items_center().gap_2().child(
            div()
                .text_sm()
                .text_color(t.colors.foreground)
                .child(panel.name.clone()),
        );
        // 类型与名称同名时不重复显示，避免出现「mpv mpv」。
        if !panel.kind_label.eq_ignore_ascii_case(&panel.name) {
            title_row = title_row.child(badge(
                panel.kind_label.clone(),
                t.colors.primary,
                t.colors.secondary,
            ));
        }
        if panel.portable {
            title_row = title_row.child(badge(
                "便携版",
                t.colors.muted_foreground,
                t.colors.secondary,
            ));
        }
        title_row = title_row.child(badge(
            panel
                .version
                .clone()
                .unwrap_or_else(|| "版本未知".to_string()),
            t.colors.muted_foreground,
            t.colors.secondary,
        ));

        let info = v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(title_row)
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(t.colors.muted_foreground)
                    .child(panel.exe_path.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(t.colors.muted_foreground)
                    .child(format!("配置目录 {}", panel.config_dir_label)),
            );

        let mut actions = h_flex().flex_none().items_center().gap_2();
        if panel.config_dir.is_some() {
            if panel.uosc.installed {
                actions = actions
                    .child(
                        Button::new(SharedString::from(format!("update-{idx}")))
                            .label("更新 uosc")
                            .with_size(Size::Small)
                            .on_click(cx.listener(move |this, _, _, cx| this.update(idx, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("undo-{idx}")))
                            .label(format!("撤回配置（{}）", panel.undo_available))
                            .with_size(Size::Small)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.undo_uosc_conf(idx, cx)),
                            ),
                    );
            } else {
                actions = actions.child(
                    Button::new(SharedString::from(format!("install-{idx}")))
                        .label("安装 uosc")
                        .primary()
                        .with_size(Size::Small)
                        .on_click(cx.listener(move |this, _, _, cx| this.install(idx, cx))),
                );
            }
        }

        let mut head = h_flex().items_start().gap_3().child(info);
        head = head.child(actions);
        body = body.child(head);

        // --- 状态摘要：一屏可扫读，问题优先 ---
        let conflict_count =
            panel.report.input_conflicts.len() + panel.report.value_conflicts.len();
        let mut summary = h_flex().flex_wrap().items_center().gap_2();
        if panel.config_dir.is_none() {
            summary = summary.child(badge("无配置目录", t.colors.warning, t.colors.secondary));
        } else if panel.uosc.installed {
            summary = summary.child(badge("uosc 已安装", t.colors.success, t.colors.secondary));
        } else {
            summary = summary.child(badge("uosc 未安装", t.colors.warning, t.colors.secondary));
        }
        summary = summary.child(badge(
            format!("脚本 {}", panel.scripts.len()),
            t.colors.muted_foreground,
            t.colors.secondary,
        ));
        if panel.deps.is_empty() {
            summary = summary.child(badge("依赖完整", t.colors.success, t.colors.secondary));
        } else {
            summary = summary.child(badge(
                format!("依赖缺失 {}", panel.deps.len()),
                t.colors.warning,
                t.colors.secondary,
            ));
        }
        if panel.config_dir.is_some() {
            if !panel.report.has_findings() {
                summary = summary.child(badge("配置正常", t.colors.success, t.colors.secondary));
            } else {
                summary = summary.child(badge(
                    format!("配置问题 {}", conflict_count + panel.report.warnings.len()),
                    t.colors.warning,
                    t.colors.secondary,
                ));
            }
        }
        if panel.undo_available > 0 {
            summary = summary.child(badge(
                format!("可撤回 {}", panel.undo_available),
                t.colors.muted_foreground,
                t.colors.secondary,
            ));
        }
        body = body.child(summary);

        // --- uosc.conf 取值：按分类分组，chip 流呈现 ---
        if let Some(options) = &panel.options {
            let mut groups: Vec<(&'static str, Vec<&UoscOptionValue>)> = Vec::new();
            for option in options {
                match groups
                    .iter_mut()
                    .find(|(name, _)| *name == option.spec.category)
                {
                    Some((_, list)) => list.push(option),
                    None => groups.push((option.spec.category, vec![option])),
                }
            }
            let mut rows = v_flex().gap_2();
            for (category, list) in groups {
                let mut chips = h_flex().flex_wrap().items_center().gap_4();
                for option in list {
                    // 空字符串与未设置都按「默认」呈现，避免出现 `key =` 这种悬空等号。
                    let value = option
                        .current
                        .clone()
                        .filter(|v| !v.trim().is_empty())
                        .unwrap_or_else(|| "默认".to_string());
                    chips = chips.child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(t.colors.muted_foreground)
                                    .child(option.spec.label),
                            )
                            // 等号弱化到几乎不可见：它只是提示「这是个 conf 键值」，
                            // 不该与键名、取值争夺注意力。
                            .child(
                                div()
                                    .text_xs()
                                    .opacity(0.45)
                                    .text_color(t.colors.muted_foreground)
                                    .child("="),
                            )
                            // 取值放进淡色底片，键与值在视觉上成组，长行也便于定位。
                            .child(badge(value, t.colors.foreground, t.colors.secondary)),
                    );
                }
                rows = rows.child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .w(px(64.))
                                .flex_none()
                                .text_xs()
                                .text_color(t.colors.muted_foreground)
                                .child(category),
                        )
                        .child(div().flex_1().min_w_0().child(chips)),
                );
            }
            body = body.child(section_divider(cx));
            body = body.child(
                v_flex()
                    .gap_2()
                    .child(section_title("uosc.conf 取值", cx))
                    .child(rows),
            );
        }

        // --- 脚本列表（含启停开关）---
        if !panel.scripts.is_empty() {
            // 整合包常见 30+ 脚本，全量展开会让单卡高度失控；默认只列前几条。
            const SCRIPT_PREVIEW: usize = 6;
            let expanded = self.expanded_scripts.contains(&idx);
            let shown = if expanded {
                panel.scripts.len()
            } else {
                panel.scripts.len().min(SCRIPT_PREVIEW)
            };
            let mut rows = v_flex().gap_1();
            for (script_idx, script) in panel.scripts.iter().take(shown).enumerate() {
                let enabled = script.enabled;
                // 名称列与 conf 列定宽，开关紧跟在 conf 列之后，多行之间纵向对齐便于扫读。
                // 不插 flex_1 撑开：否则开关会被推到卡片最右侧，中间留出一大片空白。
                let mut row = h_flex()
                    .h(px(26.))
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(196.))
                            .flex_none()
                            .truncate()
                            .text_xs()
                            .text_color(if enabled {
                                t.colors.foreground
                            } else {
                                t.colors.muted_foreground
                            })
                            .child(script.name.clone()),
                    )
                    .child(
                        div()
                            .w(px(60.))
                            .flex_none()
                            .when(script.has_script_opt, |cell| {
                                cell.child(badge(
                                    "有 conf",
                                    t.colors.muted_foreground,
                                    t.colors.secondary,
                                ))
                            }),
                    );
                row = row.child(
                    Switch::new(SharedString::from(format!("sw-{idx}-{script_idx}")))
                        .checked(enabled)
                        .with_size(Size::Small)
                        .on_click(cx.listener(move |this, _checked, _, cx| {
                            this.toggle_script(idx, script_idx, cx)
                        })),
                );
                rows = rows.child(row);
            }
            let mut section = v_flex()
                .gap_2()
                .child(section_title("脚本", cx))
                .child(rows);
            if panel.scripts.len() > SCRIPT_PREVIEW {
                let label = if expanded {
                    "收起".to_string()
                } else {
                    format!("展开全部 {}", panel.scripts.len())
                };
                // 包一层 h_flex，避免按钮在纵向容器里被拉满整行宽度。
                section = section.child(
                    h_flex().child(
                        Button::new(SharedString::from(format!("expand-{idx}")))
                            .label(label)
                            .with_size(Size::XSmall)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_scripts_expanded(idx, cx)
                            })),
                    ),
                );
            }
            body = body.child(section_divider(cx));
            body = body.child(section);
        }

        // --- 依赖 ---
        if !panel.deps.is_empty() {
            let mut rows = v_flex().gap_1();
            for issue in &panel.deps {
                rows = rows.child(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .child(div().text_xs().text_color(t.colors.warning).child("!"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(t.colors.warning)
                                .child(format!(
                                    "{} 缺少依赖：{}",
                                    issue.script,
                                    issue.missing.join("、")
                                )),
                        ),
                );
            }
            body = body.child(section_divider(cx));
            body = body.child(
                v_flex()
                    .gap_2()
                    .child(section_title("依赖检查", cx))
                    .child(rows),
            );
        }

        // --- 配置冲突 ---
        let has_conflict_detail = conflict_count > 0
            || !panel.report.warnings.is_empty()
            || panel.options_warning.is_some();
        if has_conflict_detail {
            let mut rows = v_flex().gap_1();
            for conflict in &panel.report.input_conflicts {
                let effective = &conflict.bindings[conflict.effective_index];
                rows = rows.child(div().text_xs().text_color(t.colors.warning).child(format!(
                    "按键 {}：{} 处绑定，生效「{}」（{}，行 {}）",
                    conflict.key,
                    conflict.bindings.len(),
                    effective.command,
                    effective.source,
                    effective.line_no
                )));
            }
            for conflict in &panel.report.value_conflicts {
                rows = rows.child(div().text_xs().text_color(t.colors.warning).child(format!(
                    "配置 {}：{} 处定义，生效「{}」",
                    conflict.key,
                    conflict.values.len(),
                    conflict.effective
                )));
            }
            for warning in &panel.report.warnings {
                rows = rows.child(
                    div()
                        .text_xs()
                        .text_color(t.colors.warning)
                        .child(format!("! {warning}")),
                );
            }
            if let Some(warning) = &panel.options_warning {
                rows = rows.child(
                    div()
                        .text_xs()
                        .text_color(t.colors.warning)
                        .child(format!("! {warning}")),
                );
            }
            body = body.child(section_divider(cx));
            body = body.child(
                v_flex()
                    .gap_2()
                    .child(section_title("配置检查", cx))
                    .child(rows),
            );
        }

        // --- 跨包同步 ---
        let targets: Vec<(usize, String)> = self
            .panels
            .iter()
            .enumerate()
            .filter(|(dst_idx, dst)| *dst_idx != idx && dst.config_dir.is_some())
            .map(|(dst_idx, dst)| (dst_idx, dst.name.clone()))
            .collect();
        if !panel.scripts.is_empty() && !targets.is_empty() {
            let mut rows = v_flex().gap_2();
            for script in &panel.scripts {
                let mut row = h_flex().items_center().gap_2().child(
                    div()
                        .w(px(120.))
                        .flex_none()
                        .truncate()
                        .text_xs()
                        .text_color(t.colors.foreground)
                        .child(script.name.clone()),
                );
                for (dst_idx, dst_name) in &targets {
                    let script_name = script.name.clone();
                    let dst_idx = *dst_idx;
                    row = row.child(
                        Button::new(SharedString::from(format!(
                            "sync-{idx}-{dst_idx}-{}",
                            script.name
                        )))
                        .label(dst_name.clone())
                        .with_size(Size::XSmall)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.sync_script(idx, &script_name, dst_idx, cx)
                        })),
                    );
                }
                rows = rows.child(row);
            }
            body = body.child(section_divider(cx));
            body = body.child(
                v_flex()
                    .gap_2()
                    .child(section_title("同步到其他播放器", cx))
                    .child(rows),
            );
        }

        body.into_any_element()
    }

    // -----------------------------------------------------------------------
    // 未接入页面
    // -----------------------------------------------------------------------

    fn render_pending(&self, cx: &mut Context<Self>) -> AnyElement {
        let page = self.page;
        let hint = page.milestone().unwrap_or("该模块尚未接入。");
        let action = Button::new("back-overview")
            .label("返回概览")
            .with_size(Size::Small)
            .on_click(cx.listener(|this, _, _, cx| {
                this.page = Page::Overview;
                cx.notify();
            }))
            .into_any_element();
        empty_state(page.label(), hint, Some(action), cx).into_any_element()
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.page {
            Page::Overview => self.render_overview(cx),
            Page::Players => self.render_players(cx),
            _ => self.render_pending(cx),
        };
        let top_bar = self.render_title_bar(window, cx);
        let sidebar = self.render_sidebar(cx);
        let header = self.render_page_header(cx);
        let t = cx.theme();
        let g = theme::glass(cx);

        // 背景层（规范 §4.1 第 1 层）：低饱和斜向渐变 + 两处柔和光斑。
        // 半透明玻璃表面需要有层次可透，纯色底会让材质看起来仍是平的。
        // 光斑用多层同心圆叠出衰减，替代 GPUI 缺失的背景模糊。
        let background = linear_gradient(
            150.,
            linear_color_stop(theme::color(g.background_start), 0.),
            linear_color_stop(theme::color(g.background_end), 1.),
        );

        // 根容器显式铺满并给定底色与默认文字色：文字色沿样式树继承到子节点，
        // 这是上一版「白底浅色字」的直接成因。
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(background)
            .text_color(t.colors.foreground)
            // 两处柔光：右上为主（透过卡片可见），左下为辅。都压在窗口外沿，
            // 只露出一段弧，避免在空白区形成「聚光灯」。
            .children(glow(px(460.), g.glow, px(10.), px(820.)))
            .children(glow(px(420.), g.glow, px(620.), px(-170.)))
            .child(
                v_flex().size_full().child(top_bar).child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .items_stretch()
                        .child(sidebar)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .child(header)
                                // 滚动容器：内容区高度受窗口约束，超长列表在此滚动。
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .px_6()
                                        .py_4()
                                        .overflow_y_scrollbar()
                                        .child(content),
                                ),
                        ),
                ),
            )
    }
}
