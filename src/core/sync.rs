//! core::sync —— 跨整合包脚本与配置同步（FR-03）
//!
//! 流程：`plan_script_sync`（收集源文件 → 与目标比对 → 差异计划）
//! → `apply_sync`（按冲突策略执行；覆盖前强制备份，可撤回）。

use std::fs;
use std::path::{Path, PathBuf};

use super::backup::BackupManager;

/// 冲突处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// 跳过目标已有且不同的文件
    Skip,
    /// 覆盖（覆盖前备份目标文件，可撤回）
    Overwrite,
    /// 保留双份：目标文件保留，源文件以 `.from-sync` 后缀落盘
    KeepBoth,
}

/// 单个文件的同步类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    /// 目标不存在
    New,
    /// 目标已存在且内容一致
    Identical,
    /// 目标已存在但内容不同
    Conflict,
}

/// 同步计划中的一条文件记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEntry {
    /// 相对配置目录的路径（如 `scripts/uosc/main.lua`）
    pub relative: PathBuf,
    pub kind: SyncKind,
}

/// 同步计划
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub script_name: String,
    pub source_config: PathBuf,
    pub target_config: PathBuf,
    pub entries: Vec<SyncEntry>,
}

impl SyncPlan {
    pub fn count(&self, kind: SyncKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }
}

/// 执行结果统计
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// 实际写入（新增 + 覆盖）的文件数
    pub written: usize,
    /// 跳过的文件数
    pub skipped: usize,
    /// 以双份方式保留的文件数
    pub kept_both: usize,
    /// 覆盖前产生的备份副本路径（可用于撤回）
    pub backups: Vec<PathBuf>,
}

/// 生成同步计划：脚本本体（单文件或目录）+ 其 script-opts conf
pub fn plan_script_sync(
    source_config: &Path,
    target_config: &Path,
    script_name: &str,
) -> Result<SyncPlan, String> {
    if !source_config.is_dir() {
        return Err(format!("源配置目录不存在：{}", source_config.display()));
    }
    if !target_config.is_dir() {
        return Err(format!("目标配置目录不存在：{}", target_config.display()));
    }
    let files = collect_script_files(source_config, script_name)?;
    if files.is_empty() {
        return Err(format!("源配置中未找到脚本 {script_name} 的文件"));
    }

    let mut entries = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(source_config)
            .map_err(|e| format!("计算相对路径失败：{e}"))?
            .to_path_buf();
        let target_path = target_config.join(&relative);
        let kind = if !target_path.exists() {
            SyncKind::New
        } else if same_content(&file, &target_path) {
            SyncKind::Identical
        } else {
            SyncKind::Conflict
        };
        entries.push(SyncEntry { relative, kind });
    }

    Ok(SyncPlan {
        script_name: script_name.to_string(),
        source_config: source_config.to_path_buf(),
        target_config: target_config.to_path_buf(),
        entries,
    })
}

/// 按策略执行同步（覆盖前强制备份）
pub fn apply_sync(
    plan: &SyncPlan,
    policy: ConflictPolicy,
    backups: &BackupManager,
) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();
    for entry in &plan.entries {
        let source_path = plan.source_config.join(&entry.relative);
        let target_path = plan.target_config.join(&entry.relative);
        match entry.kind {
            SyncKind::Identical => report.skipped += 1,
            SyncKind::New => {
                copy_with_parent(&source_path, &target_path)?;
                report.written += 1;
            }
            SyncKind::Conflict => match policy {
                ConflictPolicy::Skip => report.skipped += 1,
                ConflictPolicy::Overwrite => {
                    // 覆盖前备份目标文件（备份失败即中止，不产生半写状态）
                    let record = backups.backup_file(&plan.script_name, &target_path)?;
                    report.backups.push(record.backup_path);
                    copy_with_parent(&source_path, &target_path)?;
                    report.written += 1;
                }
                ConflictPolicy::KeepBoth => {
                    let keep_path = with_suffix(&target_path, ".from-sync");
                    copy_with_parent(&source_path, &keep_path)?;
                    report.kept_both += 1;
                }
            },
        }
    }
    Ok(report)
}

/// 收集某脚本在配置目录下的全部文件（绝对路径）
fn collect_script_files(config_dir: &Path, script_name: &str) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let scripts_dir = config_dir.join("scripts");

    // 单文件脚本（启用 / 禁用态）
    for candidate in [
        scripts_dir.join(format!("{script_name}.lua")),
        scripts_dir.join(format!("{script_name}.lua.disabled")),
    ] {
        if candidate.is_file() {
            out.push(candidate);
        }
    }

    // 目录型脚本（递归收集；跳过 .git 等版本库目录）
    let dir_script = scripts_dir.join(script_name);
    if dir_script.is_dir() {
        collect_dir(&dir_script, &mut out)?;
    }

    // 脚本 conf
    let conf = config_dir
        .join("script-opts")
        .join(format!("{script_name}.conf"));
    if conf.is_file() {
        out.push(conf);
    }

    Ok(out)
}

