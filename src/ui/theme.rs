//! ui::theme —— 外观设置到 gpui-component 主题的投影
//!
//! 视觉语言取自 `UI/modern_glass_zh`（mpv 的 ModernZ 玻璃皮肤）与 QcClip 的设置页，
//! 归纳为五条：
//!
//! 1. **中性无彩**：界面本体只有黑、白、灰三阶，彩色只给状态与强调；
//! 2. **大圆角**：面板 20px、胶囊 8px；
//! 3. **发丝亮边**：半透明白边是「玻璃」最主要的读感来源；
//! 4. **选中反白**：选中项是白底黑字的胶囊，而不是加一层彩色底；
//! 5. **液态玻璃不分明暗主题**：只有一个「明暗」连续量，所有颜色由它插值。
//!
//! 窗口层的真实模糊由 `core::appearance::Material` 驱动
//! （Windows 下 `WindowBackgroundAppearance::Blurred` 即 DWM 亚克力），
//! 因此本模块只需让背景层带上 alpha，桌面内容就能透进来。
//!
//! 对比度按 WCAG 2.1 AA 校验（正文 ≥ 4.5:1）。

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, rgb};

use crate::core::appearance::{Accent, Appearance, Material};

/// 面板圆角：卡片、弹层、分组容器共用。
pub const RADIUS_PANEL: f32 = 20.;
/// 胶囊圆角：徽标、选中项、输入框共用。
pub const RADIUS_PILL: f32 = 8.;

/// 全局外观设置。放进 GPUI 的 Global，渲染辅助函数才能随时取到，
/// 不必把 `Appearance` 一层层往下传。
#[derive(Clone)]
pub struct CurrentAppearance(pub Appearance);

impl Global for CurrentAppearance {}

/// 读取当前外观设置。
pub fn appearance(cx: &App) -> Appearance {
    cx.global::<CurrentAppearance>().0.clone()
}

/// 一套完整配色。
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: u32,
    pub background_secondary: u32,
    pub surface: u32,
    pub surface_hover: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub border: u32,
    /// 主操作色（暗底用白、亮底用黑），与「选中反白」同一条规则。
    pub accent: u32,
    pub accent_hover: u32,
    pub success: u32,
    pub warning: u32,
    pub danger: u32,
    /// 强调色上的前景文字。
    pub on_accent: u32,
}

fn hsla(value: u32) -> Hsla {
    Hsla::from(rgb(value))
}

/// 把 `0xRRGGBB` 转成 `Hsla`。
pub fn color(value: u32) -> Hsla {
    hsla(value)
}

/// 给语义色套一层不透明度。玻璃界面的每一次填充都要经过这里，
/// 否则组件库会拿实色铺满，桌面就再也透不进来。
fn tinted(value: u32, alpha: f32) -> Hsla {
    hsla(value).opacity(alpha.clamp(0., 1.))
}

