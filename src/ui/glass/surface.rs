//! 液态玻璃的**组件层**：一组返回 `Div` 的可复用 builder。
//!
//! 全部组件都只是「把 [`GlassLayers`] 贴到一个基础 `Div` 上」，不含状态、
//! 不订阅事件、不读配置，因此可以自由嵌进任何页面，也不会给 `core` 层带来依赖。
//! 调用方照常用 `.child(...)` / `.gap_*()` 继续组装。
//!
//! ```ignore
//! let g = theme::current_glass(cx);
//! let ratio = theme::appearance(cx).lightness_ratio();
//! glass::card(&GlassSpec::clear(), &g, ratio)
//!     .child("清透玻璃卡片")
//! ```

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{Div, Styled as _, div, linear_color_stop, linear_gradient, px};

use super::resolve::{GlassLayers, resolve};
use super::spec::{GlassPreset, GlassSpec};
use crate::ui::theme::{Glass, RADIUS_PILL};

/// 把解析好的层贴到基础容器上。所有组件共用这一处，保证层次顺序一致：
/// 圆角 → 斜向渐变底 → 1px 外缘 → 阴影层。
fn decorate(base: Div, layers: GlassLayers, radius: f32) -> Div {
    let background = linear_gradient(
        layers.gradient_angle,
        linear_color_stop(layers.gradient_start, 0.),
        linear_color_stop(layers.gradient_end, 1.),
    );
    base.rounded(px(radius))
        .bg(background)
        .border_1()
        .border_color(layers.outline)
        .shadow(layers.shadows)
}

/// 最基础的玻璃表面。不带 flex 方向，适合自己决定布局。
pub fn surface(spec: &GlassSpec, glass: &Glass, lightness_ratio: f32) -> Div {
    let radius = spec.normalized().radius;
    decorate(div(), resolve(spec, glass, lightness_ratio), radius)
}

/// 卡片：纵向 flex + 既定内距与间距，对应原先散落在页面里的 `ui::card()`。
pub fn card(spec: &GlassSpec, glass: &Glass, lightness_ratio: f32) -> Div {
    let radius = spec.normalized().radius;
    decorate(
        v_flex().gap_3().p_4(),
        resolve(spec, glass, lightness_ratio),
        radius,
    )
}

/// 常驻容器：横向 flex，用于标题栏、侧栏、底部状态条一类条状表面。
pub fn panel(spec: &GlassSpec, glass: &Glass, lightness_ratio: f32) -> Div {
    let radius = spec.normalized().radius;
    decorate(
        h_flex().items_center(),
        resolve(spec, glass, lightness_ratio),
        radius,
    )
}

/// 胶囊：徽标、选中态、标签。圆角固定用 [`RADIUS_PILL`]，参数里的 radius 不生效。
pub fn pill(spec: &GlassSpec, glass: &Glass, lightness_ratio: f32) -> Div {
    decorate(
        h_flex().items_center().gap_1().px_2().py_1(),
        resolve(spec, glass, lightness_ratio),
        RADIUS_PILL,
    )
}

/// 预设快捷入口：一行代码拿到某个预设的卡片。
pub fn preset_card(preset: GlassPreset, glass: &Glass, lightness_ratio: f32) -> Div {
    card(&preset.spec(), glass, lightness_ratio)
}

#[cfg(test)]
mod tests {
    use super::{card, pill, surface};
    use crate::core::appearance::Appearance;
    use crate::ui::glass::spec::{GlassPreset, GlassSpec};
    use crate::ui::theme;

    #[test]
    fn every_preset_builds_without_panicking() {
        let glass = theme::glass(&Appearance::default());
        for preset in GlassPreset::ALL {
            let spec = preset.spec();
            let _ = surface(&spec, &glass, 0.86);
            let _ = card(&spec, &glass, 0.86);
            let _ = super::panel(&spec, &glass, 0.16);
            let _ = pill(&spec, &glass, 0.16);
        }
    }

    #[test]
    fn extreme_parameters_still_build() {
        let glass = theme::glass(&Appearance::default());
        let wild = GlassSpec {
            radius: -40.,
            fill: 99.,
            refraction_height: 500.,
            refraction_amount: 4.,
            dispersion: 4.,
            frost: -2.,
            highlight_angle: 999.,
            highlight: 4.,
            rim: 9.,
            shadow: 9.,
            smoothness: -1.,
            depth: true,
        };
        let _ = card(&wild, &glass, 0.5);
    }
}
