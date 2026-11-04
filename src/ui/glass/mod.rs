//! `ui::glass` —— 液态玻璃组件库。
//!
//! 把「玻璃」从页面里散落的 `.bg() / .border_1() / .shadow(vec![...])` 收成三层：
//!
//! ```text
//! spec.rs     参数：GlassSpec / GlassPreset / lerp   （纯数据，不依赖 GUI 状态）
//!    ↓
//! resolve.rs  解析：spec + theme::Glass 令牌 → GlassLayers（纯函数，可单测）
//!    ↓
//! surface.rs  组件：surface / card / panel / pill   （返回 Div，调用方继续 .child()）
//! ```
//!
//! ## 为什么要分层
//!
//! 参考实现里「参数」和「颜色」常常混在一起（CSS 里直接写 `rgba(255,255,255,.3)`，
//! shader 里把 rim 颜色写进 uniform）。本仓库有两条硬约束：**语义色优先**、**玻璃必须真的透**；
//! 组件库一旦自己发明颜色，就会绕过 `theme::set_appearance` 的背景链，桌面内容再也透不进来。
//! 所以参数层完全不含颜色，颜色只在解析层从 [`crate::ui::theme::Glass`] 取。
//!
//! ## 与五份参考的对应
//!
//! 见 [`spec`] 模块文档里的对照表。一句话：**借它们的参数模型和分层顺序，
//! 不借它们的渲染管线**——GPUI 0.2 没有自定义 shader、没有 `backdrop-filter`、
//! 线性渐变只有两个 color stop、也没有径向渐变（大半径模糊还会出现多边形棱边）。
//! 因此这里做的是「屏幕空间近似」：
//!
//! - 折射 → 多条对称 inset 亮边，条数 / 偏移 / alpha 由折射参数驱动；
//! - 磨砂 → 提高面层 alpha（真实模糊由窗口层的 DWM 亚克力负责）；
//! - 色散 → 一对冷 / 暖色 inset 细边；
//! - 斜向高光 → 两个 color stop 之间的 alpha 差；
//! - 多层模糊 / Fresnel → 外投影 + 逐圈变淡的 inset 层。
//!
//! 详见 `docs/液态玻璃组件库.md`。

pub mod resolve;
pub mod spec;
pub mod surface;

// 组件库对外 API。这是个 binary crate，内部不一定每一项都用到，
// 但 `glass::card` / `glass::GlassSpec` 这种调用形式是文档约定的入口。
#[allow(unused_imports)]
pub use spec::{GlassPreset, GlassSpec};
#[allow(unused_imports)]
pub use surface::{card, panel, pill, preset_card, surface};
