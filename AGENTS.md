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
| `src/core/config/` | 无损 conf 解析（语义层 + 原文层双表示）、input 绑定、冲突检测、扫描报告、配置工作区逐行视图 | ✅ |
| `src/core/config/document.rs` | 配置目录扫描、逐行视图（注释 / 行尾说明 / 注释归属 / 生效关系）、值级编辑与撤回；`ui` 配置页的唯一数据源 | ✅ |
| `src/core/scripts/` | 脚本枚举、启停、依赖扫描；`uosc.rs` 为 uosc 专项 | ✅ |
| `src/core/filters.rs` | 滤镜清单（视频滤镜 / 着色器 / 配置组）与**注释式**开关 | ✅ |
| `src/core/skins.rs` | 皮肤方案与 uosc 配置读取（`SkinPalette` / `SkinOpacity` / `UoscLayout`） | ✅ |
| `src/core/sync.rs` | 跨包同步的计划与执行（FR-03） | ✅ |
| `src/core/backup.rs` | 写前备份与撤回，**所有写操作的必经之路** | ✅ |
| `src/core/store.rs` | 数据目录定位（便携版优先） | ✅ |
| `src/ui/mod.rs` | 页面骨架、侧栏、概览页 / 播放器页 / 脚本页 / 滤镜页 / 皮肤页 / 配置页 / 备份同步页 / 玻璃实验室与交互动作 | ✅ |
| `src/ui/theme.rs` | 设计令牌 → `ThemeColor` 语义槽位的投影 | ✅ |
| `src/ui/glass/` | 液态玻璃组件库：参数(`spec`) / 解析(`resolve`) / 组件(`surface`) 三层 | ✅ |
| `src/ui/mpv_preview.rs` | mpv 皮肤与 uosc UI 的静态预览画布（唯一允许用字面量 `0xRRGGBB` 的视图） | ✅ |
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
   - **双表示无损渲染**：`ConfDoc` / `InputDoc` 同时保存语义层 `lines`（供 `entries()` / `get()` / 冲突检测）与原文层 `raw_lines`；`render()` 直接 `raw_lines.join(sep)`。从结构化字段重新拼行必然丢掉真实整合包里的缩进注释、对齐空格、行尾空格与行尾说明；`set_key` / `set_binding` 必须同步更新被改行的原文。
   - **配置 UI 只消费逐行视图**：`ui` 不读配置字节，一律经 `core::config::document` 的 `PlayerConfigWorkspace` / `ConfigFileView` / `ConfigLineView`。整行注释、行尾说明、注释归属、生效 / 被覆盖状态都在视图里算好，页面不自己解析 conf。
   - **行尾注释只是展示层**：mpv.conf 官方语法不支持行内注释，`key=value # 说明` 里的 `# 说明` 实际会进入 value。`split_inline_comment()` 只用于显示，写入仍以原文行回写，不能改变 mpv 看到的语义；注释掉的行（`#gpu-api=…`）只展示、不给「编辑」入口。
   - **last-wins 语义**：mpv 的 `mpv.conf` / `input.conf` 同名键以最后一次出现为准。冲突检测必须给出「生效值 + 来源文件 + 行号」，不能只报「有重复」。
   - **语义色优先**：界面颜色一律取 `cx.theme().colors.*`，禁止在页面里硬编码 `rgb(0x…)`。令牌由 `src/ui/theme.rs` 投影，昼夜主题与对比度依赖这一层。
   - **布局不变量**：根容器必须有 `size_full()` + `bg(background)` + `text_color(foreground)`；内容区滚动容器必须有 `flex_1()` + `min_h_0()` + `overflow_y_scrollbar()`。缺任一项会重现「白底浅色字 / 无法滚动」的历史问题。
   - **窗口控制自绘**：窗口用 `appears_transparent: true` 隐藏系统标题栏，最小化/最大化/关闭由 `ui::window_control` 用字形绘制。**不要改回依赖组件库 `TitleBar` 的 SVG 图标按钮**——资源缺失时按钮会变成不可见的空白可点区域。
