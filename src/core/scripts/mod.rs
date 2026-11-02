//! core::scripts —— MPV 脚本清单与 uosc 专项管理（FR-02 / FR-09）

pub mod uosc;

use std::path::{Path, PathBuf};

/// 脚本目录中的单个脚本条目（顶层 .lua 文件或含 main.lua 的脚本目录）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptEntry {
    /// 展示名（去扩展名 / 目录名）
    pub name: String,
    /// 脚本入口路径（.lua 文件或目录内 main.lua）
    pub path: PathBuf,
    /// 是否存在配套的 script-opts/<name>.conf
    pub has_script_opt: bool,
}

/// 枚举播放器配置目录下的脚本清单：
/// - `scripts/*.lua` 单文件脚本
/// - `scripts/<dir>/main.lua` 目录型脚本（如 uosc）
pub fn list_scripts(config_dir: &Path) -> Vec<ScriptEntry> {
    let scripts_dir = config_dir.join("scripts");
    let Ok(entries) = std::fs::read_dir(&scripts_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        let is_single_lua = path.is_file() && name.to_lowercase().ends_with(".lua");
        let is_dir_script = path.is_dir() && path.join("main.lua").is_file();
        if !is_single_lua && !is_dir_script {
            continue;
        }
        let display_name = if is_single_lua {
            // is_single_lua 已保证 name 以任意大小写的 ".lua" 结尾，剥离 4 字节后缀
            name[..name.len() - 4].to_string()
        } else {
            name.clone()
        };
        let opt_name = if is_single_lua {
            display_name.clone()
        } else {
            name
        };
        let has_script_opt = config_dir
            .join("script-opts")
            .join(format!("{opt_name}.conf"))
            .is_file();
        out.push(ScriptEntry {
            name: display_name,
            path,
            has_script_opt,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
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
        assert_eq!(list[1].name, "uosc");
        assert!(list[1].has_script_opt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_scripts_dir_is_empty() {
        let dir = temp_dir("empty");
        assert!(list_scripts(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
