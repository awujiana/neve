//! 液态玻璃的**解析层**：把 `GlassSpec` + 主题令牌变成 GPUI 真正能画的层。
//!
//! 这是纯函数，不碰 `App` / `Window`，因此可以直接单测。GPUI 的能力边界决定了
//! 这里做的是「屏幕空间近似」而不是物理渲染：
//!
//! - 没有 `backdrop-filter`：磨砂只能靠**提高面层 alpha** 来遮挡背景；
//! - 没有自定义 shader / SDF：`refract()` 的 UV 位移被近似成**多条对称 inset 亮边**，
//!   边缘越厚（[`GlassSpec::refraction_height`]）亮边偏移越大，折射越强
//!   （[`GlassSpec::refraction_amount`]）亮边越多、越亮；
//! - 线性渐变只有 **两个 color stop**：yzrt 那套「两端白、中间透明」的三段高光做不出来，
//!   改成两个 stop 之间的 alpha 差，对应 Android `HighlightStyle.Default(angle, falloff)`；
//! - 色散没有 RGB 分通道采样：用一对冷 / 暖色 inset 细边近似 `inputBleed*`。
//!
//! 所有颜色都从 [`Glass`] 令牌派生，组件库自己不发明颜色常量。

use gpui_kit::{BoxShadow, Hsla, point, px};

use super::spec::GlassSpec;
use crate::ui::theme::Glass;

/// 解析好的一层玻璃。`surface` 模块只负责把这些值贴到 `Div` 上。
#[derive(Debug, Clone, PartialEq)]
pub struct GlassLayers {
    /// 斜向渐变角度（度）。
    pub gradient_angle: f32,
    /// 渐变起点色（高光侧，percentage = 0）。
    pub gradient_start: Hsla,
    /// 渐变终点色（percentage = 1）。
    pub gradient_end: Hsla,
    /// 外缘描边色。
    pub outline: Hsla,
    /// 由外到内的阴影层：第 0 条恒为外投影，其后是 inset 边缘层。
    pub shadows: Vec<BoxShadow>,
}

/// 只改 alpha，保留令牌的色相 / 明度。
fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla {
        a: alpha.clamp(0., 1.),
        ..color
    }
}

/// 一条 inset 边缘层。`offset` 为 (x, y)，`blur` / `spread` 为 CSS px。
fn inset_edge(color: Hsla, offset: (f32, f32), blur: f32, spread: f32) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(offset.0), px(offset.1)),
        blur_radius: px(blur),
        spread_radius: px(spread),
        inset: true,
    }
}

