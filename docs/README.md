# Aerlyn Wire 文档

Aerlyn Wire 是面向远程桌面与设备运维场景的直连二进制会话协议核心。本目录面向三类读者：

- **接入方**：实现 `MessageIo`，建立会话，收发控制面与媒体/文件数据。
- **维护者**：修改线格式、状态机、常量或测试。
- **审计者**：核对实现、验收矩阵与历史设计记录。

## 先读哪一篇

| 目标 | 文档 |
|---|---|
| 第一次接入 | [接入指南](guide/README.md) |
| 查线格式/状态机/常量 | [协议规范 1.2.0](architecture/protocol.md) |
| 理解 crate 边界 | [架构总览](architecture/README.md)、[分层与传输边界](architecture/layering.md) |
| 查需求与验收 | [需求规格](architecture/requirements.md)、[验收矩阵（历史快照）](architecture/acceptance-matrix.md) |
| 查版本与发布规则 | [版本与发布](development/release.md)、[版本策略](design/versioning.md) |
| 查历史设计 | [V4 设计档案（1.0 前）](design/protocol-v4-archive.md) |

## 使用指南

- [接入 Aerlyn Wire](guide/README.md)：依赖、`MessageIo`、握手、控制面、视频、文件、剪贴板、音频与错误处理。

## 开发指南

- [开发环境与构建](development/README.md)
- [测试与验证](development/testing.md)
- [版本与发布](development/release.md)

## 架构

- [架构总览](architecture/README.md)
- [协议规范 1.1.0](architecture/protocol.md)
- [分层与传输边界](architecture/layering.md)
- [需求规格（从代码反推）](architecture/requirements.md)
- [实现状态（历史快照）](architecture/implementation-status.md)
- [验收矩阵（历史快照）](architecture/acceptance-matrix.md)
- [漂移审计（历史快照）](architecture/drift-audit.md)

## 设计

- [设计原则与取舍](design/README.md)
- [版本策略](design/versioning.md)
- [协议 1.0 设计档案](design/protocol-v4-archive.md)

## 项目状态

- [路线图](roadmap.md)
- [已知问题](known-issues.md)

## 权威性

`architecture/protocol.md` 是线格式、状态机、常量与不变量的唯一权威。`implementation-status.md`、`drift-audit.md`、`acceptance-matrix.md` 和 `design/protocol-v4-archive.md` 是不同时间点的审计/设计快照；它们保留历史结论，不自动代表当前行为。代码与协议规范冲突时，以测试和规范条目共同核对，不以历史快照覆盖当前实现。
