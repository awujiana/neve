//! core::config::document —— 配置文档的逐行视图与值级编辑（FR-08/FR-11 的 UI 数据源）
//!
//! 分层：`ui` 不读配置文件字节，只消费这里的 `PlayerConfigWorkspace`；所有读写都在 core 层。
//!
//! 无损策略：
//! - 结构化判定（哪一行是配置项、哪个键生效）交给 `ConfDoc` / `InputDoc`；
//! - 原文行单独保留用于展示，缩进注释、对齐空格、行尾说明都不会被重新拼装抹掉；
//! - 每个文件用解析器的 `render()` 做往返自检，不通过就禁止编辑（混合行尾等场景）。
//! - 非 UTF-8 配置只做宽松展示，明确标记 `error`，不提供写入入口。
//!
//! 生效语义：mpv 对同名键「最后一次出现生效」。被覆盖的行给出生效行号，
//! 配置页据此标出「生效 / 被第 N 行覆盖」，而不是笼统报一句「有重复」。
//!
//! 写入：`set_option_value` 只替换目标行的值片段，保留该行的缩进、等号对齐、
//! 行尾空格与 `# 说明`；写前必过 `BackupManager`，`undo_edit_for` 按文件精确回退。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::conf::ConfDoc;
use super::input::{InputDoc, normalize_key};
use crate::core::backup::{BackupManager, BackupRecord};
use crate::core::players::DetectedPlayer;

/// 配置文件的语法类别，决定用哪个解析器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFileFormat {
    /// `key=value`（mpv.conf 及根级 conf）
    MpvConf,
    /// `KEY command`（input.conf）
    InputConf,
    /// script-opts 下的脚本选项，语法同 mpv.conf
    ScriptOption,
}

impl ConfigFileFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::MpvConf => "主配置",
            Self::InputConf => "按键绑定",
            Self::ScriptOption => "脚本选项",
        }
    }

    /// 路径判定：input.conf 单独走按键语法，script-opts 路径下的都算脚本选项。
    fn of(path: &Path) -> Self {
        let is_input = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("input.conf"));
        if is_input {
            return Self::InputConf;
        }
        let in_script_opts = path.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("script-opts")
        });
        if in_script_opts {
            Self::ScriptOption
        } else {
            Self::MpvConf
        }
    }
}

/// 行尾风格（展示与编辑时都要还原）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }
}

/// 一行在文档里的角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLineKind {
    Entry,
    Binding,
    /// 整行注释（含带缩进的注释）
    Comment,
    Blank,
    /// input.conf 的 `[section]`
    Section,
    /// 无法结构化的行，原样展示
    Verbatim,
}

impl ConfigLineKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Entry => "配置项",
            Self::Binding => "按键",
            Self::Comment => "注释",
            Self::Blank => "空行",
            Self::Section => "小节",
            Self::Verbatim => "原文",
        }
    }
}

/// 同名键的生效状态。`Superseded(n)` 里的 n 是 0 基生效行号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveState {
    Active,
    Superseded(usize),
    NotApplicable,
}

/// 一行的展示模型。`text` 是去掉前导缩进与行尾说明后的主文本，
/// `inline_comment` 含 `#` 原文，UI 用注释色单独渲染，两者拼起来即原文主体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLineView {
    /// 0 基行号
    pub line_no: usize,
    pub kind: ConfigLineKind,
    pub text: String,
    pub inline_comment: Option<String>,
    pub key: Option<String>,
    /// 展示用取值（已剥离行尾说明），也是编辑框的初值
    pub value: Option<String>,
    /// 整行注释去掉 `#` 后的文字
    pub comment: Option<String>,
    /// 紧邻上方注释块里最近的一条说明（注释归属）
    pub doc_comment: Option<String>,
    pub effective: EffectiveState,
    /// 前导空白字符数，用于对齐展示
    pub indent: usize,
}

impl ConfigLineView {
    /// 1 基行号（界面上给人看的编号）。
    pub fn display_line_no(&self) -> usize {
        self.line_no + 1
    }
}

