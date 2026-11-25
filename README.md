# Aerlyn Wire

Aerlyn Wire 是面向远程桌面与设备运维场景的直连二进制会话协议核心。它只负责线格式、状态机与纯逻辑算法，不持有 socket、线程、编码器或操作系统句柄。

- 传输适配：实现 `MessageIo`（`send` / `recv` / `recv_control` / `recv_timeout` / `flush` / `close`）。
- 八个平面：control / input / video / file / cursor / audio / feedback / clipboard。
- 能力：2-RTT 握手、恢复票、编码协商、36 B 媒体头、可靠流与 QUIC DATAGRAM 分片、XOR FEC、文件 credit/位图续传、剪贴板聚合预算、音频抖动缓冲/时钟漂移、RTT 与自适应流权重。

## 文档

完整目录见 [文档索引](docs/README.md)。

- 接入：[接入指南](docs/guide/README.md)
- 规范：[协议规范 1.1.0](docs/architecture/protocol.md)、[分层与传输边界](docs/architecture/layering.md)
- 架构：[架构总览](docs/architecture/README.md)、[需求条款](docs/architecture/requirements.md)
- 审计：[实现状态](docs/architecture/implementation-status.md)、[验收矩阵](docs/architecture/acceptance-matrix.md)、[漂移审计](docs/architecture/drift-audit.md)
- 设计：[设计原则](docs/design/README.md)、[版本策略](docs/design/versioning.md)、[V4 设计档案](docs/design/protocol-v4-archive.md)
- 开发：[环境与构建](docs/development/README.md)、[测试](docs/development/testing.md)、[发布](docs/development/release.md)
- 项目：[Roadmap](docs/roadmap.md)、[已知问题](docs/known-issues.md)

## 构建

```bash
cargo test
cargo doc --no-deps
```

MSRV：Rust 1.85。

## 版本

当前库版本 **1.1.0**；线协议 `proto_version = 400`，ALPN `aerlyn/4` / `aerlyn/4-tcp`。
