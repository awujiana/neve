//! input.conf（按键绑定）解析。
//!
//! 行语法：`KEY command`，尾部可选 ` # 注释`；`#` 开头为注释行；
//! `[section]` 形式的小节标记原样保留。键名大小写不敏感（见 `normalize_key`）。

/// 一条按键绑定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBinding {
    /// 键名（按文件原样保留，如 `Ctrl+c`）
    pub key: String,
    /// 绑定的命令（可能包含空格）
    pub command: String,
    /// 0 基行号
    pub line_no: usize,
    /// 尾部注释（不含 `#` 前缀）
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputLine {
    Binding(InputBinding),
    Comment(String),
    Blank,
    Verbatim(String),
}

/// input.conf 文档
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputDoc {
    lines: Vec<InputLine>,
    crlf: bool,
    trailing_newline: bool,
}

/// 键名规范化（冲突检测用启发式）：去空白并转小写。
/// mpv 对字母键不区分大小写；数字与符号键不受 lowercasing 影响，安全。
pub fn normalize_key(key: &str) -> String {
    key.trim().to_lowercase()
}

impl InputDoc {
    /// 解析 input.conf 文本（接受 LF / CRLF）
    pub fn parse(text: &str) -> Self {
        let crlf = text.contains("\r\n");
        let trailing_newline = text.ends_with('\n');
        let lines = text
            .lines()
            .map(|raw| {
                let line = raw.trim_end_matches('\r');
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    InputLine::Blank
                } else if trimmed.starts_with('#') {
                    InputLine::Comment(trimmed.to_string())
                } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    InputLine::Verbatim(trimmed.to_string())
                } else {
                    // 尾部注释：第一个 ` #` 起（避免切断命令中可能的 #）
                    let (body, comment) = match trimmed.find(" #") {
                        Some(pos) => (
                            trimmed[..pos].trim(),
                            Some(
                                trimmed[pos + 1..]
                                    .trim_start_matches('#')
                                    .trim()
                                    .to_string(),
                            ),
                        ),
                        None => (trimmed, None),
                    };
                    let mut tokens = body.splitn(2, char::is_whitespace);
                    match (tokens.next(), tokens.next()) {
                        (Some(key), Some(command)) if !command.trim().is_empty() => {
                            InputLine::Binding(InputBinding {
                                key: key.to_string(),
                                command: command.trim().to_string(),
                                line_no: 0,
                                comment,
                            })
                        }
                        _ => InputLine::Verbatim(trimmed.to_string()),
                    }
                }
            })
            .collect();
        // 回填真实行号
        let mut doc = Self {
            lines,
            crlf,
            trailing_newline,
        };
        for (line_no, line) in doc.lines.iter_mut().enumerate() {
            if let InputLine::Binding(binding) = line {
                binding.line_no = line_no;
            }
        }
        doc
    }

    /// 全部按键绑定（按文档顺序）
    pub fn bindings(&self) -> Vec<InputBinding> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                InputLine::Binding(binding) => Some(binding.clone()),
                _ => None,
            })
            .collect()
    }

    /// 修改绑定：更新该键最后一次出现位置的命令，不存在则追加
    #[allow(dead_code)] // TODO(M2-UI): 按键编辑器 UI 接入后移除此属性
    pub fn set_binding(&mut self, key: &str, command: &str) -> bool {
        for line in self.lines.iter_mut().rev() {
            if let InputLine::Binding(binding) = line
                && normalize_key(&binding.key) == normalize_key(key)
            {
                binding.command = command.to_string();
                return true;
            }
        }
        self.lines.push(InputLine::Binding(InputBinding {
            key: key.to_string(),
            command: command.to_string(),
            line_no: self.lines.len(),
            comment: None,
        }));
        false
    }

    /// 渲染回文本（与源文本逐字节一致，除非行尾风格被统一）
    pub fn render(&self) -> String {
        let sep = if self.crlf { "\r\n" } else { "\n" };
        let mut out = self
            .lines
            .iter()
            .map(|line| match line {
                InputLine::Binding(binding) => match &binding.comment {
                    Some(comment) => format!("{} {} # {}", binding.key, binding.command, comment),
                    None => format!("{} {}", binding.key, binding.command),
                },
                InputLine::Comment(text) | InputLine::Verbatim(text) => text.clone(),
                InputLine::Blank => String::new(),
            })
            .collect::<Vec<_>>()
            .join(sep);
        if self.trailing_newline {
            out.push_str(sep);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# 按键配置\nCtrl+c ignore # 示例\n\nq quit\nQ quit-watch-later\n";

    #[test]
    fn parse_extracts_bindings() {
        let doc = InputDoc::parse(SAMPLE);
        let bindings = doc.bindings();
        assert_eq!(bindings.len(), 3);
        assert_eq!(bindings[0].key, "Ctrl+c");
        assert_eq!(bindings[0].command, "ignore");
        assert_eq!(bindings[0].comment.as_deref(), Some("示例"));
        assert_eq!(bindings[0].line_no, 1);
    }

    #[test]
    fn render_round_trip_lf() {
        assert_eq!(InputDoc::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn render_round_trip_crlf() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        assert_eq!(InputDoc::parse(&crlf).render(), crlf);
    }

    #[test]
    fn malformed_lines_are_verbatim() {
        let doc = InputDoc::parse("just-a-word\n[section]\n");
        assert_eq!(doc.lines[0], InputLine::Verbatim("just-a-word".into()));
        assert_eq!(doc.lines[1], InputLine::Verbatim("[section]".into()));
        assert!(doc.bindings().is_empty());
    }

    #[test]
    fn set_binding_updates_last() {
        let mut doc = InputDoc::parse(SAMPLE);
        assert!(doc.set_binding("Q", "quit"));
        assert_eq!(doc.bindings()[2].command, "quit");
        assert!(!doc.set_binding("F12", "script-message hi")); // 新键走追加
        let last = doc.bindings().pop();
        assert_eq!(last.map(|b| b.key), Some("F12".to_string()));
    }
}
