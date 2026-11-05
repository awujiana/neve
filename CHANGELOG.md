# 更新日志

本文件记录 `Névé 粒雪`（`neve`）的重要变更，版本号遵循语义化版本 `MAJOR.MINOR.PATCH`。

## [未发布]

### 修复

- **`render()` 并非真的字节级往返**：此前 `ConfDoc` / `InputDoc` 从结构化字段重新拼行，会丢掉真实整合包里的缩进注释、对齐空格、行尾空格与行尾说明。现在两个文档同时保存语义层 `lines` 与原文层 `raw_lines`，`render()` 直接 `join(sep)`；`set_key` / `set_binding` 同步更新被改行的原文，并补了带缩进 / 对齐 / 行尾空格的 LF 与 CRLF 往返样例。
- **自绘标题栏拖不动窗口**：原先在拖动区调 `Window::start_window_move()`，但 gpui 的 Windows 平台层把它留成了空实现（`gpui-pre-windows-0.3.9` 没有重写 `platform.rs` 的默认 `fn start_window_move(&self) {}`）；更糟的是 `on_mouse_down` 会把 `WM_NCLBUTTONDOWN` 判成「已处理」并返回 0，系统连原生拖动都收不到。现在改为把拖动区登记成 `WindowControlArea::Drag`——Windows 命中测试据此返回 `HTCAPTION`，拖动、双击最大化、贴边分屏全由系统接管；拖动区不再挂任何鼠标按下监听器。非 Windows 平台仍走 `start_window_move`（macOS 有真实实现）。
- **设置页滑杆只能点、不能拖**：自己画 `Div` 轨道拿不到渲染后的 `bounds`，无法把指针位置换算成值。改用组件库 `Slider` + `Entity<SliderState>`（状态自己记录 bounds），显式 `min(0) / max(100) / step(1)`，即 1% 精度；轨道 180px，1% ≈ 1.8px。加减按钮步进同步由 5 改为 1。
- **一次拖拽写盘上百次**：`SliderEvent::Change` 在拖动中连续发出，原先每次都落盘；现在 Change 只改内存与主题，`Release` 才写一次盘。

- **窗口真的透出来了**（此前一直是实色黑板）：组件库 `Root` 会用 `theme.tokens.background` 给整窗刷一层不透明底色，压在 DWM 亚克力之上，界面因此完全不透。`ui::theme::set_appearance` 现在把这层设为全透明，窗口底色只由我们自己的根容器画带 alpha 的渐变。
- **面板不透明度改为跟随「背景不透明度」滑杆**：材质定基调、滑杆在其上增减（面板只跟随一半幅度，留出可读性的地板）；浮层（菜单 / 弹层）取玻璃的实体形态，不随滑杆变虚，否则菜单文字会浮在桌面上读不清。默认背景不透明度由 80 降到 35。
- **次级文字提亮**：`text_secondary` 暗端由 `#8a8a92` 提到 `#b8b8c0`，避免透出高饱和背景时小字辨认不出。亮端同步压深到 `#3d434e`——面板是半透的，深色壁纸会把它拉到中灰，中灰字就贴住读不出来了。
- **浅色玻璃下压不住对比度的文字**：页头说明一类**直接落在半透窗口底色上**的文字改用主色（此前用次级灰，纯黑桌面上实测只有 2.67:1）；亮端渐变取到纯白，并把底色不透明度下限提到 0.78。用新增的 `scripts/probe_contrast.py` 量：页头说明 2.67 → 5.16、卡片辅助 9.04、底部状态 7.39，全部过 WCAG AA。层级改由字号与字重区分，只有落在 `surface` 面板上的文字才继续用 `muted_foreground`。
- **移除背景装饰光斑**：右上 / 左下两处柔光在半透底上会显出圆形轮廓，删掉 `ui::glow()` 与 `Glass::glow` 令牌，背景只保留一层斜向渐变。

