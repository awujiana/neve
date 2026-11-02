//! uosc 专项支持（FR-09）：状态检测、Git 源安装/更新、uosc.conf 可视化目录。
//!
//! 安装策略（符合质量门禁 §6"先验证输入和目标，再执行写入"）：
//! - 目标已存在时**拒绝覆盖**并提示走更新流程（不触发备份需求）
//! - 默认 uosc.conf 仅在用户未自定义时安装，绝不覆盖用户配置
//! - 更新走 `git pull --ff-only`，仅动脚本本体，用户 conf 不受影响

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::backup::{BackupManager, BackupRecord};
use super::super::config::conf::ConfDoc;

/// uosc 官方仓库
#[allow(dead_code)] // TODO(M2-UI): 安装按钮接入后移除
pub const UOSC_REPO_URL: &str = "https://github.com/tomasklaen/uosc.git";

/// uosc 安装状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UoscStatus {
    /// `scripts/uosc/main.lua` 是否存在
    pub installed: bool,
    pub install_dir: PathBuf,
    /// Git 安装时的短提交号（非 Git 安装为 None）
    pub commit: Option<String>,
    /// 用户侧 `script-opts/uosc.conf` 是否存在
    pub conf_exists: bool,
}

/// uosc.conf 可视化目录项（常用子集，可按 uosc 版本扩充）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UoscOptionSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub category: &'static str,
}

/// 目录项当前取值
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UoscOptionValue {
    pub spec: UoscOptionSpec,
    /// 当前值；None 表示 conf 中未设置（走 uosc 内置默认）
    pub current: Option<String>,
}

/// 常用可视化目录。说明：uosc 选项随版本演进，此处仅收录长期稳定的核心项；
/// 未收录的键仍以通用条目形式存在于 uosc.conf 中，写入端用 `ConfDoc::set_key` 原位修改。
pub const UOSC_CATALOG: &[UoscOptionSpec] = &[
    UoscOptionSpec {
        key: "title",
        label: "标题栏（uosc 自绘标题）",
        category: "显示元素",
    },
    UoscOptionSpec {
        key: "subtitle",
        label: "字幕栏（uosc 自绘字幕）",
        category: "显示元素",
    },
    UoscOptionSpec {
        key: "timeline_size_min",
        label: "时间轴最小高度",
        category: "时间轴",
    },
    UoscOptionSpec {
        key: "timeline_size_max",
        label: "时间轴悬停展开高度",
        category: "时间轴",
    },
    UoscOptionSpec {
        key: "timeline_persistency",
        label: "时间轴持久显示配置",
        category: "时间轴",
    },
];

/// 运行 git 命令并返回 stdout（失败时携带可定位的上下文）
fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command
        .output()
        .map_err(|e| format!("无法启动 git：{e}（请确认已安装 Git 并加入 PATH）"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} 失败：{}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 检测 uosc 安装状态
pub fn uosc_status(config_dir: &Path) -> UoscStatus {
    let install_dir = config_dir.join("scripts").join("uosc");
    let installed = install_dir.join("main.lua").is_file();
    let commit = if installed && install_dir.join(".git").exists() {
        run_git(&["rev-parse", "--short", "HEAD"], Some(&install_dir))
            .ok()
            .map(|stdout| stdout.trim().to_string())
    } else {
        None
    };
    let conf_exists = config_dir.join("script-opts").join("uosc.conf").is_file();
    UoscStatus {
        installed,
        install_dir,
        commit,
        conf_exists,
    }
}

/// 从 Git 源安装 uosc。已安装时返回错误（提示走更新），绝不覆盖。
#[allow(dead_code)] // TODO(M2-UI): 安装按钮接入后移除
pub fn install_uosc(config_dir: &Path) -> Result<String, String> {
    if !config_dir.is_dir() {
        return Err(format!("配置目录不存在：{}", config_dir.display()));
    }
    let scripts_dir = config_dir.join("scripts");
    fs::create_dir_all(&scripts_dir)
        .map_err(|e| format!("无法创建脚本目录 {}：{e}", scripts_dir.display()))?;
    let target = scripts_dir.join("uosc");
    if target.exists() {
        return Err(format!(
            "uosc 已安装（{}）；如需更新请使用更新功能",
            target.display()
        ));
    }

    // 唯一临时目录：时间戳秒级已足够（安装操作由用户手动触发，天然串行）
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("neve-uosc-install-{stamp}"));
    // 幂等清理可能残留的同名临时目录；失败仅影响本次克隆目标为非空，克隆步骤会报错暴露
    let _ = fs::remove_dir_all(&tmp);

    let install_result = install_uosc_inner(config_dir, &target, &tmp);
    // 临时目录清理：非关键路径，失败不影响安装结果（残留于系统临时目录，可再清理）
    let _ = fs::remove_dir_all(&tmp);
    install_result
}

