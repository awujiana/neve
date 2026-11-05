//! ui::mpv_preview —— 皮肤页的播放器画面预览画布
//!
//! 这里画的是 **mpv 播放器自己的 UI**，不是 Neve 的界面。
//!
//! 版式全部照 `uosc.conf` + uosc 源码 + **实机截图**对齐
//! （实机截图见 `docs/screenshots/uosc-real.png`）：
//!
//! | 元素 | 真实 uosc 行为 | 来源 |
//! | --- | --- | --- |
//! | 顶栏 | 没有通栏底色，左上角只有两行小字标题（主标题 + 副标题） | `TopBar.lua` |
//! | 控制条 | `controls_size` 见方的**裸图标**，左右两簇，中间由 `space` 撑开 | `Button.lua` |
//! | 时间轴 | `background` 底槽 + `foreground` 已播放段，时间戳**反色**压在各自底色上 | `Timeline.lua` |
//! | 菜单 | 屏幕**居中**面板，`menu_min_width` 宽、`menu_item_height` 高，顶部是标题带 | `Menu.lua` |
//!
//! 预览里的颜色来自皮肤配置（`SkinPalette` 的 `0xRRGGBB`），经
//! `ui::theme::color` 转成 `Hsla`。这是唯一允许「颜色不来自 `cx.theme().colors.*`」
//! 的视图，因为它要如实还原第三方皮肤配色。
//!
//! 已知近似（不假装是像素级复刻）：
//! - 图标用 uosc **同一个** Material Icons 字体渲染同一个连字（见 `load_icon_font`），
//!   字形一致；只有光栅化（hinting / 抗锯齿）与 libass 不同，逐像素不会完全相同。
//!   字体缺失时才退回 `fallback_icon_shape` 的几何拼图；
//! - GPUI 没有文字描边，`text_border` 用极淡的深色底衬代替；
//! - 没有 shader，滤镜对画面的影响用**色调近似**表达「画面会变」；
//! - 背后没有真实解码帧，画面内容是静态示意。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    App, Div, Hsla, ParentElement as _, Styled as _, div, linear_color_stop, linear_gradient,
    prelude::*, px,
};

use crate::core::skins::{
    ControlItem, ControlKind, PlaybackState, SkinKind, SkinPalette, SkinPreset, UoscLayout,
};

use super::theme;

/// 画布按 16:9 画一个真实的播放器窗口。
const CANVAS_W: f32 = 720.;
const CANVAS_H: f32 = 405.;

/// uosc 图标字体的族名，取自字体 `name` 表 nameID 1。
///
/// uosc 的 `ass.lua` 里写的是 `opts.font = 'MaterialIconsRound-Regular'`（那是 PostScript
/// 名），libass 认这个；GPUI 走 fontdb，按**家族名**匹配，所以要写 `Material Icons Round`。
const ICON_FONT_FAMILY: &str = "Material Icons Round";

/// 图标字体是否已成功加载。没加载时退回几何拼图。
static ICON_FONT_READY: AtomicBool = AtomicBool::new(false);

/// 把 uosc 的 Material Icons 字体装进 GPUI 的字体库。
///
/// uosc 画图标的方式是**把图标名当文本渲染**（`ass:icon` → `ass:txt(…, name)`），
/// 由字体的 `liga` 连字把 `folder` / `menu_book` 这类名字替换成字形。所以要画得和
/// mpv 一样，唯一正确的做法是加载同一个字体文件、渲染同一个字符串，而不是自己
/// 用矩形和圆环拼形状。
///
/// 返回是否加载成功；失败时预览退回 `fallback_icon_shape`。
pub fn load_icon_font(cx: &App, path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if cx
        .text_system()
        .add_fonts(vec![std::borrow::Cow::Owned(bytes)])
        .is_err()
    {
        return false;
    }
    ICON_FONT_READY.store(true, Ordering::Relaxed);
    true
}

/// 图标字体是否可用（决定用真字形还是几何兜底）。
pub fn icon_font_ready() -> bool {
    ICON_FONT_READY.load(Ordering::Relaxed)
}

/// 时间轴已播放比例（示例画面 18:24 / 44:36）。
const PLAYED: f32 = 0.42;
/// 示例章节位置。
const CHAPTERS: [f32; 3] = [0.18, 0.47, 0.78];

/// 滤镜对预览画面的色调影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewTone {
    Normal,
    /// 色温 / 暖色
    Warm,
    /// 锐化 / 放大 / 冷色
    Cool,
    /// 伽马 / 提亮
    Bright,
    /// 去色块 / 柔化
    Soft,
    /// 翻转 / 旋转
    Mirror,
}

