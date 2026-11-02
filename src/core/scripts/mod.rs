//! core::scripts —— MPV 脚本清单、启停开关、依赖扫描与 uosc 专项管理（FR-02 / FR-09）

pub mod uosc;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 脚本目录中的单个脚本条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptEntry {
    /// 展示名（去扩展名 / 目录名）
    pub name: String,
    /// 入口文件（`.lua` 或目录型脚本的 `main.lua`；禁用时路径带 `.disabled` 后缀）
    pub path: PathBuf,
    /// 是否存在配套的 script-opts/<name>.conf
    pub has_script_opt: bool,
    /// 是否启用（mpv 只会加载 `.lua`，禁用态统一为 `.lua.disabled`）
    pub enabled: bool,
}

/// 依赖问题：某脚本缺少其已知依赖
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyIssue {
    pub script: String,
    pub missing: Vec<String>,
}

/// 已知脚本依赖表（保守收录，避免误报；未收录脚本不做猜测）
const KNOWN_SCRIPT_DEPS: &[(&str, &[&str])] = &[
    // uosc 的缩略图预览依赖 thumbfast
    ("uosc", &["thumbfast"]),
];

/// 枚举播放器配置目录下的脚本清单（含启用与禁用）：
/// - `scripts/*.lua` / `scripts/*.lua.disabled` 单文件脚本
/// - `scripts/<dir>/main.lua` / `main.lua.disabled` 目录型脚本
pub fn list_scripts(config_dir: &Path) -> Vec<ScriptEntry> {
    let scripts_dir = config_dir.join("scripts");
    let Ok(entries) = std::fs::read_dir(&scripts_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // 单文件脚本
        if path.is_file() {
            let (display, enabled) =
                if let Some(stripped) = strip_suffix_ci(file_name, ".lua.disabled") {
                    (stripped, false)
                } else if let Some(stripped) = strip_suffix_ci(file_name, ".lua") {
                    (stripped, true)
                } else {
                    continue;
                };
            out.push(make_entry(config_dir, display, path, enabled));
            continue;
        }

        // 目录型脚本：入口必须是 main.lua（或禁用态 main.lua.disabled）
        if path.is_dir() {
            let main_enabled = path.join("main.lua");
            let main_disabled = path.join("main.lua.disabled");
            if main_enabled.is_file() {
                out.push(make_entry(
                    config_dir,
                    file_name.to_string(),
                    main_enabled,
                    true,
                ));
            } else if main_disabled.is_file() {
                out.push(make_entry(
                    config_dir,
                    file_name.to_string(),
                    main_disabled,
                    false,
                ));
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn make_entry(config_dir: &Path, name: String, path: PathBuf, enabled: bool) -> ScriptEntry {
    let has_script_opt = config_dir
        .join("script-opts")
        .join(format!("{name}.conf"))
        .is_file();
    ScriptEntry {
        name,
        path,
        has_script_opt,
        enabled,
    }
}

/// 大小写不敏感地剥离后缀，返回剩余部分（后缀为 ASCII，切片边界安全）
fn strip_suffix_ci(text: &str, suffix: &str) -> Option<String> {
    if text.len() < suffix.len() {
        return None;
    }
    let (head, tail) = text.split_at(text.len() - suffix.len());
    tail.eq_ignore_ascii_case(suffix).then(|| head.to_string())
}

/// 禁用脚本：入口文件追加 `.disabled`（mpv 只加载 `.lua`，重命名即停用）
pub fn disable_script(entry: &ScriptEntry) -> Result<PathBuf, String> {
    if !entry.enabled {
        return Err(format!("脚本 {} 已处于禁用状态", entry.name));
    }
    let mut raw = entry.path.clone().into_os_string();
    raw.push(".disabled");
    let target = PathBuf::from(raw);
    std::fs::rename(&entry.path, &target)
        .map_err(|e| format!("禁用 {} 失败（{}）：{e}", entry.name, entry.path.display()))?;
    Ok(target)
}

/// 启用脚本：移除入口文件的 `.disabled` 后缀
pub fn enable_script(entry: &ScriptEntry) -> Result<PathBuf, String> {
    if entry.enabled {
        return Err(format!("脚本 {} 已处于启用状态", entry.name));
    }
    let path_text = entry.path.to_string_lossy();
    let Some(stripped) = path_text.strip_suffix(".disabled") else {
        return Err(format!("{} 不是禁用态脚本路径", entry.path.display()));
    };
    let target = PathBuf::from(stripped);
    std::fs::rename(&entry.path, &target)
        .map_err(|e| format!("启用 {} 失败（{}）：{e}", entry.name, entry.path.display()))?;
    Ok(target)
}

/// 依赖扫描：已安装脚本缺少已知依赖时报告（仅针对已知依赖表，不做文本猜测）
pub fn scan_dependencies(config_dir: &Path) -> Vec<DependencyIssue> {
    let installed: HashSet<String> = list_scripts(config_dir)
        .iter()
        .map(|s| s.name.to_lowercase())
        .collect();
    let mut issues = Vec::new();
    for (script, deps) in KNOWN_SCRIPT_DEPS {
        if !installed.contains(&script.to_lowercase()) {
            continue;
        }
        let missing: Vec<String> = deps
            .iter()
            .filter(|dep| !installed.contains(&dep.to_lowercase()))
            .map(|dep| (*dep).to_string())
            .collect();
        if !missing.is_empty() {
            issues.push(DependencyIssue {
                script: (*script).to_string(),
                missing,
            });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建唯一的测试目录
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neve-script-{}-{tag}", std::process::id()));
        // 幂等清理上次运行残留；失败仅说明被外部占用，create_dir_all 会再暴露问题
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            std::fs::create_dir_all(&dir).is_ok(),
            "无法创建 {}",
            dir.display()
        );
        dir
    }

    #[test]
    fn lists_single_and_dir_scripts() {
        let dir = temp_dir("list");
        let scripts = dir.join("scripts");
        let opts = dir.join("script-opts");
        assert!(std::fs::create_dir_all(scripts.join("uosc")).is_ok());
        assert!(std::fs::create_dir_all(&opts).is_ok());
        assert!(std::fs::write(scripts.join("uosc").join("main.lua"), "--").is_ok());
        assert!(std::fs::write(scripts.join("thumbfast.lua"), "--").is_ok());
        assert!(std::fs::write(opts.join("uosc.conf"), "title=yes\n").is_ok());

        let list = list_scripts(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "thumbfast");
        assert!(!list[0].has_script_opt);
        assert!(list[0].enabled);
        assert_eq!(list[1].name, "uosc");
        assert!(list[1].has_script_opt);
        assert!(list[1].enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_scripts_dir_is_empty() {
        let dir = temp_dir("empty");
        assert!(list_scripts(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disable_and_enable_round_trip() {
        let dir = temp_dir("toggle");
        let scripts = dir.join("scripts");
        assert!(std::fs::create_dir_all(&scripts).is_ok());
        assert!(std::fs::write(scripts.join("thumbfast.lua"), "--").is_ok());

        let before = list_scripts(&dir);
        assert_eq!(before.len(), 1);
        let entry = &before[0];
        assert!(entry.enabled);

        // 禁用 → 文件改名，清单显示禁用
        assert!(disable_script(entry).is_ok(), "禁用失败");
        let disabled = list_scripts(&dir);
        assert_eq!(disabled.len(), 1);
        assert!(!disabled[0].enabled);
        assert_eq!(disabled[0].name, "thumbfast");

        // 重复禁用报错
        assert!(disable_script(&disabled[0]).is_err());

        // 启用 → 恢复
        assert!(enable_script(&disabled[0]).is_ok(), "启用失败");
        let after = list_scripts(&dir);
        assert!(after[0].enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_script_disable_renames_main_entry() {
        let dir = temp_dir("dir-toggle");
        let uosc_dir = dir.join("scripts").join("uosc");
        assert!(std::fs::create_dir_all(&uosc_dir).is_ok());
        assert!(std::fs::write(uosc_dir.join("main.lua"), "--").is_ok());

        let entry = list_scripts(&dir).remove(0);
        assert!(entry.enabled);
        assert!(disable_script(&entry).is_ok());
        // 目录仍在，入口被改名
        assert!(uosc_dir.is_dir());
        assert!(uosc_dir.join("main.lua.disabled").is_file());
        let toggled = list_scripts(&dir).remove(0);
        assert!(!toggled.enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dependency_scan_reports_missing_thumbfast() {
        let dir = temp_dir("deps");
        let uosc_dir = dir.join("scripts").join("uosc");
        assert!(std::fs::create_dir_all(&uosc_dir).is_ok());
        assert!(std::fs::write(uosc_dir.join("main.lua"), "--").is_ok());

        let issues = scan_dependencies(&dir);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].script, "uosc");
        assert_eq!(issues[0].missing, vec!["thumbfast".to_string()]);

        // 安装 thumbfast 后不再报告
        assert!(std::fs::write(dir.join("scripts").join("thumbfast.lua"), "--").is_ok());
        assert!(scan_dependencies(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dependency_scan_ignores_uninstalled_scripts() {
        let dir = temp_dir("deps-none");
        assert!(scan_dependencies(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
