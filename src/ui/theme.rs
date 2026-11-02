//! ui::theme —— 设计令牌到 gpui-component 主题的投影
//!
//! 令牌取值来源：`config/ui-theme.toml`（设计基线）。运行时不解析该文件，
//! 而是把同一组取值在此处固化为常量并投影到 `ThemeColor` 语义槽位，
//! 使界面各处只需引用语义色，不再散落硬编码 RGB。
//!
//! 依据 `docs/UI设计规范.md` §4.3：正文、次级文本、边框、焦点、成功、
//! 警告、错误都使用语义令牌。对比度按 WCAG 2.1 AA 校验（正文 ≥ 4.5:1）。
//!
//! 液态玻璃（§4.1）由 `Glass` 承载。GPUI 目前没有背景模糊能力
//! （`Window::blur` 是焦点失焦，与视觉无关），因此按 §6 的回退方案实现：
//! **半透明填充 + 渐变背景 + 内侧高光边（inset shadow）+ 柔和投影**。
//! 四者叠加后仍能读出玻璃材质，只是不做真实折射模糊。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, rgb};

/// 一套完整配色。字段名对齐 `config/ui-theme.toml` 的键名。
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: u32,
    pub background_secondary: u32,
    pub surface: u32,
    pub surface_hover: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub border: u32,
    pub accent: u32,
    /// 强调色的悬停/按下态，比 `accent` 更亮（深色）或更深（浅色）。
    pub accent_hover: u32,
    pub success: u32,
    pub warning: u32,
    pub danger: u32,
    /// 强调色上的前景文字（按钮标签）。
    pub on_accent: u32,
}

/// 深色：蓝黑/石墨底，冰川蓝强调。
pub const DARK: Palette = Palette {
    background: 0x111923,
    background_secondary: 0x172330,
    surface: 0x1c2a38,
    surface_hover: 0x26394a,
    text_primary: 0xe6f0f6,
    text_secondary: 0xa8bbc9,
    border: 0x405566,
    accent: 0x83c5ea,
    accent_hover: 0xa2d8f4,
    success: 0x82d2a8,
    warning: 0xf1c66f,
    danger: 0xff9b9b,
    // 强调色本身是浅蓝，其上的文字用底色深蓝，对比度约 9.6:1。
    on_accent: 0x111923,
};

/// 浅色：雾白到冰蓝的低饱和底，深青蓝强调。
pub const LIGHT: Palette = Palette {
    background: 0xeaf2f7,
    background_secondary: 0xf5f9fc,
    surface: 0xf8fbfd,
    surface_hover: 0xe3eef5,
    text_primary: 0x172b3a,
    text_secondary: 0x526879,
    border: 0xb9ccd9,
    accent: 0x347da5,
    accent_hover: 0x28688e,
    success: 0x287a59,
    warning: 0x946300,
    danger: 0xb43d45,
    on_accent: 0xffffff,
};

fn hsla(value: u32) -> Hsla {
    Hsla::from(rgb(value))
}

/// 把 `0xRRGGBB` 转成 `Hsla`，供界面拼装渐变与装饰层使用。
pub fn color(value: u32) -> Hsla {
    hsla(value)
}

/// 液态玻璃材质令牌。
///
/// `surface` / `surface_strong` 带 alpha：叠在渐变背景上会透出底色，
/// 这是没有背景模糊时「玻璃」读感的主要来源。
#[derive(Debug, Clone, Copy)]
pub struct Glass {
    /// 背景渐变起点（窗口左上）。
    pub background_start: u32,
    /// 背景渐变终点（窗口右下）。
    pub background_end: u32,
    /// 背景装饰光斑色（低 alpha，用来让半透明表面透出层次）。
    pub glow: Hsla,
    /// 卡片等主工作层表面，半透明。
    pub surface: Hsla,
    /// 侧栏 / 标题栏等常驻容器，比卡片更实一些。
    pub surface_strong: Hsla,
    /// 内侧高光边：深色用低透明白，浅色用高透明白。
    pub highlight: Hsla,
    /// 柔和投影色。
    pub shadow: Hsla,
}

/// 深色玻璃：蓝黑底、冰川蓝光斑、白色内高光。
pub const DARK_GLASS: Glass = Glass {
    background_start: 0x0a1219,
    background_end: 0x1c2c3b,
    glow: Hsla {
        h: 0.556,
        s: 0.62,
        l: 0.55,
        a: 0.32,
    },
    surface: Hsla {
        h: 0.578,
        s: 0.34,
        l: 0.165,
        a: 0.58,
    },
    surface_strong: Hsla {
        h: 0.583,
        s: 0.36,
        l: 0.14,
        a: 0.84,
    },
    highlight: Hsla {
        h: 0.0,
        s: 0.0,
        l: 1.0,
        a: 0.18,
    },
    shadow: Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.0,
        a: 0.52,
    },
};

/// 浅色玻璃：雾白到冰蓝底、清透白光斑、白色内高光。
pub const LIGHT_GLASS: Glass = Glass {
    background_start: 0xf2f7fb,
    background_end: 0xcfe0ee,
    glow: Hsla {
        h: 0.556,
        s: 0.55,
        l: 0.72,
        a: 0.42,
    },
    surface: Hsla {
        h: 0.583,
        s: 0.45,
        l: 0.985,
        a: 0.68,
    },
    surface_strong: Hsla {
        h: 0.583,
        s: 0.45,
        l: 0.975,
        a: 0.84,
    },
    highlight: Hsla {
        h: 0.0,
        s: 0.0,
        l: 1.0,
        a: 0.90,
    },
    shadow: Hsla {
        h: 0.57,
        s: 0.32,
        l: 0.31,
        a: 0.16,
    },
};

