//! ui —— Névé 主界面（GPUI + gpui-component）
//!
//! 布局依据 `docs/UI设计规范.md` §3：顶栏 + 常驻侧栏 + 内容区 + 详情区。
//! 颜色一律取自 `ui::theme` 投影后的语义令牌（`cx.theme().colors.*`），
//! 不在页面里硬编码 RGB，从而保证昼夜主题、对比度与后续皮肤能力一致。
//!
//! 视图模型与交互动作在此聚合；core 层只提供纯逻辑与文件操作。

mod theme;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, InteractiveElementExt as _, Sizable as _, Size, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, BoxShadow, Context, Div, ElementId, FontWeight, Hsla, ImageSource,
    IntoElement, MouseButton, ObjectFit, ParentElement as _, PathPromptOptions, Pixels, Render,
    Resource, SharedString, Styled as _, Window, WindowBackgroundAppearance, div, img,
    linear_color_stop, linear_gradient, point, prelude::*, px,
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

pub use theme::set_appearance as apply_theme;

use crate::core::appearance::{
    Accent, Appearance, FontChoice, Material, TabLayout, TitlebarPosition,
};

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
    let g = theme::current_glass(cx);
    v_flex()
        .gap_3()
        .p_4()
        .rounded(px(theme::RADIUS_PANEL))
        .bg(g.surface)
        .border_1()
        .border_color(g.highlight)
        .shadow(vec![
            // 皮肤的投影很收敛（blur 4），这里放大到 24 是因为卡片本身很大，
            // 否则浮不起来；透明度同步压低，避免发灰。
            BoxShadow {
                color: g.shadow,
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-6.),
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
///
/// 用弱化的 `border`：皮肤里的分隔线是「玻璃上的细线」（#6A6A72 带透明度），
/// 直接铺满不透明边框会把卡片切成两块。
fn section_divider(cx: &App) -> Div {
    div()
        .pt_3()
        .border_t_1()
        .border_color(cx.theme().colors.border.opacity(0.45))
}

/// 小徽标。文字与底色配对，保证任意主题下对比度可读。
fn badge(label: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    h_flex()
        .h(px(18.))
        .px_2()
        .items_center()
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .text_xs()
        .child(label.into())
}

/// 标题栏菜单。参考软件把 编辑 / 窗口 / 帮助 / 关于 放在左上角，
/// 这里保留同样的四项，动作接到已有能力上。
#[derive(Clone, Copy, PartialEq, Eq)]
enum TopMenu {
    Edit,
    Window,
    Help,
    About,
}

impl TopMenu {
    const ALL: [TopMenu; 4] = [
        TopMenu::Edit,
        TopMenu::Window,
        TopMenu::Help,
        TopMenu::About,
    ];

    fn label(&self) -> &'static str {
        match self {
            TopMenu::Edit => "编辑(E)",
            TopMenu::Window => "窗口(W)",
            TopMenu::Help => "帮助(H)",
            TopMenu::About => "关于(A)",
        }
    }
}

/// 标题栏上的图标按钮（面板开关 / 搜索 / 筛选）。
///
/// 用字形而不是图标组件：组件库的 SVG 图标在资源缺失时会渲染成空白，
/// 窗口控制按钮已经踩过一次，这里保持一致。
fn toolbar_icon(
    id: &'static str,
    glyph: &'static str,
    fg: Hsla,
    hover_bg: Hsla,
    hover_fg: Hsla,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::Stateful<Div> {
    h_flex()
        .id(ElementId::Name(SharedString::from(id)))
        .w(px(26.))
        .h(px(24.))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_sm()
        .text_color(fg)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
        .on_click(on_click)
        .child(glyph)
}

/// 材质到窗口背景外观的映射。窗口创建与运行时切换共用。
pub fn window_background(material: Material) -> WindowBackgroundAppearance {
    match material {
        // 自定义背景图也要保留合成：图片缺失或加载失败时窗口才不会整块变黑。
        Material::Off => WindowBackgroundAppearance::Opaque,
        Material::Transparent => WindowBackgroundAppearance::Transparent,
        Material::Acrylic | Material::Blur | Material::CustomImage => {
            WindowBackgroundAppearance::Blurred
        }
    }
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
                .font_weight(FontWeight::SEMIBOLD)
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
    /// 外观设置：材质、明暗、字体、布局都在这里，改动即存盘并重投影。
    appearance: Appearance,
    /// 已展开完整脚本列表的播放器下标（默认只显示前几条）。
    expanded_scripts: Vec<usize>,
    /// 标题栏当前展开的菜单项。
    open_menu: Option<TopMenu>,
}

impl AppShell {
    pub fn new(
        players: Vec<DetectedPlayer>,
        panels: Vec<PlayerPanel>,
        appearance: Appearance,
    ) -> Self {
        Self {
            players,
            panels,
            policy: ConflictPolicy::Skip,
            status: None,
            page: Page::Overview,
            appearance,
            expanded_scripts: Vec::new(),
            open_menu: None,
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

    /// 不涉及窗口材质的设置改动（布局、字体、主题色等）。
    fn apply_without_window(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Appearance),
    ) {
        change(&mut self.appearance);
        theme::set_appearance(cx, self.appearance.clone());
        if let Err(error) = self.appearance.save() {
            self.status = Some(StatusMessage::warning(format!("外观设置保存失败：{error}")));
        }
        cx.notify();
    }

    /// 标题栏的「面板」图标：在侧栏与顶栏两种 Tab 布局之间切换。
    fn toggle_tab_layout(&mut self, cx: &mut Context<Self>) {
        let next = match self.appearance.tab_layout {
            TabLayout::Sidebar => TabLayout::TopBar,
            TabLayout::TopBar => TabLayout::Sidebar,
        };
        self.apply_without_window(cx, |appearance| appearance.tab_layout = next);
    }

    /// 改一项外观设置：立即重投影主题、同步窗口材质，并写回磁盘。
    ///
    /// 写盘失败不打断交互（用户正在拖滑杆），只在状态栏提示一次。
    fn update_appearance(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Appearance),
    ) {
        change(&mut self.appearance);
        theme::set_appearance(cx, self.appearance.clone());
        // 窗口材质是平台能力，改完要立刻同步给窗口。
        window.set_background_appearance(window_background(self.appearance.material));
        if let Err(error) = self.appearance.save() {
            self.status = Some(StatusMessage::warning(format!("外观设置保存失败：{error}")));
        }
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
        let g = theme::current_glass(cx);
        let fg = t.colors.foreground;
        let muted = t.colors.muted_foreground;
        let hover_bg = t.colors.accent;
        let danger_bg = t.colors.danger;
        let danger_fg = t.colors.primary_foreground;
        let maximize_glyph = if window.is_maximized() { "❐" } else { "▢" };
        let divider = t.colors.border.opacity(0.45);
        let accent_bg = t.colors.accent;

        // 左簇：面板开关 / 搜索 / 筛选 三个图标按钮 + 四个文字菜单。
        // 这是参考软件的窗口左上角结构，保留 Névé 标识在其后。
        let mut left = h_flex().flex_none().items_center().gap_1().pl_2();
        left = left.child(toolbar_icon(
            "tb-sidebar",
            "☰",
            muted,
            hover_bg,
            fg,
            cx.listener(|this, _, _, cx| {
                this.toggle_tab_layout(cx);
            }),
        ));
        left = left.child(toolbar_icon(
            "tb-search",
            "⌕",
            muted,
            hover_bg,
            fg,
            cx.listener(|this, _, _, cx| {
                this.page = Page::Settings;
                this.status = Some(StatusMessage::info("搜索框随设置页一起接入。"));
                cx.notify();
            }),
        ));
        left = left.child(toolbar_icon(
            "tb-filter",
            "▽",
            muted,
            hover_bg,
            fg,
            cx.listener(|this, _, _, cx| this.cycle_policy(cx)),
        ));
        left = left.child(div().w(px(1.)).h(px(16.)).flex_none().bg(divider));
        for menu in TopMenu::ALL {
            let active = self.open_menu == Some(menu);
            let mut item = h_flex()
                .id(ElementId::Name(SharedString::from(format!(
                    "menu-{}",
                    menu.label()
                ))))
                .relative()
                .h(px(24.))
                .px_2()
                .items_center()
                .rounded(px(6.))
                .text_xs()
                .text_color(if active { fg } else { muted })
                .cursor_pointer()
                .when(active, move |this| this.bg(accent_bg))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_menu = if this.open_menu == Some(menu) {
                        None
                    } else {
                        Some(menu)
                    };
                    cx.notify();
                }))
                .child(menu.label());
            if active {
                item = item.child(self.render_menu_popup(menu, window, cx));
            }
            left = left.child(item);
        }

        h_flex()
            .flex_none()
            .h(px(34.))
            .items_center()
            .gap_2()
            // 与侧栏同一层玻璃：常驻容器比卡片更实，靠边框与内容区分层。
            .bg(g.surface_strong)
            .border_b_1()
            .border_color(g.highlight)
            .child(left)
            // 拖动区：图标与菜单之后到右侧控件之前，与真实标题栏行为一致。
            .child(
                h_flex()
                    .id("titlebar-drag")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap_2()
                    .pl_2()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                    .on_double_click(|_, window, _| window.zoom_window())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child("Névé"),
                    )
                    .child(div().text_xs().text_color(muted).child("粒雪 · MPV 管理器")),
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

    /// 菜单弹出层。挂在菜单按钮内部（按钮自身 `relative`），
    /// 因此天然对齐到按钮左边缘，不需要手工算偏移。
    fn render_menu_popup(
        &self,
        menu: TopMenu,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = cx.theme();
        let g = theme::current_glass(cx);
        let mut panel = v_flex()
            .absolute()
            .top(px(26.))
            .left(px(0.))
            .w(px(168.))
            .p_1()
            .gap_1()
            .rounded(px(10.))
            // 浮层取玻璃的实体形态：贴在正文上方，不能跟着背景一起变虚。
            .bg(g.overlay)
            .border_1()
            .border_color(g.highlight)
            .shadow(vec![BoxShadow {
                color: g.shadow,
                offset: point(px(0.), px(8.)),
                blur_radius: px(20.),
                spread_radius: px(-4.),
                inset: false,
            }])
            .text_color(t.colors.foreground);

        let entry = |label: &'static str, disabled: bool| {
            let fg = if disabled {
                t.colors.muted_foreground
            } else {
                t.colors.foreground
            };
            h_flex()
                .h(px(26.))
                .px_2()
                .items_center()
                .rounded(px(6.))
                .text_xs()
                .text_color(fg)
                .child(label)
        };

        match menu {
            TopMenu::Edit => {
                panel = panel.child(entry("撤销最近一次写入", false));
                panel = panel.child(entry("复制播放器清单", true));
            }
            TopMenu::Window => {
                panel = panel.child(entry("最小化", false));
                panel = panel.child(entry("最大化 / 还原", false));
                panel = panel.child(entry("关闭", false));
            }
            TopMenu::Help => {
                panel = panel.child(entry("界面说明", false));
                panel = panel.child(entry("更新日志", true));
            }
            TopMenu::About => {
                panel = panel.child(entry("Névé 粒雪", false));
                panel = panel.child(entry(env!("CARGO_PKG_VERSION"), true));
            }
        }
        panel
    }

    // -----------------------------------------------------------------------
    // 侧栏
    // -----------------------------------------------------------------------

    /// 顶栏 Tab 布局：主导航变成标题栏下方的横向胶囊条。
    fn render_tab_strip(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let g = theme::current_glass(cx);
        let mut strip = h_flex().gap_1().items_center().px_4().py_2();
        for page in Page::ALL {
            let selected = page == self.page;
            let fg = if selected {
                t.colors.primary_foreground
            } else {
                t.colors.muted_foreground
            };
            let hover = t.colors.secondary;
            let mut tab = h_flex()
                .id(ElementId::Name(SharedString::from(format!(
                    "tab-{}",
                    page.label()
                ))))
                .h(px(26.))
                .px_3()
                .items_center()
                .rounded(px(theme::RADIUS_PILL))
                .text_xs()
                .text_color(fg)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
                .child(page.label());
            if selected {
                tab = tab.bg(t.colors.primary);
            } else {
                tab = tab.hover(move |style| style.bg(hover));
            }
            strip = strip.child(tab);
        }
        strip
            .bg(g.surface_strong)
            .border_b_1()
            .border_color(g.highlight)
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let mut nav = v_flex().gap_1();
        for page in Page::ALL {
            let selected = page == self.page;
            // 选中项反白：白底黑字（深色）/ 黑底白字（浅色）。
            let fg = if selected {
                t.colors.primary_foreground
            } else {
                t.colors.sidebar_foreground
            };
            let mut item = h_flex()
                .id(ElementId::Name(SharedString::from(format!(
                    "nav-{}",
                    page.label()
                ))))
                .h(px(32.))
                .px_3()
                .gap_2()
                .items_center()
                .rounded(px(theme::RADIUS_PILL))
                .text_sm()
                .text_color(fg)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
                .child(div().text_color(fg).child(page.label()));
            if selected {
                // modern_glass 的选中态是「反白胶囊」，不叠彩色底、不加左侧标记条。
                item = item.bg(t.colors.primary);
            } else {
                // 悬停底色弱于选中底色，避免两者混淆。
                let hover = t.colors.sidebar_accent.opacity(0.55);
                item = item.hover(move |style| style.bg(hover));
            }
            nav = nav.child(item);
        }

        let g = theme::current_glass(cx);
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
                .font_weight(FontWeight::SEMIBOLD)
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

        head.border_b_1()
            .border_color(theme::current_glass(cx).highlight)
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
                .font_weight(FontWeight::SEMIBOLD)
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

    // -----------------------------------------------------------------------
    // 设置页（外观）
    // -----------------------------------------------------------------------

    /// 百分比行：减号 + 轨道 + 加号 + 数值。
    ///
    /// 没做拖拽：GPUI 里要拿到指针相对轨道的坐标得额外维护 bounds，
    /// 加减按钮更稳，且同样能到任意值。
    fn percent_control(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        value: u8,
        set: fn(&mut Appearance, u8),
    ) -> AnyElement {
        let t = cx.theme();
        let muted = t.colors.muted_foreground;
        let track = t.colors.border.opacity(0.35);
        let fill_color = t.colors.primary;
        let fg = t.colors.foreground;
        let hover = t.colors.secondary;
        const TRACK: f32 = 180.;
        let fill = (TRACK * (f32::from(value) / 100.)).max(2.);

        let minus = h_flex()
            .id(ElementId::Name(SharedString::from(format!("{id}-minus"))))
            .w(px(26.))
            .h(px(24.))
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_sm()
            .text_color(muted)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                let next = value.saturating_sub(5);
                this.update_appearance(window, cx, |appearance| set(appearance, next));
            }))
            .child("−");
        let plus = h_flex()
            .id(ElementId::Name(SharedString::from(format!("{id}-plus"))))
            .w(px(26.))
            .h(px(24.))
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_sm()
            .text_color(muted)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                let next = (value + 5).min(100);
                this.update_appearance(window, cx, |appearance| set(appearance, next));
            }))
            .child("+");

        h_flex()
            .items_center()
            .gap_3()
            .child(minus)
            .child(
                div()
                    .w(px(TRACK))
                    .h(px(4.))
                    .flex_none()
                    .rounded_full()
                    .bg(track)
                    .child(div().w(px(fill)).h(px(4.)).rounded_full().bg(fill_color)),
            )
            .child(plus)
            .child(
                div()
                    .w(px(38.))
                    .text_xs()
                    .text_color(fg)
                    .child(format!("{value}%")),
            )
            .into_any_element()
    }

    /// 主题色板。
    fn accent_swatches(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme();
        let ring = t.colors.foreground;
        let current = self.appearance.accent;
        let mut row = h_flex().items_center().gap_2();
        for accent in Accent::ALL {
            let selected = accent == current;
            let color = theme::color(accent.swatch());
            let mut dot = h_flex()
                .id(ElementId::Name(SharedString::from(format!(
                    "accent-{}",
                    accent.label()
                ))))
                .w(px(22.))
                .h(px(22.))
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(color)
                .cursor_pointer()
                .when(selected, move |this| this.border_2().border_color(ring))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.update_appearance(window, cx, |appearance| appearance.accent = accent);
                }));
            if selected {
                dot = dot.child(div().text_xs().text_color(theme::color(0x000000)).child(""));
            }
            row = row.child(dot);
        }
        row.into_any_element()
    }

    /// 自定义背景图：铺在渐变之下，占满整窗。
    ///
    /// 图片缺失或解码失败时 `img` 什么都不画，窗口仍由系统合成兜底，不会整块变黑。
    fn background_layer(&self, g: theme::Glass) -> Option<AnyElement> {
        let path = self.appearance.background_image.trim();
        if self.appearance.material != Material::CustomImage || path.is_empty() {
            return None;
        }
        let source = ImageSource::Resource(Resource::Path(Arc::from(Path::new(path))));
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    img(source)
                        .w_full()
                        .h_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(g.background_alpha),
                )
                .into_any_element(),
        )
    }

    /// 选一张图片做窗口背景。系统文件对话框异步返回，结果回到主线程再落盘。
    fn choose_background_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择窗口背景图片".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            // 外层 `Result` 是通道本身被取消，内层才是文件对话框的结果。
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // 对话框可能被取消，也可能实体已释放；这两种情况都不值得打断界面。
            let _ = this.update_in(cx, |this, window, cx| {
                this.update_appearance(window, cx, |appearance| {
                    appearance.material = Material::CustomImage;
                    appearance.background_image = path.to_string_lossy().into_owned();
                });
            });
        })
        .detach();
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let a = self.appearance.clone();
        let mut body = v_flex().gap_4();

        // 控件先各自构造成局部变量，再交给 setting_row：
        // 若写成 setting_row(cx, …, self.segmented(cx, …))，参数从左到右求值，
        // `cx` 会先被不可变借用，后面需要可变借用就冲突了。
        let font_control = segmented(
            cx,
            "font",
            &FontChoice::ALL,
            a.font,
            |value| value.label(),
            |_| true,
            |appearance, value| appearance.font = value,
        );
        let material_control = segmented(
            cx,
            "material",
            &Material::ALL,
            a.material,
            |value| value.label(),
            |_| true,
            |appearance, value| appearance.material = value,
        );
        let background_path = if a.background_image.trim().is_empty() {
            SharedString::from("未设置")
        } else {
            SharedString::from(a.background_image.trim().to_string())
        };
        let image_control = h_flex()
            .gap_2()
            .child(
                Button::new("pick-background")
                    .label("选择…")
                    .with_size(Size::Small)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_background_image(window, cx);
                    })),
            )
            .child(
                div()
                    .max_w(px(260.))
                    .overflow_hidden()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(background_path),
            )
            .into_any_element();
        let opacity_control = self.percent_control(
            cx,
            "bg-opacity",
            a.background_opacity,
            |appearance, value| {
                appearance.background_opacity = value;
            },
        );
        let lightness_control =
            self.percent_control(cx, "lightness", a.lightness, |appearance, value| {
                appearance.lightness = value;
            });
        let titlebar_control = segmented(
            cx,
            "titlebar",
            &TitlebarPosition::ALL,
            a.titlebar_position,
            |value| value.label(),
            |value| value.implemented(),
            |appearance, value| appearance.titlebar_position = value,
        );
        let tab_control = segmented(
            cx,
            "tablayout",
            &TabLayout::ALL,
            a.tab_layout,
            |value| value.label(),
            |_| true,
            |appearance, value| appearance.tab_layout = value,
        );
        let animations_on = a.animations;
        let animations_control = h_flex()
            .child(
                Switch::new("animations")
                    .checked(animations_on)
                    .with_size(Size::Small)
                    .on_click(cx.listener(move |this, checked, window, cx| {
                        let value = *checked;
                        this.update_appearance(window, cx, |appearance| {
                            appearance.animations = value;
                        });
                    })),
            )
            .into_any_element();
        let accent_control = self.accent_swatches(cx);
        let locked = a.lock_window_color;
        let lock_control = h_flex()
            .child(
                Switch::new("lock-color")
                    .checked(locked)
                    .with_size(Size::Small)
                    .on_click(cx.listener(move |this, checked, window, cx| {
                        let value = *checked;
                        this.update_appearance(window, cx, |appearance| {
                            appearance.lock_window_color = value;
                        });
                    })),
            )
            .into_any_element();
        let version = div()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(env!("CARGO_PKG_VERSION"))
            .into_any_element();

        // --- 外观 ---
        let mut look = card(cx).gap_0();
        look = look.child(section_title("外观", cx));
        look = look.child(setting_row(
            cx,
            "应用字体",
            "选择应用界面字体，修改后立即生效。",
            font_control,
        ));
        look = look.child(section_divider(cx));
        look = look.child(setting_row(
            cx,
            "窗口材质",
            "透明 / 亚克力 / 模糊 需要系统合成支持，Windows 下走 DWM。",
            material_control,
        ));
        look = look.child(section_divider(cx));
        look = look.child(setting_row(
            cx,
            "自定义背景图",
            "选一张图片铺满窗口；选完自动切到「自定义背景图」材质。",
            image_control,
        ));
        look = look.child(section_divider(cx));
        look = look.child(setting_row(
            cx,
            "背景不透明度",
            "数值越低，桌面内容透出得越多；材质为「关闭」时恒为不透明。",
            opacity_control,
        ));
        look = look.child(section_divider(cx));
        look = look.child(setting_row(
            cx,
            "明暗",
            "液态玻璃不再区分深色 / 浅色主题，明暗在这里连续调节。",
            lightness_control,
        ));
        body = body.child(look);

        // --- 布局与动画 ---
        let mut layout = card(cx).gap_0();
        layout = layout.child(section_title("布局与动画", cx));
        layout = layout.child(setting_row(
            cx,
            "标题栏位置",
            "标题栏显示在窗口的顶部或底部；左右两侧的竖排标题栏尚未实现。",
            titlebar_control,
        ));
        layout = layout.child(section_divider(cx));
        layout = layout.child(setting_row(
            cx,
            "Tab 栏布局",
            "主导航放在左侧常驻侧栏，或标题栏下方的横向 Tab 条。",
            tab_control,
        ));
        layout = layout.child(section_divider(cx));
        layout = layout.child(setting_row(
            cx,
            "动画与动效",
            "管理界面中的过渡与微动效。关闭后交互立即切换。",
            animations_control,
        ));
        body = body.child(layout);

        // --- 主题 ---
        let mut theme_card = card(cx).gap_0();
        theme_card = theme_card.child(section_title("主题", cx));
        theme_card = theme_card.child(setting_row(
            cx,
            "主题颜色",
            "只影响开关、进度与强调文字，界面主体保持中性。",
            accent_control,
        ));
        theme_card = theme_card.child(section_divider(cx));
        theme_card = theme_card.child(setting_row(
            cx,
            "锁定窗口颜色",
            "开启后窗口底色不再随「明暗」变化，保持当前取值。",
            lock_control,
        ));
        body = body.child(theme_card);

        // --- 关于 ---
        let mut about = card(cx).gap_0();
        about = about.child(section_title("关于", cx));
        about = about.child(setting_row(
            cx,
            "Névé 粒雪",
            "PCL2 式 MPV 内核播放器管理器。界面风格取自 UI/modern_glass_zh。",
            version,
        ));
        body = body.child(about);

        body.into_any_element()
    }

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

