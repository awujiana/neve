# Névé（粒雪）

> 冰川之源 —— PCL2 式 **MPV 播放器 / 播放器脚本管理器**
> 纯 Rust + GPUI 原生 GPU 渲染

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

- [ ] `core::mpv` — mpv 检测 / 多版本安装切换 / 进程管理
- [ ] `core::scripts` — lua 脚本（uosc 系）安装 / 启停 / 依赖脚本扫描
- [ ] `core::config` — mpv.conf / input.conf 解析、可视化编辑、备份回滚
- [ ] `ui` — 主界面：侧栏导航 + 脚本列表 + 配置编辑器
- [ ] `update` — 自更新与 mpv 版本源（shinchiro/zhongfly build）检查

## 同级项目

- `../mpv-Bingshan` —— 另一条技术线（Tauri 2 / Rust + TS）的同类管理器，二者共享需求规划