### 新增

- **皮肤页与 uosc 预览画布（`core::skins` + `ui::mpv_preview`）**：把「皮肤」从四行空状态换成可预览的方案列表——1 套内置 OSC + 从各播放器 `script-opts/uosc.conf` 读出的 uosc 方案。预览按 **16:9（720×405）** 画布居中渲染，按方案类型分支：uosc 画顶栏 + 居中菜单 + 底部控制条，内置 OSC 画左上标题 + 经典底部条。
- **预览严格对齐 uosc 真实行为**：`SkinPalette` 只保留 uosc `config_defaults.color` 里真实存在的 10 个键（`foreground` / `foreground_text` / `background` / `background_text` / `window_border` / `curtain` / `success` / `error` / `match` / `heatmap`），默认值取 uosc 官方值；`SkinOpacity` 扩到 7 项（`timeline` / `position` / `chapters` / `controls` / `menu` / `title` / `curtain`），其中 **`controls=0`** 意味着按钮常态没有底色块；`UoscLayout` 的 `top_bar_size=40` / `timeline_size=30` / `controls_size=32` / `controls_spacing=2` / `controls_margin=8` / `menu_item_height=36` / `menu_min_width=260` / `menu_padding=1` / `border_radius=2` / `window_border_size=1` 全部改为 uosc 官方值。此前自造的 `menu_background` / `menu_foreground` / `menu_selection` / `timeline_track` / `time_current` 等 9 个键已删除。
- **`controls=` 解析保留图标名与 tooltip**：`command:{icon}:{cmd}` / `button:{icon}` / `cycle:{icon}:{prop}:...` / `toggle:{icon}:{prop}` 的第一个参数就是图标名，此前被压平成 `command` 导致预览里全是同一个兜底字形。新增 `ControlKind::{Button, Speed, Gap, Space}` 与 `ControlItem { kind, icon, tooltip, scale }`，`label()` 优先返回整合包写的中文 tooltip。另修掉 `<video,audio>subtitles` 这类**前缀里的逗号被当条目分隔符**的切分 bug（`split_control_list()` 做括号深度感知）。
- **预览按实机截图重画**：用同一份 `portable_config` 启动真实 mpv 观察后对齐三处——顶栏**不是通栏底色**，只有左上两行小字（主标题胶囊 + 副标题）与右上半透明窗口字形；控制条是**裸图标**（`background_opacity = active ? 1 : opacity.controls`，默认 0），放不下时复刻 uosc「从中间往外左右交替隐藏」的策略（`fit_controls()`）；时间轴时间戳**反色**（已播放段上 `foreground_text` 压在 `foreground` 上，未播放段反过来），章节是骑在 `fay - 1` 的小菱形。图标用 `div` 几何形状拼（GPUI 无 `text_shadow`，组件库图标需注册 `AssetSource`）。基准截图归档为 `docs/screenshots/uosc-real.png`。
- **脚本页**：扫描 `scripts/` 下所有脚本（含目录式脚本与单文件脚本），统计已安装 / 已启用 / 已停用 / 依赖问题；带 `script-opts` 的脚本标注出来并可「同步到…」；底部「uosc 专项管理」单独给出状态与「安装 uosc / 撤回 uosc.conf / 查看皮肤预览」三个动作。真实整合包下为 `43 已安装 / 43 已启用 / 24 带 script-opts`。
- **滤镜页（`core::filters`）**：把 `input.conf` 里的滤镜开关命令、`shaders/` 下的 GLSL、`mpv.conf` 里的 profile 配置组收成一张可切换的清单，分 `VideoFilter` / `Shader` / `Profile` 三类。切换**只做注释 / 取消注释**（`toggle_line()` / `toggle_profile()`），不改命令本身，写前经 `BackupManager` 备份，行号与快捷键徽标直接展示给用户核对。真实整合包下为 `10 视频滤镜 / 29 着色器 / 41 配置组 / 27 当前启用`。
- **备份与同步页**：按操作分组列出备份记录（时间、文件、大小、占用），支持逐个撤回；跨整合包同步给出「源 / 目标 / 脚本」三行选择器 + 「生成同步计划」，**先生成计划再执行**，计划里列出新增 / 一致 / 冲突，避免盲写；恢复前会再备份一次当前文件。
- **本轮测试 75 → 93 项**：新增 `core::filters` 的注释切换往返与三类扫描、`core::skins` 的 uosc 配色 10 键映射、`controls=` 保留图标名、`<video,audio>` 前缀逗号不切分、真实 uosc 选项读取。
- **配置管理页（`Page::Configs`）**：从空状态换成真实页面。按播放器分组扫描 `mpv.conf` / `input.conf` / 根级其它 `*.conf` / `script-opts/*.conf`（含一层子目录，顺序稳定），按**原文逐行**展示：行号、类型、整行注释、` #` 起的行尾说明、紧邻上方注释块的归属说明、生效 / 「被第 N 行覆盖」、缩进。统计徽标给出行数、配置项、注释、生效、被覆盖、LF/CRLF 与 `render()` 往返自检；另有「显示注释 / 隐藏注释」「显示全文 / 只看配置项」过滤、重新扫描、打开配置目录、冲突与警告清单（生效值 + 来源 + 行号）。默认只列 12 个文件、240 行，超出可展开，避免单页高度失控。
- **配置注释可见的底层能力 `core::config::document`**：`ConfigFileView` / `ConfigLineView` 把注释与生效关系一次算全，`ui` 不再读配置字节。mpv.conf 官方不支持行内注释，所以行尾说明只在展示层拆分，写入仍以原文行回写，不改变 mpv 语义。
- **值级配置编辑与撤回**：只有「可编辑 + 生效 + 有键」的行给「编辑」入口；`set_option_value()` 只替换生效行的 value 片段，保留缩进、等号后对齐、值后 padding 与行尾说明；写前经 `BackupManager` 备份，写后用解析器做往返与结构化自检；「撤回编辑（N）」按 canonical path 精确匹配备份并还原。编辑期间切换播放器 / 文件会取消保存。非 UTF-8、混合行尾或往返失败的文件只展示、禁止编辑。
- **MPV 播放器管理动作**：播放器页新增「查看配置」（跳配置页并选中对应播放器）、「打开配置目录」（Windows `explorer` / macOS `open` / 其它 `xdg-open`）、「启动播放器」、「探测版本」（改为按需调用，不再空跑）。
- **配置文档单测 8 项 + 解析器 / 播放器回归测试**：注释与行尾说明可见、last-wins 标记被覆盖行、input 按键归一化、LF/CRLF 往返、扫描顺序、编辑保注释 → 撤回还原、编辑保留行尾注释、非 UTF-8 只展示；测试总数 60 → 75。
- **配置页实拍截图**：`docs/screenshots/configs-glass.png`（完整首屏）与 `configs-entries.png`（注释 + 配置项 + 生效状态 + 编辑入口）。
- **液态玻璃组件库 `ui::glass`**：结合 WebGL 折射、iOS 私有玻璃滤镜、Android Backdrop、纯 CSS 多重内高光与油猴脚本的模糊配方，收成三层——`spec.rs`（不含颜色的参数 + 五档预设 + 插值）、`resolve.rs`（参数 + `theme::Glass` 令牌 → 具体颜色/阴影层的纯函数）、`surface.rs`（`surface / card / panel / pill / preset_card` 五个无状态 builder）。GPUI 没有自定义 shader、`backdrop-filter`、径向渐变，线性渐变也只有 2 个 stop，因此折射用对称 inset 亮边近似、色散用冷暖细边、磨砂靠面层 alpha、斜向高光靠两 stop 的 alpha 差；真实背景模糊仍由窗口层的 DWM 亚克力负责。详见 `docs/液态玻璃组件库.md`。
- **玻璃实验室页**：侧栏最后一项，五种预设可切换，折射带宽度 / 折射强度 / 磨砂 / 色散四行参数按 1px / 1% 步进实时调预览卡，并显示解析出的边缘层数、高光角与面层 alpha。参数只作用于预览，不写 `appearance.conf`、不改全局卡片。页面同时是组件库 `panel / pill / surface / preset_card` 四个入口的活示例。
- **组件库单测 14 项**：预设归一化、插值端点/中点/角度最短弧、越界夹取、渐变方向、磨砂增实、折射增层、alpha 全在 0..1、亮端压低白色亮线、五个预设 × 四种组件可构造、极端参数不崩。测试总数 46 → 60。
- **组件库文档 `docs/液态玻璃组件库.md`**：五源参数对照表、GPUI 能力边界、API 参考、参数到视觉的映射规则、项目约定与回归清单；已加入顶部文档导航与 README 文档节。
- **交互实测脚本**：`scripts/probe_drag.py` 拉起真实窗口，用系统级鼠标事件验证三件事——标题栏拖动位移、滑杆手动拖拽、1% 精度（拖到 57% 必须精确命中而不是跳到 55/60）。滑杆轨道由截图像素扫描自动定位（行亮度中位数 + 150..220px 连续横条），不写死坐标。实测结果：位移 (120, 90)、35→50%、86→40%、50→57% 全部通过。
- **自定义背景图**：设置页「外观」新增一行，可挑一张图片铺满窗口（选完自动切到「自定义背景图」材质）；图片由 `Resource::Path` 交给 GPUI 的 `img` 加载，缺失或解码失败时窗口仍由系统合成兜底，不会整块变黑。
- **默认改为浅色玻璃**：明暗默认值由 18 提到 86，亮端色板整体转冷调（蓝灰）：白 / 浅灰半透面板配淡蓝灰渐变底。想回到深色玻璃把明暗拉到 30 以下即可。
- **外缘描边令牌 `Glass::outline`**：暗底用亮线勾边、亮底反过来用一道极淡的暗线（近白面板在近白背景上不加暗线连边界都看不出），取代原先一律用高光边勾边的写法。
- **透明度验证脚本**：`scripts/probe_transparency.py` 在 Neve 后面铺几块高饱和纯色窗口再抓屏，用「彩色像素占比 / 平均饱和度」量化透出程度；`scripts/probe_opacity.py` 用 `PrintWindow` 验证底色层真的带 alpha。**不要用 mpv 全屏当背景**——它会盖在最上层，抓到的只有 mpv。
- **对比度验证脚本**：`scripts/probe_contrast.py` 量截图里文字与底色的实际对比度，替代「看着有点淡」的主观判断；用法 `python scripts/probe_contrast.py <png> "名称:x1,y1,x2,y2"`。
- **外观设置体系**（`core::appearance`）：窗口材质、背景不透明度、明暗、主题颜色、主题字体、锁定窗口颜色、标题栏位置、Tab 栏布局、动画开关，全部可在设置页实时调整并持久化到 `appearance.conf`。配置损坏或字段缺失一律退回默认值，不影响启动。
- **真实窗口模糊**：`Material` 映射到 `WindowBackgroundAppearance`，Windows 下「亚克力 / 模糊」走 `ACCENT_ENABLE_ACRYLICBLURBEHIND`（DWM 亚克力），「透明」走透明渐变。这是真正由系统合成的背景模糊，不是拿半透明色块凑的。
- **标题栏左上角改造**：面板开关 / 搜索 / 筛选三个图标按钮 + `编辑(E) 窗口(W) 帮助(H) 关于(A)` 四个菜单，保留「Névé 粒雪 · MPV 管理器」标识；菜单为点击展开的下拉面板。
- **设置页**：分组卡片 + 「左侧标题说明 / 右侧控件」的行式布局，含分段控件、百分比滑杆、开关与色板。
- **Tab 栏布局**：主导航可在左侧常驻侧栏与标题栏下方横向 Tab 条之间切换。
- **标题栏位置**：支持顶部与底部（左右两侧竖排标题栏未实现，设置页置灰）。
- **液态玻璃不再区分深浅主题**：取消主题开关，改为单一「明暗」连续量（0–100），所有语义色由它插值得到。