fn collect_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("读取目录 {} 失败：{e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            collect_dir(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn same_content(a: &Path, b: &Path) -> bool {
    match (fs::read(a), fs::read(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn copy_with_parent(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}：{e}", parent.display()))?;
    }
    fs::copy(source, target).map(|_bytes| ()).map_err(|e| {
        format!(
            "复制 {} -> {} 失败：{e}",
            source.display(),
            target.display()
        )
    })
}

/// 在文件名后追加后缀（保留原扩展名语义：main.lua -> main.lua.from-sync）
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-sync-{}-{tag}", std::process::id()));
        // 幂等清理上次运行残留；失败仅说明被外部占用，create_dir_all 会再暴露问题
        let _ = fs::remove_dir_all(&dir);
        assert!(
            fs::create_dir_all(&dir).is_ok(),
            "无法创建 {}",
            dir.display()
        );
        dir
    }

    /// 造一个含 uosc 目录脚本 + conf 的配置目录
    fn make_source(root: &Path) -> PathBuf {
        let config = root.join("source");
        let uosc = config.join("scripts").join("uosc");
        assert!(fs::create_dir_all(uosc.join("lib")).is_ok());
        assert!(fs::write(uosc.join("main.lua"), "-- main v2").is_ok());
        assert!(fs::write(uosc.join("lib").join("util.lua"), "-- util").is_ok());
        let opts = config.join("script-opts");
        assert!(fs::create_dir_all(&opts).is_ok());
        assert!(fs::write(opts.join("uosc.conf"), "title=yes\n").is_ok());
        config
    }

    #[test]
    fn plan_marks_new_conflict_identical() {
        let root = temp_root("plan");
        let source = make_source(&root);
        let target = root.join("target");
        // 目标：conf 内容不同（冲突）、main.lua 缺失（新增）
        assert!(fs::create_dir_all(target.join("script-opts")).is_ok());
        assert!(fs::write(target.join("script-opts").join("uosc.conf"), "title=no\n").is_ok());
        // util.lua 内容相同（一致）
        let target_lib = target.join("scripts").join("uosc").join("lib");
        assert!(fs::create_dir_all(&target_lib).is_ok());
        assert!(fs::write(target_lib.join("util.lua"), "-- util").is_ok());

        let plan = plan_script_sync(&source, &target, "uosc");
        assert!(plan.is_ok(), "生成计划失败：{:?}", plan.as_ref().err());
        let Ok(plan) = plan else { return };
        assert_eq!(plan.count(SyncKind::New), 1);
        assert_eq!(plan.count(SyncKind::Conflict), 1);
        assert_eq!(plan.count(SyncKind::Identical), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_overwrite_backs_up_and_can_be_undone() {
        let root = temp_root("overwrite");
        let source = make_source(&root);
        let target = root.join("target");
        assert!(fs::create_dir_all(target.join("script-opts")).is_ok());
        assert!(fs::write(target.join("script-opts").join("uosc.conf"), "title=no\n").is_ok());

        let plan = plan_script_sync(&source, &target, "uosc");
        assert!(plan.is_ok());
        let Ok(plan) = plan else { return };

        let backups = BackupManager::new(root.join("backups"));
        let report = apply_sync(&plan, ConflictPolicy::Overwrite, &backups);
        assert!(report.is_ok(), "同步失败：{:?}", report.as_ref().err());
        let Ok(report) = report else { return };

        assert_eq!(report.written, 3, "main.lua + util.lua + uosc.conf");
        assert_eq!(report.backups.len(), 1, "仅覆盖 conf 需要备份");

        let conf = fs::read_to_string(target.join("script-opts").join("uosc.conf"));
        assert!(conf.is_ok());
        let Ok(conf) = conf else { return };
        assert_eq!(conf, "title=yes\n");

        // 撤回：目标 conf 回到覆盖前
        let undone = backups.restore_latest("uosc");
        assert!(undone.is_ok(), "撤回失败：{:?}", undone.as_ref().err());
        let restored = fs::read_to_string(target.join("script-opts").join("uosc.conf"));
        assert!(restored.is_ok());
        let Ok(restored) = restored else { return };
        assert_eq!(restored, "title=no\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_skip_leaves_target_untouched() {
        let root = temp_root("skip");
        let source = make_source(&root);
        let target = root.join("target");
        assert!(fs::create_dir_all(target.join("script-opts")).is_ok());
        assert!(fs::write(target.join("script-opts").join("uosc.conf"), "title=no\n").is_ok());

        let plan = plan_script_sync(&source, &target, "uosc");
        assert!(plan.is_ok());
        let Ok(plan) = plan else { return };
        let backups = BackupManager::new(root.join("backups"));
        let report = apply_sync(&plan, ConflictPolicy::Skip, &backups);
        assert!(report.is_ok());
        let Ok(report) = report else { return };
        assert_eq!(report.skipped, 1);
        assert_eq!(report.written, 2);

        let conf = fs::read_to_string(target.join("script-opts").join("uosc.conf"));
        assert!(conf.is_ok());
        let Ok(conf) = conf else { return };
        assert_eq!(conf, "title=no\n", "跳过策略不得改动目标文件");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_keep_both_writes_suffixed_copy() {
        let root = temp_root("keepboth");
        let source = make_source(&root);
        let target = root.join("target");
        assert!(fs::create_dir_all(target.join("script-opts")).is_ok());
        assert!(fs::write(target.join("script-opts").join("uosc.conf"), "title=no\n").is_ok());

        let plan = plan_script_sync(&source, &target, "uosc");
        assert!(plan.is_ok());
        let Ok(plan) = plan else { return };
        let backups = BackupManager::new(root.join("backups"));
        let report = apply_sync(&plan, ConflictPolicy::KeepBoth, &backups);
        assert!(report.is_ok());
        let Ok(report) = report else { return };
        assert_eq!(report.kept_both, 1);

        assert!(target.join("script-opts").join("uosc.conf").is_file());
        assert!(
            target
                .join("script-opts")
                .join("uosc.conf.from-sync")
                .is_file()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_script_is_rejected() {
        let root = temp_root("missing");
        let source = make_source(&root);
        let target = root.join("target");
        assert!(fs::create_dir_all(&target).is_ok());
        assert!(plan_script_sync(&source, &target, "no-such-script").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
