//! 液态玻璃的**参数层**。
//!
//! 五份参考实现虽然技术栈完全不同，调的其实是同一组量：
//!
//! | 概念 | rxing365 (WebGL) | GlassExplorer (iOS 私有) | AndroidLiquidGlass | yzrt / 源码.js (CSS) | 本模块 |
//! |---|---|---|---|---|---|
//! | 边缘折射带宽度 | `u_edgeWidth` / height map | `inputInnerRefractionHeight` | `refractionHeight` | inset 亮边的 blur/spread | [`GlassSpec::refraction_height`] |
//! | 折射强度 | `u_refractStrength` / normalScale | `inputInnerRefractionAmount` | `refractionAmount` | inset 亮边条数与 alpha | [`GlassSpec::refraction_amount`] |
//! | 色散 / 渗色 | RGB 分通道采样 | `inputBleedAmount` / `Opacity` | `chromaticAberration` | — | [`GlassSpec::dispersion`] |
//! | 磨砂 / 多层模糊 | `u_frostAmount` + blur layer | `inputBlurDistance0..4` | `depthEffect` / blur | `backdrop-filter: blur()` | [`GlassSpec::frost`] |
//! | 斜向高光 | Fresnel + specular | `highlightsDisplayAngle` | `HighlightStyle.Default(angle=45)` | `:after` 45° 两段白 | [`GlassSpec::highlight_angle`] |
//! | 平滑度 | — | `smoothness` | — | — | [`GlassSpec::smoothness`] |
//! | 白点提亮 | — | `boostWhitePoint` | — | — | 由 `theme::glass()` 的亮端 alpha 下限承担 |
//!
//! 这些参数本身不含颜色：颜色一律来自 [`crate::ui::theme::Glass`] 令牌，
//! 由 [`crate::ui::glass::resolve`] 解析成 GPUI 能画的具体层。
//! 这样组件库不会绕过「语义色优先」的约定，也不会破坏半透背景链。

use crate::ui::theme::RADIUS_PANEL;

/// 一块玻璃的全部可调量。
///
/// 所有数值都允许越界传入，[`GlassSpec::normalized`] 会夹到合法区间；
/// 因此外部可以直接把滑杆 / 配置值写进来，不必自己判边界。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassSpec {
    /// 圆角半径（CSS px）。
    pub radius: f32,
    /// 面层不透明度倍率。GPUI 没有 `backdrop-filter`，面板「实不实」只能靠 alpha。
    pub fill: f32,
    /// 边缘折射带宽度（px）。对应 Android `refractionHeight`、iOS `input*RefractionHeight`。
    pub refraction_height: f32,
    /// 折射强度 0..1。决定 inset 亮边的条数、偏移与 alpha，近似 SDF 边缘法线。
    pub refraction_amount: f32,
    /// 色散 / 渗色 0..1。对应 `chromaticAberration` 与 `inputBleedAmount`。
    pub dispersion: f32,
    /// 磨砂 0..1。越大面层越实、高光越弱，近似 frosted blur layer。
    pub frost: f32,
    /// 斜向高光角度（0..360 度，0 为向上、顺时针）。
    pub highlight_angle: f32,
    /// 高光强度 0..1。
    pub highlight: f32,
    /// 外缘描边倍率。
    pub rim: f32,
    /// 外投影倍率。
    pub shadow: f32,
    /// 平滑度 0..1（iOS `smoothness`）。越大 inset 亮边越软、层数越少。
    pub smoothness: f32,
    /// 深度效果：Android `depthEffect` / iOS `contentLensing`。开启后补一道向内暗边。
    pub depth: bool,
}

impl Default for GlassSpec {
    fn default() -> Self {
        Self::regular()
    }
}

impl GlassSpec {
    /// 常规玻璃：折射与磨砂平衡，界面默认用这一档。
    pub const fn regular() -> Self {
        Self {
            radius: RADIUS_PANEL,
            fill: 1.,
            refraction_height: 10.,
            refraction_amount: 0.45,
            dispersion: 0.,
            frost: 0.30,
            highlight_angle: 45.,
            highlight: 0.60,
            rim: 1.,
            shadow: 1.,
            smoothness: 0.50,
            depth: false,
        }
    }

    /// 素面：无玻璃层，用来做 A/B 对照和降级（低性能设备 / 关闭材质）。
    pub const fn plain() -> Self {
        Self {
            radius: RADIUS_PANEL,
            fill: 1.,
            refraction_height: 0.,
            refraction_amount: 0.,
            dispersion: 0.,
            frost: 0.,
            highlight_angle: 45.,
            highlight: 0.,
            rim: 0.50,
            shadow: 0.40,
            smoothness: 1.,
            depth: false,
        }
    }

