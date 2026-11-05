//! core::skins —— MPV UI 皮肤（uosc 主题 / 内置 OSC）的扫描与预览数据（FR-10）
//!
//! uosc 把整套界面参数都写在 `script-opts/uosc.conf` 里：`color=` 是配色表，
//! `controls=` 是底部控制条的元素序列，`top_bar_*` / `timeline_*` / `menu_*`
//! 决定顶栏、时间轴与菜单的尺寸。皮肤页的预览画布据此**按真实配置绘制**，
//! 而不是画一张示意图：不同整合包的 `color=` 不同，切换方案时预览配色随之变化。
//!
//! 切换皮肤沿用 FR-10 的「单一 OSC 提供者」原则：uosc 需要 `mpv.conf` 里的
//! `osc=no`，内置 OSC 则把这一行注释掉。绝不删除用户文件，写入前必备份。
//!
//! `core` 层只输出 `u32`（`0xRRGGBB`），颜色到 `Hsla` 的转换在 `ui::theme::color` 完成。

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::backup::{BackupManager, BackupRecord};
use crate::core::config::conf::ConfDoc;
use crate::core::filters;
use crate::core::players::DetectedPlayer;

/// UI 提供者类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinKind {
    /// mpv 内置的 osc.lua（经典底部控制条）
    BuiltinOsc,
    /// uosc 脚本自绘界面
    Uosc,
}

impl SkinKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltinOsc => "内置 OSC",
            Self::Uosc => "uosc",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::BuiltinOsc => {
                "mpv 自带 osc.lua：底部时间条 + 按钮行，配置在 script-opts/osc.conf"
            }
            Self::Uosc => "uosc 自绘界面：顶栏、时间轴、控制条、菜单与字幕，全部由 uosc.conf 驱动",
        }
    }
}

/// uosc 的 `color=` 配色表。
///
/// 键名严格对齐 uosc 的 `config_defaults.color`（见 uosc `main.lua`）：
/// 只有 `foreground` / `foreground_text` / `background` / `background_text` /
/// `window_border` / `curtain` / `success` / `error` / `match` / `heatmap` 十个键。
/// 菜单、时间轴、按钮的配色**全部由这十个颜色 + `opacity` 推导**，
/// 所以这里不再自造 `menu_*` / `timeline_*` 之类的键——那会让预览和真实 uosc 对不上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinPalette {
    /// 主前景色：进度条已播放段、按钮图标、菜单标题底色
    pub foreground: u32,
    /// 前景色之上的文字色（进度条已播放段里的时间、菜单标题文字）
    pub foreground_text: u32,
    /// 主背景色：时间轴底槽、菜单底板、顶栏标题胶囊
    pub background: u32,
    /// 背景色之上的文字色（菜单项、未播放段时间）
    pub background_text: u32,
    /// 窗口外框
    pub window_border: u32,
    /// 暂停遮罩
    pub curtain: u32,
    pub success: u32,
    pub error: u32,
    /// `match`：菜单搜索命中高亮
    pub accent: u32,
    /// `heatmap`：YouTube 热力图
    pub heatmap: u32,
}

impl Default for SkinPalette {
    /// uosc 官方默认值（`ffffff` / `000000` / `000000` / `ffffff` / …）。
    fn default() -> Self {
        Self {
            foreground: 0xff_ff_ff,
            foreground_text: 0x00_00_00,
            background: 0x00_00_00,
            background_text: 0xff_ff_ff,
            window_border: 0x00_00_00,
            curtain: 0x11_11_11,
            success: 0xa5_e0_75,
            error: 0xff_61_6e,
            accent: 0x69_c5_ff,
            heatmap: 0x00_ad_ee,
        }
    }
}

impl SkinPalette {
    /// 解析 `color=key=RRGGBB,key=RRGGBB,…`。未知键忽略，非法 hex 回退默认。
    pub fn from_color_value(value: &str) -> Self {
        let mut palette = Self::default();
        for pair in value.split(',') {
            let Some(eq) = pair.find('=') else { continue };
            let key = pair[..eq].trim();
            let Some(rgb) = parse_hex(pair[eq + 1..].trim()) else {
                continue;
            };
            match key {
                "foreground" => palette.foreground = rgb,
                "foreground_text" => palette.foreground_text = rgb,
                "background" => palette.background = rgb,
                "background_text" => palette.background_text = rgb,
                "window_border" => palette.window_border = rgb,
                "curtain" => palette.curtain = rgb,
                "success" => palette.success = rgb,
                "error" => palette.error = rgb,
                "match" => palette.accent = rgb,
                "heatmap" => palette.heatmap = rgb,
                _ => {}
            }
        }
        palette
    }