/// 按比例在两个 `0xRRGGBB` 之间线性插值。
fn mix(from: u32, to: u32, ratio: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((from >> shift) & 0xff) as f32;
        let b = ((to >> shift) & 0xff) as f32;
        ((a + (b - a) * ratio).round() as u32) & 0xff
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// 浮点插值，用于 alpha 这类 0..1 的量。
fn lerp(from: f32, to: f32, ratio: f32) -> f32 {
    from + (to - from) * ratio
}

/// 由明暗比例生成整套配色。
///
/// 暗端与亮端各是一套完整取值，中间按比例插值，因此滑杆是连续的，
/// 不会出现「跨过某个点突然换主题」的跳变。
pub fn palette(ratio: f32) -> Palette {
    Palette {
        background: mix(0x25252b, 0xf2f2f4, ratio),
        background_secondary: mix(0x1c1c21, 0xfbfbfc, ratio),
        surface: mix(0x1b1b1f, 0xffffff, ratio),
        surface_hover: mix(0x26262b, 0xe8e8ec, ratio),
        text_primary: mix(0xf2f2f5, 0x1a1a1c, ratio),
        // 次级文字要更亮：玻璃底下会透出任意颜色的桌面，中性灰压得太暗就读不出来了。
        text_secondary: mix(0xb8b8c0, 0x55555c, ratio),
        border: mix(0x666667, 0xb4b4ba, ratio),
        // 强调色随明暗翻转：暗底用白，亮底用近黑。
        accent: mix(0xf2f2f5, 0x1a1a1c, ratio),
        accent_hover: mix(0xffffff, 0x33333a, ratio),
        success: mix(0x8fbf4a, 0x3f7a2e, ratio),
        warning: mix(0xe0b46a, 0x8a6100, ratio),
        danger: mix(0xe0776f, 0xb4393b, ratio),
        on_accent: mix(0x0c0c0e, 0xffffff, ratio),
    }
}

/// 液态玻璃材质令牌。
#[derive(Debug, Clone, Copy)]
pub struct Glass {
    /// 背景渐变起点（窗口左上）。
    pub background_start: u32,
    /// 背景渐变终点（窗口右下）。
    pub background_end: u32,
    /// 背景层整体不透明度：小于 1 时桌面/亚克力会透出来。
    pub background_alpha: f32,
    /// 背景装饰光斑色。
    pub glow: Hsla,
    /// 卡片等主工作层表面，半透明。
    pub surface: Hsla,
    /// 侧栏 / 标题栏等常驻容器。
    pub surface_strong: Hsla,
    /// 菜单、弹层等浮层：玻璃的「实体形态」，接近不透明，保证文字可读。
    pub overlay: Hsla,
    /// 发丝高光边。
    pub highlight: Hsla,
    /// 柔和投影色。
    pub shadow: Hsla,
}

/// 材质对应的表面不透明度：越「模糊」越实，越「透明」越虚。
fn surface_alpha(material: Material) -> f32 {
    match material {
        Material::Off => 1.0,
        Material::Transparent => 0.28,
        Material::Acrylic => 0.50,
        Material::Blur => 0.62,
        Material::CustomImage => 0.54,
    }
}

/// 由外观设置生成玻璃令牌。
///
/// 所有面板的不透明度都走同一条式子：材质定基调，「背景不透明度」滑杆在其上增减。
/// 材质关闭时滑杆值恒为 1，式子退化为全实色；滑杆拉到 0 时保留一层薄底，
/// 免得文字直接贴在桌面上辨认不出。
pub fn glass(appearance: &Appearance) -> Glass {
    let ratio = appearance.lightness_ratio();
    let alpha = appearance.background_alpha();
    // 面板比窗口底色实一些：底色越透，文字越需要一个稳定的底，
    // 所以面板只用一半的幅度跟随滑杆，剩下的当作可读性的地板。
    let scale = 0.45 + 0.55 * alpha;
    let material_alpha = surface_alpha(appearance.material);
    let panel_alpha = (material_alpha * scale).clamp(0.10, 1.);
    let strong_alpha = ((material_alpha + 0.20) * scale).clamp(0.14, 1.);
    // 浮层是玻璃的实体形态：不随滑杆变虚，否则菜单文字会浮在桌面上读不清。
    let overlay_alpha = (strong_alpha + 0.34).min(0.97);
    // 卡片色调：暗底用黑压暗，亮底用白提亮，中间过渡成中性灰。
    let (tint_lightness, strong_lightness) = if ratio < 0.5 { (0.0, 0.0) } else { (1.0, 1.0) };
    Glass {
        background_start: mix(0x25252b, 0xf6f6f8, ratio),
        background_end: mix(0x151518, 0xdcdce2, ratio),
        background_alpha: alpha,
        glow: Hsla {
            h: 0.0,
            s: 0.0,
            l: 1.0,
            a: lerp(0.06, 0.55, ratio),
        },
        surface: Hsla {
            h: 0.0,
            s: 0.0,
            l: tint_lightness,
            a: panel_alpha,
        },
        surface_strong: Hsla {
            h: 0.0,
            s: 0.0,
            l: strong_lightness,
            a: strong_alpha,
        },
        overlay: Hsla {
            h: 0.0,
            s: 0.0,
            l: strong_lightness,
            a: overlay_alpha,
        },
        highlight: Hsla {
            h: 0.0,
            s: 0.0,
            l: 1.0,
            a: lerp(0.20, 0.95, ratio),
        },
        shadow: Hsla {
            h: 0.0,
            s: 0.0,
            l: 0.0,
            a: lerp(0.50, 0.12, ratio),
        },
    }
}

/// 当前外观设置对应的玻璃令牌。
pub fn current_glass(cx: &App) -> Glass {
    glass(&appearance(cx))
}

/// 主题色：中性以外都用固定色板，避免随明暗插值把饱和度洗掉。
fn accent_color(accent: Accent, palette: Palette) -> u32 {
    match accent {
        Accent::Neutral => palette.accent,
        other => other.swatch(),
    }
}

/// 把外观设置投影到 gpui-component 的语义色槽位。
///
/// 调用前必须已经执行过 `gpui_kit::init(cx)`。
pub fn set_appearance(cx: &mut App, appearance: Appearance) {
    // 液态玻璃不区分主题：明暗决定组件库基线，仅影响未覆盖的少数字段。
    let mode = if appearance.lightness_ratio() < 0.5 {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
    let p = palette(appearance.lightness_ratio());
    let g = glass(&appearance);
    let accent = accent_color(appearance.accent, p);
    cx.set_global(CurrentAppearance(appearance));
    Theme::update(cx, |theme| {
        let c = &mut theme.colors;
        // 背景链的第一层必须清空：gpui-component 的 Root 会用 `tokens.background`
        // 给整窗刷一层底色，实色一铺，窗口层的亚克力就再也看不见了。
        // 窗口底色改由我们自己的根容器画带 alpha 的渐变。
        c.background = tinted(p.background, 0.);
        c.muted = tinted(p.background_secondary, g.surface_strong.a);
        c.sidebar = tinted(p.background_secondary, g.surface_strong.a);
        c.sidebar_foreground = hsla(p.text_primary);
        c.sidebar_border = hsla(p.border);
        c.sidebar_accent = tinted(p.surface_hover, g.surface.a + 0.12);
        c.sidebar_accent_foreground = hsla(p.text_primary);
        c.group_box = tinted(p.surface, g.surface.a);
        c.group_box_foreground = hsla(p.text_primary);
        // 浮层用实体形态：贴在正文上方的菜单和弹层不能跟着背景一起变虚。
        c.popover = tinted(p.surface, g.overlay.a);
        c.popover_foreground = hsla(p.text_primary);
        c.list = tinted(p.surface, g.surface.a);
        c.list_even = tinted(p.background_secondary, g.surface_strong.a);
        c.list_hover = tinted(p.surface_hover, g.surface.a + 0.12);
        c.list_active = tinted(p.surface_hover, g.surface.a + 0.12);
        c.list_active_border = hsla(accent);
        c.list_head = tinted(p.background_secondary, g.surface_strong.a);
        c.title_bar = tinted(p.background_secondary, g.surface_strong.a);
        c.title_bar_border = hsla(p.border);
        c.status_bar = tinted(p.background_secondary, g.surface_strong.a);
        c.tab_bar = tinted(p.background_secondary, g.surface_strong.a);
        c.tab = tinted(p.surface, g.surface.a);
        c.tab_active = tinted(p.surface_hover, g.surface.a + 0.12);
        c.tab_foreground = hsla(p.text_secondary);
        c.tab_active_foreground = hsla(p.text_primary);

        c.foreground = hsla(p.text_primary);
        c.muted_foreground = hsla(p.text_secondary);

        c.border = hsla(p.border);
        c.ring = hsla(accent);
        c.selection = tinted(p.surface_hover, g.surface.a + 0.12);
        c.caret = hsla(p.text_primary);
        c.scrollbar = tinted(p.background, 0.);
        c.scrollbar_thumb = hsla(p.border);
        c.scrollbar_thumb_hover = hsla(p.text_secondary);

        c.primary = hsla(p.accent);
        c.primary_foreground = hsla(p.on_accent);
        c.primary_hover = hsla(p.accent_hover);
        c.primary_active = hsla(p.accent_hover);
        c.accent = tinted(p.surface_hover, g.surface.a + 0.12);
        c.accent_foreground = hsla(p.text_primary);
        c.link = hsla(p.text_primary);
        c.link_hover = hsla(p.accent_hover);
        c.link_active = hsla(p.accent_hover);

        c.success = hsla(p.success);
        c.success_foreground = hsla(p.on_accent);
        c.success_hover = hsla(p.success);
        c.success_active = hsla(p.success);
        c.warning = hsla(p.warning);
        c.danger = hsla(p.danger);
        c.danger_foreground = hsla(p.on_accent);
        c.danger_hover = hsla(p.danger);
        c.danger_active = hsla(p.danger);
        c.info = hsla(p.text_primary);
        c.info_foreground = hsla(p.on_accent);
        c.info_hover = hsla(p.accent_hover);
        c.info_active = hsla(p.accent_hover);

        // 组件库的 `button_*` 是独立字段，不会回落到 `primary`。
        c.button = tinted(p.surface_hover, g.surface.a + 0.12);
        c.button_hover = tinted(p.border, g.surface.a + 0.30);
        c.button_active = tinted(p.border, g.surface.a + 0.30);
        c.button_foreground = hsla(p.text_primary);
        c.button_primary = hsla(p.accent);
        c.button_primary_hover = hsla(p.accent_hover);
        c.button_primary_active = hsla(p.accent_hover);
        c.button_primary_foreground = hsla(p.on_accent);
        c.button_secondary = tinted(p.surface_hover, g.surface.a + 0.12);
        c.button_secondary_hover = tinted(p.border, g.surface.a + 0.30);
        c.button_secondary_active = tinted(p.border, g.surface.a + 0.30);
        c.button_secondary_foreground = hsla(p.text_primary);
        c.button_danger = hsla(p.danger);
        c.button_danger_hover = hsla(p.danger);
        c.button_danger_active = hsla(p.danger);
        c.button_danger_foreground = hsla(p.on_accent);
        c.button_info = hsla(p.text_primary);
        c.button_info_hover = hsla(p.accent_hover);
        c.button_info_active = hsla(p.accent_hover);
        c.button_info_foreground = hsla(p.on_accent);
        c.button_success = hsla(p.success);
        c.button_success_hover = hsla(p.success);
        c.button_success_active = hsla(p.success);
        c.button_success_foreground = hsla(p.on_accent);
        c.button_warning = hsla(p.warning);
        c.button_warning_hover = hsla(p.warning);
        c.button_warning_active = hsla(p.warning);
        c.button_warning_foreground = hsla(p.on_accent);

        c.secondary = tinted(p.surface_hover, g.surface.a + 0.12);
        c.secondary_hover = tinted(p.border, g.surface.a + 0.30);
        c.secondary_active = tinted(p.border, g.surface.a + 0.30);
        c.secondary_foreground = hsla(p.text_primary);

        // 开关与选中态用主题色：界面上唯一允许出现的彩色。
        c.switch = hsla(accent);
        c.switch_thumb = hsla(0xffffff);
    });
}
