# Névé Rust 代码风格与质量门禁

> 版本：v1.0 | 状态：强制规范 | 适用范围：Névé 全部 Rust 源码与工作流
> 最后更新：2026-11-02

本文是 Névé 项目的强制 Rust 工程规范。所有贡献者、自动生成代码和 AI 辅助修改均适用。规则用于统一代码阅读体验、减少审查分歧，并让机械化重构具有可预测性。

## 1. 规范优先级

发生冲突时按以下顺序处理：

1. Rust 语言语义、安全性与标准库约束。
2. 本文的项目强制规则及 `rustfmt.toml`。
3. [Rust Style Guide](https://doc.rust-lang.org/style-guide/) 与 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)。
4. 模块内既有且没有违反以上规则的局部约定。

不得以“个人偏好”“原代码就是这样”或格式化工具未覆盖为理由绕过强制规则。确有必要的例外必须在最小作用域内使用 lint 属性，并写明原因；禁止在 crate 或模块根部整体关闭门禁。

## 2. 格式化与排版

格式由仓库根目录的 `rustfmt.toml` 唯一管理，提交前必须运行：

```powershell
cargo fmt --all
cargo fmt --all -- --check
```

强制基线：

| 项目 | 规则 |
|---|---|
| 工具链 | stable rustfmt；配置不得依赖 nightly-only 选项 |
| Edition | Rust 2024；`edition` 与 `style_edition` 均为 `2024` |
| 行宽 | `max_width = 100` |
| 缩进 | 4 个空格，禁止 Tab |
| 换行 | LF（`newline_style = "Unix"`） |
| 导入 | 启用 rustfmt 导入排序；分组与可见性保持清晰 |
| 尾随逗号 | 多行列表使用尾随逗号，由 rustfmt 统一维护 |
| 缩进方式 | 使用标准块缩进，不手工对齐表达式或字段列 |

不得手工调整格式来抵抗 rustfmt。格式化造成的纯排版变化应与功能修改保持在同一提交中，避免反复格式化产生噪音。

## 3. 命名、导入与模块

- 类型、trait、枚举及枚举变体使用 `UpperCamelCase`。
- 函数、方法、模块、局部变量使用 `snake_case`。
- 常量与静态变量使用 `SCREAMING_SNAKE_CASE`。
- 类型名应表达领域概念；布尔值优先使用 `is_`、`has_`、`can_` 等可读前缀。
- 转换方法按语义使用 `as_`（低成本视图转换）、`to_`（借用输入并生成拥有值，可能分配）、`into_`（消耗 `self`）。
- getter 使用字段语义命名，例如 `fn path(&self) -> &Path`；只有查询可能失败或存在歧义时才使用 `get_`。
- 已导入的类型直接使用短类型名；顶层函数和模块成员调用保留模块路径以表达来源，例如 `mem::transmute`。避免无意义的通配导入。
- 按职责组织模块，避免循环依赖和超大 `mod.rs`。模块可见性默认私有；只有确有跨模块调用需求才提升为 `pub(crate)` 或 `pub`。
- 依赖方向遵循架构文档：UI 调用 core，core 不依赖 GPUI；平台能力通过明确边界接入。

## 4. 所有权、借用与表达式

- 能借用时优先借用：只读参数优先 `&T` / `&str`，避免为了调用方便而克隆。
- `.clone()` 必须有明确语义理由，例如需要独立所有权、跨线程 `'static` 生命周期或明确的快照；不得将其作为借用检查器报错的默认补丁。
- `Rc` / `Arc` 仅用于确实需要共享所有权的场景。跨线程共享使用 `Arc`，单线程共享优先普通借用或所有权传递。
- 优先用迭代器组合表达映射、筛选和聚合；当循环包含复杂副作用、状态机、提前退出或调试步骤时，普通 `for` 循环更清楚，应直接使用。
- 用 `?` 传播可恢复错误。生产代码禁止无说明的 `unwrap()` / `expect()`；只有已由局部代码建立且无法继续传播的硬不变量，才可使用带原因的 `expect()`。
- 对可预期的边界情况使用早返回和 `continue`，避免不必要的深层嵌套。
- 禁止用 `let _ = fallible_call()` 静默吞掉重要错误；确需忽略时添加说明，或显式处理结果。
- 不为消除编译错误而引入不必要的 `String`、`Vec`、装箱或引用计数；先确认 API 是否能接受切片、迭代器或借用。

## 5. 应用代码与公共库 API

Névé 当前是桌面应用二进制 crate，`core` 仍位于同一 crate 内。日常实现以简单、可维护和清晰的调用路径为优先；只有形成稳定复用边界时，才把模块提升为公共库 API。

### 5.1 应用代码（当前默认）

- 参数可直接使用 `&[T]`、`&str`、`Vec<T>` 或具体领域类型，不为假想的调用方引入泛型。
- 返回 `Vec<T>` 在调用方需要拥有完整结果、需要排序/缓存或数据规模有限时完全合理。
- 优先使用明确的领域结构体和枚举，避免用复杂 trait 抽象简单流程。
- 错误类型应能帮助 UI 给出可理解反馈；内部模块可按实际需要使用具体错误或 `anyhow` 风格的上下文，但不得丢失关键原因。

### 5.2 未来公共库或稳定公共模块

- 公共 API 必须经过调用方视角审查，避免暴露内部实现细节。
- 输入可接受 `impl AsRef<Path>`、`impl IntoIterator<Item = T>` 等抽象，但仅在确实提升调用灵活性时使用。
- 若调用方需要惰性处理或组合操作，优先返回 `impl Iterator` / 具名迭代器；若 API 语义就是快照、缓存或拥有的集合，返回 `Vec` 合理。不得机械地禁止 `Vec`。
- 公共类型应实现有意义的 `Debug`；按语义考虑 `Clone`、`PartialEq`、`Eq`、`Hash`、`Default`，不得为满足清单而实现不合理的 trait。
- 转换使用 `From` / `TryFrom`，不要直接实现 `Into` / `TryInto`。
- 公共错误类型实现 `std::error::Error`，并在跨线程/异步边界需要时满足 `Send + Sync`。`Display` 保持简洁、稳定，不包含句末标点。
- 集合类型按需提供 `iter()`、`iter_mut()`、`into_iter()`，并遵循标准库的迭代器命名习惯。
- 破坏性 API 变更必须考虑兼容策略，并在变更说明中记录。

## 6. 错误、安全与日志

- `unsafe` 在整个 crate 中禁止；Cargo lint 已设为 `unsafe_code = "forbid"`。确需 FFI 或底层操作时，必须先调整架构并进行单独设计审查，不能局部压制该规则。
- 不得使用 `panic!`、`unreachable!` 或 `todo!` 处理正常运行时输入、文件内容、系统状态或外部依赖失败。
- Clippy 对 `unwrap()`、`expect()`、`panic!`、`unreachable!`、`todo!`、`unimplemented!` 和 `dbg!` 启用显式 lint；CI 的 `-D warnings` 会将这些警告升级为失败。确有不变量依据的 `expect()` 例外必须在调用点使用 `#[allow(clippy::expect_used)]` 并附原因注释。
- 文件系统、注册表和配置写入必须遵循架构文档中的备份与恢复要求；先验证输入和目标，再执行写入。
- 错误信息应包含足以定位操作的上下文，但不得记录访问令牌、密码或用户文件内容等敏感信息。
- 使用结构化且有意义的日志级别；库/核心逻辑避免直接输出面向最终用户的 UI 文案。

## 7. 依赖与 Cargo 约定

- 优先使用标准库；新增依赖前说明必要性、维护状态、许可和体积/安全影响。
- 不得为了少量辅助代码引入大型依赖。
- 直接依赖必须由源码使用；移除功能时同步清理不再使用的依赖。
- 提交应用仓库的 `Cargo.lock`，保证 CI 与开发者解析到一致的依赖版本。
- 禁止提交 `target/`、本地 IDE 状态、密钥、缓存和临时产物。

## 8. 强制质量门禁

所有门禁必须通过后才能合并到 `main`。本地使用 PowerShell 执行：

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

门禁定义：

| 门禁 | 失败条件 | 目的 |
|---|---|---|
| rustfmt | 存在未格式化代码 | 保证排版一致 |
| Clippy | 任意 warning 或 error | 将 lint 警告视为构建失败 |
| Tests | 任意测试失败 | 防止行为回归 |

`.github/workflows/ci.yml` 在 Windows stable 环境执行以上全部检查。Pull Request 与推送到 `main` 均触发；CI 红灯时不得合并。不得通过 `#[allow]`、全局配置或删除测试绕过失败。例外必须有具体原因、最小作用域和后续清理事项。

要让 GitHub 从平台层面阻止未通过检查的合并，仓库管理员还必须在 `Settings → Branches`（或 Rulesets）为 `main` 启用分支保护，并将 `Format, lint, and test (Windows)` 设为必需状态检查。仅添加 CI workflow 不会自动启用这项合并限制。

### 本地提交前检查

```powershell
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

## 9. Code Review 检查清单

- [ ] 格式检查通过，导入与命名符合本文。
- [ ] 没有为绕过借用问题而滥用 `.clone()`、`Rc` 或 `Arc`。
- [ ] 错误路径有明确处理；没有无理由的 `unwrap()` / `expect()` / `panic!()`。
- [ ] 模块职责清楚、可见性最小化、依赖方向正确。
- [ ] 新增公共 API 有调用方视角的设计依据和必要 trait。
- [ ] 新增依赖有必要性说明；锁文件已更新。
- [ ] Clippy、测试和 `git diff --check` 全部通过。
- [ ] 涉及文件或系统设置写入时具备备份、失败恢复和用户可理解的结果反馈。

## 10. 参考资料

- [Rust Style Guide](https://doc.rust-lang.org/style-guide/)
- [rustfmt configuration](https://rust-lang.github.io/rustfmt/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rust Programming Language：Ownership](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)
- [Clippy lint list](https://doc.rust-lang.org/clippy/lintlist.html)