/// 一个配置文件的完整视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFileView {
    /// 相对配置目录的路径，统一用 `/` 分隔
    pub name: String,
    pub path: PathBuf,
    pub format: ConfigFileFormat,
    pub lines: Vec<ConfigLineView>,
    pub line_ending: LineEnding,
    pub trailing_newline: bool,
    /// 解析器 `render()` 是否与原文逐字节一致；false 时禁止编辑
    pub round_trip: bool,
    pub entry_count: usize,
    pub comment_count: usize,
    pub inline_comment_count: usize,
    pub blank_count: usize,
    pub effective_count: usize,
    pub superseded_count: usize,
    /// 读取/编码问题；有值时只展示、不编辑
    pub error: Option<String>,
}

impl ConfigFileView {
    /// 是否允许在界面上编辑（UTF-8 + 往返自检通过）。
    pub fn editable(&self) -> bool {
        self.error.is_none() && self.round_trip
    }

    /// 一句话概括文件状态，供文件标签的副标题使用。
    pub fn summary(&self) -> String {
        format!(
            "{} 项 · {} 注释 · {}",
            self.entry_count,
            self.comment_count + self.inline_comment_count,
            self.line_ending.label()
        )
    }
}

/// 一个播放器的配置工作区。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerConfigWorkspace {
    pub player_name: String,
    pub config_dir: Option<PathBuf>,
    pub files: Vec<ConfigFileView>,
}

/// 从检测结果构建全部工作区（与 `ui::build_panels` 的播放器顺序一致）。
pub fn build_workspaces(players: &[DetectedPlayer]) -> Vec<PlayerConfigWorkspace> {
    players
        .iter()
        .map(|player| PlayerConfigWorkspace {
            player_name: player.name.clone(),
            config_dir: player.config_dir.clone(),
            files: player
                .config_dir
                .as_deref()
                .map(scan_config_files)
                .unwrap_or_default(),
        })
        .collect()
}

/// 扫描配置目录：主配置 → 根级其它 conf → script-opts（含一层子目录）。
///
/// 顺序固定，界面切换文件时不会因 `read_dir` 的返回顺序跳动。
pub fn scan_config_files(config_dir: &Path) -> Vec<ConfigFileView> {
    const PRIMARY: [&str; 2] = ["mpv.conf", "input.conf"];

    let mut paths: Vec<PathBuf> = Vec::new();
    for name in PRIMARY {
        let path = config_dir.join(name);
        if path.is_file() {
            paths.push(path);
        }
    }

    if let Ok(entries) = fs::read_dir(config_dir) {
        let mut others: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && is_conf_file(path)
                    && !path.file_name().is_some_and(|name| {
                        PRIMARY
                            .iter()
                            .any(|primary| name.to_string_lossy().eq_ignore_ascii_case(primary))
                    })
            })
            .collect();
        others.sort();
        paths.extend(others);
    }

    let script_opts = config_dir.join("script-opts");
    if let Ok(entries) = fs::read_dir(&script_opts) {
        let mut nested: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && is_conf_file(&path) {
                nested.push(path);
            } else if path.is_dir()
                && let Ok(inner) = fs::read_dir(&path)
            {
                for child in inner.flatten() {
                    let child_path = child.path();
                    if child_path.is_file() && is_conf_file(&child_path) {
                        nested.push(child_path);
                    }
                }
            }
        }
        nested.sort();
        paths.extend(nested);
    }

    paths
        .iter()
        .map(|path| read_config_file(config_dir, path))
        .collect()
}

fn is_conf_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("conf"))
}