impl PreviewTone {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "原始画面",
            Self::Warm => "暖色色调",
            Self::Cool => "锐利冷色",
            Self::Bright => "提亮画面",
            Self::Soft => "柔和降噪",
            Self::Mirror => "镜像 / 旋转",
        }
    }

    /// 依滤镜名挑一个可视的色调，让「切换滤镜」在预览里立刻有反馈。
    pub fn for_filter(name: &str) -> Self {
        if name.contains("色温") || name.contains("动态范围") {
            Self::Warm
        } else if name.contains("伽马") || name.contains("亮度") {
            Self::Bright
        } else if name.contains("去色块") || name.contains("降噪") || name.contains("去糊") {
            Self::Soft
        } else if name.contains("翻转") || name.contains("旋转") || name.contains("填充") {
            Self::Mirror
        } else if name.contains("锐化")
            || name.contains("放大")
            || name.contains("FSRCNNX")
            || name.contains("NNEDI3")
            || name.contains("Anime4K")
        {
            Self::Cool
        } else {
            Self::Normal
        }
    }

    /// 画面渐变的两端色（与皮肤底色混合，保证任何配色下都是「一帧画面」）。
    fn gradient(self, palette: SkinPalette) -> (u32, u32) {
        let base = palette.background;
        match self {
            Self::Normal => (blend(base, 0x2f_4a_5e, 0.55), blend(base, 0x12_1c_26, 0.55)),
            Self::Warm => (blend(base, 0x8a_5a_38, 0.62), blend(base, 0x2a_16_12, 0.60)),
            Self::Cool => (blend(base, 0x35_6d_8f, 0.70), blend(base, 0x0e_22_34, 0.60)),
            Self::Bright => (blend(base, 0xc2_d2_de, 0.70), blend(base, 0x50_66_78, 0.55)),
            Self::Soft => (blend(base, 0x5c_67_70, 0.50), blend(base, 0x2a_31_37, 0.50)),
            Self::Mirror => (blend(base, 0x5e_3f_6b, 0.58), blend(base, 0x1b_14_2c, 0.55)),
        }
    }
}

/// 渲染一整个 mpv 播放器窗口预览。
pub fn render_mpv_preview(
    preset: &SkinPreset,
    tone: PreviewTone,
    _filter_label: Option<&str>,
    cx: &App,
) -> Div {
    let layout = &preset.layout;
    let palette = layout.palette;
    let (start, end) = tone.gradient(palette);
    let video = linear_gradient(
        135.,
        linear_color_stop(theme::color(start), 0.),
        linear_color_stop(theme::color(end), 1.),
    );

    let mut window = div()
        .relative()
        .w(px(CANVAS_W))
        .h(px(CANVAS_H))
        .overflow_hidden()
        .rounded(px(layout.border_radius.clamp(0., 8.)))
        .bg(video)
        .border_1()
        .border_color(theme::color(palette.window_border))
        .child(scene_content(palette, tone));

    window = match preset.kind {
        // uosc 的菜单是居中的大面板，正好压在字幕位置上；面板只有 0.9 不透明度，
        // 再叠一层字幕会糊成一片，所以 uosc 分支不画字幕。
        SkinKind::Uosc => window
            .child(uosc_top_bar(layout, palette))
            // 菜单最后画：uosc 把菜单当独立浮层压在控制条之上，
            // 先画控制条会让图标从菜单面板里透出来。
            .child(uosc_bottom(layout, palette))
            .child(uosc_menu(layout, palette)),
        SkinKind::BuiltinOsc => window
            .child(subtitle_line(layout, palette))
            .child(builtin_top_hint(palette))
            .child(builtin_osc_bottom(layout, palette)),
    };

    // 卡片里居中放一个 16:9 的「播放器窗口」，两侧留白而不是把画布拉满。
    v_flex().w_full().gap_2().child(
        h_flex()
            .w_full()
            .justify_center()
            .p_3()
            .rounded(px(12.))
            .bg(cx.theme().colors.secondary.opacity(0.45))
            .child(window),
    )
}

