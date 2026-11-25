# Roadmap

当前版本：**1.1.0**（wire `400`）。本页只列有明确状态的事项，不承诺日期。

## 已完成

- 1.0.0：协议核心、握手/恢复、六平面、文件/剪贴板/音频、验收矩阵冻结。
- 1.1.0：文件平面拆 upload/download；`FileDone`/`FileError` 双向；`MAX_VIDEO_FRAGMENTS` 与 `MAX_FRAGMENTS` 分名。

## 维护中

- 文档结构与索引维护。
- 传输适配器契约（`MessageIo` / `Transport`）的边界说明。
- 历史审计页与当前规范的区分。

## 未排期

- 2.0.0 版本协商；只有出现真实的跨大版本共存需求时才启动。
- standalone 内的 client/host 示例；当前边界是只发布 wire。
- 编解码器、文件系统、压缩、SHA-256 的产品级实现；这些属于宿主。

## 相关页面

- [版本策略](design/versioning.md)
- [已知问题](known-issues.md)
- [实现状态](architecture/implementation-status.md)
