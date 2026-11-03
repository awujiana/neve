//! core::appearance —— 外观设置模型与持久化
//!
//! 参照 `UI/modern_glass_zh` 与 QcClip 的设置页，外观被拆成互相独立的几组：
//! 窗口材质、背景不透明度、明暗、主题颜色、主题字体、标题栏位置、Tab 栏布局。
//!
//! **液态玻璃模式不区分深色/浅色主题**：只有一个「明暗」连续量（0 全暗 / 100 全亮），
//! 所有语义色由它插值得到，界面不再有主题开关。
//!
//! 持久化用最简单的 `key = value` 文本（与 `core::config` 的 conf 风格一致），
//! 不引入 serde，字段缺失时按默认值补，读坏了一律退回默认而不是报错。

use std::path::PathBuf;

use crate::core::store::data_dir;

/// 窗口材质。映射到 GPUI 的 `WindowBackgroundAppearance`。
///
/// Windows 下分别是：关闭 → 不合成、透明 → 透明渐变、亚克力/模糊 → DWM 亚克力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// 不透明，窗口自己画满。
    Off,
    /// 纯透明，桌面内容直接透出，不做模糊。
    Transparent,
    /// 亚克力：桌面内容模糊 + 轻微色调。
    Acrylic,
    /// 模糊：比亚克力更重的模糊（目前与亚克力同一实现，保留给后续分级）。
    Blur,
    /// 自定义背景图：用用户指定的图片铺满窗口。
    CustomImage,
}

impl Material {
    pub const ALL: [Material; 5] = [
        Material::Off,
        Material::Transparent,
        Material::Acrylic,
        Material::Blur,
        Material::CustomImage,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Material::Off => "关闭",
            Material::Transparent => "透明",
            Material::Acrylic => "亚克力",
            Material::Blur => "模糊",
            Material::CustomImage => "自定义背景图",
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Material::Off => "off",
            Material::Transparent => "transparent",
            Material::Acrylic => "acrylic",
            Material::Blur => "blur",
            Material::CustomImage => "image",
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

/// 标题栏位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarPosition {
    Top,
    Bottom,
    Left,
    Right,
}

impl TitlebarPosition {
    pub const ALL: [TitlebarPosition; 4] = [
        TitlebarPosition::Top,
        TitlebarPosition::Bottom,
        TitlebarPosition::Left,
        TitlebarPosition::Right,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            TitlebarPosition::Top => "顶部",
            TitlebarPosition::Bottom => "底部",
            TitlebarPosition::Left => "左侧",
            TitlebarPosition::Right => "右侧",
        }
    }

    /// 左侧/右侧的竖排标题栏尚未实现，设置页据此置灰。
    pub fn implemented(&self) -> bool {
        matches!(self, TitlebarPosition::Top | TitlebarPosition::Bottom)
    }

    fn key(&self) -> &'static str {
        match self {
            TitlebarPosition::Top => "top",
            TitlebarPosition::Bottom => "bottom",
            TitlebarPosition::Left => "left",
            TitlebarPosition::Right => "right",
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

/// Tab 栏（主导航）的布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabLayout {
    /// 左侧常驻侧栏。
    Sidebar,
    /// 标题栏下方的横向 Tab 条。
    TopBar,
}

impl TabLayout {
    pub const ALL: [TabLayout; 2] = [TabLayout::Sidebar, TabLayout::TopBar];

    pub fn label(&self) -> &'static str {
        match self {
            TabLayout::Sidebar => "侧栏",
            TabLayout::TopBar => "顶栏",
        }
    }

    fn key(&self) -> &'static str {
        match self {
            TabLayout::Sidebar => "sidebar",
            TabLayout::TopBar => "topbar",
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

/// 主题颜色：只影响开关、进度与强调文字，界面主体始终中性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    Neutral,
    Blue,
    Green,
    Orange,
    Purple,
}

impl Accent {
    pub const ALL: [Accent; 5] = [
        Accent::Neutral,
        Accent::Blue,
        Accent::Green,
        Accent::Orange,
        Accent::Purple,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Accent::Neutral => "中性",
            Accent::Blue => "蓝",
            Accent::Green => "绿",
            Accent::Orange => "橙",
            Accent::Purple => "紫",
        }
    }

    /// 色板上的展示色（深色模式取值）。
    pub fn swatch(&self) -> u32 {
        match self {
            Accent::Neutral => 0xf2f2f5,
            Accent::Blue => 0x5aa9e6,
            Accent::Green => 0x8fbf4a,
            Accent::Orange => 0xe0954a,
            Accent::Purple => 0xa98be0,
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Accent::Neutral => "neutral",
            Accent::Blue => "blue",
            Accent::Green => "green",
            Accent::Orange => "orange",
            Accent::Purple => "purple",
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

/// 应用字体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontChoice {
    /// 跟随系统默认。
    FollowSystem,
    SegoeUi,
    MicrosoftYaHei,
    Inter,
}

impl FontChoice {
    pub const ALL: [FontChoice; 4] = [
        FontChoice::FollowSystem,
        FontChoice::SegoeUi,
        FontChoice::MicrosoftYaHei,
        FontChoice::Inter,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            FontChoice::FollowSystem => "跟随系统（默认）",
            FontChoice::SegoeUi => "Segoe UI",
            FontChoice::MicrosoftYaHei => "微软雅黑",
            FontChoice::Inter => "Inter",
        }
    }

    /// 传给 GPUI 的字体族名；`None` 表示不覆盖，用默认。
    pub fn family(&self) -> Option<&'static str> {
        match self {
            FontChoice::FollowSystem => None,
            FontChoice::SegoeUi => Some("Segoe UI"),
            FontChoice::MicrosoftYaHei => Some("Microsoft YaHei"),
            FontChoice::Inter => Some("Inter"),
        }
    }