/// 画面内容：用几块几何形状模拟一帧影片，滤镜色调只改渐变，不改结构。
fn scene_content(palette: SkinPalette, tone: PreviewTone) -> Div {
    let mirrored = tone == PreviewTone::Mirror;
    let horizon = theme::color(blend(palette.background, 0xb0_c4_d2, 0.35)).opacity(0.35);
    let hill = theme::color(blend(palette.background, 0x08_10_18, 0.65)).opacity(0.75);
    // 太阳不用强调色：uosc 的 `match` 只用于菜单搜索命中，
    // 拿它当画面主色会让预览一眼假。
    let sun = theme::color(blend(palette.background, 0xff_e7_c4, 0.55)).opacity(0.55);
    let sun_x = if mirrored { 92. } else { 470. };
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w_full()
        .h_full()
        .child(
            div()
                .absolute()
                .bottom(px(96.))
                .left(px(0.))
                .w_full()
                .h(px(1.))
                .bg(horizon),
        )
        .child(
            div()
                .absolute()
                .top(px(64.))
                .left(px(sun_x))
                .w(px(38.))
                .h(px(38.))
                .rounded(px(19.))
                .bg(sun),
        )
        .child(
            div()
                .absolute()
                .bottom(px(0.))
                .left(px(if mirrored { 300. } else { -40. }))
                .w(px(330.))
                .h(px(96.))
                .rounded(px(110.))
                .bg(hill),
        )
        .child(
            div()
                .absolute()
                .bottom(px(0.))
                .right(px(if mirrored { 190. } else { -60. }))
                .w(px(300.))
                .h(px(68.))
                .rounded(px(90.))
                .bg(hill),
        )
}

/// 字幕：mpv 自己渲染 ASS，位置在控制条之上。
fn subtitle_line(layout: &UoscLayout, palette: SkinPalette) -> Div {
    let bottom = layout.controls_size.clamp(22., 40.)
        + layout.controls_margin.clamp(4., 14.) * 2.
        + layout.timeline_size.clamp(16., 40.);
    div()
        .absolute()
        .bottom(px(bottom + 8.))
        .left(px(0.))
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .px_2()
                .text_sm()
                .text_color(theme::color(palette.background_text))
                .child("这是一行示例字幕"),
        )
}

/// uosc 顶栏：**没有通栏底色**，左上角只有两行小字。
///
/// 实机截图里 `top_bar=no-border` 的表现就是「主标题 + 副标题」两行文字，
/// 靠 `text_border` 的描边在画面上可读；GPUI 没有描边，用极淡的深色底衬代替。
fn uosc_top_bar(layout: &UoscLayout, palette: SkinPalette) -> Div {
    if layout.top_bar.eq_ignore_ascii_case("no") {
        return div().h(px(0.));
    }
    let height = layout.top_bar_size.clamp(28., 48.);
    let radius = layout.border_radius.clamp(0., 8.);
    let backing = alpha(palette.background, 0.32);
    let text = theme::color(palette.background_text);

    let mut titles = v_flex().gap(px(2.)).items_start();
    if layout.top_bar_title {
        titles = titles.child(
            div()
                .px(px(4.))
                .rounded(px(radius))
                .bg(backing)
                .text_size(px((height * 0.3).clamp(10., 15.)))
                .text_color(text)
                .child("示例影片 · Episode 01"),
        );
    }
    if layout.top_bar_alt_place.eq_ignore_ascii_case("below") {
        titles = titles.child(
            div()
                .px(px(4.))
                .rounded(px(radius))
                .bg(backing)
                .text_size(px((height * 0.24).clamp(9., 12.)))
                .text_color(alpha_hsla(text, 0.8))
                .child("1920x1080 · 24 fps · H.265"),
        );
    }

    let mut bar = h_flex()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w_full()
        .h(px(height))
        .items_start()
        .px(px((height * 0.16).max(6.)))
        .pt(px((height * 0.14).max(4.)))
        .child(titles)
        .child(div().flex_1());

    if layout.top_bar_controls {
        // uosc 的窗口按钮平时是半透明的，鼠标靠近才实心。
        let mut controls = h_flex().items_center().gap(px(6.));
        for glyph in ["─", "□", "✕"] {
            controls = controls.child(
                div()
                    .px(px(3.))
                    .rounded(px(radius))
                    .text_size(px(11.))
                    .text_color(alpha_hsla(text, 0.55))
                    .child(glyph),
            );
        }
        bar = bar.child(controls);
    }
    bar
}

/// uosc 底部：控制条（左右两簇裸图标）+ 时间轴。
fn uosc_bottom(layout: &UoscLayout, palette: SkinPalette) -> Div {
    let controls_size = layout.controls_size.clamp(22., 40.);
    let spacing = layout.controls_spacing.clamp(0., 8.);
    let margin = layout.controls_margin.clamp(4., 14.);
    let bar_h = if layout.timeline_style.eq_ignore_ascii_case("line") {
        layout.timeline_line_width.clamp(2., 8.)
    } else {
        layout.timeline_size.clamp(16., 40.)
    };
    let border = layout.timeline_border.clamp(0., 3.);

    v_flex()
        .absolute()
        .bottom(px(0.))
        .left(px(0.))
        .w_full()
        .child(controls_row(
            layout,
            palette,
            controls_size,
            spacing,
            margin,
        ))
        .child(
            // 时间轴上沿的分隔带：`timeline_border` 高、背景色实心。
            div().w_full().h(px(border)).bg(alpha(
                palette.background,
                layout.opacity.timeline.clamp(0., 1.),
            )),
        )
        .child(timeline_bar(layout, palette, bar_h))
}