// ---------------------------------------------------------------------------
// 设置页控件
// ---------------------------------------------------------------------------

fn segmented<T: Copy + PartialEq + 'static>(
    cx: &mut Context<AppShell>,
    id_prefix: &'static str,
    all: &'static [T],
    current: T,
    label: fn(&T) -> &'static str,
    enabled: fn(&T) -> bool,
    apply: fn(&mut Appearance, T),
) -> AnyElement {
    let track_bg = cx.theme().colors.secondary;
    let mut row = h_flex().gap_1().p_1().rounded(px(10.)).bg(track_bg);
    for item in all {
        let value = *item;
        let selected = value == current;
        let usable = enabled(&value);
        let mut button = seg_button(
            SharedString::from(format!("{id_prefix}-{}", label(&value))),
            label(&value),
            selected,
            usable,
            cx,
        );
        if usable {
            button = button.on_click(cx.listener(move |this, _, window, cx| {
                this.update_appearance(window, cx, |appearance| apply(appearance, value));
            }));
        }
        row = row.child(button);
    }
    row.into_any_element()
}

/// 一行设置：左侧标题 + 说明，右侧控件。
fn setting_row(
    cx: &App,
    title: &'static str,
    description: &'static str,
    control: AnyElement,
) -> Div {
    let t = cx.theme();
    h_flex()
        .items_center()
        .gap_4()
        .py_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_sm().text_color(t.colors.foreground).child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(t.colors.muted_foreground)
                        .child(description),
                ),
        )
        .child(control)
}

