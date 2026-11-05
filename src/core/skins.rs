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
    pub layout: UoscLayout,
    pub note: String,
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
pub fn parse_controls(value: &str) -> Vec<ControlItem> {
    let mut out = Vec::new();
    for raw in split_control_list(value) {
        let mut item = raw.trim();
        if item.is_empty() {
            continue;
        }
        // 可见性前缀 <...>
        if item.starts_with('<') {
            let Some(end) = item.find('>') else { continue };
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
            },
            "gap" => ControlItem {
                kind: ControlKind::Gap,
                icon: String::new(),
                tooltip,
                scale: scale_of(0.3),
            },
            "speed" => ControlItem {
                kind: ControlKind::Speed,
                icon: "speed".to_string(),
                tooltip,
                scale: scale_of(1.3),
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
            },
            shorthand => ControlItem {
                kind: ControlKind::Button,
                icon: shorthand.to_string(),
                tooltip,
                scale: 1.,
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
        assert_eq!(
            icons,
            vec!["open-file", "history", "", "toggle_on", "", "fullscreen"]
        );
        assert_eq!(controls[0].kind, ControlKind::Button);
        assert_eq!(controls[2].kind, ControlKind::Gap);
        assert_eq!(controls[2].scale, 0.3, "gap 缺省比例 0.3");
        assert_eq!(controls[4].kind, ControlKind::Space);
        assert_eq!(controls[1].tooltip.as_deref(), Some("最近播放"));
        assert_eq!(controls[1].label(), "最近播放");
    }

    #[test]
    fn control_prefix_with_comma_is_not_split() {
        let controls = parse_controls("<video,audio>subtitles,<audio,has_image+!video>image,space");
        let icons: Vec<&str> = controls.iter().map(|item| item.icon.as_str()).collect();
        assert_eq!(icons, vec!["subtitles", "image", ""]);
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