/// 控制条：`controls_size` 见方的裸图标，`space` 把剩余宽度摊开成左右两簇。
fn controls_row(
    layout: &UoscLayout,
    palette: SkinPalette,
    size: f32,
    spacing: f32,
    margin: f32,
) -> Div {
    let fg = theme::color(palette.foreground);
    // 先按模拟的播放状态过滤：真实 `controls=` 里绝大多数条目都带 `<video>` /
    // `<idle>` / `<has_chapter>` 这类条件，不过滤会把二十多个图标全画出来。
    let state = PlaybackState::SAMPLE_VIDEO;
    let shown: Vec<ControlItem> = layout
        .controls
        .iter()
        .filter(|item| item.visible_in(&state))
        .cloned()
        .collect();
    let visible = fit_controls(&shown, size, spacing, margin);
    let mut row = h_flex()
        .w_full()
        .h(px(size))
        .px(px(margin))
        .mb(px(margin))
        .items_center()
        .gap(px(spacing));
    for (item, show) in shown.iter().zip(visible) {
        if show {
            row = row.child(control_cell(item, size, fg));
        }
    }
    row
}

/// 复刻 uosc 的「放不下就从中间往外隐藏」策略（`Controls:update_dimensions`）。
///
/// 整合包的 `controls=` 往往有三四十个条目，全排开是 1000+ px，而窗口只有 720px。
/// 真实 uosc 会从中线开始左右交替隐藏，直到装得下——这就是实机截图里
/// 只剩左右两簇十来个图标的原因。不这么做，预览会挤成一条密不透风的图标带。
fn fit_controls(controls: &[ControlItem], size: f32, spacing: f32, margin: f32) -> Vec<bool> {
    let width_of = |item: &ControlItem| match item.kind {
        ControlKind::Space => 0.,
        ControlKind::Gap => size * item.scale + spacing,
        ControlKind::Speed => size * item.scale.max(1.2) + spacing,
        ControlKind::Button => size + spacing,
    };
    let available = CANVAS_W - margin * 2.;
    let mut visible = vec![true; controls.len()];
    let mut total: f32 = controls.iter().map(width_of).sum();
    if total <= available || controls.is_empty() {
        return visible;
    }

    let count = controls.len() as i32;
    let mut cursor = (count as f32 / 2.).ceil() as i32;
    for step in 0..count {
        cursor += if step % 2 == 0 { 1 } else { -1 };
        if cursor <= 0 || cursor >= count {
            continue;
        }
        let index = cursor as usize;
        if matches!(controls[index].kind, ControlKind::Space | ControlKind::Gap) || !visible[index]
        {
            continue;
        }
        visible[index] = false;
        total -= width_of(&controls[index]);
        if total <= available {
            break;
        }
    }
    visible
}

/// 一个控制项。`Button` 只有图标没有底色（uosc 默认 `opacity.controls=0`）。
fn control_cell(item: &ControlItem, size: f32, fg: Hsla) -> Div {
    match item.kind {
        ControlKind::Space => div().flex_1(),
        ControlKind::Gap => div().w(px(size * item.scale)).flex_none(),
        ControlKind::Speed => h_flex()
            .w(px(size * item.scale.max(1.2)))
            .h_full()
            .flex_none()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .text_color(alpha_hsla(fg, 0.9))
            .child("1.0×"),
        ControlKind::Button => h_flex()
            .w(px(size))
            .h(px(size))
            .flex_none()
            .items_center()
            .justify_center()
            .child(icon_glyph(&item.icon, size * 0.7, fg)),
    }
}

/// 画一个 uosc 图标。
///
/// 走的是**真字形**：把 uosc 的图标名当文本、用 uosc 自己的 Material Icons 字体渲染。
/// `Button.lua` 里 `font_size = round((by - ay) * 0.7)`，所以字号是按钮边长的 0.7 倍；
/// 行高也设成同一个值，让文字行盒等于 em 盒，图标才能在按钮里居中（默认行高约 1.2em，
/// 直接用会把图标往下压）。
fn icon_glyph(icon: &str, size: f32, color: Hsla) -> Div {
    if !icon_font_ready() {
        return fallback_icon_shape(icon, size, color);
    }
    div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .font_family(ICON_FONT_FAMILY)
        .text_size(px(size))
        .line_height(px(size))
        .text_color(color)
        .child(icon.to_string())
}

