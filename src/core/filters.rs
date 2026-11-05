//! core::filters —— MPV 视频滤镜 / GLSL 着色器 / 配置组的扫描与切换
//!
//! 真实整合包把「切换滤镜」写在两处：
//! - `input.conf` 的 `#menu: 视频滤镜 > …`（`vf toggle …`）与
//!   `#menu: 着色器 > …`（`change-list glsl-shaders toggle …`）；
//!   被整合包隐藏的入口用 `# 菜单入口已隐藏：着色器 > …` 标注。
//! - `mpv.conf` 的 `[Profile]` 配置组 + `profile=XXX` 生效行
//!   （如 `[HQ]` / `[FSRCNNX]` / `[AnimeJaNai]`）。
//!
//! 这里沿用整合包作者自己的开关习惯：**切换 = 注释 / 取消注释那一行**。
//! 不改写命令、不重新拼装配置、不删除任何文件，因此天然无损、可撤回。
//! 所有写入都经 `BackupManager`，并在写后做 `render()` 往返自检。
//!
//! `ui` 不读配置字节，只消费 `FilterCatalog`。

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::backup::{BackupManager, BackupRecord};
use crate::core::config::conf::ConfDoc;
use crate::core::config::input::InputDoc;

const MENU_MARKER: &str = "#menu:";
const HIDDEN_MARKER: &str = "菜单入口已隐藏：";
const FILTER_HEADS: [&str; 2] = ["视频滤镜", "着色器"];
/// 扫描 `shaders/` 时最多收集的文件数，避免大型整合包把 UI 撑爆。
const MAX_SHADER_FILES: usize = 400;

/// 滤镜条目的来源类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// `vf toggle …` 视频滤镜
    VideoFilter,
    /// `change-list glsl-shaders toggle …` 着色器
    Shader,
    /// `mpv.conf` 里的 `[Profile]` 配置组
    Profile,
}

impl FilterKind {
    /// 三类切换入口的固定顺序；UI 用它生成分段标签，保证 `Profile` 变体真实被构造。
    pub const ALL: [Self; 3] = [Self::VideoFilter, Self::Shader, Self::Profile];

    pub fn label(self) -> &'static str {
        match self {
            Self::VideoFilter => "视频滤镜",
            Self::Shader => "着色器",
            Self::Profile => "配置组",
        }
    }
}

/// 一条可切换的滤镜/着色器菜单项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterEntry {
    /// 菜单末级名称（如「开/关 自适应锐化」）
    pub name: String,
    /// 上级分类（如「着色器 / IGV」）
    pub category: String,
    pub kind: FilterKind,
    /// 快捷键；同一快捷键的第二条命令没有按键，为 None
    pub shortcut: Option<String>,
    /// 命令原文（不含按键）
    pub command: String,
    /// 着色器路径（引号内的 `~~/shaders/…`）
    pub target: Option<String>,
    /// 该行是否生效（未被注释）
    pub enabled: bool,
    /// 0 基行号，切换时按此精确定位
    pub line_no: usize,
    /// 所在文件名，`input.conf` / `mpv.conf`
    pub source: String,
}

impl FilterEntry {
    /// 界面上的一行说明：目标存在时补上路径末段。
    pub fn target_label(&self) -> Option<String> {
        self.target.as_ref().map(|path| {
            path.rsplit('/')
                .next()
                .unwrap_or(path.as_str())
                .trim_matches('"')
                .to_string()
        })
    }
}

/// `mpv.conf` 里的一个 `[Profile]` 配置组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderProfile {
    pub name: String,
    pub description: String,
    /// 段内启用的 `glsl-shaders` 目标
    pub shaders: Vec<String>,
    /// 段内有效配置项数量（不含 `profile-desc` 与注释）
    pub option_count: usize,
    /// `profile=XXX` 行是否生效
    pub enabled: bool,
    /// `profile=XXX` 所在行；None 表示该配置组还没有对应激活行
    pub profile_line: Option<usize>,
    /// `[XXX]` 段标题所在行
    pub header_line: usize,
}

