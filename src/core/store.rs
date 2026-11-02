//! core::store —— 应用数据目录定位（FR-05 最小实现）
//!
//! 便携模式：exe 同目录存在 `portable_data/` → 数据随目录整体移动
//! 安装模式：`%APPDATA%/neve/`
//! 注意：安装版"自定义数据位置"（FR-05 完整版）在此模块后续版本实现。

use std::path::PathBuf;

/// 返回 Neve 数据目录（便携优先），目录可能尚不存在（调用方按需创建）
pub fn data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let portable = exe_dir.join("portable_data");
        if portable.is_dir() {
            return portable;
        }
    }
    std::env::var_os("APPDATA")
        .map(|ad| PathBuf::from(ad).join("neve"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_returns_usable_path() {
        // 冒烟：非空且为绝对路径
        let dir = data_dir();
        assert!(dir.is_absolute(), "数据目录应为绝对路径：{}", dir.display());
    }
}