/// 字体缺失时的几何兜底：按图标语义拼形状。
///
/// 只在 `load_icon_font` 失败时用到（例如播放器没带 `fonts/MaterialIconsRound-Regular.otf`）。
/// 正常路径走 `icon_glyph` 的真字形——形状拼得再像也不是 uosc 的图标。
fn fallback_icon_shape(icon: &str, size: f32, color: Hsla) -> Div {
    let s = size;
    let base = div().relative().w(px(s)).h(px(s)).flex_none();
    match icon {
        "open-file" => base
            .child(
                fill(s * 0.5, s * 0.14, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.2))
                    .left(px(s * 0.08)),
            )
            .child(
                fill(s * 0.84, s * 0.5, color, s * 0.07)
                    .absolute()
                    .top(px(s * 0.34))
                    .left(px(s * 0.08)),
            ),
        "history" | "person_search" | "image_search" => base
            .child(
                ring(s * 0.56, color, s * 0.28)
                    .absolute()
                    .top(px(s * 0.1))
                    .left(px(s * 0.1)),
            )
            .child(
                fill(s * 0.08, s * 0.26, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.62))
                    .left(px(s * 0.64)),
            ),
        "danmaku" | "toggle_on" => base
            .child(
                fill(s * 0.68, s * 0.34, color, s * 0.17)
                    .absolute()
                    .top(px(s * 0.33))
                    .left(px(s * 0.06)),
            )
            .child(
                fill(s * 0.22, s * 0.22, color, s * 0.11)
                    .absolute()
                    .top(px(s * 0.39))
                    .left(px(s * 0.48)),
            ),
        "danmaku_menu" | "grid" => {
            let cell = s * 0.28;
            let mut node = base;
            for (x, y) in [(0.08, 0.08), (0.5, 0.08), (0.08, 0.5), (0.5, 0.5)] {
                node = node.child(
                    fill(cell, cell, color, s * 0.05)
                        .absolute()
                        .top(px(s * y))
                        .left(px(s * x)),
                );
            }
            node
        }
        "analytics" | "stats" => base
            .child(
                fill(s * 0.16, s * 0.42, color, s * 0.02)
                    .absolute()
                    .bottom(px(s * 0.14))
                    .left(px(s * 0.14)),
            )
            .child(
                fill(s * 0.16, s * 0.62, color, s * 0.02)
                    .absolute()
                    .bottom(px(s * 0.14))
                    .left(px(s * 0.42)),
            )
            .child(
                fill(s * 0.16, s * 0.3, color, s * 0.02)
                    .absolute()
                    .bottom(px(s * 0.14))
                    .left(px(s * 0.7)),
            ),
        "image" => base
            .child(
                ring(s * 0.8, color, s * 0.06)
                    .absolute()
                    .top(px(s * 0.1))
                    .left(px(s * 0.1)),
            )
            .child(
                fill(s * 0.18, s * 0.18, color, s * 0.09)
                    .absolute()
                    .top(px(s * 0.24))
                    .left(px(s * 0.24)),
            )
            .child(
                fill(s * 0.34, s * 0.22, color, s * 0.03)
                    .absolute()
                    .bottom(px(s * 0.22))
                    .left(px(s * 0.22)),
            ),
        "picture_in_picture_alt" => base
            .child(
                ring(s * 0.84, color, s * 0.05)
                    .absolute()
                    .top(px(s * 0.08))
                    .left(px(s * 0.08)),
            )
            .child(
                fill(s * 0.36, s * 0.26, color, s * 0.03)
                    .absolute()
                    .bottom(px(s * 0.18))
                    .right(px(s * 0.18)),
            ),
        "fullscreen" => base
            .child(
                fill(s * 0.24, s * 0.06, color, 0.)
                    .absolute()
                    .top(px(s * 0.12))
                    .left(px(s * 0.12)),
            )
            .child(
                fill(s * 0.06, s * 0.24, color, 0.)
                    .absolute()
                    .top(px(s * 0.12))
                    .left(px(s * 0.12)),
            )
            .child(
                fill(s * 0.24, s * 0.06, color, 0.)
                    .absolute()
                    .top(px(s * 0.12))
                    .right(px(s * 0.12)),
            )
            .child(
                fill(s * 0.06, s * 0.24, color, 0.)
                    .absolute()
                    .top(px(s * 0.12))
                    .right(px(s * 0.12)),
            )
            .child(
                fill(s * 0.24, s * 0.06, color, 0.)
                    .absolute()
                    .bottom(px(s * 0.12))
                    .left(px(s * 0.12)),
            )
            .child(
                fill(s * 0.06, s * 0.24, color, 0.)
                    .absolute()
                    .bottom(px(s * 0.12))
                    .left(px(s * 0.12)),
            )
            .child(
                fill(s * 0.24, s * 0.06, color, 0.)
                    .absolute()
                    .bottom(px(s * 0.12))
                    .right(px(s * 0.12)),
            )
            .child(
                fill(s * 0.06, s * 0.24, color, 0.)
                    .absolute()
                    .bottom(px(s * 0.12))
                    .right(px(s * 0.12)),
            ),
        "menu" | "staff_credits_main" => base
            .child(
                fill(s * 0.72, s * 0.09, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.24))
                    .left(px(s * 0.14)),
            )
            .child(
                fill(s * 0.72, s * 0.09, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.46))
                    .left(px(s * 0.14)),
            )
            .child(
                fill(s * 0.48, s * 0.09, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.68))
                    .left(px(s * 0.14)),
            ),
        "playlist" | "items" => base
            .child(
                fill(s * 0.16, s * 0.16, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.22))
                    .left(px(s * 0.1)),
            )
            .child(
                fill(s * 0.16, s * 0.16, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.22))
                    .left(px(s * 0.44)),
            )
            .child(
                fill(s * 0.16, s * 0.16, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.22))
                    .left(px(s * 0.7)),
            )
            .child(
                fill(s * 0.8, s * 0.1, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.58))
                    .left(px(s * 0.1)),
            ),
        "subtitles" => base
            .child(
                ring(s * 0.84, color, s * 0.06)
                    .absolute()
                    .top(px(s * 0.2))
                    .left(px(s * 0.08)),
            )
            .child(
                fill(s * 0.5, s * 0.08, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.44))
                    .left(px(s * 0.22)),
            )
            .child(
                fill(s * 0.34, s * 0.08, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.6))
                    .left(px(s * 0.22)),
            ),
        "video" => base
            .child(
                ring(s * 0.84, color, s * 0.06)
                    .absolute()
                    .top(px(s * 0.1))
                    .left(px(s * 0.08)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(s * 0.3))
                    .left(px(s * 0.36))
                    .text_size(px(s * 0.36))
                    .text_color(color)
                    .child("▶"),
            ),
        "chapters" | "editions" => base
            .child(
                fill(s * 0.7, s * 0.14, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.2))
                    .left(px(s * 0.15)),
            )
            .child(
                fill(s * 0.7, s * 0.14, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.44))
                    .left(px(s * 0.15)),
            )
            .child(
                fill(s * 0.4, s * 0.14, color, s * 0.03)
                    .absolute()
                    .top(px(s * 0.68))
                    .left(px(s * 0.15)),
            ),
        "play-pause" => base
            .child(
                fill(s * 0.14, s * 0.56, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.22))
                    .left(px(s * 0.28)),
            )
            .child(
                fill(s * 0.14, s * 0.56, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.22))
                    .left(px(s * 0.58)),
            ),
        "audio" => base.child(glyph("♪", s * 0.7, color)),
        "bookmark_border" => base.child(
            ring(s * 0.6, color, s * 0.04)
                .absolute()
                .top(px(s * 0.2))
                .left(px(s * 0.2)),
        ),
        "star_rate" => base.child(glyph("★", s * 0.72, color)),
        "tune" => base
            .child(
                fill(s * 0.7, s * 0.1, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.3))
                    .left(px(s * 0.15)),
            )
            .child(
                fill(s * 0.7, s * 0.1, color, s * 0.02)
                    .absolute()
                    .top(px(s * 0.58))
                    .left(px(s * 0.15)),
            )
            .child(
                fill(s * 0.18, s * 0.18, color, s * 0.09)
                    .absolute()
                    .top(px(s * 0.26))
                    .left(px(s * 0.3)),
            )
            .child(
                fill(s * 0.18, s * 0.18, color, s * 0.09)
                    .absolute()
                    .top(px(s * 0.54))
                    .left(px(s * 0.55)),
            ),
        "shuffle" => base.child(glyph("⇄", s * 0.8, color)),
        "loop-playlist" => base.child(glyph("↻", s * 0.8, color)),
        "loop-file" => base.child(glyph("↺", s * 0.8, color)),
        "next" | "last" | "skip_next" => base.child(glyph("⏭", s * 0.75, color)),
        "prev" | "first" | "skip_previous" => base.child(glyph("⏮", s * 0.75, color)),
        _ => base.child(
            ring(s * 0.56, color, s * 0.28)
                .absolute()
                .top(px(s * 0.22))
                .left(px(s * 0.22)),
        ),
    }
}