3. **技术禁令**：
   - `unsafe_code = "forbid"`，任何 `unsafe` 都会被 CI 拒绝。需要系统能力时换安全封装或放弃该路径。
   - 禁止 `unwrap()` / `expect()` / `panic!` / `todo!()` / `unimplemented!()` / `dbg!()`；用 `?`、`let ... else`、`Option` 组合子替代，确实必要时写 `#[expect(..., reason = "…")]`。
   - 禁止绕过 `gpui-kit` 单依赖入口直接引入 `gpui` / `gpui-component` 其它版本——`gpui-pre` / `gpui-fast` 在 crates.io 不含原生平台实现，版本矩阵一乱就编译不过。
   - `src/core/` 内禁止 `use gpui_kit::*`，core 层必须可脱离 GUI 单测。
   - **界面风格是液态玻璃，不许改成扁平 / 不透明**。卡片一律用 `ui::card()`（内部走 `ui::glass::card`）或 `ui::theme::glass(cx)` 取材质，禁止在页面里直接写 `rgb()` / 不透明背景色，也不要再手拼 `BoxShadow` 玻璃层——参数与分层统一由 `src/ui/glass/` 负责。
   - **组件库的参数层不含颜色**：`GlassSpec` / `GlassPreset` 只有数值，颜色一律在 `resolve.rs` 从 `theme::Glass` 令牌派生。给参数层加颜色常量会绕过语义色投影，也会破坏半透背景链（实色一铺，窗口层亚克力就看不见了）。新增玻璃能力先读 `docs/液态玻璃组件库.md` §3 的 GPUI 能力边界。
   - **配置编辑必须值级替换 + 写前备份**：`document::set_option_value()` 只替换生效行的 value 片段，保留缩进、等号后对齐、值后 padding 与行尾说明；写前经 `BackupManager::backup_file`，写后用解析器做 `render()` 往返与结构化自检。非 UTF-8、混合行尾或往返自检失败的文件标记 `error`，一律禁止编辑与保存。
   - **组件库输入框也要自己的 bounds**：配置编辑框必须挂在 `Entity<InputState>` 上（与 `SliderState` 同理），自绘 `Div` 处理不了光标、选中与键盘事件。编辑期间切换播放器 / 文件要取消保存，避免把值写到别的文件。
   - **玻璃必须真的透**：改任何背景相关代码都要沿「背景链」逐层确认没有实色铺底——组件库 `Root` 会用 `theme.tokens.background` 给**整窗**刷一层底色，只要它是实色，窗口层的亚克力就完全看不见，界面会退化成一块黑板。清掉它的办法是在 `ui::theme::set_appearance` 里把 `colors.background` 设为全透明，窗口底色改由我们自己的根容器画带 alpha 的渐变。验证方式见 `scripts/probe_transparency.py`。
   - **次级文字不能压太暗**：玻璃底下会透出任意颜色的桌面，中性灰（`text_secondary`）压到 `#8a8a92` 一档时，落在高饱和背景上就读不出来了。
   - **不要给背景加装饰光斑**。曾经用 `ui::glow()`（64 层同心圆叠径向衰减）在右上/左下放两处柔光，实测在半透底上会显出圆形轮廓，已整体移除；背景只保留一层斜向渐变。GPUI 也没有径向渐变与大半径模糊（`blur_radius` 几百像素会因采样精度不足出现多边形棱边），别再尝试这条路。
   - **压在半透底色上的文字不能用次级色**：窗口底色是半透的，深色壁纸会把它拉到中灰（实测 151/255），次级灰字只有 2.7:1。页头说明一类直接落在底色上的文字一律用 `foreground`，层级靠字号与字重区分；只有落在 `surface` 面板上的文字才用 `muted_foreground`。改完用 `scripts/probe_contrast.py` 量一遍，正文要 ≥ 4.5:1。
   - **亮端底色不透明度有下限 0.78**：纯黑桌面上，底色不透明度要 ≈0.70 才能把窗口底抬到 ≈175/255 让正文压住 AA。下限只随明暗上升，深色端仍是 0.20——想要极透就把明暗拉到暗端，两条线互不干扰。
   - **窗口拖动只能走 `window_control_area(WindowControlArea::Drag)`**：`Window::start_window_move()` 在 Windows 平台层是空实现，自己接管必然拖不动；而拖动区一旦挂 `on_mouse_down`，gpui 会把 `WM_NCLBUTTONDOWN` 判成已处理、返回 0，系统的原生拖动也被掐断。两者不要同时用（非 Windows 分支才保留 `start_window_move`）。改完用 `scripts/probe_drag.py` 实测位移，别靠双击最大化生效来推断拖动也能用。
   - **滑杆必须挂在 `Entity<SliderState>` 上**：`SliderState` 自己记录渲染后的 `bounds` 才能把指针换算成值，自绘 `Div` 轨道做不到。默认 `0..100 / step 1` 正好是 1% 精度，显式写出来防止组件库改默认值。`SliderEvent::Change` 拖拽中连续发出、只改内存；`Release` 才落盘。`cx.subscribe()` 返回 `#[must_use]` 的 `Subscription`，末尾要 `.detach()`。
   - **滤镜切换只做注释 / 取消注释**：`core::filters` 的 `toggle_line()` / `toggle_profile()` 只在行首加 / 去 `# `，**不改命令本身**。理由同 §「双表示无损渲染」——真实 `input.conf` 里有对齐空格、行尾说明与条件前缀，重写命令必然丢。写前必须经 `BackupManager::backup_file`。实测一次切换只多 2 字节。
   - **`SkinPalette` 严格对齐 uosc 的 10 个配色键**：`main.lua` 的 `config_defaults.color` 只有 `foreground` / `foreground_text` / `background` / `background_text` / `window_border` / `curtain` / `success` / `error` / `match` / `heatmap`。**不要自造 `menu_*` / `timeline_track` / `time_current` 这类键**——uosc 根本没有，造出来只会让预览变成另一套配色。`SkinOpacity` 与 `UoscLayout` 的默认值同理，一律取 uosc 官方值（`controls=0`、`timeline_size=30`、`controls_spacing=2`、`menu_item_height=36`、`border_radius=2`）。
   - **预览改版式前必须打开真实 mpv 对照**：`ui::mpv_preview` 的版式依据是 uosc 源码 + 实机截图（`docs/screenshots/uosc-real.png`），不是配置字面量。三条容易想错的：顶栏**没有通栏底色**（只有左上两行小字 + 右上窗口字形）、控制条常态**没有底色块**（`opacity.controls=0`，裸图标）、放不下时 uosc **从中间往外左右交替隐藏**（`fit_controls()`）。凭想象改必然再次被指出「这不是 uosc 的 UI」。
   - **`ui::mpv_preview` 是硬编码颜色的唯一例外**：预览画的是**播放器的**皮肤配色，不是本应用的语义色，因此该文件允许直接用 `0xRRGGBB`（取自 `SkinPalette`）。其余任何视图仍禁止硬编码颜色。GPUI 无 `text_shadow` / 自定义 shader / 径向渐变，预览图标用 `div` 几何形状拼，别试图加载 Material Icons（组件库图标需注册 `AssetSource`，`gpui_kit::application()` 未挂载）。
   - **恢复前先再备份**：`core::backup::restore()` 会先备份当前文件再覆盖，否则「撤回」本身不可撤回。备份页与配置页的撤回入口都必须走这条路径。
   - **跨包同步先生成计划再执行**：`core::sync` 必须先产出含「新增 / 一致 / 冲突」三类的计划给用户过目，再执行写入。禁止一步直接写目标目录。
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
- **改 `core/config`**：必须补往返渲染测试（LF 与 CRLF 各一），确认注释、缩进、对齐空格与行序未被破坏；改逐行视图还要确认行尾说明与生效 / 被覆盖标记仍在。
- **改写操作**：确认走 `BackupManager`，并补一条「写入 → 撤回 → 内容还原」的测试，配置编辑还要断言注释与行尾说明没被改掉。
- **改配置页**：`cargo run` 实测，确认注释可见、行尾说明拆分、生效 / 被覆盖徽标、编辑条与撤回入口；截图存 `docs/screenshots/`。
- **改皮肤页 / 预览画布**：先启动真实 mpv 观察当前 uosc 的实际形态（同一份 `portable_config`，`--ontop=yes` 便于截图），再对着 `docs/screenshots/uosc-real.png` 逐项复核顶栏、控制条、时间轴、菜单四处。改完截图存 `docs/screenshots/`。
- **改滤镜页**：切换开关后必须校验目标文件的字节数与哈希变化（预期只差 `# ` 两个字节），再从备份还原并断言哈希回到原值。**不要拿用户的真实 `input.conf` 当试验田而不还原**。
- **改备份 / 同步页**：确认恢复路径先备份再覆盖，同步执行前一定能看到计划。
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
- 不要给 uosc 皮肤配置自造配色键，也不要凭配置想象预览版式——先开真实 mpv 看。
- 不要在 `ui::mpv_preview` 以外的视图里硬编码颜色；该文件的例外仅限取自 `SkinPalette` 的播放器配色。
- 不要在标题用 VS16 变体 emoji（`⚠️` / `🖼️`），会破坏自动锚点。
- 不要删除 `AGENTS.md` / `CONTRIBUTING.md` / `CHANGELOG.md`；它们承载协作与 AI 约定。
- 不要手改 `<!-- badges:start -->` / `<!-- language-nav:start -->` 之间的内容，也不要绕过脚本直接改徽章链接。
