//! core::config —— MPV 配置文件解析、冲突检测（FR-11）
//!
//! 设计目标：**无损解析**。保留原始行序、注释与空行，只有真正的配置行被结构化，
//! 渲染时可字节级还原（除行尾风格统一外），满足"已捕获的原始内容仍可恢复"。

pub mod conf;
pub mod conflicts;
pub mod document;
pub mod input;
pub mod report;
