//! 通用 key=value conf 文档（mpv.conf / script-opts/*.conf）。
//!
//! mpv.conf 语法约定：`key=value`；`#` 开头的整行为注释；不支持行内注释
//! （值中可合法包含 `#`）。重复键时 mpv 按"最后一次出现生效"处理。

/// 文档中的一行（结构化或原样保留）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfLine {
    /// `key=value`
    Entry { key: String, value: String },
    /// `# ...` 注释（保留原文）
    Comment(String),
    /// 空白行
    Blank,
    /// 无法结构化的行，原样保留
    Verbatim(String),
}

/// 一条配置项及其位置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfEntry {
    pub key: String,
    pub value: String,
    /// 0 基行号
    pub line_no: usize,
}

/// conf 文档：行序、注释、空行全部保留
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfDoc {
    /// 结构化判定（语义层）：供 `entries()` / `get()` / 冲突检测使用。
    lines: Vec<ConfLine>,
    /// 每行原文（去掉行尾 `\r`）。渲染以它为准，才能真正做到字节级往返：
    /// 真实整合包配置普遍带缩进注释、对齐空格与行尾空格，从 `lines` 重新拼
    /// 一行必然把这些空白丢掉。`set_key` 会同步更新被改的那一行。
    raw_lines: Vec<String>,
    /// 源文本是否使用 CRLF 行尾（渲染时按此还原）
    crlf: bool,
    /// 源文本是否以换行结尾
    trailing_newline: bool,
}

impl ConfDoc {
    /// 解析 conf 文本（接受 LF / CRLF）
    pub fn parse(text: &str) -> Self {
        let crlf = text.contains("\r\n");
        let trailing_newline = text.ends_with('\n');
        let raw_lines: Vec<String> = text
            .lines()
            .map(|raw| raw.trim_end_matches('\r').to_string())
            .collect();
        let lines = raw_lines
            .iter()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    ConfLine::Blank
                } else if trimmed.starts_with('#') {
                    ConfLine::Comment(trimmed.to_string())
                } else if let Some(eq) = trimmed.find('=') {
                    let key = trimmed[..eq].trim().to_string();
                    if key.is_empty() {
                        ConfLine::Verbatim(trimmed.to_string())
                    } else {
                        let value = trimmed[eq + 1..].trim().to_string();
                        ConfLine::Entry { key, value }
                    }
                } else {
                    ConfLine::Verbatim(trimmed.to_string())
                }
            })
            .collect();
        Self {
            lines,
            raw_lines,
            crlf,
            trailing_newline,
        }
    }

    /// 全部配置项（按文档顺序；重复键如实返回）
    pub fn entries(&self) -> Vec<ConfEntry> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(line_no, line)| match line {
                ConfLine::Entry { key, value } => Some(ConfEntry {
                    key: key.clone(),
                    value: value.clone(),
                    line_no,
                }),
                _ => None,
            })
            .collect()
    }

    /// 读取键的当前生效值（mpv 语义：最后一次出现生效）
    pub fn get(&self, key: &str) -> Option<String> {
        self.lines.iter().rev().find_map(|line| match line {
            ConfLine::Entry {
                key: entry_key,
                value,
            } if entry_key == key => Some(value.clone()),
            _ => None,
        })
    }

    /// 修改键值：更新最后一次出现的位置（保持 mpv 生效语义），不存在则追加。
    ///
    /// 只重写被改的那一行的原文，其余行（含注释、缩进、对齐空格）原样不动。
    pub fn set_key(&mut self, key: &str, value: &str) -> bool {
        for (idx, line) in self.lines.iter_mut().enumerate().rev() {
            if let ConfLine::Entry {
                key: existing_key,
                value: existing_value,
            } = line
                && existing_key == key
            {
                *existing_value = value.to_string();
                if let Some(raw) = self.raw_lines.get_mut(idx) {
                    *raw = format!("{key}={value}");
                }
                return true;
            }
        }
        self.lines.push(ConfLine::Entry {
            key: key.to_string(),
            value: value.to_string(),
        });
        self.raw_lines.push(format!("{key}={value}"));
        false
    }

    /// 渲染回文本（与源文本逐字节一致，除非行尾风格被统一）
    pub fn render(&self) -> String {
        let sep = if self.crlf { "\r\n" } else { "\n" };
        let mut out = self.raw_lines.join(sep);
        if self.trailing_newline {
            out.push_str(sep);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# profile\nvo=gpu\n\nvolume=100 # 保留原样\nvolume=55\n";

    #[test]
    fn parse_preserves_structure() {
        let doc = ConfDoc::parse(SAMPLE);
        let entries = doc.entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            ConfEntry {
                key: "vo".into(),
                value: "gpu".into(),
                line_no: 1,
            }
        );
        // 注释与空行被保留
        assert_eq!(doc.lines[0], ConfLine::Comment("# profile".into()));
        assert_eq!(doc.lines[2], ConfLine::Blank);
    }

    #[test]
    fn render_round_trip_lf() {
        assert_eq!(ConfDoc::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn render_round_trip_crlf() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        assert_eq!(ConfDoc::parse(&crlf).render(), crlf);
    }

    #[test]
    fn get_last_wins() {
        let doc = ConfDoc::parse(SAMPLE);
        assert_eq!(doc.get("volume"), Some("55".to_string()));
        assert_eq!(doc.get("missing"), None);
    }

    #[test]
    fn set_key_updates_in_place() {
        let mut doc = ConfDoc::parse(SAMPLE);
        assert!(doc.set_key("volume", "80"));
        // 修改的是最后一次出现（生效行），注释行不动
        assert_eq!(
            doc.render(),
            "# profile\nvo=gpu\n\nvolume=100 # 保留原样\nvolume=80\n"
        );
    }

    #[test]
    fn set_key_appends_missing() {
        let mut doc = ConfDoc::parse("# only comment\n");
        assert!(!doc.set_key("vo", "gpu"));
        assert_eq!(doc.get("vo"), Some("gpu".to_string()));
    }

    /// 真实整合包配置的常见形态：缩进注释、对齐空格、行尾空格、行尾 `#` 说明。
    const MESSY: &str = concat!(
        "##  基本说明  \n",
        "  # 缩进注释\n",
        "\n",
        "conf  =~~/files/mpv.conf   \n",
        "vo=gpu-next        # 视频输出驱动\n",
        "\t# 制表符注释\t\n",
        "volume = 55\n",
    );

    #[test]
    fn render_round_trip_messy_lf() {
        assert_eq!(ConfDoc::parse(MESSY).render(), MESSY);
    }

    #[test]
    fn render_round_trip_messy_crlf() {
        let crlf = MESSY.replace('\n', "\r\n");
        assert_eq!(ConfDoc::parse(&crlf).render(), crlf);
    }

    #[test]
    fn set_key_leaves_other_lines_untouched() {
        let mut doc = ConfDoc::parse(MESSY);
        assert!(doc.set_key("volume", "80"));
        assert_eq!(doc.render(), MESSY.replace("volume = 55", "volume=80"));
    }
}
