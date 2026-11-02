# AGENTS.md

> 本文件供 **AI 编码 / 写作 agent**（如 WorkBuddy、Copilot、Claude）阅读，用于了解本仓库的结构、约定与禁忌，从而在自动改动时保持风格一致。
> 人类贡献者请阅读 [`CONTRIBUTING.md`](CONTRIBUTING.md)（含完整命名规范）。

## 项目概述

- **名称**：Névé 粒雪
- **GitHub 仓库名（英文名）**：neve
- **类型**：代码仓库（AGPL-3.0）
- **维护者**：awujian
- **一句话**：一个用纯 Rust + GPUI 写的桌面管理器，统一管理所有 MPV 内核播放器的检测、脚本安装、配置编辑与跨包同步，所有写操作可预览、可备份、可撤回。

## 仓库结构

| 路径 | 用途 | 是否发布 |
| --- | --- | --- |
| `src/main.rs` | 入口：检测播放器 → 构建视图模型 → 初始化组件库 → 投影设计令牌 → 开窗 | ✅ |
| `src/core/` | 纯逻辑层，不依赖 GPUI，可单测 | ✅ |
| `src/core/players.rs` | 三路播放器检测（常见路径 / 注册表 / PATH） | ✅ |
| `src/core/config/` | 无损 conf 解析、input 绑定、冲突检测、扫描报告 | ✅ |
| `src/core/scripts/` | 脚本枚举、启停、依赖扫描；`uosc.rs` 为 uosc 专项 | ✅ |
| `src/core/sync.rs` | 跨包同步的计划与执行（FR-03） | ✅ |
| `src/core/backup.rs` | 写前备份与撤回，**所有写操作的必经之路** | ✅ |
| `src/core/store.rs` | 数据目录定位（便携版优先） | ✅ |
| `src/ui/mod.rs` | 页面骨架、侧栏、概览页、播放器页与交互动作 | ✅ |
| `src/ui/theme.rs` | 设计令牌 → `ThemeColor` 语义槽位的投影 | ✅ |
| `docs/` | 设计文档、架构文档、UI 规范、质量门禁、`screenshots/` | ✅ |
| `config/ui-theme.toml` | 主题令牌的设计基线（运行时取值固化在 `ui/theme.rs`） | ✅ |
| `scripts/` | `update_badges.py`（徽章生成）、`badges.config.json`（唯一数据源） | ✅ |
| `CONTRIBUTING.md` / `AGENTS.md` / `CHANGELOG.md` / `LICENSE` | 协作与协议文件 | ✅ |
| `target/` | 构建产物 | ❌ gitignore |

## 必须遵守的约定

1. **命名规范（不缩写）**：项目 `kebab-case`；Rust 模块 `snake_case`、类型 `PascalCase`、函数 `snake_case` 动词开头，全部全拼；名称不重复；文件名即模块名。允许的缩写仅 `url`/`id`/`ui`/`ux`/`http`/`json`/`toml`/`exe`/`lua`/`mpv`/`uosc`/`osc`/`conf`。详见 `CONTRIBUTING.md`。
2. **项目核心不变量**：
   - **写前必备份**：任何对播放器目录或注册表的写入，必须先经 `BackupManager::backup_file`，再由 `restore_latest` 提供撤回入口。绕过 `core::backup` 的写操作一律视为缺陷。
   - **无损配置解析**：`core::config::conf` 必须保留行序、注释、空行与 CRLF，`render()` 必须能字节级往返。禁止用「读成 HashMap 再写回」的实现替换它。
   - **last-wins 语义**：mpv 的 `mpv.conf` / `input.conf` 同名键以最后一次出现为准。冲突检测必须给出「生效值 + 来源文件 + 行号」，不能只报「有重复」。
   - **语义色优先**：界面颜色一律取 `cx.theme().colors.*`，禁止在页面里硬编码 `rgb(0x…)`。令牌由 `src/ui/theme.rs` 投影，昼夜主题与对比度依赖这一层。
   - **布局不变量**：根容器必须有 `size_full()` + `bg(background)` + `text_color(foreground)`；内容区滚动容器必须有 `flex_1()` + `min_h_0()` + `overflow_y_scrollbar()`。缺任一项会重现「白底浅色字 / 无法滚动」的历史问题。
   - **窗口控制自绘**：窗口用 `appears_transparent: true` 隐藏系统标题栏，最小化/最大化/关闭由 `ui::window_control` 用字形绘制。**不要改回依赖组件库 `TitleBar` 的 SVG 图标按钮**——资源缺失时按钮会变成不可见的空白可点区域。