- **界面骨架落地**（`src/ui/mod.rs`）：改为「自绘标题栏 + 常驻侧栏 + 内容区 + 滚动列表」布局，落地概览页（统计卡、待处理事项、最近操作）与播放器页；脚本、皮肤、配置、备份与同步、设置五个页面给出空状态与里程碑归属，不再伪装成空列表。
- **设计令牌接入运行时**（`src/ui/theme.rs`）：把 `config/ui-theme.toml` 的配色固化为 `DARK` / `LIGHT` 两套调色板并投影到 gpui-component 的 `ThemeColor` 语义槽位，补齐 `button_*` 系列（组件库这些字段不会回落到 `primary`，不覆盖就会沿用内置色，浅色下表现为黑底主按钮）。
- **昼夜主题切换**：标题栏提供浅/深切换，切换后所有语义色同步变化，无需重启。
- **液态玻璃材质体系**（`ui::theme::Glass`）：把「玻璃」抽象成一组令牌——背景渐变两端色、光斑色、卡片表面、常驻容器表面、内高光边、投影色——深浅各一份；卡片、标题栏、侧栏统一从令牌取材质，页面不再硬编码颜色。卡片由四层构成：半透明填充、1px 亮边、内侧 inset 高光、向下柔和投影。
- **背景层**：低饱和斜向渐变，为半透明表面提供可透的层次（曾另有两处柔和光斑，因在半透底上显出圆形轮廓已移除）。
- **自绘窗口控制按钮**（`ui::window_control`）：最小化 / 最大化 / 关闭用字形绘制，替代依赖 SVG 资源的组件库按钮。
- **操作反馈分级**（`StatusMessage`）：成功 / 警告 / 信息 / 错误四类，各带字形与语义色；同步被跳过时降级为警告，扫描无结果时降级为警告而非「成功」。
- **脚本列表渐进披露**：整合包常见 30+ 脚本，默认只列前 6 条并给出「展开全部 N」，避免单卡高度失控。
- **仓库配套文件**：新增 `CONTRIBUTING.md`（内联完整命名规范）、`AGENTS.md`（五段式 AI 约定）、`CHANGELOG.md`（本文件）、`scripts/update_badges.py` 与 `scripts/badges.config.json`（徽章唯一数据源）。