    /// 清透玻璃：强折射、薄面层、带一点色散，最接近「放大镜边缘」。
    pub const fn clear() -> Self {
        Self {
            radius: RADIUS_PANEL,
            fill: 0.55,
            refraction_height: 18.,
            refraction_amount: 0.85,
            dispersion: 0.35,
            frost: 0.05,
            highlight_angle: 45.,
            highlight: 0.85,
            rim: 1.,
            shadow: 0.70,
            smoothness: 0.25,
            depth: true,
        }
    }

    /// 磨砂玻璃：面层实、高光弱、几乎不折射，文字可读性最好。
    pub const fn frosted() -> Self {
        Self {
            radius: RADIUS_PANEL,
            fill: 1.25,
            refraction_height: 6.,
            refraction_amount: 0.20,
            dispersion: 0.,
            frost: 0.90,
            highlight_angle: 45.,
            highlight: 0.30,
            rim: 0.80,
            shadow: 0.80,
            smoothness: 0.80,
            depth: false,
        }
    }

    /// 强调玻璃：强色散 + 强高光，用于很短的强调元素（徽标、选中态），不要大面积铺。
    pub const fn vibrant() -> Self {
        Self {
            radius: RADIUS_PANEL,
            fill: 0.90,
            refraction_height: 22.,
            refraction_amount: 0.90,
            dispersion: 0.85,
            frost: 0.20,
            highlight_angle: 45.,
            highlight: 1.,
            rim: 1.20,
            shadow: 1.10,
            smoothness: 0.15,
            depth: true,
        }
    }

    /// 把所有字段夹到合法区间。解析层永远只对归一化后的参数负责。
    pub fn normalized(self) -> Self {
        Self {
            radius: self.radius.clamp(0., 64.),
            fill: self.fill.clamp(0.20, 1.50),
            refraction_height: self.refraction_height.clamp(0., 32.),
            refraction_amount: self.refraction_amount.clamp(0., 1.),
            dispersion: self.dispersion.clamp(0., 1.),
            frost: self.frost.clamp(0., 1.),
            highlight_angle: self.highlight_angle.rem_euclid(360.),
            highlight: self.highlight.clamp(0., 1.),
            rim: self.rim.clamp(0., 1.50),
            shadow: self.shadow.clamp(0., 1.50),
            smoothness: self.smoothness.clamp(0., 1.),
            depth: self.depth,
        }
    }

    /// 两套参数之间的线性插值，用于预设切换动画与「材质强度」这类连续量。
    /// 布尔字段没有中间态，过半才切换。
    ///
    /// 当前界面还没有逐帧过渡，所以 binary 内暂未调用；这是组件库对外 API，
    /// 由单测覆盖端点、中点与角度最短路。
    #[allow(dead_code)]
    pub fn lerp(from: Self, to: Self, t: f32) -> Self {
        let t = t.clamp(0., 1.);
        let f = from.normalized();
        let g = to.normalized();
        // 端点直接返回：走插值式会引入浮点误差（0.2 变成 0.19999999），
        // 预设切换时 `assert_eq` 与「参数没变」的判断都会踩到。
        if t == 0. {
            return f;
        }
        if t == 1. {
            return g;
        }
        let mix = |a: f32, b: f32| a + (b - a) * t;
        // 角度走最短弧，避免 350° → 10° 时绕一整圈。
        let mut delta = (g.highlight_angle - f.highlight_angle).rem_euclid(360.);
        if delta > 180. {
            delta -= 360.;
        }
        Self {
            radius: mix(f.radius, g.radius),
            fill: mix(f.fill, g.fill),
            refraction_height: mix(f.refraction_height, g.refraction_height),
            refraction_amount: mix(f.refraction_amount, g.refraction_amount),
            dispersion: mix(f.dispersion, g.dispersion),
            frost: mix(f.frost, g.frost),
            highlight_angle: (f.highlight_angle + delta * t).rem_euclid(360.),
            highlight: mix(f.highlight, g.highlight),
            rim: mix(f.rim, g.rim),
            shadow: mix(f.shadow, g.shadow),
            smoothness: mix(f.smoothness, g.smoothness),
            depth: if t < 0.5 { f.depth } else { g.depth },
        }
        .normalized()
    }
}

/// 开箱即用的预设。参数可在预设之上继续微调。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlassPreset {
    /// 常规玻璃。
    #[default]
    Regular,
    /// 素面（对照 / 降级）。
    Plain,
    /// 清透。
    Clear,
    /// 磨砂。
    Frosted,
    /// 强调。
    Vibrant,
}