/// 读一个配置文件并构建逐行视图。非 UTF-8 用宽松解码展示，但标记为不可编辑。
fn read_config_file(root: &Path, path: &Path) -> ConfigFileView {
    let name = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let format = ConfigFileFormat::of(path);
    let mut view = ConfigFileView {
        name,
        path: path.to_path_buf(),
        format,
        lines: Vec::new(),
        line_ending: LineEnding::Lf,
        trailing_newline: false,
        round_trip: false,
        entry_count: 0,
        comment_count: 0,
        inline_comment_count: 0,
        blank_count: 0,
        effective_count: 0,
        superseded_count: 0,
        error: None,
    };

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            view.error = Some(format!("读取失败：{error}"));
            return view;
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            // 整合包配置常见 GBK；展示要能看，写入必须拒绝，避免把编码写坏。
            view.error = Some("非 UTF-8 编码，仅展示；请在外部编辑器转码后再编辑".to_string());
            String::from_utf8_lossy(error.as_bytes()).into_owned()
        }
    };

    view.line_ending = if text.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };
    view.trailing_newline = text.ends_with('\n');
    view.round_trip = match format {
        ConfigFileFormat::InputConf => InputDoc::parse(&text).render() == text,
        _ => ConfDoc::parse(&text).render() == text,
    };
    view.lines = build_lines(&text, format);

    for line in &view.lines {
        match line.kind {
            ConfigLineKind::Entry | ConfigLineKind::Binding => view.entry_count += 1,
            ConfigLineKind::Comment => view.comment_count += 1,
            ConfigLineKind::Blank => view.blank_count += 1,
            _ => {}
        }
        if line.inline_comment.is_some() {
            view.inline_comment_count += 1;
        }
        match line.effective {
            EffectiveState::Active => view.effective_count += 1,
            EffectiveState::Superseded(_) => view.superseded_count += 1,
            EffectiveState::NotApplicable => {}
        }
    }
    view
}

/// 把原文逐行展开成视图。结构化结果只用来判定行角色与生效关系，
/// 展示文本一律取自原文行，因此缩进注释与行尾说明都能原样呈现。
fn build_lines(text: &str, format: ConfigFileFormat) -> Vec<ConfigLineView> {
    // 行号 → (键, 展示用值来源行的原文片段)
    let mut structured: HashMap<usize, (String, ConfigLineKind)> = HashMap::new();
    let mut last_by_key: HashMap<String, usize> = HashMap::new();

    match format {
        ConfigFileFormat::InputConf => {
            for binding in InputDoc::parse(text).bindings() {
                structured.insert(
                    binding.line_no,
                    (binding.key.clone(), ConfigLineKind::Binding),
                );
                last_by_key.insert(normalize_key(&binding.key), binding.line_no);
            }
        }
        _ => {
            for entry in ConfDoc::parse(text).entries() {
                structured.insert(entry.line_no, (entry.key.clone(), ConfigLineKind::Entry));
                last_by_key.insert(entry.key, entry.line_no);
            }
        }
    }

    let mut pending_comments: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (line_no, raw_with_cr) in text.lines().enumerate() {
        let raw = raw_with_cr.trim_end_matches('\r');
        let trimmed = raw.trim();
        let indent = raw.len() - raw.trim_start().len();

        let (kind, key) = if trimmed.is_empty() {
            (ConfigLineKind::Blank, None)
        } else if trimmed.starts_with('#') {
            (ConfigLineKind::Comment, None)
        } else if let Some((entry_key, entry_kind)) = structured.get(&line_no) {
            (*entry_kind, Some(entry_key.clone()))
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            (ConfigLineKind::Section, None)
        } else {
            (ConfigLineKind::Verbatim, None)
        };

        // 只有配置项/按键行才拆行尾说明；mpv.conf 里 ` #` 之后往往是中文参数说明，
        // 展示层拆成注释色，但不改变解析器把整串当值处理的语义。
        let (text_main, inline_comment) = match kind {
            ConfigLineKind::Entry | ConfigLineKind::Binding => match split_inline_comment(raw) {
                Some((head, tail)) => (head.trim_start().to_string(), Some(tail)),
                None => (raw.trim().to_string(), None),
            },
            _ => (raw.trim_start().to_string(), None),
        };

        let value = match kind {
            ConfigLineKind::Entry => text_main
                .find('=')
                .and_then(|pos| text_main.get(pos + 1..))
                .map(|v| v.trim().to_string()),
            ConfigLineKind::Binding => text_main
                .find(char::is_whitespace)
                .and_then(|pos| text_main.get(pos..))
                .map(|v| v.trim().to_string()),
            _ => None,
        };

        let mut comment = None;
        let mut doc_comment = None;
        match kind {
            ConfigLineKind::Comment => {
                let note = trimmed.trim_start_matches('#').trim().to_string();
                if !note.is_empty() {
                    pending_comments.push(note.clone());
                }
                comment = Some(note);
            }
            ConfigLineKind::Entry | ConfigLineKind::Binding => {
                // 注释归属：取紧邻上方注释块里最近的一条，避免把整个文件头
                // 几十行说明一股脑挂到第一项上。
                doc_comment = pending_comments
                    .iter()
                    .rev()
                    .find(|note| !note.is_empty())
                    .cloned();
                pending_comments.clear();
            }
            ConfigLineKind::Section | ConfigLineKind::Verbatim => pending_comments.clear(),
            // 空行不清空：注释与它说明的配置项之间常隔一行留白。
            ConfigLineKind::Blank => {}
        }

        let effective = match &key {
            Some(entry_key) => {
                let normalized = if format == ConfigFileFormat::InputConf {
                    normalize_key(entry_key)
                } else {
                    entry_key.clone()
                };
                match last_by_key.get(&normalized) {
                    Some(&last) if last == line_no => EffectiveState::Active,
                    Some(&last) => EffectiveState::Superseded(last),
                    None => EffectiveState::NotApplicable,
                }
            }
            None => EffectiveState::NotApplicable,
        };

        out.push(ConfigLineView {
            line_no,
            kind,
            text: text_main,
            inline_comment,
            key,
            value,
            comment,
            doc_comment,
            effective,
            indent,
        });
    }
    out
}