/// 分段控件的一个按钮。
fn seg_button(
    id: SharedString,
    label: &'static str,
    selected: bool,
    enabled: bool,
    cx: &App,
) -> gpui_kit::Stateful<Div> {
    let t = cx.theme();
    let (fg, bg) = if !enabled {
        (t.colors.muted_foreground.opacity(0.6), None)
    } else if selected {
        (t.colors.primary_foreground, Some(t.colors.primary))
    } else {
        (t.colors.foreground, None)
    };
    h_flex()
        .id(ElementId::Name(id))
        .h(px(24.))
        .px_3()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_xs()
        .text_color(fg)
        .when_some(bg, |this, color| this.bg(color))
        .when(enabled && !selected, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(t.colors.secondary))
        })
        .child(label)
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.page {
            Page::Overview => self.render_overview(cx),
            Page::Players => self.render_players(cx),
            Page::Settings => self.render_settings(cx),
            _ => self.render_pending(cx),
        };
        let top_bar = self.render_title_bar(window, cx);
        let header = self.render_page_header(cx);
        let tab_layout = self.appearance.tab_layout;
        let titlebar_at_bottom = self.appearance.titlebar_position == TitlebarPosition::Bottom;
        let sidebar = matches!(tab_layout, TabLayout::Sidebar).then(|| self.render_sidebar(cx));
        let tab_strip = matches!(tab_layout, TabLayout::TopBar).then(|| self.render_tab_strip(cx));
        let font = self.appearance.font.family();
        let t = cx.theme();
        let g = theme::current_glass(cx);

        // 背景层：斜向渐变 + 两处柔和光斑。
        // 背景整体带 alpha：材质为透明/亚克力时窗口层已开启系统合成，
        // 这里留出透明度，桌面内容才能真的透进来。
        let background = linear_gradient(
            150.,
            linear_color_stop(
                theme::color(g.background_start).opacity(g.background_alpha),
                0.,
            ),
            linear_color_stop(
                theme::color(g.background_end).opacity(g.background_alpha),
                1.,
            ),
        );

        let mut main = h_flex().flex_1().min_h_0().items_stretch();
        if let Some(sidebar) = sidebar {
            main = main.child(sidebar);
        }
        let mut column = v_flex().flex_1().min_w_0().min_h_0();
        if let Some(tab_strip) = tab_strip {
            column = column.child(tab_strip);
        }
        column = column
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
            );
        main = main.child(column);

        // 标题栏位置：顶部（默认）或底部。标题栏是同一个元素，只换挂载位置。
        let mut shell = v_flex().size_full();
        shell = if titlebar_at_bottom {
            shell.child(main).child(top_bar)
        } else {
            shell.child(top_bar).child(main)
        };

        // 根容器显式铺满并给定底色与默认文字色：文字色沿样式树继承到子节点。
        // 底色只有薄薄一层渐变，其余交给窗口层的系统合成，桌面才透得出来。
        let root = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(background)
            .text_color(t.colors.foreground)
            .children(glow(px(460.), g.glow, px(10.), px(820.)))
            .children(glow(px(420.), g.glow, px(620.), px(-170.)))
            .when_some(self.background_layer(g), |this, image| this.child(image))
            .child(shell);
        match font {
            Some(family) => root.font_family(family),
            None => root,
        }
    }
}