/// 当前模式对应的玻璃材质。
pub fn glass(cx: &App) -> Glass {
    glass_for(cx.theme().mode)
}

/// 指定模式对应的玻璃材质。
pub fn glass_for(mode: ThemeMode) -> Glass {
    if mode.is_dark() {
        DARK_GLASS
    } else {
        LIGHT_GLASS
    }
}

/// 当前模式对应的配色。
pub fn palette(mode: ThemeMode) -> Palette {
    if mode.is_dark() { DARK } else { LIGHT }
}

/// 把配色投影到 gpui-component 的语义色槽位，并切换到 `mode`。
///
/// 调用前必须已经执行过 `gpui_kit::init(cx)`（其中会初始化主题全局量）。
pub fn apply(cx: &mut App, mode: ThemeMode) {
    Theme::change(mode, None, cx);
    let p = palette(mode);
    Theme::update(cx, |theme| {
        let c = &mut theme.colors;
        // 底色与表面
        c.background = hsla(p.background);
        c.muted = hsla(p.background_secondary);
        c.sidebar = hsla(p.background_secondary);
        c.sidebar_foreground = hsla(p.text_primary);
        c.sidebar_border = hsla(p.border);
        c.sidebar_accent = hsla(p.surface_hover);
        c.sidebar_accent_foreground = hsla(p.text_primary);
        c.group_box = hsla(p.surface);
        c.group_box_foreground = hsla(p.text_primary);
        c.popover = hsla(p.surface);
        c.popover_foreground = hsla(p.text_primary);
        c.list = hsla(p.surface);
        c.list_even = hsla(p.background_secondary);
        c.list_hover = hsla(p.surface_hover);
        c.list_active = hsla(p.surface_hover);
        c.list_active_border = hsla(p.accent);
        c.list_head = hsla(p.background_secondary);
        c.title_bar = hsla(p.background_secondary);
        c.title_bar_border = hsla(p.border);
        c.status_bar = hsla(p.background_secondary);
        c.tab_bar = hsla(p.background_secondary);
        c.tab = hsla(p.surface);
        c.tab_active = hsla(p.surface_hover);
        c.tab_foreground = hsla(p.text_secondary);
        c.tab_active_foreground = hsla(p.text_primary);

        // 文本
        c.foreground = hsla(p.text_primary);
        c.muted_foreground = hsla(p.text_secondary);

        // 描边与焦点
        c.border = hsla(p.border);
        c.ring = hsla(p.accent);
        c.selection = hsla(p.surface_hover);
        c.caret = hsla(p.accent);
        c.scrollbar = hsla(p.background);
        c.scrollbar_thumb = hsla(p.border);
        c.scrollbar_thumb_hover = hsla(p.text_secondary);

        // 强调
        c.primary = hsla(p.accent);
        c.primary_foreground = hsla(p.on_accent);
        c.primary_hover = hsla(p.accent_hover);
        c.primary_active = hsla(p.accent_hover);
        c.accent = hsla(p.surface_hover);
        c.accent_foreground = hsla(p.text_primary);
        c.link = hsla(p.accent);
        c.link_hover = hsla(p.accent_hover);
        c.link_active = hsla(p.accent_hover);

        // 状态
        c.success = hsla(p.success);
        c.success_foreground = hsla(p.on_accent);
        c.success_hover = hsla(p.success);
        c.success_active = hsla(p.success);
        c.warning = hsla(p.warning);
        c.danger = hsla(p.danger);
        c.danger_foreground = hsla(p.on_accent);
        c.danger_hover = hsla(p.danger);
        c.danger_active = hsla(p.danger);
        c.info = hsla(p.accent);
        c.info_foreground = hsla(p.on_accent);
        c.info_hover = hsla(p.accent_hover);
        c.info_active = hsla(p.accent_hover);

        // 按钮。组件库的 `button_*` 是独立字段，不会回落到 `primary`，
        // 不显式覆盖就会沿用内置主题色（浅色下表现为黑底按钮）。
        c.button = hsla(p.surface_hover);
        c.button_hover = hsla(p.border);
        c.button_active = hsla(p.border);
        c.button_foreground = hsla(p.text_primary);
        c.button_primary = hsla(p.accent);
        c.button_primary_hover = hsla(p.accent_hover);
        c.button_primary_active = hsla(p.accent_hover);
        c.button_primary_foreground = hsla(p.on_accent);
        c.button_secondary = hsla(p.surface_hover);
        c.button_secondary_hover = hsla(p.border);
        c.button_secondary_active = hsla(p.border);
        c.button_secondary_foreground = hsla(p.text_primary);
        c.button_danger = hsla(p.danger);
        c.button_danger_hover = hsla(p.danger);
        c.button_danger_active = hsla(p.danger);
        c.button_danger_foreground = hsla(p.on_accent);
        c.button_info = hsla(p.accent);
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

        // 次级底色（徽标、次级按钮共用）
        c.secondary = hsla(p.surface_hover);
        c.secondary_hover = hsla(p.border);
        c.secondary_active = hsla(p.border);
        c.secondary_foreground = hsla(p.text_primary);

        // 开关
        c.switch = hsla(p.accent);
        c.switch_thumb = hsla(p.surface);
    });
}
