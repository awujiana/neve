# 贡献指南

> 本文件面向人类贡献者。AI 编码 / 写作 agent 请阅读 [`AGENTS.md`](AGENTS.md)。

感谢你愿意为 **Névé 粒雪**（GitHub 仓库名 `neve`）出一份力。本项目是一个使用 **纯 Rust + GPUI** 编写的 MPV 内核播放器管理器，目标是让「检测播放器、装脚本、改配置、跨包同步」这些体力活变成可预览、可撤销的操作。

## 参与方式

1. **报告问题**：提交 Issue，附系统版本（Windows 版本 / GPU）、复现步骤、相关日志与截图。
2. **修复与实现**：从 `main` 切出特性分支，改动完成后提 PR，说明「做了什么 / 为什么」。
3. **写文档**：设计文档、架构文档、UI 规范与代码同等重要，文档改动同样走 PR。
4. **翻译**：补齐 `README.en.md` 等译文文件后，顶部语言徽章会自动从「待补」升级。

## 提交 Issue / PR 前请自查

- [ ] 已在最新 `main` 上复现，而非旧版本残留
- [ ] 已运行 `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`，三项全绿
- [ ] 涉及界面改动时，附深色与浅色两套截图
- [ ] 涉及文件写入的改动，走 `core::backup` 备份路径，不直接覆盖用户配置
- [ ] 已在 `CHANGELOG.md` 的 `[未发布]` 下补充条目

## 命名规范（不缩写）

命名是本项目的硬性门禁，评审会直接打回不合规的命名。

### 总则

1. **一律全拼，禁止缩写**：`cfg` → `config`、`btn` → `button`、`img` → `image`、`usr` → `user`。
2. **名称不重复**：项目名全局唯一；同目录文件名唯一（大小写不敏感）；同模块导出名唯一；组件名全局唯一。
3. **名称即语义**：看到名字能判断用途，禁止 `utils`、`common`、`misc`、`temp`、`data1` 这类泛化名。

### 项目命名

- `kebab-case`，全小写，2–5 个单词。
- 不含版本号、日期、作者名；不与同账号下已有仓库重名。

### 文件命名

| 文件类型 | 规则 | 示例 |
| --- | --- | --- |
| 文档 | `kebab-case.md` | `release-notes.md` |
| Rust 模块 | `snake_case.rs`，与模块名一致 | `players.rs` → `mod players` |
| 主题 / 配置 | `kebab-case.toml` / `kebab-case.json` | `ui-theme.toml` |

- 同一目录内不得出现同名文件（含仅大小写不同）。
- 避免用 `mod.rs` 之外的方式制造同名歧义；`mod.rs` 只做模块声明与再导出。

### 函数与组件命名

- **函数**：`snake_case`，动词开头，全拼，例如 `detect_players`、`scan_dependencies`、`plan_script_sync`。
- **类型 / 组件**：`PascalCase`，名词，全拼，例如 `DetectedPlayer`、`PlayerPanel`、`BackupManager`。
- **常量**：`UPPER_SNAKE_CASE`，例如 `KNOWN_SCRIPT_DEPS`、`UOSC_REPO_URL`。
- **文件名即模块名**：`core/scripts/uosc.rs` 导出 `core::scripts::uosc` 下的符号。

### 变量与缩写白名单

- 变量名同样全拼：`path`、`config_dir`、`script_entry`，不写 `p`、`cfg_dir`、`sc`。
- 允许保留的通用缩写仅限业界固定写法：`url`、`id`、`ui`、`ux`、`http`、`json`、`toml`、`exe`、`lua`、`mpv`、`uosc`、`osc`、`conf`。

## 代码约定

### 质量门禁（必须全绿）

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

- `unsafe_code = "forbid"`：不得引入 `unsafe`。需要系统能力时优先找安全封装或换实现路径。
- `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented` / `dbg_macro` 均为警告，CI 下等于拒绝。用 `?`、`let ... else`、`Option` 组合子替代。
- 测试里也适用；确实无法避免时用 `#[expect(..., reason = "…")]` 并写明原因。

### 分层边界

- `core/`：纯逻辑与文件操作，不依赖 GPUI，可单测。
- `ui/`：视图模型与交互，只调用 `core/` 暴露的接口，不直接读原始文件。
- `platform/`：系统集成（注册表、文件关联、自启动）。
- **任何写操作前必须走 `core::backup`**，这是不可协商的不变量。

### 文档约定

- 正文与代码不一致时以代码为准，并在同一次改动中修正文档。
- 设计文档的验收标准用 Given-When-Then 写法。
- 界面改动同步更新 `docs/UI设计规范.md` 与 `docs/screenshots/`。

## 行为准则

- 讨论对事不对人；技术分歧用可复现的证据（日志、截图、测试）说明。
- 不提交他人的私有配置、密钥或未脱敏路径。
- 破坏性变更（数据格式、配置语义、公开接口）必须写进 `CHANGELOG.md` 的「破坏性变更」。

## 许可证

本项目基于 [AGPL-3.0](LICENSE) 开源。提交 PR 即表示你同意以同一协议授权你的贡献。

版权所有 © awujian。