3. **技术禁令**：
   - `unsafe_code = "forbid"`，任何 `unsafe` 都会被 CI 拒绝。需要系统能力时换安全封装或放弃该路径。
   - 禁止 `unwrap()` / `expect()` / `panic!` / `todo!()` / `unimplemented!()` / `dbg!()`；用 `?`、`let ... else`、`Option` 组合子替代，确实必要时写 `#[expect(..., reason = "…")]`。
   - 禁止绕过 `gpui-kit` 单依赖入口直接引入 `gpui` / `gpui-component` 其它版本——`gpui-pre` / `gpui-fast` 在 crates.io 不含原生平台实现，版本矩阵一乱就编译不过。
   - `src/core/` 内禁止 `use gpui_kit::*`，core 层必须可脱离 GUI 单测。
   - **界面风格是液态玻璃，不许改成扁平 / 不透明**。卡片一律用 `ui::card()` 或 `ui::theme::glass(cx)` 取材质，禁止在页面里直接写 `rgb()` / 不透明背景色。
   - **不要用大半径阴影做柔光**。GPUI 的模糊采样精度不足，`blur_radius` 到几百像素会出现多边形棱边与网格接缝；背景光斑统一走 `ui::glow()`（多层同心圆 + 收敛衰减）。
   - 不要把窗口控制按钮换回组件库的 `TitleBar`：其按钮依赖 SVG 资源，资源缺失时整块区域不可见。
4. **README 风格**：顶部 shields.io 徽章（`<p align="center">` 纯 HTML 居中）→ 双名标题 → 语言切换徽章 + 文档导航 → `[!WARNING]` AI 声明 → 目录 → 正文 → 底部三节（关联仓库 / 作者联系方式 / 开源协议）。
5. **双名标题**：README 顶部**中文名 `<h1>Névé 粒雪</h1>` 在上、GitHub 仓库名 `<h2>neve</h2>` 在下**，两者都要有。
6. **协议**：代码 AGPL-3.0，`LICENSE` 不许改动。
7. **文档索引**：「文档」节只列**操作文档**；测试文件不进索引，仅一句提示（本项目测试写在 `src/**/mod tests`）。
8. **提交信息**：说明「做了什么 / 为什么」；单人仓库可直接提交 `main`，多人协作从 `main` 切特性分支。
9. **提交身份**：`git config user.name` 必须是 `awujian`，`user.email` 使用维护者 GitHub 绑定邮箱。旧邮箱一旦进了提交元数据，「改文件」和 `force push` 都清不掉——普通仓库要靠删库重建，fork 仓库只能请 GitHub Support 做 GC。
10. **维护**：每次改动同步 `CHANGELOG.md`（Keep a Changelog，语义化版本 `MAJOR.MINOR.PATCH`）；文档与代码不一致时以代码为准并在同一次改动中修正。
11. **徽章由脚本生成**：顶部徽章用 `<!-- badges:start -->` / `<!-- badges:end -->`，语言切换与文档导航用 `<!-- language-nav:start -->` / `<!-- language-nav:end -->` 包裹；改徽章 / 语言 / 文档入口只改 `scripts/badges.config.json`，再跑 `python scripts/update_badges.py`。禁止手改标记内内容，提交前 `--check` 必须通过。
12. **徽章必须居中**：用 `<p align="center">` 包纯 HTML `<a><img></a>`，不要用 Markdown 图片语法；语言徽章三态为当前绿 / 可切换蓝 / 待补灰（无链接）。

## 改动前检查

- **改 Rust 代码**：三条门禁全绿才提交。

  ```bash
  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features
  ```

- **改界面**：`cargo run` 实际启动，分别截图深色与浅色两套，确认：根容器有底色、内容区可滚动、控件尺寸正常、文本对比度 ≥ 4.5:1。截图存到 `docs/screenshots/`。
- **改 `core/config`**：必须补往返渲染测试（LF 与 CRLF 各一），确认注释与行序未被破坏。
- **改写操作**：确认走 `BackupManager`，并补一条「写入 → 撤回 → 内容还原」的测试。
- **改徽章**：只改 `scripts/badges.config.json`，跑 `python scripts/update_badges.py`，再跑 `--check`。
- **改文档**：正文与代码不一致时以代码为准，并同步 `CHANGELOG.md`。
- **提交**：`git status` 确认不含 `target/`、临时文件与本地绝对路径截图。

## 不要做的事

- 不要手写 `⚠️ Warning` 代替 GitHub 警示块 `> [!WARNING]`。
- 不要在页面里硬编码颜色常量，不要新增 `const FG: u32 = 0x…` 这类令牌绕过写法。
- 不要移除根容器的 `size_full()` / `bg()` / `text_color()`，也不要移除滚动容器的 `min_h_0()`。
- 不要用组件库 `TitleBar` 的默认窗口控制按钮替换 `ui::window_control`。
- 不要缩写命名、不要同名文件（含仅大小写不同）、不要用数字后缀解决冲突（`PlayerPanel2` ❌）。
- 不要在 `src/core/` 里引入 GPUI 依赖，也不要在 `ui/` 里直接读配置文件字节——必须经 `core` 的解析器。
- 不要绕过 `core::backup` 直接写用户目录。
- 不要在标题用 VS16 变体 emoji（`⚠️` / `🖼️`），会破坏自动锚点。
- 不要删除 `AGENTS.md` / `CONTRIBUTING.md` / `CHANGELOG.md`；它们承载协作与 AI 约定。
- 不要手改 `<!-- badges:start -->` / `<!-- language-nav:start -->` 之间的内容，也不要绕过脚本直接改徽章链接。