### 改进

- **`ui::card()` 收敛到组件库**：原先在页面里手拼 `bg / border / 两条 BoxShadow`，现在只负责挑参数与取令牌，四层玻璃统一由 `glass::card(&GlassSpec::regular(), …)` 解析，页面不再重复玻璃配方。
- **配色改为中性无彩**：界面本体只用黑 / 白 / 灰，彩色只留给开关、进度与状态，对应 `modern_glass_zh` 里那颗绿色进度点。选中态改成「反白胶囊」（白底黑字 / 黑底白字），不再叠彩色底。
- **圆角放大到 20px / 8px**，徽标改为全圆角，贴近参考皮肤的尺度。

- **播放器类型名可读化**（`core/players.rs`）：新增 `PlayerKind::display_name()`，界面不再显示 `Debug` 输出（如 `Mpv`、`UnknownMpvKernel("x")`），改为 `mpv` / `mpv.net` / `青山整合包` / `未知内核`。
- **播放器卡片重排**：头部合并「名称 + 类型 + 便携 + 版本」为徽标行，路径与配置目录各占一行并按需截断；原先把所有信息平铺、每个分区各占一段的写法导致单卡过高。
- **状态摘要行**：uosc 状态、脚本数、依赖、配置检查压缩成一排徽标，问题优先展示，详情分区仅在有问题时才展开。
- **uosc.conf 取值**：按 `category` 分组，以 `标签 = 值` 呈现；未设置与空字符串统一显示「默认」，修掉 `key =` 悬空等号。取值改为淡色底片（chip），等号弱化到 45% 不透明度，键与值在视觉上成组。
- **脚本行对齐**：名称列与 `有 conf` 列定宽，开关紧跟徽标列，多行之间纵向对齐便于扫读。此前用 `flex_1()` 把开关推到卡片最右，中间留出一大片空白。
- **侧栏选中态**：选中项用背景 + 左侧标记 + 强调色文字三重表达，悬停底色降为 45% 不透明度，两者不再混淆。
- **README**：徽章改为 `<p align="center">` 纯 HTML 并由脚本生成，标题补上 `<h2>neve</h2>`，展示图换成真实截图。