/// 一个播放器配置目录的滤镜全景。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterCatalog {
    pub config_dir: PathBuf,
    pub input_path: PathBuf,
    pub mpv_path: PathBuf,
    pub entries: Vec<FilterEntry>,
    pub profiles: Vec<ShaderProfile>,
    /// 相对 `shaders/` 的 GLSL 路径（统一 `/` 分隔）
    pub shader_files: Vec<String>,
    /// mpv.conf 当前生效的 `glsl-shaders` 目标
    pub active_shaders: Vec<String>,
    /// 读取/编码问题；有值时禁止一切切换
    pub error: Option<String>,
}

impl FilterCatalog {
    /// 播放器没有可用配置目录时的占位全景：UI 仍可渲染，但一律禁止切换。
    pub fn unavailable(config_dir: &Path, error: impl Into<String>) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            input_path: config_dir.join("input.conf"),
            mpv_path: config_dir.join("mpv.conf"),
            entries: Vec::new(),
            profiles: Vec::new(),
            shader_files: Vec::new(),
            active_shaders: Vec::new(),
            error: Some(error.into()),
        }
    }

    /// 是否允许写入（两个目标文件都能无损往返）。
    pub fn editable(&self) -> bool {
        self.error.is_none()
    }

    pub fn count(&self, kind: FilterKind) -> usize {
        if kind == FilterKind::Profile {
            self.profiles.len()
        } else {
            self.entries
                .iter()
                .filter(|entry| entry.kind == kind)
                .count()
        }
    }

    pub fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|e| e.enabled).count()
            + self.profiles.iter().filter(|p| p.enabled).count()
    }
}

/// 扫描一个配置目录，构建滤镜全景。
pub fn scan_filters(config_dir: &Path) -> FilterCatalog {
    let input_path = config_dir.join("input.conf");
    let mpv_path = config_dir.join("mpv.conf");
    let mut catalog = FilterCatalog {
        config_dir: config_dir.to_path_buf(),
        input_path: input_path.clone(),
        mpv_path: mpv_path.clone(),
        entries: Vec::new(),
        profiles: Vec::new(),
        shader_files: collect_shader_files(&config_dir.join("shaders")),
        active_shaders: Vec::new(),
        error: None,
    };

    let (input_text, input_error) = read_text(&input_path);
    let (mpv_text, mpv_error) = read_text(&mpv_path);
    if let Some(error) = input_error.as_ref().or(mpv_error.as_ref()) {
        catalog.error = Some(error.clone());
    }

    if let Some(text) = input_text.as_deref() {
        for (line_no, raw) in text.lines().enumerate() {
            if let Some(entry) = parse_menu_line(raw.trim_end_matches('\r'), line_no) {
                catalog.entries.push(entry);
            }
        }
    }
    if let Some(text) = mpv_text.as_deref() {
        let lines: Vec<String> = text
            .lines()
            .map(|raw| raw.trim_end_matches('\r').to_string())
            .collect();
        catalog.profiles = parse_profiles(&lines);
        catalog.active_shaders = parse_active_shaders(&lines);
    }

    // 混合行尾会让 render() 无法逐字节还原，一律禁止写入。
    if catalog.error.is_none()
        && (has_mixed_endings(input_text.as_deref()) || has_mixed_endings(mpv_text.as_deref()))
    {
        catalog.error =
            Some("配置含混合行尾（LF / CRLF），为避免写入破坏格式已禁用切换".to_string());
    }
    catalog
}