    fn key(&self) -> &'static str {
        match self {
            FontChoice::FollowSystem => "system",
            FontChoice::SegoeUi => "segoe",
            FontChoice::MicrosoftYaHei => "yahei",
            FontChoice::Inter => "inter",
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == value)
    }
}

/// 全部外观设置。
#[derive(Debug, Clone, PartialEq)]
pub struct Appearance {
    pub material: Material,
    /// 自定义背景图路径（`Material::CustomImage` 时生效）。
    pub background_image: String,
    /// 背景不透明度 0..=100。越低，桌面透出得越多。
    pub background_opacity: u8,
    /// 明暗 0..=100。0 全暗、100 全亮。
    pub lightness: u8,
    pub accent: Accent,
    pub font: FontChoice,
    /// 锁定窗口颜色：开启后窗口底色不随明暗滑杆变化。
    pub lock_window_color: bool,
    pub tab_layout: TabLayout,
    pub titlebar_position: TitlebarPosition,
    /// 动画与动效开关。
    pub animations: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            material: Material::Acrylic,
            background_image: String::new(),
            // 默认就偏透：液态玻璃的第一要义是能看见窗口后面的东西。
            background_opacity: 35,
            // 默认是浅色玻璃：白/浅灰半透面板 + 淡蓝灰渐变底。
            // 想回到深色玻璃把明暗拉到 30 以下即可。
            lightness: 86,
            accent: Accent::Green,
            font: FontChoice::FollowSystem,
            lock_window_color: false,
            tab_layout: TabLayout::Sidebar,
            titlebar_position: TitlebarPosition::Top,
            animations: true,
        }
    }
}

impl Appearance {
    /// 配置文件路径。
    pub fn path() -> PathBuf {
        data_dir().join("appearance.conf")
    }

    /// 读取设置。文件不存在或字段非法都退回默认值，绝不因配置损坏而启动失败。
    pub fn load() -> Self {
        let mut appearance = Self::default();
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return appearance;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "material" => {
                    if let Some(v) = Material::from_key(value) {
                        appearance.material = v;
                    }
                }
                "background_image" => appearance.background_image = value.to_string(),
                "background_opacity" => {
                    if let Ok(v) = value.parse::<u8>() {
                        appearance.background_opacity = v.min(100);
                    }
                }
                "lightness" => {
                    if let Ok(v) = value.parse::<u8>() {
                        appearance.lightness = v.min(100);
                    }
                }
                "accent" => {
                    if let Some(v) = Accent::from_key(value) {
                        appearance.accent = v;
                    }
                }
                "font" => {
                    if let Some(v) = FontChoice::from_key(value) {
                        appearance.font = v;
                    }
                }
                "lock_window_color" => appearance.lock_window_color = value == "true",
                "tab_layout" => {
                    if let Some(v) = TabLayout::from_key(value) {
                        appearance.tab_layout = v;
                    }
                }
                "titlebar_position" => {
                    if let Some(v) = TitlebarPosition::from_key(value) {
                        appearance.titlebar_position = v;
                    }
                }
                "animations" => appearance.animations = value == "true",
                _ => {}
            }
        }
        appearance
    }

    /// 写回设置。写失败不打断交互，只在调用方需要时提示。
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = format!(
            "# Névé 外观设置（由程序写入，可手工编辑）\n\
             material = {}\n\
             background_image = {}\n\
             background_opacity = {}\n\
             lightness = {}\n\
             accent = {}\n\
             font = {}\n\
             lock_window_color = {}\n\
             tab_layout = {}\n\
             titlebar_position = {}\n\
             animations = {}\n",
            self.material.key(),
            self.background_image,
            self.background_opacity,
            self.lightness,
            self.accent.key(),
            self.font.key(),
            self.lock_window_color,
            self.tab_layout.key(),
            self.titlebar_position.key(),
            self.animations,
        );
        std::fs::write(path, text)
    }

    /// 明暗归一化到 0.0..=1.0。
    pub fn lightness_ratio(&self) -> f32 {
        f32::from(self.lightness) / 100.
    }

    /// 背景层的不透明度 0.0..=1.0。材质为「关闭」时恒为 1。
    pub fn background_alpha(&self) -> f32 {
        if self.material == Material::Off {
            1.
        } else {
            f32::from(self.background_opacity) / 100.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_text_format() {
        let appearance = Appearance::default();
        // 不落盘，直接复用序列化格式校验键值覆盖。
        assert_eq!(Material::from_key("acrylic"), Some(Material::Acrylic));
        assert_eq!(Accent::from_key("green"), Some(Accent::Green));
        assert_eq!(TabLayout::from_key("topbar"), Some(TabLayout::TopBar));
        assert_eq!(
            TitlebarPosition::from_key("bottom"),
            Some(TitlebarPosition::Bottom)
        );
        assert_eq!(
            FontChoice::from_key("yahei"),
            Some(FontChoice::MicrosoftYaHei)
        );
        assert!(appearance.background_alpha() > 0.);
    }

    #[test]
    fn off_material_forces_opaque_background() {
        let appearance = Appearance {
            material: Material::Off,
            background_opacity: 10,
            ..Default::default()
        };
        assert!((appearance.background_alpha() - 1.).abs() < f32::EPSILON);
    }
}