#[allow(dead_code)] // TODO(M2-UI): 随 install_uosc 一并接入
fn install_uosc_inner(config_dir: &Path, target: &Path, tmp: &Path) -> Result<String, String> {
    run_git(
        &[
            "clone",
            "--depth",
            "1",
            UOSC_REPO_URL,
            &tmp.to_string_lossy(),
        ],
        None,
    )?;

    // 仓库结构验证：uosc/main.lua 必须存在，防止上游布局变更导致空装
    let src_script = tmp.join("uosc");
    if !src_script.join("main.lua").is_file() {
        return Err("仓库结构不符合预期（缺少 uosc/main.lua），已中止安装".to_string());
    }
    copy_dir_recursive(&src_script, target)
        .map_err(|e| format!("复制 uosc 脚本到 {} 失败：{e}", target.display()))?;

    // 默认 conf 仅在用户未自定义时安装
    let src_conf = tmp.join("script-opts").join("uosc.conf");
    if src_conf.is_file() {
        let opts_dir = config_dir.join("script-opts");
        fs::create_dir_all(&opts_dir).map_err(|e| format!("无法创建 script-opts 目录：{e}"))?;
        let dst_conf = opts_dir.join("uosc.conf");
        if !dst_conf.exists() {
            fs::copy(&src_conf, &dst_conf).map_err(|e| format!("复制默认 uosc.conf 失败：{e}"))?;
        }
    }

    run_git(&["rev-parse", "--short", "HEAD"], Some(target))
        .map(|stdout| format!("安装完成，提交 {}", stdout.trim()))
}

/// 更新已安装的 uosc（仅支持 Git 安装方式；用户 uosc.conf 不会被改动）
#[allow(dead_code)] // TODO(M2-UI): 更新按钮接入后移除
pub fn update_uosc(config_dir: &Path) -> Result<String, String> {
    let target = config_dir.join("scripts").join("uosc");
    if !target.join("main.lua").is_file() {
        return Err("uosc 未安装，请先安装".to_string());
    }
    if !target.join(".git").exists() {
        return Err("uosc 未通过 Git 安装，无法增量更新；请卸载后重新安装".to_string());
    }
    run_git(&["pull", "--ff-only"], Some(&target))?;
    run_git(&["rev-parse", "--short", "HEAD"], Some(&target))
        .map(|stdout| format!("更新完成，提交 {}", stdout.trim()))
}

/// 递归复制目录
#[allow(dead_code)] // TODO(M2-UI): 随 install_uosc 一并接入
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let entry_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if entry_type.is_dir() {
            copy_dir_recursive(&entry_path, &target)?;
        } else {
            fs::copy(&entry_path, &target)?;
        }
    }
    Ok(())
}

/// 读取 uosc.conf 中可视化目录各项的当前值。
/// 语义：conf 中被注释/未写的项视为走 uosc 内置默认（current = None）。
pub fn read_uosc_values(config_dir: &Path) -> Result<Vec<UoscOptionValue>, String> {
    let conf_path = config_dir.join("script-opts").join("uosc.conf");
    let text = fs::read_to_string(&conf_path)
        .map_err(|e| format!("读取 {} 失败：{e}", conf_path.display()))?;
    let doc = ConfDoc::parse(&text);
    Ok(UOSC_CATALOG
        .iter()
        .map(|spec| UoscOptionValue {
            spec: *spec,
            current: doc.get(spec.key),
        })
        .collect())
}

/// 原位修改 uosc.conf 目录项（使用默认备份根，写入前自动备份，可撤回）
#[allow(dead_code)] // TODO(M2-UI): 可视化编辑控件接入后移除
pub fn write_uosc_value(
    config_dir: &Path,
    key: &str,
    value: &str,
) -> Result<Option<BackupRecord>, String> {
    let manager = BackupManager::with_default_root();
    write_uosc_value_with(&manager, config_dir, key, value)
}

