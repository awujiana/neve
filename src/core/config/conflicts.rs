//! 冲突检测（FR-11）：input.conf 按键重复绑定、conf 同键多来源覆盖链。
//!
//! 语义约定：文档按传入顺序即 mpv 加载优先顺序，靠后者覆盖靠前者；
//! 单个文档内部重复键也按"最后一次出现生效"处理。

use std::collections::HashMap;

use super::conf::ConfDoc;
use super::input::{InputDoc, normalize_key};

/// input.conf 的一处绑定（跨来源汇总用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingSource {
    pub command: String,
    /// 来源标签（如 "input.conf"、"uosc 推荐"）
    pub source: String,
    pub line_no: usize,
}

/// 同一键的重复绑定冲突
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConflict {
    /// 规范化后的键名
    pub key: String,
    /// 全部绑定（按优先顺序，靠后生效）
    pub bindings: Vec<BindingSource>,
    /// 各绑定命令是否互不相同（true 时通常需要人工裁决）
    pub commands_differ: bool,
    /// 实际生效的绑定下标（最后一个）
    pub effective_index: usize,
}

/// conf 值冲突组
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueConflict {
    pub key: String,
    /// 全部取值（按优先顺序，靠后生效）
    pub values: Vec<ValueSource>,
    /// 实际生效值
    pub effective: String,
    /// 是否所有取值相同（重复定义但无值分歧）
    pub same_value: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueSource {
    pub value: String,
    pub source: String,
    pub line_no: usize,
}

/// 检测按键重复绑定：同一（规范化）键被绑定多次即冲突。
/// `docs` 顺序 = 加载优先顺序，靠后覆盖靠前。
pub fn detect_input_conflicts(docs: &[(&str, &InputDoc)]) -> Vec<KeyConflict> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Vec<BindingSource>> = HashMap::new();
    for (source, doc) in docs {
        for binding in doc.bindings() {
            let key = normalize_key(&binding.key);
            if !map.contains_key(&key) {
                order.push(key.clone());
            }
            map.entry(key).or_default().push(BindingSource {
                command: binding.command,
                source: (*source).to_string(),
                line_no: binding.line_no,
            });
        }
    }
    order
        .into_iter()
        .filter_map(|key| {
            let bindings = map.remove(&key)?;
            if bindings.len() < 2 {
                return None;
            }
            let first = bindings[0].command.clone();
            let commands_differ = bindings.iter().any(|b| b.command != first);
            let effective_index = bindings.len() - 1;
            Some(KeyConflict {
                key,
                bindings,
                commands_differ,
                effective_index,
            })
        })
        .collect()
}

/// 检测 conf 同键多来源冲突（含单文档内部重复键）。
/// `docs` 顺序 = 加载优先顺序（如主 mpv.conf → include → 脚本 conf）。
pub fn detect_value_conflicts(docs: &[(&str, &ConfDoc)]) -> Vec<ValueConflict> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Vec<ValueSource>> = HashMap::new();
    for (source, doc) in docs {
        for entry in doc.entries() {
            if !map.contains_key(&entry.key) {
                order.push(entry.key.clone());
            }
            map.entry(entry.key).or_default().push(ValueSource {
                value: entry.value,
                source: (*source).to_string(),
                line_no: entry.line_no,
            });
        }
    }
    order
        .into_iter()
        .filter_map(|key| {
            let values = map.remove(&key)?;
            if values.len() < 2 {
                return None;
            }
            let first = values[0].value.clone();
            let same_value = values.iter().all(|v| v.value == first);
            let effective = values.last().map(|v| v.value.clone())?;
            Some(ValueConflict {
                key,
                values,
                effective,
                same_value,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_conflict_groups_by_normalized_key() {
        let main_conf = InputDoc::parse("Ctrl+c ignore\nq quit\n");
        let uosc_conf = InputDoc::parse("ctrl+c quit\n");
        let conflicts = detect_input_conflicts(&[("input.conf", &main_conf), ("uosc", &uosc_conf)]);
        assert_eq!(conflicts.len(), 1);
        let conflict = &conflicts[0];
        assert_eq!(conflict.key, "ctrl+c");
        assert_eq!(conflict.bindings.len(), 2);
        assert!(conflict.commands_differ);
        assert_eq!(conflict.effective_index, 1);
        assert_eq!(conflict.bindings[1].source, "uosc");
    }

    #[test]
    fn same_command_duplicates_are_flagged_but_marked() {
        let doc = InputDoc::parse("q quit\nq quit\n");
        let conflicts = detect_input_conflicts(&[("input.conf", &doc)]);
        assert_eq!(conflicts.len(), 1);
        assert!(!conflicts[0].commands_differ);
    }

    #[test]
    fn value_conflict_reports_effective_and_sameness() {
        let main_conf = ConfDoc::parse("volume=100\n");
        let script_conf = ConfDoc::parse("volume=55\n");
        let conflicts =
            detect_value_conflicts(&[("mpv.conf", &main_conf), ("script.conf", &script_conf)]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].effective, "55");
        assert!(!conflicts[0].same_value);
    }

    #[test]
    fn duplicate_same_value_conflict_detected() {
        let doc = ConfDoc::parse("vo=gpu\nvo=gpu\n");
        let conflicts = detect_value_conflicts(&[("mpv.conf", &doc)]);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].same_value);
    }

    #[test]
    fn no_conflicts_when_keys_unique() {
        let doc = InputDoc::parse("q quit\nQ quit-watch-later\n");
        // Q 与 q 规范化后同为 "q" —— 这是真实冲突（后绑定生效）
        assert_eq!(detect_input_conflicts(&[("input.conf", &doc)]).len(), 1);
        let distinct = InputDoc::parse("q quit\nw noop\n");
        assert!(detect_input_conflicts(&[("input.conf", &distinct)]).is_empty());
    }
}