    /// 配色的简短说明，供方案列表展示。
    pub fn summary(&self) -> String {
        format!(
            "前景 #{:06X} / 背景 #{:06X} / 强调 #{:06X}",
            self.foreground, self.background, self.accent
        )
    }
}

/// `opacity=` 里预览会用到的几项。默认值取自 uosc `config_defaults.opacity`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinOpacity {
    /// 时间轴底槽
    pub timeline: f32,
    /// 时间轴已播放段（uosc 默认 1，即实心）
    pub position: f32,
    /// 章节菱形
    pub chapters: f32,
    /// 按钮底色。uosc 默认 0：按钮常态只有图标、没有底色块
    pub controls: f32,
    pub menu: f32,
    /// 顶栏标题胶囊
    pub title: f32,
    pub curtain: f32,
}

impl Default for SkinOpacity {
    fn default() -> Self {
        Self {
            timeline: 0.9,
            position: 1.0,
            chapters: 0.8,
            controls: 0.0,
            menu: 1.0,
            title: 1.0,
            curtain: 0.8,
        }
    }
}

/// `controls=` 里一个条目的种类。uosc 按种类决定宽度分配方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    /// 固定 `controls_size` 见方的图标按钮
    Button,
    /// 速度滑块，比普通按钮宽
    Speed,
    /// 固定比例的间隙
    Gap,
    /// 把剩余宽度摊开的弹簧，用来左右分组
    Space,
}

/// `controls=` 里一个条目。
#[derive(Debug, Clone, PartialEq)]
pub struct ControlItem {
    pub kind: ControlKind,
    /// 图标名：快捷指令名（`play-pause`），或 `command:` / `button:` /
    /// `cycle:` / `toggle:` 的 `{icon}` 参数（如 `danmaku`、`history`）。
    pub icon: String,
    pub tooltip: Option<String>,
    /// `gap` / `speed` 的尺寸系数
    pub scale: f32,
    /// 可见性条件：外层是「或」（逗号分隔的组），内层是「与」（`+` 连接），
    /// 布尔表示是否带 `!` 取反。空表示无条件可见。
    ///
    /// uosc 的写法是 `<video,audio>`（有视频**或**有音频）、`<has_audio+!audio>`
    /// （有音轨且当前不是纯音频）、`<!idle>`（非空闲）。真实整合包的 `controls=`
    /// 里几乎每个条目都带条件，忽略它就会把 20 多个图标全画出来，和实机完全不同。
    pub dispositions: Vec<Vec<(String, bool)>>,
}

/// 预览里模拟的播放状态，用来求值 `dispositions`。
///
/// 字段名与 uosc 的判定名一一对应：`has_*` 直接叫 `has_*`，`is_*` 在 uosc 里写作
/// `video` / `audio` / `image` / `idle` / `stream`（`Controls.lua` 会给它们加 `is_` 前缀）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackState {
    pub is_idle: bool,
    pub is_video: bool,
    pub is_audio: bool,
    pub is_image: bool,
    pub is_stream: bool,
    pub has_video: bool,
    pub has_audio: bool,
    pub has_image: bool,
    pub has_sub: bool,
    pub has_chapter: bool,
    pub has_playlist: bool,
    pub has_many_edition: bool,
}

impl PlaybackState {
    /// 预览画布模拟的是一段带字幕、带章节的视频。
    pub const SAMPLE_VIDEO: Self = Self {
        is_idle: false,
        is_video: true,
        is_audio: false,
        is_image: false,
        is_stream: false,
        has_video: true,
        has_audio: true,
        has_image: false,
        has_sub: true,
        has_chapter: true,
        has_playlist: false,
        has_many_edition: false,
    };

    fn get(&self, name: &str) -> bool {
        match name {
            "is_idle" | "idle" => self.is_idle,
            "is_video" | "video" => self.is_video,
            "is_audio" | "audio" => self.is_audio,
            "is_image" | "image" => self.is_image,
            "is_stream" | "stream" => self.is_stream,
            "has_video" => self.has_video,
            "has_audio" => self.has_audio,
            "has_image" => self.has_image,
            "has_sub" => self.has_sub,
            "has_chapter" => self.has_chapter,
            "has_playlist" => self.has_playlist,
            "has_many_edition" => self.has_many_edition,
            _ => false,
        }
    }
}

impl ControlItem {
    /// 给人看的名字：优先用 tooltip（整合包都写了中文提示）。
    pub fn label(&self) -> String {
        match self.kind {
            ControlKind::Space => "space".to_string(),
            ControlKind::Gap => format!("gap:{}", self.scale),
            ControlKind::Button | ControlKind::Speed => {
                self.tooltip.clone().unwrap_or_else(|| self.icon.clone())
            }
        }
    }