/// 原位修改 uosc.conf 目录项（可注入备份管理器，便于测试与自定义数据目录）。
/// 返回值：Some(备份记录) 表示覆盖了已有 conf，可用 `BackupManager::restore` 撤回；
/// None 表示首次创建 conf，无内容可撤回。
#[allow(dead_code)] // TODO(M2-UI): 可视化编辑控件接入后移除
pub fn write_uosc_value_with(
    manager: &BackupManager,
    config_dir: &Path,
    key: &str,
    value: &str,
) -> Result<Option<BackupRecord>, String> {
    if !UOSC_CATALOG.iter().any(|spec| spec.key == key) {
        return Err(format!("键 {key} 不在 uosc 可视化目录中，已拒绝写入"));
    }
    let opts_dir = config_dir.join("script-opts");
    fs::create_dir_all(&opts_dir).map_err(|e| format!("无法创建 script-opts 目录：{e}"))?;
    let conf_path = opts_dir.join("uosc.conf");
    let (mut doc, backup) = match fs::read_to_string(&conf_path) {
        Ok(text) => {
            // 覆盖已有文件：先备份再写，失败即中止（保证"任何写入前都有备份"）
            let record = manager.backup_file("uosc.conf", &conf_path)?;
            (ConfDoc::parse(&text), Some(record))
        }
        Err(_) => (ConfDoc::default(), None),
    };
    doc.set_key(key, value);
    fs::write(&conf_path, doc.render())
        .map_err(|e| format!("写入 {} 失败：{e}", conf_path.display()))?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建唯一的测试目录
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-uosc-{}-{tag}", std::process::id()));
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
    fn status_reports_not_installed() {
        let dir = temp_dir("status-missing");
        let status = uosc_status(&dir);
        assert!(!status.installed);
        assert_eq!(status.commit, None);
        assert!(!status.conf_exists);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_detects_non_git_install() {
        let dir = temp_dir("status-nongit");
        let uosc_dir = dir.join("scripts").join("uosc");
        assert!(fs::create_dir_all(&uosc_dir).is_ok(), "无法创建 uosc 目录");
        assert!(fs::write(uosc_dir.join("main.lua"), "--").is_ok());
        let status = uosc_status(&dir);
        assert!(status.installed);
        assert_eq!(status.commit, None); // 无 .git → 非 Git 安装
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_rejects_existing_target() {
        let dir = temp_dir("install-exists");
        let uosc_dir = dir.join("scripts").join("uosc");
        assert!(fs::create_dir_all(&uosc_dir).is_ok(), "无法创建 uosc 目录");
        assert!(fs::write(uosc_dir.join("main.lua"), "--").is_ok());
        // 已安装短路返回错误，不应触碰网络
        assert!(install_uosc(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_requires_install() {
        let dir = temp_dir("update-missing");
        assert!(update_uosc(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_and_write_catalog_values() {
        let dir = temp_dir("catalog");
        let opts = dir.join("script-opts");
        assert!(fs::create_dir_all(&opts).is_ok(), "无法创建 script-opts");
        assert!(fs::write(opts.join("uosc.conf"), "#timeline_size_min=40\ntitle=yes\n").is_ok());

        let first = read_uosc_values(&dir);
        assert!(
            first.is_ok(),
            "读取可视化目录失败：{:?}",
            first.as_ref().err()
        );
        let Ok(values) = first else { return };
        let title = values.iter().find(|v| v.spec.key == "title");
        assert_eq!(
            title.and_then(|v| v.current.clone()),
            Some("yes".to_string())
        );
        // 注释掉的默认值 → 走 uosc 内置默认
        let timeline = values.iter().find(|v| v.spec.key == "timeline_size_min");
        assert_eq!(timeline.and_then(|v| v.current.clone()), None);

        let manager = BackupManager::new(dir.join("backups"));
        assert!(
            write_uosc_value_with(&manager, &dir, "timeline_size_min", "48").is_ok(),
            "写入失败"
        );

        let second = read_uosc_values(&dir);
        assert!(second.is_ok(), "二次读取失败：{:?}", second.as_ref().err());
        let Ok(values) = second else { return };
        let timeline = values.iter().find(|v| v.spec.key == "timeline_size_min");
        assert_eq!(
            timeline.and_then(|v| v.current.clone()),
            Some("48".to_string())
        );

        // 原有注释行与 title 行不被破坏
        let conf = fs::read_to_string(opts.join("uosc.conf"));
        assert!(
            conf.is_ok(),
            "回读 uosc.conf 失败：{:?}",
            conf.as_ref().err()
        );
        let Ok(conf) = conf else { return };
        assert!(conf.contains("timeline_size_min=48"));
        assert!(conf.contains("title=yes"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_rejects_unknown_key() {
        let dir = temp_dir("write-guard");
        let manager = BackupManager::new(dir.join("backups"));
        assert!(write_uosc_value_with(&manager, &dir, "made_up_key", "1").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_creates_backup_and_can_be_undone() {
        let dir = temp_dir("write-undo");
        let opts = dir.join("script-opts");
        assert!(fs::create_dir_all(&opts).is_ok(), "无法创建 script-opts");
        assert!(fs::write(opts.join("uosc.conf"), "title=yes\n").is_ok());
        let manager = BackupManager::new(dir.join("backups"));

        let first = write_uosc_value_with(&manager, &dir, "title", "no");
        assert!(first.is_ok(), "写入失败：{:?}", first.as_ref().err());
        let Ok(backup) = first else { return };
        assert!(backup.is_some(), "覆盖已有 conf 应返回备份记录");

        // 撤回：内容回到写入前
        let undone = manager.restore_latest("uosc.conf");
        assert!(undone.is_ok(), "撤回失败：{:?}", undone.as_ref().err());
        let conf = fs::read_to_string(opts.join("uosc.conf"));
        assert!(conf.is_ok(), "回读失败");
        let Ok(conf) = conf else { return };
        assert_eq!(conf, "title=yes\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