/// 一个居中的文字字形（少数图标只能靠字符表达）。
fn glyph(text: &str, size: f32, color: Hsla) -> Div {
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w_full()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size))
        .text_color(color)
        .child(text.to_string())
}

fn fill(width: f32, height: f32, color: Hsla, radius: f32) -> Div {
    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(radius))
        .bg(color)
}

fn ring(size: f32, color: Hsla, radius: f32) -> Div {
    div()
        .w(px(size))
        .h(px(size))
        .rounded(px(radius))
        .border_1()
        .border_color(color)
}

/// uosc 时间轴：底槽 + 已播放段 + **反色**时间戳。
fn timeline_bar(layout: &UoscLayout, palette: SkinPalette, height: f32) -> Div {
    let track = alpha(palette.background, layout.opacity.timeline.clamp(0., 1.));
    let played = alpha(palette.foreground, layout.opacity.position.clamp(0., 1.));
    let over_played = theme::color(palette.foreground_text);
    let over_track = theme::color(palette.background_text);
    let chapter = alpha(palette.foreground, layout.opacity.chapters.clamp(0., 1.));

    let mut bar = div().relative().w_full().h(px(height)).bg(track).child(
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .h_full()
            .w(relative(PLAYED))
            .bg(played),
    );

    // 章节标记：uosc 画的是骑在时间轴上沿的小菱形，这里用同尺寸方块近似。
    for at in CHAPTERS {
        bar = bar.child(
            div()
                .absolute()
                .top(px(-3.))
                .left(relative(at))
                .w(px(5.))
                .h(px(5.))
                .rounded(px(1.))
                .bg(chapter),
        );
    }

    bar.child(
        h_flex()
            .absolute()
            .inset_0()
            .items_center()
            .justify_between()
            .px(px((height * 0.22).max(4.)))
            .text_size(px((height * 0.42).clamp(9., 15.)))
            .child(
                div()
                    .text_color(over_played)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("18:24"),
            )
            .child(div().text_color(over_track).child("-26:12")),
    )
}

