//! 冲突检测报告的结构化聚合（FR-11 UI 数据源）。
//!
//! 把"读文件 → 解析 → 冲突检测 → 往返自检"封装为可被 UI 直接渲染的报告，
//! 替代早期的 stderr 文本诊断。

use std::fs;
use std::path::PathBuf;

use super::conf::ConfDoc;
use super::conflicts::{
    KeyConflict, ValueConflict, detect_input_conflicts, detect_value_conflicts,
};
use super::input::InputDoc;
use crate::core::players::DetectedPlayer;

/// 单个播放器配置目录的扫描报告
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerConfigReport {
    pub player_name: String,
    pub config_dir: PathBuf,
    /// input.conf 按键绑定冲突
    pub input_conflicts: Vec<KeyConflict>,
    /// mpv.conf 值冲突
    pub value_conflicts: Vec<ValueConflict>,
    /// 扫描过程中的非致命警告（如往返渲染差异）
    pub warnings: Vec<String>,
    /// 成功读取并解析的文件名
    pub scanned_files: Vec<String>,
}

impl PlayerConfigReport {
    pub fn has_findings(&self) -> bool {
        !self.input_conflicts.is_empty()
            || !self.value_conflicts.is_empty()
            || !self.warnings.is_empty()
    }
}

/// 扫描全部播放器的配置目录；无配置目录的播放器不产生报告
pub fn scan_player_configs(players: &[DetectedPlayer]) -> Vec<PlayerConfigReport> {
    let mut reports = Vec::new();
    for player in players {
        let Some(config_dir) = &player.config_dir else {
            continue;
        };
        let mut report = PlayerConfigReport {
            player_name: player.name.clone(),
            config_dir: config_dir.clone(),
            ..PlayerConfigReport::default()
        };

        let input_path = config_dir.join("input.conf");
        if let Ok(text) = fs::read_to_string(&input_path) {
            report.scanned_files.push("input.conf".to_string());
            let doc = InputDoc::parse(&text);
            if doc.render() != text {
                report
                    .warnings
                    .push("input.conf 往返渲染与原文存在差异（可能为混合行尾）".to_string());
            }
            report.input_conflicts = detect_input_conflicts(&[("input.conf", &doc)]);
        }

        let mpv_conf_path = config_dir.join("mpv.conf");
        if let Ok(text) = fs::read_to_string(&mpv_conf_path) {
            report.scanned_files.push("mpv.conf".to_string());
            let doc = ConfDoc::parse(&text);
            if doc.render() != text {
                report
                    .warnings
                    .push("mpv.conf 往返渲染与原文存在差异（可能为混合行尾）".to_string());
            }
            report.value_conflicts = detect_value_conflicts(&[("mpv.conf", &doc)]);
        }

        reports.push(report);
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 创建唯一的测试目录（按进程 id + 标签命名，避免并行测试互相干扰）
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-test-{}-{tag}", std::process::id()));
        // 幂等清理上次运行残留；失败仅说明目录被外部占用，create_dir_all 会再暴露问题
        let _ = fs::remove_dir_all(&dir);
        assert!(
            fs::create_dir_all(&dir).is_ok(),
            "无法创建测试目录 {}",
            dir.display()
        );
        dir
    }

    /// 尽力写文本文件（测试前置条件，失败即断言失败）
    fn write_file(path: &Path, text: &str) {
        assert!(fs::write(path, text).is_ok(), "无法写入 {}", path.display());
    }

    fn make_player(config_dir: &Path) -> DetectedPlayer {
        DetectedPlayer {
            name: "测试播放器".to_string(),
            kind: crate::core::players::PlayerKind::Mpv,
            exe_path: config_dir.join("mpv.exe"),
            version: None,
            config_dir: Some(config_dir.to_path_buf()),
            source: crate::core::players::DetectSource::CommonPath,
        }
    }

    #[test]
    fn scan_reports_input_key_conflicts() {
        let dir = temp_dir("input-conflict");
        write_file(&dir.join("input.conf"), "q quit\nQ quit\n");
        let players = [make_player(&dir)];
        let reports = scan_player_configs(&players);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].input_conflicts.len(), 1);
        assert_eq!(reports[0].input_conflicts[0].key, "q");
        assert!(reports[0].has_findings());
        // 尽力清理；失败留给下次运行幂等清理
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_reports_value_conflicts_and_clean_case() {
        let dir = temp_dir("value-clean");
        write_file(&dir.join("input.conf"), "q quit\n");
        write_file(&dir.join("mpv.conf"), "vo=gpu\nvo=gpu\n");
        let players = [make_player(&dir)];
        let reports = scan_player_configs(&players);
        assert_eq!(reports[0].input_conflicts.len(), 0);
        assert_eq!(reports[0].value_conflicts.len(), 1);
        assert!(reports[0].value_conflicts[0].same_value);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn player_without_config_dir_is_skipped() {
        let mut player = make_player(Path::new("Z:/definitely-missing-neve"));
        player.config_dir = None;
        assert!(scan_player_configs(&[player]).is_empty());
    }
}