    /// 在当前播放状态下是否可见。条件为空表示永远可见。
    pub fn visible_in(&self, state: &PlaybackState) -> bool {
        if self.dispositions.is_empty() {
            return true;
        }
        self.dispositions.iter().any(|group| {
            group
                .iter()
                .all(|(name, expected)| state.get(name) == *expected)
        })
    }
}

/// 从 uosc.conf 解析出的布局参数，预览画布据此定位每个元素。
#[derive(Debug, Clone, PartialEq)]
pub struct UoscLayout {
    pub top_bar: String,
    pub top_bar_size: f32,
    pub top_bar_title: bool,
    pub top_bar_controls: bool,
    /// `top_bar_alt_title`，uosc 支持 `${media-title}` 这类模板，这里原样保留
    pub top_bar_alt_title: Option<String>,
    pub top_bar_alt_place: String,
    pub timeline_style: String,
    pub timeline_size: f32,
    pub timeline_line_width: f32,
    /// 时间轴与画面之间的分隔带高度
    pub timeline_border: f32,
    pub progress: String,
    /// `controls=` 的元素序列（保留图标名与 tooltip）
    pub controls: Vec<ControlItem>,
    pub controls_size: f32,
    pub controls_spacing: f32,
    pub controls_margin: f32,
    pub menu_item_height: f32,
    pub menu_min_width: f32,
    pub menu_padding: f32,
    pub volume_side: String,
    pub window_border_size: f32,
    pub scale: f32,
    pub border_radius: f32,
    pub palette: SkinPalette,
    pub opacity: SkinOpacity,
}

impl Default for UoscLayout {
    /// uosc 官方默认值（`main.lua` 的 `config_defaults`）。
    fn default() -> Self {
        Self {
            top_bar: "no-border".to_string(),
            top_bar_size: 40.,
            top_bar_title: true,
            top_bar_controls: true,
            top_bar_alt_title: None,
            top_bar_alt_place: "below".to_string(),
            timeline_style: "bar".to_string(),
            timeline_size: 30.,
            timeline_line_width: 2.,
            timeline_border: 1.,
            progress: "windowed".to_string(),
            controls: parse_controls("menu,subtitles,audio,space,play-pause,playlist,fullscreen"),
            controls_size: 32.,
            controls_spacing: 2.,
            controls_margin: 8.,
            menu_item_height: 36.,
            menu_min_width: 260.,
            menu_padding: 1.,
            volume_side: "right".to_string(),
            window_border_size: 1.,
            scale: 1.,
            border_radius: 2.,
            palette: SkinPalette::default(),
            opacity: SkinOpacity::default(),
        }
    }
}

/// 一个可切换的皮肤方案。
#[derive(Debug, Clone, PartialEq)]
pub struct SkinPreset {
    pub name: String,
    pub player_name: String,
    pub kind: SkinKind,
    pub config_dir: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    /// uosc 用的 Material Icons 字体文件，存在时预览直接用真字形渲染图标。
    pub icon_font: Option<PathBuf>,
    pub layout: UoscLayout,
    pub note: String,
}

/// 在播放器配置目录里找 uosc 的图标字体。
///
/// uosc 的图标不是画出来的，是 `fonts/` 里 `MaterialIconsRound-Regular.otf` 的连字
/// 字形（`ass.lua` 里 `opts.font = 'MaterialIconsRound-Regular'`）。预览要画得一样，
/// 就必须用同一个字体文件，而不是自己拼几何形状。
pub fn find_icon_font(config_dir: &Path) -> Option<PathBuf> {
    let fonts = config_dir.join("fonts");
    let preferred = fonts.join("MaterialIconsRound-Regular.otf");
    if preferred.is_file() {
        return Some(preferred);
    }
    let entries = fs::read_dir(&fonts).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if name.contains("material") && (name.ends_with(".otf") || name.ends_with(".ttf")) {
            return Some(path);
        }
    }
    None
}

impl SkinPreset {
    pub fn key(&self) -> String {
        format!("{}-{}", self.player_name, self.kind.label())
    }
}