/// uosc 菜单：屏幕居中，顶部标题带 + 条目 + 脚注。
fn uosc_menu(layout: &UoscLayout, palette: SkinPalette) -> Div {
    let width = layout.menu_min_width.clamp(210., 330.);
    let item_h = layout.menu_item_height.clamp(26., 46.);
    let radius = (layout.border_radius + layout.menu_padding).clamp(0., 10.);
    let pad = layout.menu_padding.clamp(0., 4.);
    let panel = alpha(palette.background, layout.opacity.menu.clamp(0.35, 1.));
    let header = alpha(
        palette.foreground,
        (layout.opacity.menu * 0.8).clamp(0.2, 1.),
    );
    let header_text = theme::color(palette.foreground_text);
    let text = theme::color(palette.background_text);
    let hint = alpha(palette.background_text, 0.55);
    let selected = alpha(palette.foreground, layout.opacity.menu * 0.15);
    let icon = alpha(palette.background_text, 0.85);

    // 图标名一律用 uosc 真实的 Material Icons 连字名（见 main.lua 的主菜单定义），
    // 不能写 `playlist` / `audio` 这类 shorthand 别名——字体里没有那些连字。
    let items = [
        ("list_alt", "播放列表", "›"),
        ("library_books", "章节", "›"),
        ("graphic_eq", "音轨", "›"),
        ("closed_caption", "字幕", "›"),
        ("smart_display", "视频", "›"),
        ("settings", "设置", "›"),
    ];
    let footnote_h = (item_h * 0.72).max(16.);
    let height = pad * 2. + item_h + item_h * items.len() as f32 + footnote_h;

    let mut menu = v_flex()
        .absolute()
        .left(px(((CANVAS_W - width) / 2.).round()))
        .top(px(((CANVAS_H - height) / 2.).round()))
        .w(px(width))
        .p(px(pad))
        .rounded(px(radius))
        .bg(panel)
        .overflow_hidden();

    // 标题带：`foreground` 铺底，文字用 `foreground_text`。
    menu = menu.child(
        h_flex()
            .h(px(item_h))
            .px(px(item_h * 0.3))
            .items_center()
            .rounded(px(radius))
            .bg(header)
            .text_size(px((item_h * 0.42).clamp(11., 17.)))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(header_text)
            .child("播放菜单"),
    );

    for (idx, (icon_name, label, hint_text)) in items.iter().enumerate() {
        let first = idx == 0;
        menu = menu.child(
            h_flex()
                .h(px(item_h))
                .px(px(item_h * 0.3))
                .gap(px(item_h * 0.22))
                .items_center()
                .rounded(px(radius))
                .when(first, move |this| this.bg(selected))
                // Menu.lua：图标字号 = item_height * 0.48 * font_scale，再乘 1.5
                .child(icon_glyph(icon_name, item_h * 0.48 * 1.5, icon))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px((item_h * 0.4).clamp(10., 15.)))
                        .text_color(text)
                        .child(*label),
                )
                .child(
                    div()
                        .text_size(px((item_h * 0.38).clamp(9., 14.)))
                        .text_color(hint)
                        .child(*hint_text),
                ),
        );
    }

    menu.child(
        div()
            .h(px(footnote_h))
            .px(px(item_h * 0.3))
            .flex()
            .items_center()
            .text_size(px((item_h * 0.3).max(9.)))
            .text_color(hint)
            .child("uosc · 点击标题可搜索"),
    )
}