/// 把参数与令牌解析成具体层。
///
/// `lightness_ratio` 取 [`crate::core::appearance::Appearance::lightness_ratio`]，
/// 只用来决定亮端该压低白色 inset 亮线（近白面板上白线没有对比度）、
/// 以及 depth 暗边的方向；颜色本身仍全部来自令牌。
pub fn resolve(spec: &GlassSpec, glass: &Glass, lightness_ratio: f32) -> GlassLayers {
    let s = spec.normalized();
    let is_light = lightness_ratio >= 0.5;

    // ── 面层 ────────────────────────────────────────────────────────────
    // 磨砂越强越「实」：GPUI 没有背景模糊，只能靠 alpha 挡住桌面内容。
    // 上限 0.98 留一线透明，避免整块盖成不透明卡片、把窗口层的亚克力全吃掉。
    let fill = with_alpha(
        glass.surface,
        glass.surface.a * s.fill * (1. + s.frost * 0.35),
    );

    // ── 斜向高光 ────────────────────────────────────────────────────────
    // 两个 stop 的 alpha 差模拟 45° 打光；磨砂会吃掉高光，平滑度会让过渡更缓。
    let sheen =
        (s.highlight * (1. - s.frost * 0.50) * (1. - s.smoothness * 0.25) * 0.14).clamp(0., 0.14);
    let gradient_start = with_alpha(fill, fill.a + sheen);
    let gradient_end = with_alpha(fill, fill.a - sheen * 0.5);

    let outline = with_alpha(glass.outline, glass.outline.a * s.rim);

    // ── 阴影层 ──────────────────────────────────────────────────────────
    let mut shadows = Vec::with_capacity(7);
    shadows.push(BoxShadow {
        color: with_alpha(glass.shadow, glass.shadow.a * s.shadow),
        offset: point(px(0.), px(8. * s.shadow)),
        blur_radius: px(24. * s.shadow.max(0.20) * (1. + s.smoothness * 0.35)),
        spread_radius: px(-6.),
        inset: false,
    });

    // 边缘法线：refractionHeight 决定偏移，refractionAmount 决定条数与亮度。
    // 亮端面板本身接近白，白 inset 线要压暗一档才有对比度。
    let edge = 1. + s.refraction_height * 0.18;
    let blur = 1.5 + s.smoothness * 2.5 + s.refraction_height * 0.08;
    let rim_strength = s.refraction_amount * (1. - s.frost * 0.40) * (1. - s.smoothness * 0.20);
    let rim_alpha = glass.highlight.a * rim_strength * if is_light { 0.45 } else { 0.85 };
    // 主亮线永远存在：它是「玻璃」最主要的读感来源。
    shadows.push(inset_edge(
        with_alpha(glass.highlight, rim_alpha.max(0.04)),
        (0., 1.5),
        blur,
        0.,
    ));

    if s.refraction_amount > 0.20 {
        // 一对对称的斜向亮边，对应 yzrt 的 inset 2px / -2px 两条白边，
        // 也对应 WebGL 里 normal 在圆角处指向两个方向。
        let pair_alpha = rim_alpha * 0.55 * (s.refraction_amount - 0.20) / 0.80;
        shadows.push(inset_edge(
            with_alpha(glass.highlight, pair_alpha),
            (edge, -edge),
            blur + 0.5,
            0.,
        ));
        shadows.push(inset_edge(
            with_alpha(glass.highlight, pair_alpha),
            (-edge, edge),
            blur + 0.5,
            0.,
        ));
    }

    if s.refraction_amount > 0.65 {
        // 再外一圈更淡的亮边，模拟 SDF 边缘外侧的 Fresnel 衰减。
        let outer_alpha = rim_alpha * 0.30;
        shadows.push(inset_edge(
            with_alpha(glass.highlight, outer_alpha),
            (0., edge * 2.),
            blur + 2.,
            -1.,
        ));
    }

    if s.dispersion > 0. {
        // 色散：冷色向内偏、暖色向外偏，近似 RGB 分通道折射。
        // alpha 上限很低——彩边只是暗示，不能真的把边缘染成彩虹。
        let dispersion_alpha = s.dispersion * 0.18;
        shadows.push(inset_edge(
            Hsla {
                h: 0.54,
                s: 0.72,
                l: 0.62,
                a: dispersion_alpha,
            },
            (-edge, -edge),
            blur + 0.5,
            0.,
        ));
        shadows.push(inset_edge(
            Hsla {
                h: 0.07,
                s: 0.80,
                l: 0.62,
                a: dispersion_alpha,
            },
            (edge, edge),
            blur + 0.5,
            0.,
        ));
    }

    if s.depth {
        // depthEffect / contentLensing：一道向内暗边，让面层有厚度。
        shadows.push(inset_edge(
            with_alpha(glass.shadow, glass.shadow.a * 0.55),
            (0., edge),
            blur + 1.,
            -1.,
        ));
    }

    GlassLayers {
        gradient_angle: s.highlight_angle,
        gradient_start,
        gradient_end,
        outline,
        shadows,
    }
}

#[cfg(test)]
mod tests {
    use super::{GlassLayers, resolve};
    use crate::ui::glass::spec::GlassSpec;
    use crate::ui::theme;

    fn layers(spec: GlassSpec) -> GlassLayers {
        let glass = theme::glass(&crate::core::appearance::Appearance::default());
        resolve(&spec, &glass, 0.86)
    }

    #[test]
    fn gradient_start_is_brighter_than_end() {
        let layers = layers(GlassSpec::regular());
        assert!(layers.gradient_start.a > layers.gradient_end.a);
        assert!(layers.gradient_start.a <= 1.);
        assert!(layers.gradient_end.a >= 0.);
    }

    #[test]
    fn frost_makes_the_surface_more_opaque() {
        let clear = layers(GlassSpec::clear());
        let frosted = layers(GlassSpec::frosted());
        assert!(frosted.gradient_start.a > clear.gradient_start.a);
    }

    #[test]
    fn stronger_refraction_adds_more_edge_layers() {
        let plain = layers(GlassSpec::plain());
        let regular = layers(GlassSpec::regular());
        let vibrant = layers(GlassSpec::vibrant());
        assert!(plain.shadows.len() < regular.shadows.len());
        assert!(regular.shadows.len() < vibrant.shadows.len());
    }

    #[test]
    fn first_shadow_is_always_the_outer_drop_shadow() {
        for preset in crate::ui::glass::spec::GlassPreset::ALL {
            let layers = layers(preset.spec());
            assert!(!layers.shadows[0].inset, "{} 的外投影丢了", preset.label());
        }
    }

    #[test]
    fn every_alpha_stays_in_range() {
        for preset in crate::ui::glass::spec::GlassPreset::ALL {
            let layers = layers(preset.spec());
            for color in [
                layers.gradient_start,
                layers.gradient_end,
                layers.outline,
                layers.shadows[0].color,
            ] {
                assert!((0.0..=1.0).contains(&color.a));
            }
            for shadow in &layers.shadows {
                assert!((0.0..=1.0).contains(&shadow.color.a));
            }
        }
    }

    #[test]
    fn light_surfaces_damp_white_inset_highlights() {
        let glass = theme::glass(&crate::core::appearance::Appearance::default());
        let spec = GlassSpec::clear();
        let dark = resolve(&spec, &glass, 0.10);
        let light = resolve(&spec, &glass, 0.90);
        assert!(light.shadows[1].color.a < dark.shadows[1].color.a);
    }
}