/// 从 uosc.conf 文本解析布局。缺项走 uosc 官方默认值。
pub fn parse_layout(text: &str) -> UoscLayout {
    let doc = ConfDoc::parse(text);
    let mut layout = UoscLayout::default();
    if let Some(value) = doc.get("color") {
        layout.palette = SkinPalette::from_color_value(&value);
    }
    if let Some(value) = doc.get("opacity") {
        layout.opacity = parse_opacity(&value);
    }
    string_option(&doc, "top_bar", &mut layout.top_bar);
    string_option(&doc, "timeline_style", &mut layout.timeline_style);
    string_option(&doc, "progress", &mut layout.progress);
    string_option(&doc, "volume", &mut layout.volume_side);
    layout.top_bar_size = number_option(&doc, "top_bar_size", layout.top_bar_size);
    layout.top_bar_title = bool_option(&doc, "top_bar_title", layout.top_bar_title);
    layout.top_bar_controls = bool_option(&doc, "top_bar_controls", layout.top_bar_controls);
    layout.timeline_size = number_option(&doc, "timeline_size", layout.timeline_size);
    layout.timeline_line_width =
        number_option(&doc, "timeline_line_width", layout.timeline_line_width);
    layout.timeline_border = number_option(&doc, "timeline_border", layout.timeline_border);
    layout.controls_size = number_option(&doc, "controls_size", layout.controls_size);
    layout.controls_spacing = number_option(&doc, "controls_spacing", layout.controls_spacing);
    layout.controls_margin = number_option(&doc, "controls_margin", layout.controls_margin);
    layout.menu_item_height = number_option(&doc, "menu_item_height", layout.menu_item_height);
    layout.menu_min_width = number_option(&doc, "menu_min_width", layout.menu_min_width);
    layout.menu_padding = number_option(&doc, "menu_padding", layout.menu_padding);
    layout.window_border_size =
        number_option(&doc, "window_border_size", layout.window_border_size);
    layout.scale = number_option(&doc, "scale", layout.scale);
    layout.border_radius = number_option(&doc, "border_radius", layout.border_radius);
    if let Some(value) = doc.get("top_bar_alt_title")
        && !value.trim().is_empty()
    {
        layout.top_bar_alt_title = Some(value);
    }
    string_option(
        &doc,
        "top_bar_alt_title_place",
        &mut layout.top_bar_alt_place,
    );
    if let Some(value) = doc.get("controls") {
        layout.controls = parse_controls(&value);
    }
    layout
}

