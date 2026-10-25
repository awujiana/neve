//! core::players —— MPV 内核播放器检测与扫描
//!
//! 三路探测后合并去重：
//! 1. 常见安装路径扫描（Program Files / LocalAppData\Programs / 便携目录 / 同级目录）
//! 2. Windows 注册表卸载信息（Uninstall 键的 DisplayName / DisplayIcon）
//! 3. 环境变量 PATH 中的可执行文件
//!
//! 版本探测（`probe_version`）为惰性操作（需启动子进程），默认不在扫描时调用。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 播放器类型（MPV 内核系）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerKind {
    /// mpv 本体
    Mpv,
    /// mpv.net
    MpvNet,
    /// Celluloid（GNOME）
    Celluloid,
    /// IINA（macOS）
    Iina,
    /// mpv-Bingshan 整合包
    Bingshan,
    /// 其他 MPV 内核衍生
    UnknownMpvKernel(String),
}

/// 探测来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectSource {
    CommonPath,
    Registry,
    PathEnv,
}

/// 检测到的播放器
#[derive(Debug, Clone)]
pub struct DetectedPlayer {
    pub name: String,
    pub kind: PlayerKind,
    pub exe_path: PathBuf,
    pub version: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub source: DetectSource,
}

impl DetectedPlayer {
    /// 是否便携版（exe 旁存在 portable_config 目录）
    pub fn is_portable(&self) -> bool {
        self.exe_path
            .parent()
            .map(|dir| dir.join("portable_config").is_dir())
            .unwrap_or(false)
    }
}

/// 已知播放器可执行文件名（小写）
const KNOWN_EXES: &[(&str, PlayerKind, &str)] = &[
    ("mpv.exe", PlayerKind::Mpv, "mpv"),
    ("mpv.net.exe", PlayerKind::MpvNet, "mpv.net"),
    ("mpvnet.exe", PlayerKind::MpvNet, "mpv.net"),
    ("celluloid.exe", PlayerKind::Celluloid, "Celluloid"),
    ("iina.exe", PlayerKind::Iina, "IINA"),
    ("bingshan.exe", PlayerKind::Bingshan, "mpv-Bingshan"),
];

/// 目录名 → 整合包识别（同级目录中的 MPV 包）
const PACKAGE_DIR_HINTS: &[&str] = &["mpv-bingshan", "bingshan", "mpv包", "冰山"];

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 执行完整检测：三路探测 + 按规范化路径去重
pub fn detect_players() -> Vec<DetectedPlayer> {
    let mut found: Vec<DetectedPlayer> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

    let mut push = |p: DetectedPlayer, seen: &mut BTreeSet<PathBuf>, out: &mut Vec<DetectedPlayer>| {
        let key = p.exe_path.canonicalize().unwrap_or_else(|_| p.exe_path.clone());
        if seen.insert(key) {
            out.push(p);
        }
    };

    for p in scan_common_paths() {
        push(p, &mut seen, &mut found);
    }
    for p in scan_registry() {
        push(p, &mut seen, &mut found);
    }
    for p in scan_path_env() {
        push(p, &mut seen, &mut found);
    }

    // 补充配置目录
    for p in &mut found {
        if p.config_dir.is_none() {
            p.config_dir = find_config_dir(&p.exe_path);
        }
    }

    found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.exe_path.cmp(&b.exe_path)));
    found
}

// ---------------------------------------------------------------------------
// 1. 常见路径扫描
// ---------------------------------------------------------------------------

/// 候选根目录：系统安装位置 + 便携位置 + 当前工作区
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut push_root = |p: PathBuf| if p.is_dir() { roots.push(p) };

    if let Some(pf) = std::env::var_os("ProgramFiles") {
        push_root(PathBuf::from(pf));
    }
    if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
        push_root(PathBuf::from(pf86));
    }
    if let Some(lad) = std::env::var_os("LOCALAPPDATA") {
        push_root(PathBuf::from(&lad).join("Programs"));
        push_root(PathBuf::from(lad));
    }
    if let Some(ad) = std::env::var_os("APPDATA") {
        push_root(PathBuf::from(ad));
    }
    // 播放器可能就放在工作区附近（便携习惯）
    if let Ok(cwd) = std::env::current_dir() {
        push_root(cwd.clone());
        if let Some(parent) = cwd.parent() {
            push_root(parent.to_path_buf());
        }
    }
    roots
}

/// 在候选根目录（深度 ≤ 3）中查找已知播放器 exe
fn scan_common_paths() -> Vec<DetectedPlayer> {
    let mut out = Vec::new();
    for root in candidate_roots() {
        scan_dir_recursive(&root, 0, &mut out);
    }
    out
}

fn scan_dir_recursive(dir: &Path, depth: u8, out: &mut Vec<DetectedPlayer>) {
    if depth > 2 {
        return;
    }
    // 目录名命中整合包提示时，先看包内 mpv.exe
    if let Some(dirname) = dir.file_name().and_then(|n| n.to_str()) {
        let dl = dirname.to_lowercase();
        if PACKAGE_DIR_HINTS.iter().any(|h| dl.contains(h)) {
            if let Some(p) = try_known_exe(dir) {
                out.push(p);
            }
        }
    }
    if let Some(p) = try_known_exe(dir) {
        out.push(p);
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        // 跳过明显的系统/缓存目录
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    let n = n.to_lowercase();
                    n.starts_with('$')
                        || matches!(n.as_str(), "windows" | "system32" | "node_modules" | ".git")
                })
                .unwrap_or(true);
            if skip {
                continue;
            }
            scan_dir_recursive(&path, depth + 1, out);
        }
    }
}