impl GlassPreset {
    /// 全部预设，顺序即展示顺序。
    pub const ALL: [Self; 5] = [
        Self::Regular,
        Self::Plain,
        Self::Clear,
        Self::Frosted,
        Self::Vibrant,
    ];

    /// 中文短名，用于界面按钮。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Regular => "常规",
            Self::Plain => "素面",
            Self::Clear => "清透",
            Self::Frosted => "磨砂",
            Self::Vibrant => "强调",
        }
    }

    /// 一句话说明，用于实验室页与文档。
    pub const fn description(self) -> &'static str {
        match self {
            Self::Regular => "折射与磨砂平衡，界面默认档。",
            Self::Plain => "关掉所有玻璃层，用来做 A/B 对照。",
            Self::Clear => "薄面层 + 强折射 + 轻微色散，边缘像放大镜。",
            Self::Frosted => "面层最实、高光最弱，长文字可读性最好。",
            Self::Vibrant => "强色散与强高光，只适合徽标一类小面积元素。",
        }
    }

    /// 预设对应的参数。
    pub const fn spec(self) -> GlassSpec {
        match self {
            Self::Regular => GlassSpec::regular(),
            Self::Plain => GlassSpec::plain(),
            Self::Clear => GlassSpec::clear(),
            Self::Frosted => GlassSpec::frosted(),
            Self::Vibrant => GlassSpec::vibrant(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GlassPreset, GlassSpec};

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn presets_are_all_normalized() {
        for preset in GlassPreset::ALL {
            let spec = preset.spec();
            assert_eq!(spec.normalized(), spec, "{} 预设参数越界", preset.label());
        }
    }

    #[test]
    fn preset_labels_are_unique_and_non_empty() {
        for (index, preset) in GlassPreset::ALL.iter().enumerate() {
            assert!(!preset.label().is_empty());
            assert!(!preset.description().is_empty());
            for other in &GlassPreset::ALL[index + 1..] {
                assert_ne!(preset.label(), other.label());
            }
        }
    }

    #[test]
    fn interpolation_hits_both_endpoints() {
        let a = GlassSpec::clear();
        let b = GlassSpec::frosted();
        assert_eq!(GlassSpec::lerp(a, b, 0.), a);
        assert_eq!(GlassSpec::lerp(a, b, 1.), b);
        assert_eq!(GlassSpec::lerp(a, b, -3.), a);
        assert_eq!(GlassSpec::lerp(a, b, 7.), b);
    }

    #[test]
    fn interpolation_midpoint_is_the_average() {
        let a = GlassSpec::plain();
        let b = GlassSpec::vibrant();
        let mid = GlassSpec::lerp(a, b, 0.5);
        assert!(near(
            mid.refraction_amount,
            (a.refraction_amount + b.refraction_amount) / 2.
        ));
        assert!(near(mid.dispersion, (a.dispersion + b.dispersion) / 2.));
        // 布尔字段过半才切到目标。
        assert_eq!(mid.depth, b.depth);
    }

    #[test]
    fn angle_takes_the_short_arc() {
        let mut a = GlassSpec::regular();
        a.highlight_angle = 350.;
        let mut b = GlassSpec::regular();
        b.highlight_angle = 10.;
        let mid = GlassSpec::lerp(a, b, 0.5);
        assert!(
            near(mid.highlight_angle, 0.),
            "实际 {}",
            mid.highlight_angle
        );
    }

    #[test]
    fn normalization_clamps_every_numeric_field() {
        let wild = GlassSpec {
            radius: -40.,
            fill: 99.,
            refraction_height: -5.,
            refraction_amount: 4.,
            dispersion: -1.,
            frost: 3.,
            highlight_angle: 725.,
            highlight: -0.5,
            rim: 8.,
            shadow: -2.,
            smoothness: 12.,
            depth: true,
        }
        .normalized();
        assert_eq!(wild.radius, 0.);
        assert_eq!(wild.fill, 1.50);
        assert_eq!(wild.refraction_height, 0.);
        assert_eq!(wild.refraction_amount, 1.);
        assert_eq!(wild.dispersion, 0.);
        assert_eq!(wild.frost, 1.);
        assert!(near(wild.highlight_angle, 5.));
        assert_eq!(wild.highlight, 0.);
        assert_eq!(wild.rim, 1.50);
        assert_eq!(wild.shadow, 0.);
        assert_eq!(wild.smoothness, 1.);
    }
}