/// 拆出 ` #` 起的行尾说明，返回 (主文本, 含 `#` 的尾部)。
fn split_inline_comment(raw: &str) -> Option<(&str, String)> {
    let pos = raw.find(" #")?;
    let head = raw.get(..pos)?.trim_end();
    let tail = raw.get(pos + 1..)?.trim_end();
    if tail.is_empty() {
        return None;
    }
    Some((head, tail.to_string()))
}

// ---------------------------------------------------------------------------
// 编辑与撤回
// ---------------------------------------------------------------------------

/// 一次编辑的结果摘要（给状态栏用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    pub key: String,
    pub value: String,
    pub previous: Option<String>,
    pub backup_stamp: String,
}

/// 修改一个配置项/按键绑定的值，保留注释、缩进与对齐，写前备份。
///
/// 不走「解析成 Map 再拼回去」，而是只替换生效行里的值片段，
/// 其余行连空白都原样不动；替换后用解析器校验该行仍可结构化。
pub fn set_option_value(
    file: &Path,
    format: ConfigFileFormat,
    key: &str,
    value: &str,
    backups: &BackupManager,
) -> Result<EditOutcome, String> {
    let text = fs::read_to_string(file).map_err(|error| {
        format!(
            "无法按 UTF-8 读取 {}：{error}。非 UTF-8 配置请在外部编辑器修改。",
            file.display()
        )
    })?;
    if !matches_round_trip(&text, format) {
        return Err(
            "该文件往返自检未通过（可能是混合行尾），为避免破坏注释与行序已拒绝编辑".to_string(),
        );
    }

    let line_ending = if text.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };
    let mut raw_lines: Vec<String> = text
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();

    let target_line = effective_line_of(&text, format, key);
    let previous = target_line
        .and_then(|idx| raw_lines.get(idx))
        .map(|raw| display_value_of(raw, format));
    let new_line = match format {
        ConfigFileFormat::InputConf => format!("{key} {value}"),
        _ => format!("{key}={value}"),
    };
    match target_line {
        Some(idx) => {
            let source = raw_lines.get(idx).cloned().unwrap_or_default();
            let replaced = match format {
                ConfigFileFormat::InputConf => replace_binding_command(&source, value),
                _ => replace_entry_value(&source, value),
            }
            .ok_or_else(|| "无法定位该行中的取值片段，已拒绝编辑".to_string())?;
            if let Some(target) = raw_lines.get_mut(idx) {
                *target = replaced;
            }
        }
        None => raw_lines.push(new_line),
    }

    let separator = line_ending.separator();
    let mut rendered = raw_lines.join(separator);
    if text.ends_with('\n') {
        rendered.push_str(separator);
    }
    if !matches_round_trip(&rendered, format) || !still_structured(&rendered, format, key) {
        return Err("编辑后的内容无法通过解析自检，已取消写入（原文件未改动）".to_string());
    }

    let record = backups.backup_file(&operation_of(file), file)?;
    fs::write(file, &rendered).map_err(|error| {
        format!(
            "写入 {} 失败：{error}；原文件已备份，可用「撤回」恢复。",
            file.display()
        )
    })?;
    Ok(EditOutcome {
        key: key.to_string(),
        value: value.to_string(),
        previous,
        backup_stamp: record.stamp,
    })
}

