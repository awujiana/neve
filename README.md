# Névé（粒雪）

> 冰川之源 —— PCL2 式 **MPV 内核播放器管理器**
> 纯 Rust + GPUI 原生 GPU 渲染

## 定位

面向**所有使用 MPV 内核的播放器**（mpv、mpv.net、Celluloid、IINA 等及其衍生整合包）的统一管理器：

- **播放器管理**：检测本机所有 MPV 内核播放器，安装 / 切换 / 更新播放器包（含 `../mpv-Bingshan` 产出的整合包）
- **脚本管理**：MPV 脚本（lua 脚本、uosc 系）的安装 / 启停 / 更新 / 依赖扫描
- **一键配置**：
  - 视频文件关联注册（右键"打开方式"/默认程序）
  - input.conf 按键配置（可视化编辑、冲突检测）
  - mpv.conf 配置（分类项编辑、备份回滚）
  - 脚本 conf 配置（各脚本的 conf 文件统一入口编辑）

交互体验对标 PCL2：清爽的侧栏导航、动画流畅、开箱即用。

## 技术栈

| 层 | 选型 |
|---|---|
| 语言 | Rust 2024 edition（需 1.85+ 稳定版工具链） |
| UI | GPUI 0.2（Zed 的 GPU 加速原生渲染框架，支持 Windows） |
| 组件 | gpui-component 0.8（可选，60+ 桌面组件：列表/侧栏/输入框/弹窗） |
| 打包 | 单二进制，无 WebView 依赖 |

## 当前状态

骨架阶段：`src/main.rs` 为 GPUI 最小窗口示例，验证渲染链路。

## 构建

```powershell
# 1. 安装 Rust（如未安装）：https://rustup.rs
rustup default stable

# 2. 构建并运行
cargo run --release
```

> 注意：首次构建需编译 GPUI 及其依赖（vcpkg/图形后端），耗时较长；之后增量编译很快。

## 模块规划

- [x] `core::players` — MPV 内核播放器检测（常见路径 / 注册表卸载键 / PATH 三路扫描 + 去重、便携配置目录识别、惰性版本探测）
- [ ] `core::packages` — 播放器包安装/更新（mpv-Bingshan 包、shinchiro/zhongfly build 源）
- [ ] `core::scripts` — lua 脚本（uosc 系）安装 / 启停 / 更新 / 依赖脚本扫描
- [ ] `core::config` — mpv.conf / input.conf / 脚本 conf 解析、可视化编辑、备份回滚
- [ ] `core::assoc` — 视频文件关联注册（Windows 注册表 / 默认程序）
- [ ] `ui` — 主界面：侧栏导航 + 播放器列表 + 脚本管理 + 配置编辑器
- [ ] `update` — 自更新与播放器版本源检查

## 同级项目

- `../mpv-Bingshan` —— MPV 整合包制作仓库（包体打包/分发），Neve 负责其包的安装与管理