/// 宽松读文本：非 UTF-8 只展示、不允许写入。
fn read_text(path: &Path) -> (Option<String>, Option<String>) {
    match fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => (Some(text), None),
            Err(_) => (
                Some(String::from_utf8_lossy(&bytes).to_string()),
                Some(format!(
                    "{} 不是 UTF-8，已按宽松编码展示并禁用切换",
                    path.display()
                )),
            ),
        },
        Err(_) => (None, None), // 文件不存在不是错误：空目录也应有干净的展示
    }
}

fn has_mixed_endings(text: Option<&str>) -> bool {
    text.is_some_and(|value| value.contains("\r\n") && value.replace("\r\n", "").contains('\n'))
}

/// 解析一行 input.conf 菜单标注。
fn parse_menu_line(raw: &str, line_no: usize) -> Option<FilterEntry> {
    let (path_text, prefix) = if let Some(pos) = raw.find(MENU_MARKER) {
        (raw[pos + MENU_MARKER.len()..].trim(), &raw[..pos])
    } else {
        let pos = raw.find(HIDDEN_MARKER)?;
        (raw[pos + HIDDEN_MARKER.len()..].trim(), &raw[..pos])
    };
    let segments: Vec<&str> = path_text
        .split('>')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let head = (*segments.first()?).to_string();
    if !FILTER_HEADS.iter().any(|known| head.starts_with(known)) {
        return None;
    }
    let name = (*segments.last()?).to_string();
    let category = if segments.len() > 2 {
        segments[1..segments.len() - 1].join(" / ")
    } else {
        head.clone()
    };
    // 行首的 `#` 表示整合包把这个入口隐藏了；行尾的 `#menu:` 不算。
    let enabled = !raw.trim_start().starts_with('#');
    let body = prefix.trim().trim_start_matches('#').trim();
    let (shortcut, command) = split_shortcut(body);
    if command.is_empty() {
        return None;
    }
    let kind = if head.starts_with("着色器") || command.starts_with("change-list glsl-shaders") {
        FilterKind::Shader
    } else {
        FilterKind::VideoFilter
    };
    Some(FilterEntry {
        name,
        category,
        kind,
        shortcut,
        command: command.to_string(),
        target: quoted_target(&command),
        enabled,
        line_no,
        source: "input.conf".to_string(),
    })
}

/// 首段是快捷键还是命令？整合包里同一个快捷键的第二条命令没有按键前缀。
fn split_shortcut(body: &str) -> (Option<String>, String) {
    let mut parts = body.splitn(2, char::is_whitespace);
    let Some(first) = parts.next() else {
        return (None, String::new());
    };
    let is_command = [
        "change-list",
        "vf",
        "af",
        "cycle",
        "show-text",
        "script-binding",
    ]
    .iter()
    .any(|verb| first.eq_ignore_ascii_case(verb));
    if is_command {
        return (None, body.trim().to_string());
    }
    let Some(rest) = parts.next() else {
        return (Some(first.to_string()), String::new());
    };
    (Some(first.to_string()), rest.trim().to_string())
}