/// 精确撤回某个文件的最近一次编辑（不受同名的其它播放器文件干扰）。
pub fn undo_edit_for(file: &Path, backups: &BackupManager) -> Result<BackupRecord, String> {
    let record = backups
        .list(&operation_of(file))
        .into_iter()
        .find(|record| same_file(&record.original_path, file))
        .ok_or_else(|| format!("{} 没有可撤回的备份", file.display()))?;
    backups.restore(&record)?;
    Ok(record)
}

/// 该文件还有多少次可撤回的备份。
pub fn available_undo(file: &Path, backups: &BackupManager) -> usize {
    backups
        .list(&operation_of(file))
        .iter()
        .filter(|record| same_file(&record.original_path, file))
        .count()
}

fn operation_of(file: &Path) -> String {
    file.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "conf".to_string())
}

fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn matches_round_trip(text: &str, format: ConfigFileFormat) -> bool {
    match format {
        ConfigFileFormat::InputConf => InputDoc::parse(text).render() == text,
        _ => ConfDoc::parse(text).render() == text,
    }
}

/// 生效行的 0 基行号（最后一次出现）。
fn effective_line_of(text: &str, format: ConfigFileFormat, key: &str) -> Option<usize> {
    match format {
        ConfigFileFormat::InputConf => InputDoc::parse(text)
            .bindings()
            .into_iter()
            .rev()
            .find(|binding| normalize_key(&binding.key) == normalize_key(key))
            .map(|binding| binding.line_no),
        _ => ConfDoc::parse(text)
            .entries()
            .into_iter()
            .rev()
            .find(|entry| entry.key == key)
            .map(|entry| entry.line_no),
    }
}

/// 编辑后自检：该行仍能被解析成同一个键。
fn still_structured(text: &str, format: ConfigFileFormat, key: &str) -> bool {
    effective_line_of(text, format, key).is_some()
}

fn display_value_of(raw: &str, format: ConfigFileFormat) -> String {
    let head = match split_inline_comment(raw) {
        Some((head, _)) => head.trim_start().to_string(),
        None => raw.trim().to_string(),
    };
    match format {
        ConfigFileFormat::InputConf => head
            .find(char::is_whitespace)
            .and_then(|pos| head.get(pos..))
            .map(|v| v.trim().to_string())
            .unwrap_or_default(),
        _ => head
            .find('=')
            .and_then(|pos| head.get(pos + 1..))
            .map(|v| v.trim().to_string())
            .unwrap_or_default(),
    }
}