/// 内置 osc.lua：左上角一行标题提示。
fn builtin_top_hint(palette: SkinPalette) -> Div {
    h_flex()
        .absolute()
        .top(px(10.))
        .left(px(12.))
        .px_2()
        .py_1()
        .rounded(px(3.))
        .bg(alpha(palette.background, 0.55))
        .text_xs()
        .text_color(theme::color(palette.background_text))
        .child("示例影片 · Episode 01 [1920x1080]")
}

/// 内置 osc.lua：经典细时间条 + 一行按钮。
fn builtin_osc_bottom(layout: &UoscLayout, palette: SkinPalette) -> Div {
    let fg = theme::color(palette.background_text);
    let bar = h_flex()
        .w_full()
        .h(px(4.))
        .bg(alpha(palette.background, 0.55))
        .child(
            div()
                .w(relative(PLAYED))
                .h_full()
                .bg(alpha(palette.foreground, 0.9)),
        )
        .child(div().w(px(7.)).h(px(7.)).rounded(px(4.)).bg(fg));

    let buttons = h_flex()
        .w_full()
        .px_3()
        .py_2()
        .items_center()
        .justify_center()
        .gap_4()
        .bg(alpha(palette.background, 0.72))
        .text_size(px(13.))
        .text_color(fg)
        .child("⏮")
        .child("⏯")
        .child("⏭")
        .child("18:24 / 44:36")
        .child("♪")
        .child("⛶");

    v_flex()
        .absolute()
        .bottom(px(0.))
        .left(px(0.))
        .w_full()
        .child(
            div()
                .w_full()
                .px(px(10.))
                .py(px(4.))
                .bg(alpha(palette.background, 0.45))
                .child(bar),
        )
        .child(buttons)
        .child(
            div()
                .h(px(layout.border_radius.clamp(2., 8.)))
                .bg(alpha(palette.foreground, 0.06)),
        )
}

/// 方案列表里的小配色卡：直接展示皮肤四个主色。
pub fn render_palette_swatch(palette: SkinPalette) -> Div {
    h_flex()
        .gap_1()
        .items_center()
        .child(color_chip(palette.background))
        .child(color_chip(palette.foreground))
        .child(color_chip(palette.foreground_text))
        .child(color_chip(palette.accent))
}

fn color_chip(value: u32) -> Div {
    div()
        .w(px(14.))
        .h(px(14.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::color(value).opacity(0.65))
        .bg(theme::color(value))
}

/// 颜色插值（与 `ui::theme::mix` 同规则，预览专用）。
fn blend(from: u32, to: u32, ratio: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((from >> shift) & 0xff) as f32;
        let b = ((to >> shift) & 0xff) as f32;
        ((a + (b - a) * ratio).round() as u32) & 0xff
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn alpha(value: u32, opacity: f32) -> Hsla {
    theme::color(value).opacity(opacity.clamp(0., 1.))
}

fn alpha_hsla(color: Hsla, opacity: f32) -> Hsla {
    color.opacity(opacity.clamp(0., 1.))
}

/// 相对宽度的轻量写法：`relative(0.42)` 等价于 42%。
fn relative(ratio: f32) -> gpui_kit::DefiniteLength {
    gpui_kit::relative(ratio.clamp(0., 1.))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_name_maps_to_a_visible_tone() {
        assert_eq!(
            PreviewTone::for_filter("开/关 色温修正 6500"),
            PreviewTone::Warm
        );
        assert_eq!(
            PreviewTone::for_filter("开/关 自适应锐化"),
            PreviewTone::Cool
        );
        assert_eq!(
            PreviewTone::for_filter("开/关 垂直翻转"),
            PreviewTone::Mirror
        );
        assert_eq!(PreviewTone::for_filter("清空"), PreviewTone::Normal);
    }

    #[test]
    fn tones_produce_distinct_gradients() {
        let palette = SkinPalette::default();
        let normal = PreviewTone::Normal.gradient(palette);
        let warm = PreviewTone::Warm.gradient(palette);
        let cool = PreviewTone::Cool.gradient(palette);
        assert_ne!(normal, warm);
        assert_ne!(normal, cool);
        assert_ne!(warm, cool);
    }

    #[test]
    fn blend_stays_in_rgb_range() {
        assert_eq!(blend(0x00_00_00, 0xff_ff_ff, 0.), 0x00_00_00);
        assert_eq!(blend(0x00_00_00, 0xff_ff_ff, 1.), 0xff_ff_ff);
        assert_eq!(blend(0x10_20_30, 0x10_20_30, 0.5), 0x10_20_30);
    }
}
