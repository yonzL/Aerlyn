# Aerlyn Wire

Aerlyn Wire 是面向远程桌面与设备运维场景的直连二进制会话协议核心。它只负责线格式、状态机与纯逻辑算法，不持有 socket、线程、编码器或操作系统句柄。

- 传输适配：实现 `MessageIo`（`send` / `recv` / `recv_control` / `recv_timeout` / `flush` / `close`）。
- 八个平面：control / input / video / file / cursor / audio / feedback / clipboard。
- 能力：2-RTT 握手、恢复票、编码协商、36 B 媒体头、可靠流与 QUIC DATAGRAM 分片、XOR FEC、文件 credit/位图续传、剪贴板聚合预算、音频抖动缓冲/时钟漂移、RTT 与自适应流权重。

## 文档

- [协议规范 1.0.0](docs/protocol.md) —— 线格式、状态机、常量与不变量的唯一权威。
- [需求条款](docs/65-aerlyn-wire-requirements-from-code.md) —— 从实现反推的需求 ID 与验收证据。
- [实现状态](docs/51-aerlyn-wire-implementation-status.md) 与 [漂移审计](docs/52-protocol-drift-audit.md) —— 历史审计档案。
- [设计档案](docs/45-protocol-spec-v4.md) —— 1.0.0 冻结前的设计与决策记录。

## 构建

```bash
cargo test
cargo doc --no-deps
```

MSRV：Rust 1.85。

## 版本

当前库版本 **1.0.0**；线协议 `proto_version = 400`，ALPN `aerlyn/4` / `aerlyn/4-tcp`。