/// 只替换 `key=value` 里的 value，保留等号前的排版、值后对齐空格与行尾说明。
fn replace_entry_value(raw: &str, value: &str) -> Option<String> {
    let eq = raw.find('=')?;
    let head = raw.get(..=eq)?;
    let after = raw.get(eq + 1..)?;
    let leading_len = after.len() - after.trim_start().len();
    let leading = after.get(..leading_len)?;
    let body = after.trim_start();
    let cut = body.find(" #").unwrap_or(body.len());
    let value_part = body.get(..cut)?;
    let value_trimmed = value_part.trim_end();
    let padding = value_part.get(value_trimmed.len()..)?;
    let tail = body.get(cut..)?;
    Some(format!("{head}{leading}{value}{padding}{tail}"))
}

/// 只替换 `KEY command` 里的 command，保留缩进、键名、对齐空格与行尾注释。
fn replace_binding_command(raw: &str, command: &str) -> Option<String> {
    let body = raw.trim_start();
    let indent = raw.get(..raw.len() - body.len())?;
    let split = body.find(char::is_whitespace)?;
    let key = body.get(..split)?;
    let rest = body.get(split..)?;
    let rest_trimmed = rest.trim_start();
    let spaces = rest.get(..rest.len() - rest_trimmed.len())?;
    let cut = rest_trimmed.find(" #").unwrap_or(rest_trimmed.len());
    let command_part = rest_trimmed.get(..cut)?;
    let command_trimmed = command_part.trim_end();
    let padding = command_part.get(command_trimmed.len()..)?;
    let tail = rest_trimmed.get(cut..)?;
    Some(format!("{indent}{key}{spaces}{command}{padding}{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与真实整合包一致的形态：缩进注释、行尾说明、重复键、CRLF 友好。
    const SAMPLE: &str = concat!(
        "# 基本播放优化\n",
        "vo=gpu-next        # <gpu|gpu-next> 视频输出驱动\n",
        "\n",
        "  # 音量相关\n",
        "volume=100\n",
        "volume=55\n",
        "\n",
        "cache=yes     # 启用缓存\n",
    );

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-doc-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(
            fs::create_dir_all(&dir).is_ok(),
            "无法创建 {}",
            dir.display()
        );
        dir
    }

    #[test]
    fn comments_are_visible_with_text_and_inline_notes() {
        let lines = build_lines(SAMPLE, ConfigFileFormat::MpvConf);
        assert_eq!(lines[0].kind, ConfigLineKind::Comment);
        assert_eq!(lines[0].comment.as_deref(), Some("基本播放优化"));
        assert_eq!(
            lines[1].inline_comment.as_deref(),
            Some("# <gpu|gpu-next> 视频输出驱动")
        );
        assert_eq!(lines[1].value.as_deref(), Some("gpu-next"));
        assert_eq!(lines[1].doc_comment.as_deref(), Some("基本播放优化"));
        assert_eq!(lines[3].indent, 2);
        assert_eq!(lines[4].doc_comment.as_deref(), Some("音量相关"));
        assert_eq!(lines[7].inline_comment.as_deref(), Some("# 启用缓存"));
    }

    #[test]
    fn last_wins_marks_superseded_line_with_effective_line() {
        let lines = build_lines(SAMPLE, ConfigFileFormat::MpvConf);
        assert_eq!(lines[4].effective, EffectiveState::Superseded(5));
        assert_eq!(lines[5].effective, EffectiveState::Active);
        assert_eq!(lines[5].display_line_no(), 6);
    }

    #[test]
    fn input_bindings_use_normalized_keys() {
        let text = "Ctrl+c ignore\nctrl+c quit # 覆盖\nq quit\n";
        let lines = build_lines(text, ConfigFileFormat::InputConf);
        assert_eq!(lines[0].kind, ConfigLineKind::Binding);
        assert_eq!(lines[0].effective, EffectiveState::Superseded(1));
        assert_eq!(lines[1].effective, EffectiveState::Active);
        assert_eq!(lines[1].inline_comment.as_deref(), Some("# 覆盖"));
    }

    #[test]
    fn file_view_round_trips_lf_and_crlf() {
        let dir = temp_dir("roundtrip");
        let lf = dir.join("mpv.conf");
        let crlf = dir.join("input.conf");
        assert!(fs::write(&lf, SAMPLE).is_ok());
        assert!(fs::write(&crlf, SAMPLE.replace('\n', "\r\n")).is_ok());
        let lf_view = read_config_file(&dir, &lf);
        let crlf_view = read_config_file(&dir, &crlf);
        assert!(lf_view.round_trip, "LF 往返自检应通过");
        assert!(crlf_view.round_trip, "CRLF 往返自检应通过");
        assert_eq!(crlf_view.line_ending, LineEnding::Crlf);
        assert_eq!(lf_view.line_ending, LineEnding::Lf);
        assert_eq!(lf_view.comment_count, 2);
        assert_eq!(lf_view.inline_comment_count, 2);
        assert_eq!(lf_view.entry_count, 4);
        assert_eq!(lf_view.superseded_count, 1);
        assert!(lf_view.editable());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_collects_primary_then_script_opts_in_stable_order() {
        let dir = temp_dir("scan");
        let opts = dir.join("script-opts");
        assert!(fs::create_dir_all(&opts).is_ok());
        for name in ["mpv.conf", "input.conf", "menu.conf"] {
            assert!(fs::write(dir.join(name), "a=b\n").is_ok());
        }
        assert!(fs::write(opts.join("uosc.conf"), "a=b\n").is_ok());
        let files = scan_config_files(&dir);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "mpv.conf",
                "input.conf",
                "menu.conf",
                "script-opts/uosc.conf"
            ]
        );
        assert_eq!(files[1].format, ConfigFileFormat::InputConf);
        assert_eq!(files[3].format, ConfigFileFormat::ScriptOption);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_keeps_comments_then_undo_restores_original() {
        let dir = temp_dir("edit-undo");
        let file = dir.join("mpv.conf");
        assert!(fs::write(&file, SAMPLE).is_ok());
        let backups = BackupManager::new(dir.join("backups"));

        let outcome = set_option_value(&file, ConfigFileFormat::MpvConf, "volume", "80", &backups);
        assert!(outcome.is_ok(), "编辑失败：{:?}", outcome.as_ref().err());
        let edited = fs::read_to_string(&file);
        assert!(edited.is_ok());
        let Ok(edited) = edited else { return };
        assert!(edited.contains("volume=80"), "生效行应被修改：{edited}");
        assert!(edited.contains("volume=100"), "被覆盖行不应改动：{edited}");
        assert!(
            edited.contains("# <gpu|gpu-next> 视频输出驱动"),
            "行尾说明必须保留：{edited}"
        );
        assert!(
            edited.contains("  # 音量相关"),
            "缩进注释必须保留：{edited}"
        );
        assert_eq!(available_undo(&file, &backups), 1);

        assert!(undo_edit_for(&file, &backups).is_ok());
        assert_eq!(fs::read_to_string(&file).ok().as_deref(), Some(SAMPLE));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_preserves_inline_comment_on_edited_line() {
        let dir = temp_dir("edit-inline");
        let file = dir.join("mpv.conf");
        assert!(fs::write(&file, SAMPLE).is_ok());
        let backups = BackupManager::new(dir.join("backups"));
        assert!(set_option_value(&file, ConfigFileFormat::MpvConf, "vo", "gpu", &backups).is_ok());
        let text = fs::read_to_string(&file);
        assert!(text.is_ok());
        let Ok(text) = text else { return };
        assert!(text.contains("vo=gpu"), "新值应写入：{text}");
        assert!(
            text.contains("# <gpu|gpu-next> 视频输出驱动"),
            "值换了，行尾说明仍应在：{text}"
        );
        assert!(text.contains("  # 音量相关"), "别的行一字不动：{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_utf8_file_is_display_only() {
        let dir = temp_dir("encoding");
        let file = dir.join("mpv.conf");
        // GBK 编码的中文注释
        assert!(fs::write(&file, b"# \xd6\xd0\xce\xc4\nvo=gpu\n").is_ok());
        let view = read_config_file(&dir, &file);
        assert!(view.error.is_some());
        assert!(!view.editable());
        assert_eq!(view.lines.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
