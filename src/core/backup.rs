//! core::backup —— 写操作前的备份与撤回（架构文档 §4.2）
//!
//! 目录布局：
//! ```text
//! <backup_root>/<operation>/<stamp>/<file_name>      备份副本
//! <backup_root>/<operation>/<stamp>/.neve-orig       原始绝对路径（单行）
//! ```
//! `stamp` 为 Unix 纪元纳秒，天然按时间排序；`list` 返回新→旧，
//! `restore` 把备份副本写回原路径即"撤回修改"。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 一次备份记录（同时是撤回句柄）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRecord {
    /// 操作名（同时作为备份分组目录，如 "uosc.conf"）
    pub operation: String,
    /// 备份副本路径
    pub backup_path: PathBuf,
    /// 原始文件绝对路径
    pub original_path: PathBuf,
    /// 时间戳目录名（纪元纳秒）
    pub stamp: String,
}

/// 备份管理器
#[derive(Debug, Clone)]
pub struct BackupManager {
    root: PathBuf,
}

impl BackupManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 使用 Neve 数据目录下的 `backups/` 作为备份根
    pub fn with_default_root() -> Self {
        Self::new(super::store::data_dir().join("backups"))
    }

    /// 备份单个文件（文件必须已存在；返回撤回句柄）
    pub fn backup_file(&self, operation: &str, file: &Path) -> Result<BackupRecord, String> {
        if !file.is_file() {
            return Err(format!("待备份文件不存在：{}", file.display()));
        }
        let original = absolute(file)?;
        let stamp = epoch_nanos()?;
        let dir = self.root.join(operation).join(&stamp);
        fs::create_dir_all(&dir).map_err(|e| format!("无法创建备份目录 {}：{e}", dir.display()))?;

        let file_name = original
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "backup".to_string());
        let backup_path = dir.join(&file_name);
        fs::copy(&original, &backup_path).map_err(|e| {
            format!(
                "备份 {} -> {} 失败：{e}",
                original.display(),
                backup_path.display()
            )
        })?;
        // 记录原始路径，供撤回时定位（单行文本，避免引入 JSON 依赖）
        fs::write(
            dir.join(".neve-orig"),
            original.to_string_lossy().as_bytes(),
        )
        .map_err(|e| format!("写入备份元数据失败：{e}"))?;

        Ok(BackupRecord {
            operation: operation.to_string(),
            backup_path,
            original_path: original,
            stamp,
        })
    }

    /// 列出某操作的备份记录（新 → 旧）
    pub fn list(&self, operation: &str) -> Vec<BackupRecord> {
        let op_dir = self.root.join(operation);
        let Ok(entries) = fs::read_dir(&op_dir) else {
            return Vec::new();
        };
        let mut records: Vec<BackupRecord> = entries
            .flatten()
            .filter_map(|entry| {
                let dir = entry.path();
                if !dir.is_dir() {
                    return None;
                }
                let stamp = dir.file_name()?.to_string_lossy().to_string();
                let original_path = fs::read_to_string(dir.join(".neve-orig"))
                    .ok()
                    .map(|text| PathBuf::from(text.trim()))?;
                // 取备份目录中的第一个普通文件作为副本
                let backup_path = fs::read_dir(&dir)
                    .ok()?
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| p.is_file() && p.file_name().is_some_and(|n| n != ".neve-orig"))?;
                Some(BackupRecord {
                    operation: operation.to_string(),
                    backup_path,
                    original_path,
                    stamp,
                })
            })
            .collect();
        records.sort_by(|a, b| b.stamp.cmp(&a.stamp));
        records
    }

    /// 撤回指定备份：把备份副本写回原路径（父目录缺失时创建）
    #[allow(dead_code)] // TODO(M2-UI): 撤回按钮接入后移除
    pub fn restore(&self, record: &BackupRecord) -> Result<(), String> {
        if !record.backup_path.is_file() {
            return Err(format!(
                "备份副本已不存在：{}",
                record.backup_path.display()
            ));
        }
        if let Some(parent) = record.original_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建目标目录 {}：{e}", parent.display()))?;
        }
        fs::copy(&record.backup_path, &record.original_path).map_err(|e| {
            format!(
                "撤回失败（{} -> {}）：{e}",
                record.backup_path.display(),
                record.original_path.display()
            )
        })?;
        Ok(())
    }

    /// 撤回某操作最近一次修改
    #[allow(dead_code)] // TODO(M2-UI): 撤回按钮接入后移除
    pub fn restore_latest(&self, operation: &str) -> Result<BackupRecord, String> {
        let mut records = self.list(operation);
        if records.is_empty() {
            return Err(format!("操作 {operation} 没有可撤回的备份"));
        }
        let latest = records.remove(0);
        self.restore(&latest)?;
        Ok(latest)
    }
}

/// 取绝对路径（相对路径按当前工作目录展开）
fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|e| format!("无法获取当前目录以展开相对路径：{e}"))
}

fn epoch_nanos() -> Result<String, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .map_err(|e| format!("系统时间异常，无法生成备份时间戳：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-backup-{}-{tag}", std::process::id()));
        // 幂等清理上次运行残留；失败仅说明被外部占用，create_dir_all 会再暴露问题
        let _ = fs::remove_dir_all(&dir);
        assert!(
            fs::create_dir_all(&dir).is_ok(),
            "无法创建 {}",
            dir.display()
        );
        dir
    }

    #[test]
    fn backup_then_restore_undoes_change() {
        let dir = temp_dir("undo");
        let target = dir.join("uosc.conf");
        assert!(fs::write(&target, "title=yes\n").is_ok());

        let manager = BackupManager::new(dir.join("backups"));
        let record = manager.backup_file("uosc.conf", &target);
        assert!(record.is_ok(), "备份失败：{:?}", record.as_ref().err());
        let Ok(record) = record else { return };
        assert_eq!(record.original_path, target);

        // 模拟写入新内容
        assert!(fs::write(&target, "title=no\n").is_ok());

        // 撤回
        assert!(manager.restore(&record).is_ok());
        let restored = fs::read_to_string(&target);
        assert!(restored.is_ok());
        let Ok(text) = restored else { return };
        assert_eq!(text, "title=yes\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_is_newest_first_and_restore_latest_works() {
        let dir = temp_dir("latest");
        let target = dir.join("mpv.conf");
        assert!(fs::write(&target, "v1\n").is_ok());
        let manager = BackupManager::new(dir.join("backups"));
        assert!(manager.backup_file("mpv.conf", &target).is_ok());
        assert!(fs::write(&target, "v2\n").is_ok());
        assert!(manager.backup_file("mpv.conf", &target).is_ok());
        assert!(fs::write(&target, "v3\n").is_ok());

        let records = manager.list("mpv.conf");
        assert_eq!(records.len(), 2);
        assert!(records[0].stamp >= records[1].stamp, "应按新→旧排序");

        let undone = manager.restore_latest("mpv.conf");
        assert!(undone.is_ok(), "撤回失败：{:?}", undone.as_ref().err());
        let text = fs::read_to_string(&target);
        assert!(text.is_ok());
        let Ok(text) = text else { return };
        assert_eq!(text, "v2\n"); // 撤回到最近一次备份时的内容
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_missing_file_is_rejected() {
        let dir = temp_dir("missing");
        let manager = BackupManager::new(dir.join("backups"));
        assert!(manager.backup_file("x", &dir.join("nope.conf")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_latest_without_backups_errors() {
        let dir = temp_dir("empty");
        let manager = BackupManager::new(dir.join("backups"));
        assert!(manager.restore_latest("nothing").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