/// 在 dir 下检查所有已知 exe 文件名
fn try_known_exe(dir: &Path) -> Option<DetectedPlayer> {
    for (exe, kind, name) in KNOWN_EXES {
        let exe_path = dir.join(exe);
        if exe_path.is_file() {
            return Some(DetectedPlayer {
                name: name.to_string(),
                kind: kind.clone(),
                exe_path,
                version: None,
                config_dir: None,
                source: DetectSource::CommonPath,
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 2. 注册表扫描
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn scan_registry() -> Vec<DetectedPlayer> {
    use winreg::enums::*;
    use winreg::RegKey;

    const UNINSTALL_KEYS: &[&str] = &[
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    const HIVES: &[isize] = &[HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER];

    let mut out = Vec::new();
    for hive in HIVES {
        for key_path in UNINSTALL_KEYS {
            let Ok(hive_key) = RegKey::predef(*hive).open_subkey_with_flags(key_path, KEY_READ) else {
                continue;
            };
            for sub in hive_key.enum_keys().flatten() {
                let Ok(item) = hive_key.open_subkey_with_flags(&sub, KEY_READ) else { continue };
                let display_name: Option<String> = item.get_value("DisplayName").ok();
                let Some(display_name) = display_name else { continue };
                let dl = display_name.to_lowercase();
                if !dl.contains("mpv") && !dl.contains("celluloid") && !dl.contains("iina") && !dl.contains("bingshan") {
                    continue;
                }
                let Some(exe_path) = display_icon_to_exe(&item) else { continue };
                if !exe_path.is_file() {
                    continue;
                }
                let kind = classify(&exe_path, &display_name);
                out.push(DetectedPlayer {
                    name: display_name,
                    kind,
                    exe_path,
                    version: item.get_value::<String, _>("DisplayVersion").ok(),
                    config_dir: None,
                    source: DetectSource::Registry,
                });
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn scan_registry() -> Vec<DetectedPlayer> {
    Vec::new()
}

/// DisplayIcon 常为 `"C:\path\mpv.exe",0` 或裸路径 —— 提取 exe 路径
#[cfg(windows)]
fn display_icon_to_exe(item: &winreg::RegKey) -> Option<PathBuf> {
    let raw: String = item.get_value("DisplayIcon").ok()?;
    let raw = raw.split(',').next()?.trim().trim_matches('"').to_string();
    let p = PathBuf::from(&raw);
    let is_exe = |p: &Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("exe"))
            .unwrap_or(false)
    };
    if is_exe(&p) {
        Some(p)
    } else {
        let with_exe = p.with_extension("exe");
        with_exe.is_file().then_some(with_exe)
    }
}

// ---------------------------------------------------------------------------
// 3. PATH 环境变量扫描
// ---------------------------------------------------------------------------

fn scan_path_env() -> Vec<DetectedPlayer> {
    let Some(path_var) = std::env::var_os("PATH") else { return Vec::new() };
    let mut out = Vec::new();
    for dir in std::env::split_paths(&path_var) {
        if let Some(p) = try_known_exe(&dir) {
            if p.source == DetectSource::CommonPath {
                out.push(DetectedPlayer { source: DetectSource::PathEnv, ..p });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 分类 / 配置目录 / 版本
// ---------------------------------------------------------------------------

fn classify(exe_path: &Path, display_name: &str) -> PlayerKind {
    let file = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    for (exe, kind, _) in KNOWN_EXES {
        if file == *exe {
            return kind.clone();
        }
    }
    PlayerKind::UnknownMpvKernel(display_name.to_string())
}

/// 查找配置目录：便携 portable_config → %APPDATA%\mpv(.net) → exe 旁 config
fn find_config_dir(exe_path: &Path) -> Option<PathBuf> {
    let exe_dir = exe_path.parent()?;
    let candidates = [
        exe_dir.join("portable_config"),
        exe_dir.join("config"),
    ];
    for c in candidates {
        if c.is_dir() {
            return Some(c);
        }
    }
    if let Some(ad) = std::env::var_os("APPDATA") {
        let ad = PathBuf::from(ad);
        for c in [ad.join("mpv"), ad.join("mpv.net")] {
            if c.is_dir() {
                return Some(c);
            }
        }
    }
    None
}

/// 惰性版本探测：运行 `exe --version` 并解析首行（阻塞操作，勿在 UI 线程批量调用）
pub fn probe_version(exe_path: &Path) -> Option<String> {
    let output = Command::new(exe_path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&output.stdout).lines().next()?.trim();
    (!first.is_empty()).then(|| first.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_filename() {
        assert_eq!(classify(Path::new("C:/x/mpv.exe"), "x"), PlayerKind::Mpv);
        assert_eq!(classify(Path::new("C:/x/other.exe"), "Some Pack"), PlayerKind::UnknownMpvKernel("Some Pack".into()));
    }

    #[test]
    fn detect_players_runs() {
        // 冒烟测试：只要求不 panic
        let _ = detect_players();
    }
}