/// 按逗号切分 `controls=`，但**不能切开 `<video,audio>` 这类可见性前缀**：
/// 前缀里的逗号是 OR 分组，盲切会把 `<video,audio>subtitles` 拆成两半，
/// 后半段变成一个不存在的元素名，预览里就会冒出一堆兜底图标。
fn split_control_list(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = (depth - 1).max(0),
            ',' if depth == 0 => {
                out.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(&value[start..]);
    out
}

/// 解析 `controls=`。
///
/// uosc 的条目语法是
/// `[<disposition>]{element}[:{param}][#{badge}[>{limit}]][?{tooltip}]`，
/// 其中 `command:` / `button:` / `cycle:` / `toggle:` 的**第一个参数就是图标名**。
/// 之前只留元素名会把它们统统压成 `command` / `button`，预览里自然只剩一排小圆点。
/// uosc `elements/Controls.lua` 里 `shorthands` 表的等价物：别名 → 展开后的配置。
///
/// 这张表必须照抄，原因有两层：
///
/// 1. `open-file` / `menu` / `subtitles` 这些是**别名**，不是图标名。uosc 先展开成
///    `command:folder:…` / `command:menu_book:…`，图标取的是展开后的第一个参数。
///    不展开就会拿别名去查字形，字体里根本没有这些连字。
/// 2. 冒号后的第一个参数就是 Material Icons 的连字名（`folder` / `menu_book` /
///    `closed_caption` …）。预览用同一个字体渲染同一个字符串，字形才能和 mpv 一致。
const CONTROL_SHORTHANDS: &[(&str, &str)] = &[
    ("play-pause", "cycle:pause:pause:no/yes=play_arrow"),
    ("menu", "command:menu_book:script-binding uosc/menu-blurred"),
    (
        "subtitles",
        "command:closed_caption:script-binding uosc/subtitles",
    ),
    ("audio", "command:graphic_eq:script-binding uosc/audio"),
    (
        "audio-device",
        "command:speaker:script-binding uosc/audio-device",
    ),
    ("video", "command:smart_display:script-binding uosc/video"),
    ("playlist", "command:list_alt:script-binding uosc/playlist"),
    (
        "chapters",
        "command:library_books:script-binding uosc/chapters",
    ),
    (
        "editions",
        "command:movie_filter:script-binding uosc/editions",
    ),
    (
        "stream-quality",
        "command:high_quality:script-binding uosc/stream-quality",
    ),
    ("open-file", "command:folder:script-binding uosc/open-file"),
    ("items", "command:list_alt:script-binding uosc/items"),
    ("prev", "command:arrow_back_ios:script-binding uosc/prev"),
    ("next", "command:arrow_forward_ios:script-binding uosc/next"),
    ("first", "command:first_page:script-binding uosc/first"),
    ("last", "command:last_page:script-binding uosc/last"),
    ("loop-playlist", "cycle:repeat:loop-playlist:no/inf!"),
    ("loop-file", "cycle:repeat_one:loop-file:no/inf!"),
    ("shuffle", "toggle:shuffle:shuffle"),
    ("autoload", "toggle:hdr_auto:autoload@uosc"),
    (
        "fullscreen",
        "cycle:crop_free:fullscreen:no/yes=fullscreen_exit!",
    ),
];

/// 把控制条里的别名展开成 uosc 实际使用的配置串。未命中别名则原样返回。
fn expand_control_shorthand(item: &str) -> &str {
    CONTROL_SHORTHANDS
        .iter()
        .find(|(name, _)| *name == item)
        .map(|(_, expansion)| *expansion)
        .unwrap_or(item)
}

/// 解析 `<...>` 里的可见性条件。
///
/// 语法：逗号分隔的「或」组，组内用 `+` 连接「与」条件，`!` 前缀表示取反。
/// - `<video,audio>` → 有视频**或**有音频
/// - `<has_audio+!audio>` → 有音轨**且**当前不是纯音频
/// - `<!idle>` → 非空闲
///
/// uosc 会给 `video` / `audio` / `image` / `idle` / `stream` 补 `is_` 前缀，`has_*`
/// 保持原样；这里统一补好，求值时按名字查 `PlaybackState`。
fn parse_dispositions(value: &str) -> Vec<Vec<(String, bool)>> {
    value
        .split(',')
        .map(|group| {
            group
                .split('+')
                .filter_map(|condition| {
                    let condition = condition.trim();
                    if condition.is_empty() {
                        return None;
                    }
                    let (name, expected) = match condition.strip_prefix('!') {
                        Some(rest) => (rest.trim(), false),
                        None => (condition, true),
                    };
                    if name.is_empty() {
                        return None;
                    }
                    let name = if name.starts_with("has_") || name.starts_with("is_") {
                        name.to_string()
                    } else {
                        format!("is_{name}")
                    };
                    Some((name, expected))
                })
                .collect::<Vec<_>>()
        })
        .filter(|group: &Vec<(String, bool)>| !group.is_empty())
        .collect()
}

pub fn parse_controls(value: &str) -> Vec<ControlItem> {
    let mut out = Vec::new();
    for raw in split_control_list(value) {
        let mut item = raw.trim();
        if item.is_empty() {
            continue;
        }
        // 可见性前缀 <...>：uosc 的写法是「或组」，组内用 `+` 表示「与」，`!` 取反。
        let mut dispositions = Vec::new();
        if item.starts_with('<') {
            let Some(end) = item.find('>') else { continue };
            dispositions = parse_dispositions(&item[1..end]);
            item = &item[end + 1..];
        }
        // 徽标 #{badge}[>{limit}]：先截断，后面的提示与参数才干净
        if let Some(hash) = item.find('#') {
            item = &item[..hash];
        }
        // 提示 ?tooltip
        let (item, tooltip) = match item.find('?') {
            Some(question) => {
                let hint = item[question + 1..].trim();
                (
                    &item[..question],
                    (!hint.is_empty()).then(|| hint.to_string()),
                )
            }
            None => (item, None),
        };
        // 别名必须在拆参数之前展开，否则拿到的是别名而不是图标名。
        let item = expand_control_shorthand(item.trim());
        let (name, param) = match item.split_once(':') {
            Some((name, param)) => (name.trim(), Some(param.trim())),
            None => (item.trim(), None),
        };
        if name.is_empty() {
            continue;
        }
        let scale_of = |fallback: f32| {
            param
                .and_then(|text| text.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.)
                .unwrap_or(fallback)
        };
        let control = match name {
            "space" => ControlItem {
                kind: ControlKind::Space,
                icon: String::new(),
                tooltip,
                scale: 1.,
                dispositions,
            },
            "gap" => ControlItem {
                kind: ControlKind::Gap,
                icon: String::new(),
                tooltip,
                scale: scale_of(0.3),
                dispositions,
            },
            "speed" => ControlItem {
                kind: ControlKind::Speed,
                icon: "speed".to_string(),
                tooltip,
                scale: scale_of(1.3),
                dispositions,
            },
            "command" | "button" | "cycle" | "toggle" => ControlItem {
                kind: ControlKind::Button,
                // 第一个参数是图标名，其余是命令 / 属性，预览不需要
                icon: param
                    .and_then(|text| text.split(':').next())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .unwrap_or(name)
                    .to_string(),
                tooltip,
                scale: 1.,
                dispositions,
            },
            shorthand => ControlItem {
                kind: ControlKind::Button,
                icon: shorthand.to_string(),
                tooltip,
                scale: 1.,
                dispositions,
            },
        };
        out.push(control);
    }
    out
}

/// 扫描所有播放器，构建「多皮肤共存」的方案列表。
///
/// 顺序固定：内置 OSC → 各播放器的 uosc（按检测顺序）→ 若一个 uosc 都没有，
/// 追加官方默认 uosc，保证预览画布永远有内容可画。
pub fn scan_skins(players: &[DetectedPlayer]) -> Vec<SkinPreset> {
    let mut out = vec![SkinPreset {
        name: "mpv 内置 OSC".to_string(),
        player_name: "通用".to_string(),
        kind: SkinKind::BuiltinOsc,
        config_dir: None,
        source_path: None,
        icon_font: None,
        layout: UoscLayout::default(),
        note: "经典底部时间条与按钮行；切换后 mpv.conf 的 osc=no 会被注释掉".to_string(),
    }];

    for player in players {
        let Some(dir) = player.config_dir.as_deref() else {
            continue;
        };
        let conf = dir.join("script-opts").join("uosc.conf");
        if !conf.is_file() {
            continue;
        }
        let Ok(bytes) = fs::read(&conf) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let layout = parse_layout(&text);
        out.push(SkinPreset {
            name: format!("{} · uosc", player.name),
            player_name: player.name.clone(),
            kind: SkinKind::Uosc,
            config_dir: Some(dir.to_path_buf()),
            source_path: Some(conf),
            icon_font: find_icon_font(dir),
            note: layout.palette.summary(),
            layout,
        });
    }

    if out.len() == 1 {
        out.push(SkinPreset {
            name: "uosc 官方默认".to_string(),
            player_name: "通用".to_string(),
            kind: SkinKind::Uosc,
            config_dir: None,
            source_path: None,
            icon_font: None,
            layout: UoscLayout::default(),
            note: "没有检测到 uosc.conf，这里展示官方默认配色与控件布局".to_string(),
        });
    }
    out
}

/// 应用皮肤：uosc 需要 `osc=no`，内置 OSC 则注释掉该行。
///
/// 返回 Some(备份记录) 表示确实写了盘，None 表示配置已经是目标状态。
pub fn apply_skin(
    mpv_path: &Path,
    kind: SkinKind,
    backups: &BackupManager,
) -> Result<Option<BackupRecord>, String> {
    let bytes =
        fs::read(mpv_path).map_err(|error| format!("读取 {} 失败：{error}", mpv_path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{} 不是 UTF-8，已拒绝写入", mpv_path.display()))?;
    let want_osc_disabled = kind == SkinKind::Uosc;
    let crlf = text.contains("\r\n");
    let trailing_newline = text.ends_with('\n');
    let separator = if crlf { "\r\n" } else { "\n" };
    let lines: Vec<String> = text
        .lines()
        .map(|raw| raw.trim_end_matches('\r').to_string())
        .collect();
    let found = lines.iter().position(|raw| {
        let body = raw.trim().trim_start_matches('#').trim();
        let Some(eq) = body.find('=') else {
            return false;
        };
        body[..eq].trim().eq_ignore_ascii_case("osc")
    });

    if let Some(line_no) = found {
        let enabled = !lines[line_no].trim_start().starts_with('#');
        if enabled == want_osc_disabled {
            return Ok(None);
        }
        return filters::toggle_line(mpv_path, line_no, want_osc_disabled, backups).map(Some);
    }
    if !want_osc_disabled {
        return Ok(None);
    }
    let mut lines = lines;
    lines.push("osc=no".to_string());
    let mut rendered = lines.join(separator);
    if trailing_newline {
        rendered.push_str(separator);
    }
    if ConfDoc::parse(&rendered).render() != rendered {
        return Err("写入后往返自检未通过，已放弃写入".to_string());
    }
    let record = backups.backup_file("mpv.conf", mpv_path)?;
    fs::write(mpv_path, rendered)
        .map_err(|error| format!("写入 {} 失败：{error}", mpv_path.display()))?;
    Ok(Some(record))
}

fn parse_hex(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn string_option(doc: &ConfDoc, key: &str, slot: &mut String) {
    if let Some(value) = doc.get(key)
        && !value.is_empty()
    {
        *slot = value;
    }
}

fn number_option(doc: &ConfDoc, key: &str, fallback: f32) -> f32 {
    doc.get(key)
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value >= 0.)
        .unwrap_or(fallback)
}

fn bool_option(doc: &ConfDoc, key: &str, fallback: bool) -> bool {
    match doc.get(key).as_deref() {
        Some("yes" | "true" | "1") => true,
        Some("no" | "false" | "0") => false,
        _ => fallback,
    }
}

fn parse_opacity(value: &str) -> SkinOpacity {
    let mut opacity = SkinOpacity::default();
    for pair in value.split(',') {
        let Some(eq) = pair.find('=') else { continue };
        let key = pair[..eq].trim();
        let Ok(number) = pair[eq + 1..].trim().parse::<f32>() else {
            continue;
        };
        let number = number.clamp(0., 1.);
        match key {
            "timeline" => opacity.timeline = number,
            "position" => opacity.position = number,
            "chapters" => opacity.chapters = number,
            "controls" => opacity.controls = number,
            "menu" => opacity.menu = number,
            "title" => opacity.title = number,
            "curtain" => opacity.curtain = number,
            _ => {}
        }
    }
    opacity
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-skin-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok());
        dir
    }

    const UOSC: &str = concat!(
        "timeline_style=bar\n",
        "timeline_size=30\n",
        "timeline_border=1\n",
        "progress=windowed\n",
        "top_bar=no-border\n",
        "top_bar_size=40\n",
        "top_bar_title=yes\n",
        "top_bar_alt_title=${?media-title:${media-title}}\n",
        "top_bar_alt_title_place=below\n",
        "controls=open-file,<idle>command:history:script-binding recentmenu/open?最近播放,",
        "gap,<!idle>command:analytics:script-binding stats/display-stats?统计,",
        "space,<video>video,subtitles,<video,audio>speed,space,fullscreen\n",
        "controls_size=32\n",
        "controls_spacing=2\n",
        "controls_margin=8\n",
        "volume=right\n",
        "window_border_size=1\n",
        "scale=1\n",
        "border_radius=2\n",
        "menu_item_height=36\n",
        "menu_min_width=260\n",
        "menu_padding=1\n",
        "color=foreground=FFFBFE,foreground_text=1C1B1F,background=1C1B1F,background_text=FFFBFE\n",
        "opacity=menu=0.9,submenu=0.7,curtain=0.5\n",
    );

    #[test]
    fn color_value_maps_to_palette() {
        let palette = SkinPalette::from_color_value(
            "foreground=FFFBFE,foreground_text=1C1B1F,background=1C1B1F,background_text=FFFBFE,badkey=zzzzzz",
        );
        assert_eq!(palette.foreground, 0xff_fb_fe);
        assert_eq!(palette.background, 0x1c_1b_1f);
        assert_eq!(palette.accent, 0x69_c5_ff, "未设置 match 时走 uosc 默认");
        assert_eq!(palette.window_border, 0x00_00_00, "缺项走默认");
    }

    #[test]
    fn layout_reads_real_uosc_options() {
        let layout = parse_layout(UOSC);
        assert_eq!(layout.palette.foreground, 0xff_fb_fe);
        assert_eq!(layout.top_bar, "no-border");
        assert_eq!(layout.top_bar_size, 40.);
        assert!(layout.top_bar_title);
        assert_eq!(
            layout.top_bar_alt_title.as_deref(),
            Some("${?media-title:${media-title}}")
        );
        assert_eq!(layout.top_bar_alt_place, "below");
        assert_eq!(layout.timeline_size, 30.);
        assert_eq!(layout.timeline_border, 1.);
        assert_eq!(layout.controls_size, 32.);
        assert_eq!(layout.controls_margin, 8.);
        assert_eq!(layout.border_radius, 2.);
        assert_eq!(layout.menu_min_width, 260.);
        assert_eq!(layout.window_border_size, 1.);
        assert_eq!(layout.opacity.menu, 0.9);
        assert_eq!(layout.opacity.timeline, 0.9, "未设置时走 uosc 默认");
        assert_eq!(layout.opacity.controls, 0., "uosc 默认按钮无底色");
    }

    #[test]
    fn controls_keep_icon_from_command_params() {
        let controls = parse_controls(concat!(
            "open-file,<idle>command:history:script-binding recentmenu/open?最近播放,gap,",
            "<video>cycle:toggle_on:show_danmaku:on=toggle_on/off=toggle_off?弹幕开关,",
            "space,fullscreen",
        ));
        let icons: Vec<&str> = controls.iter().map(|item| item.icon.as_str()).collect();
        // `open-file` / `fullscreen` 是 uosc 的别名，展开后才是真图标名。
        assert_eq!(
            icons,
            vec!["folder", "history", "", "toggle_on", "", "crop_free"]
        );
        assert_eq!(controls[0].kind, ControlKind::Button);
        assert_eq!(controls[2].kind, ControlKind::Gap);
        assert_eq!(controls[2].scale, 0.3, "gap 缺省比例 0.3");
        assert_eq!(controls[4].kind, ControlKind::Space);
        assert_eq!(controls[1].tooltip.as_deref(), Some("最近播放"));
        assert_eq!(controls[1].label(), "最近播放");
    }

    #[test]
    fn shorthands_expand_to_material_icon_names() {
        // 别名不展开就会拿 `menu` / `subtitles` 去查字形，字体里没有这些连字。
        let cases = [
            ("menu", "menu_book"),
            ("subtitles", "closed_caption"),
            ("audio", "graphic_eq"),
            ("video", "smart_display"),
            ("playlist", "list_alt"),
            ("chapters", "library_books"),
            ("open-file", "folder"),
            ("prev", "arrow_back_ios"),
            ("loop-playlist", "repeat"),
            ("loop-file", "repeat_one"),
            ("shuffle", "shuffle"),
            ("fullscreen", "crop_free"),
            ("play-pause", "pause"),
        ];
        for (shorthand, icon) in cases {
            let controls = parse_controls(shorthand);
            assert_eq!(controls.len(), 1, "{shorthand} 应解析出一项");
            assert_eq!(controls[0].icon, icon, "{shorthand} 应展开成 {icon}");
        }
    }

    #[test]
    fn dispositions_follow_playback_state() {
        let controls = parse_controls(concat!(
            "open-file,<idle>command:history:x?最近播放,<video>video,",
            "<has_chapter>chapters,<has_audio+!audio>audio,<video,audio>subtitles",
        ));
        let state = PlaybackState::SAMPLE_VIDEO;
        let visible: Vec<&str> = controls
            .iter()
            .filter(|item| item.visible_in(&state))
            .map(|item| item.icon.as_str())
            .collect();
        // `<idle>` 在有视频时隐藏；`<has_audio+!audio>` 因当前是视频（非纯音频）而可见。
        assert_eq!(
            visible,
            vec![
                "folder",
                "smart_display",
                "library_books",
                "graphic_eq",
                "closed_caption"
            ]
        );
        assert!(!controls[1].visible_in(&state), "`<idle>` 在播放时应隐藏");
    }

    #[test]
    fn disposition_or_group_matches_any_branch() {
        let controls = parse_controls("<video,audio>subtitles");
        assert_eq!(controls[0].dispositions.len(), 2, "逗号是「或」组");
        let video_only = PlaybackState {
            is_video: true,
            is_audio: false,
            ..PlaybackState::SAMPLE_VIDEO
        };
        let audio_only = PlaybackState {
            is_video: false,
            is_audio: true,
            ..PlaybackState::SAMPLE_VIDEO
        };
        let image = PlaybackState {
            is_video: false,
            is_audio: false,
            is_image: true,
            ..PlaybackState::SAMPLE_VIDEO
        };
        assert!(controls[0].visible_in(&video_only));
        assert!(controls[0].visible_in(&audio_only));
        assert!(!controls[0].visible_in(&image), "纯图片时字幕按钮应隐藏");
    }

    #[test]
    fn control_prefix_with_comma_is_not_split() {
        let controls = parse_controls("<video,audio>subtitles,<audio,has_image+!video>image,space");
        let icons: Vec<&str> = controls.iter().map(|item| item.icon.as_str()).collect();
        // `subtitles` 是别名，展开后是 `closed_caption`。
        assert_eq!(icons, vec!["closed_caption", "image", ""]);
        assert_eq!(controls.len(), 3, "前缀里的逗号不能当成条目分隔符");
    }

    #[test]
    fn missing_options_fall_back_to_uosc_defaults() {
        let layout = parse_layout("# 只有注释\n");
        let default = UoscLayout::default();
        assert_eq!(layout.palette, default.palette);
        assert_eq!(layout.top_bar_size, default.top_bar_size);
        assert_eq!(layout.controls, default.controls);
    }

    #[test]
    fn apply_skin_toggles_osc_and_can_be_undone() {
        let dir = temp_dir("apply");
        let mpv = dir.join("mpv.conf");
        let original = "vo=gpu-next\n#osc=no   # 内置 OSC 已关闭\n";
        assert!(fs::write(&mpv, original).is_ok());
        let backups = BackupManager::new(dir.join("backups"));

        let written = apply_skin(&mpv, SkinKind::BuiltinOsc, &backups);
        assert!(written.is_ok(), "切换失败：{:?}", written.as_ref().err());
        assert!(written.unwrap_or(None).is_none(), "已是目标状态，不应写盘");

        let enabled = apply_skin(&mpv, SkinKind::Uosc, &backups);
        assert!(enabled.is_ok(), "启用失败：{:?}", enabled.as_ref().err());
        let text = fs::read_to_string(&mpv).unwrap_or_default();
        assert!(text.contains("osc=no   # 内置 OSC 已关闭"));
        assert!(!text.contains("#osc=no"));

        let undone = backups.restore_latest("mpv.conf");
        assert!(undone.is_ok(), "撤回失败：{:?}", undone.as_ref().err());
        assert_eq!(fs::read_to_string(&mpv).unwrap_or_default(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_skin_appends_osc_line_when_missing() {
        let dir = temp_dir("append");
        let mpv = dir.join("mpv.conf");
        assert!(fs::write(&mpv, "vo=gpu-next\n").is_ok());
        let backups = BackupManager::new(dir.join("backups"));
        let written = apply_skin(&mpv, SkinKind::Uosc, &backups);
        assert!(written.is_ok(), "启用失败：{:?}", written.as_ref().err());
        let text = fs::read_to_string(&mpv).unwrap_or_default();
        assert_eq!(text, "vo=gpu-next\nosc=no\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