/// 取命令里第一对引号之间的内容（着色器路径）。
fn quoted_target(command: &str) -> Option<String> {
    let start = command.find('"')?;
    let rest = &command[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 解析 `[Profile]` 配置组及其 `profile=XXX` 激活行。
fn parse_profiles(lines: &[String]) -> Vec<ShaderProfile> {
    // 先收集所有 profile= 行（含被注释的），供后面判断启用状态与定位。
    let mut activations: Vec<(String, usize, bool)> = Vec::new();
    for (line_no, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim();
        let commented = trimmed.starts_with('#');
        let body = trimmed.trim_start_matches('#').trim();
        let Some(eq) = body.find('=') else { continue };
        let key = body[..eq].trim();
        let value = plain_value(&body[eq + 1..]);
        if key.eq_ignore_ascii_case("profile") {
            for name in value.split([',', ';']) {
                let name = name.trim();
                if !name.is_empty() {
                    activations.push((name.to_string(), line_no, !commented));
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut current: Option<ShaderProfile> = None;
    for (line_no, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
            if let Some(done) = current.take() {
                out.push(done);
            }
            let name = trimmed[1..trimmed.len() - 1].trim().to_string();
            current = Some(ShaderProfile {
                name,
                description: String::new(),
                shaders: Vec::new(),
                option_count: 0,
                enabled: false,
                profile_line: None,
                header_line: line_no,
            });
            continue;
        }
        let Some(profile) = current.as_mut() else {
            continue;
        };
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq].trim();
        let value = plain_value(&trimmed[eq + 1..]);
        if key.eq_ignore_ascii_case("profile-desc") {
            profile.description = value.to_string();
            continue;
        }
        profile.option_count += 1;
        if key.eq_ignore_ascii_case("glsl-shaders") {
            for shader in quoted_targets(&value) {
                profile.shaders.push(shader);
            }
        }
    }
    if let Some(done) = current {
        out.push(done);
    }

    for profile in &mut out {
        if let Some((_, line_no, enabled)) = activations
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(&profile.name))
        {
            profile.profile_line = Some(*line_no);
            profile.enabled = *enabled;
        }
    }
    out
}

/// 全局生效的 `glsl-shaders`（mpv 用逗号/分号分隔多个路径）。
///
/// 第一个 `[Profile]` 段标题之后的所有配置项都属于该配置组，**只有该组被
/// `profile=XXX` 激活时才生效**；它们已经收在 `ShaderProfile.shaders` 里，
/// 不能混进「当前生效」，否则停用某个配置组时这里仍会显示它的着色器。
fn parse_active_shaders(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_profile = false;
    for raw in lines {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
            in_profile = true;
            continue;
        }
        if in_profile || trimmed.starts_with('#') {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        if trimmed[..eq].trim().eq_ignore_ascii_case("glsl-shaders") {
            let value = plain_value(&trimmed[eq + 1..]);
            for shader in quoted_targets(&value) {
                out.push(shader);
            }
        }
    }
    out
}

/// 去掉 mpv.conf 值后附的 ` # 说明`（官方不支持行内注释，展示层才拆）。
fn plain_value(value: &str) -> String {
    let head = match value.find(" #") {
        Some(pos) => &value[..pos],
        None => value,
    };
    head.trim().trim_matches('"').to_string()
}

/// 从 `glsl-shaders=~~/a.glsl,~~/b.glsl` 或带引号的写法里取出全部路径。
fn quoted_targets(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in value.split([',', ';']) {
        let candidate = part.trim().trim_matches('"').trim();
        if !candidate.is_empty() {
            out.push(candidate.to_string());
        }
    }
    out
}

/// 递归收集 `shaders/` 下的 `.glsl`（跳过缓存目录，限制深度与数量）。
fn collect_shader_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_shader_files_inner(dir, dir, 0, &mut out);
    out.sort();
    out
}

fn collect_shader_files_inner(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 4 || out.len() >= MAX_SHADER_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if out.len() >= MAX_SHADER_FILES {
            return;
        }
        if path.is_dir() {
            let skip = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("cache"));
            if !skip {
                collect_shader_files_inner(root, &path, depth + 1, out);
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("glsl"))
        {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            out.push(relative);
        }
    }
}

/// 切换某一行的注释状态（滤镜菜单入口 / profile 激活行共用）。
///
/// 只动目标行的行首 `# `，其余缩进、对齐、行尾说明原样保留。
/// 写前必过 `BackupManager`，写后用对应解析器做 `render()` 往返自检。
pub fn toggle_line(
    file: &Path,
    line_no: usize,
    enabled: bool,
    backups: &BackupManager,
) -> Result<BackupRecord, String> {
    let bytes = fs::read(file).map_err(|error| format!("读取 {} 失败：{error}", file.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{} 不是 UTF-8，已拒绝写入", file.display()))?;
    if has_mixed_endings(Some(&text)) {
        return Err("配置含混合行尾，已拒绝写入".to_string());
    }
    let is_input = file
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("input.conf"));
    let crlf = text.contains("\r\n");
    let trailing_newline = text.ends_with('\n');
    let separator = if crlf { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text
        .lines()
        .map(|raw| raw.trim_end_matches('\r').to_string())
        .collect();
    let Some(raw) = lines.get_mut(line_no) else {
        return Err(format!("行号 {line_no} 已失效，请重新扫描"));
    };
    let currently_enabled = !raw.trim_start().starts_with('#');
    if currently_enabled == enabled {
        return Err(if enabled {
            "该行已经启用".to_string()
        } else {
            "该行已经停用".to_string()
        });
    }
    *raw = if enabled {
        strip_comment(raw)
    } else {
        add_comment(raw)
    };
    let rendered = join_lines(&lines, separator, trailing_newline);
    if !round_trip_ok(&rendered, is_input) {
        return Err("写入后往返自检未通过，已放弃写入".to_string());
    }

    let operation = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "config".to_string());
    let record = backups.backup_file(&operation, file)?;
    fs::write(file, rendered).map_err(|error| format!("写入 {} 失败：{error}", file.display()))?;
    Ok(record)
}

/// 切换一个 `[Profile]` 配置组：已有 `profile=XXX` 行就切该行，否则追加一行。
pub fn toggle_profile(
    mpv_path: &Path,
    profile_name: &str,
    enabled: bool,
    backups: &BackupManager,
) -> Result<BackupRecord, String> {
    let bytes =
        fs::read(mpv_path).map_err(|error| format!("读取 {} 失败：{error}", mpv_path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{} 不是 UTF-8，已拒绝写入", mpv_path.display()))?;
    if has_mixed_endings(Some(&text)) {
        return Err("mpv.conf 含混合行尾，已拒绝写入".to_string());
    }
    let crlf = text.contains("\r\n");
    let trailing_newline = text.ends_with('\n');
    let separator = if crlf { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text
        .lines()
        .map(|raw| raw.trim_end_matches('\r').to_string())
        .collect();
    let found = lines.iter().position(|raw| {
        let body = raw.trim().trim_start_matches('#').trim();
        let Some(eq) = body.find('=') else {
            return false;
        };
        body[..eq].trim().eq_ignore_ascii_case("profile")
            && plain_value(&body[eq + 1..])
                .split([',', ';'])
                .any(|name| name.trim().eq_ignore_ascii_case(profile_name))
    });
    if let Some(line_no) = found {
        return toggle_line(mpv_path, line_no, enabled, backups);
    }
    if !enabled {
        return Err(format!("配置组 {profile_name} 没有可停用的 profile 行"));
    }
    lines.push(format!("profile={profile_name}"));
    let rendered = join_lines(&lines, separator, trailing_newline);
    if !round_trip_ok(&rendered, false) {
        return Err("写入后往返自检未通过，已放弃写入".to_string());
    }
    let record = backups.backup_file("mpv.conf", mpv_path)?;
    fs::write(mpv_path, rendered)
        .map_err(|error| format!("写入 {} 失败：{error}", mpv_path.display()))?;
    Ok(record)
}

fn join_lines(lines: &[String], separator: &str, trailing_newline: bool) -> String {
    let mut out = lines.join(separator);
    if trailing_newline {
        out.push_str(separator);
    }
    out
}

/// 用解析器确认新文本能被无损往返（混合行尾等情形会失败）。
fn round_trip_ok(text: &str, is_input: bool) -> bool {
    if is_input {
        InputDoc::parse(text).render() == text
    } else {
        ConfDoc::parse(text).render() == text
    }
}

/// 去掉行首注释符，保留缩进与后续对齐。
fn strip_comment(raw: &str) -> String {
    let indent_len = raw.len() - raw.trim_start().len();
    let (indent, body) = raw.split_at(indent_len);
    let body = body.strip_prefix('#').unwrap_or(body);
    let body = body.strip_prefix(' ').unwrap_or(body);
    format!("{indent}{body}")
}

/// 在第一个非空白字符前插入 `# `，保留缩进。
fn add_comment(raw: &str) -> String {
    let indent_len = raw.len() - raw.trim_start().len();
    let (indent, body) = raw.split_at(indent_len);
    format!("{indent}# {body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-filter-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok());
        dir
    }

    const INPUT: &str = concat!(
        "CTRL+`           vf clr \"\"                                      #menu: 视频滤镜 > 清空\n",
        "ALT+v            vf toggle vflip                                   #menu: 视频滤镜 > 开/关 垂直翻转\n",
        "CTRL+1           change-list glsl-shaders toggle \"~~/shaders/igv/SSimSuperRes.glsl\"   #menu: 着色器 > IGV > 开/关 轻量放大\n",
        "#                change-list glsl-shaders toggle \"~~/shaders/igv/adaptive-sharpen_luma.glsl\"  #menu: 着色器 > IGV > 开/关 锐化-luma\n",
        "# CTRL+2         change-list glsl-shaders clr \"\"                  # 菜单入口已隐藏：着色器 > 清空\n",
    );

    const MPV: &str = concat!(
        "vo=gpu-next\n",
        "profile=HQ                  # 最常用的内置算法配置组\n",
        "#profile=FSRCNNX             # 放大配置组\n",
        "glsl-shaders=\"~~/shaders/igv/KrigBilateral.glsl\"\n",
        "\n",
        "[HQ]\n",
        " profile-desc=内置高质量\n",
        " scale=ewa_lanczossharp\n",
        "[FSRCNNX]\n",
        " profile-desc=FSRCNNX 放大\n",
        " glsl-shaders=\"~~/shaders/igv/FSRCNNX_x2_8-0-4-1.glsl\"\n",
    );

    #[test]
    fn menu_lines_are_parsed_with_category_and_state() {
        let entries: Vec<FilterEntry> = INPUT
            .lines()
            .enumerate()
            .filter_map(|(line_no, raw)| parse_menu_line(raw, line_no))
            .collect();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].kind, FilterKind::VideoFilter);
        assert_eq!(entries[0].shortcut.as_deref(), Some("CTRL+`"));
        assert!(entries[0].enabled);
        assert_eq!(entries[2].kind, FilterKind::Shader);
        assert_eq!(entries[2].category, "IGV");
        assert_eq!(
            entries[2].target.as_deref(),
            Some("~~/shaders/igv/SSimSuperRes.glsl")
        );
        assert_eq!(
            entries[2].target_label().as_deref(),
            Some("SSimSuperRes.glsl")
        );
        assert!(entries[3].shortcut.is_none(), "续行没有快捷键");
        assert!(!entries[3].enabled, "行首注释 = 停用");
        assert!(!entries[4].enabled, "隐藏入口 = 停用");
    }

    #[test]
    fn non_filter_menus_are_ignored() {
        let raw = "J   cycle sub-ass-vsfilter-color-compat   #menu: 字幕 > 兼容性 > 切换";
        assert!(parse_menu_line(raw, 0).is_none());
    }

    #[test]
    fn profiles_keep_activation_state_and_shader_targets() {
        let lines: Vec<String> = MPV.lines().map(|raw| raw.to_string()).collect();
        let profiles = parse_profiles(&lines);
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "HQ");
        assert_eq!(profiles[0].description, "内置高质量");
        assert!(profiles[0].enabled);
        assert_eq!(profiles[0].profile_line, Some(1));
        assert_eq!(profiles[0].option_count, 1);
        assert!(!profiles[1].enabled);
        assert_eq!(profiles[1].profile_line, Some(2));
        assert_eq!(
            profiles[1].shaders,
            vec!["~~/shaders/igv/FSRCNNX_x2_8-0-4-1.glsl".to_string()]
        );
        assert_eq!(
            parse_active_shaders(&lines),
            vec!["~~/shaders/igv/KrigBilateral.glsl".to_string()]
        );
    }

    #[test]
    fn toggle_line_preserves_alignment_and_can_be_undone() {
        let dir = temp_dir("toggle");
        let input = dir.join("input.conf");
        assert!(fs::write(&input, INPUT).is_ok());
        let backups = BackupManager::new(dir.join("backups"));

        let record = toggle_line(&input, 1, false, &backups);
        assert!(record.is_ok(), "停用失败：{:?}", record.as_ref().err());
        let after = fs::read_to_string(&input);
        assert!(after.is_ok());
        let Ok(after) = after else { return };
        assert!(after.contains("# ALT+v            vf toggle vflip"));
        assert!(after.contains("CTRL+`           vf clr"), "其它行不得变化");
        assert!(record.as_ref().is_ok(), "切换应返回备份句柄");

        let restored = backups.restore_latest("input.conf");
        assert!(restored.is_ok(), "撤回失败：{:?}", restored.as_ref().err());
        assert_eq!(fs::read_to_string(&input).unwrap_or_default(), INPUT);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn toggle_profile_switches_existing_line_and_appends_missing_one() {
        let dir = temp_dir("profile");
        let mpv = dir.join("mpv.conf");
        assert!(fs::write(&mpv, MPV).is_ok());
        let backups = BackupManager::new(dir.join("backups"));

        let enabled = toggle_profile(&mpv, "FSRCNNX", true, &backups);
        assert!(enabled.is_ok(), "启用失败：{:?}", enabled.as_ref().err());
        let text = fs::read_to_string(&mpv).unwrap_or_default();
        assert!(text.contains("profile=FSRCNNX             # 放大配置组"));
        assert!(text.contains("vo=gpu-next"), "原有行不得丢失");

        let appended = toggle_profile(&mpv, "AnimeJaNai", true, &backups);
        assert!(appended.is_ok(), "追加失败：{:?}", appended.as_ref().err());
        let text = fs::read_to_string(&mpv).unwrap_or_default();
        assert!(text.ends_with("profile=AnimeJaNai\n"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_collects_entries_profiles_and_shader_files() {
        let dir = temp_dir("scan");
        assert!(fs::write(dir.join("input.conf"), INPUT).is_ok());
        assert!(fs::write(dir.join("mpv.conf"), MPV).is_ok());
        let shaders = dir.join("shaders").join("igv");
        assert!(fs::create_dir_all(&shaders).is_ok());
        assert!(fs::write(shaders.join("a.glsl"), "//").is_ok());
        assert!(fs::create_dir_all(dir.join("shaders").join("cache")).is_ok());
        assert!(fs::write(dir.join("shaders").join("cache").join("b.glsl"), "//").is_ok());

        let catalog = scan_filters(&dir);
        assert!(catalog.editable());
        assert_eq!(catalog.entries.len(), 5);
        assert_eq!(catalog.profiles.len(), 2);
        assert_eq!(catalog.shader_files, vec!["igv/a.glsl".to_string()]);
        assert_eq!(catalog.enabled_count(), 3 + 1); // 3 条启用菜单 + HQ 配置组
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn crlf_round_trip_is_preserved() {
        let dir = temp_dir("crlf");
        let mpv = dir.join("mpv.conf");
        let text = "vo=gpu-next\r\nprofile=HQ\r\n";
        assert!(fs::write(&mpv, text).is_ok());
        let backups = BackupManager::new(dir.join("backups"));
        let result = toggle_profile(&mpv, "HQ", false, &backups);
        assert!(result.is_ok(), "停用失败：{:?}", result.as_ref().err());
        let after = fs::read_to_string(&mpv).unwrap_or_default();
        assert_eq!(after, "vo=gpu-next\r\n# profile=HQ\r\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