### 修复

- **背景光斑出现同心圆环带**：早期用 9 层同心圆叠 alpha 近似径向渐变，深色底上每层跳变约 1.1%，肉眼可见一圈圈环带。改为 64 层并让 alpha 增量随半径向外收敛，实测扫描最大色阶跳变已回落到渲染噪声水平。同期试过「透明圆 + 大半径阴影模糊」，但 GPUI 的模糊采样精度不足，`blur_radius` 放大到 260px 时会出现多边形棱边与网格接缝，已放弃。
- **白底浅色字**：根容器此前缺少 `size_full()` / `bg()` / `text_color()`，深色模式下浅色文字落在未着色的白色背景上。现在根容器显式给定底色并让文字色沿样式树继承。
- **内容区无法滚动**：播放器多于一屏时内容被裁掉。内容区改为 `flex_1()` + `min_h_0()` + `overflow_y_scrollbar()`。
- **开关尺寸失控**：`Switch` 默认 `Size::Medium`，在密集列表里显得巨大，统一改为 `Size::Small`。
- **浅色主题主按钮变黑**：`button_primary` 等字段未随令牌覆盖，沿用内置主题的近黑色。已在 `ui/theme.rs` 显式设置。
- **徽标底色不可见**：徽标原先取 `muted` 作底色，在浅色下与卡片表面几乎同色，改为取 `secondary`（`surface_hover`）。
- **窗口标题栏与深色主题冲突**：系统标题栏在深色应用上仍为白色，改为 `appears_transparent` + 自绘标题栏，并设置窗口标题供任务栏显示。

### 已知限制

- 检测与配置扫描仍在启动时同步执行，播放器较多时会阻塞窗口出现（计划移入后台线程）。
- 脚本、皮肤、备份与同步三个页面仍以空状态与里程碑说明占位（脚本启停 / 跨包同步在播放器页、配置管理在配置页、外观定制在设置页已可用）；独立视图随 M3 / M4 接入。
- 界面语言目前只有中文，中英文切换（FR-09）未实现。
- 标题栏的左右竖排布局尚未实现（设置页已置灰）。
- 动画开关目前只保存状态，界面尚无过渡动画可关。
- GPUI 不提供径向渐变、`backdrop-filter` 与自定义 fragment shader，液态玻璃的边缘折射是**屏幕空间近似**（多层对称 inset 亮边 + 冷暖色散细边 + 斜向高光）；真实背景模糊由窗口层 DWM 亚克力负责，背景装饰光斑已整体移除。

## 更早的版本

`0.1.0` 之前未引入 CHANGELOG，早期提交（仓库初始化、`core::players` 检测、`core::config` 无损解析与冲突检测、`core::scripts` 与 uosc 专项、`core::backup`、`core::sync`）的细节见 `git log`，此处不做追溯编造。
