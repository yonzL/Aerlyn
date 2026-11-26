# 架构总览

Aerlyn Wire 是一个**纯协议核心**：它定义消息、状态机、分片、窗口、协商与纯算法，不拥有 socket、线程、编码器、文件系统或操作系统句柄。传输适配与产品编排由外部 crate 提供。

## 分层

```text
L3  host / client / product
      │  使用 SessionMachine、消息编码、窗口算法
L2  transport adapter
      │  实现 MessageIo（及可选的 Transport 扩展）
L1  protocol engine
      │  SessionMachine、握手、control 状态、分片/重组
L0  wire types
         channel framing、JSON control、二进制头、TLV、常量
```

`aerlyn-wire` 只覆盖 L0/L1。`opsdesk-net` 是它的 L2 参考实现；`opsdesk-service` / `opsdesk-client-core` 是 L3 参考宿主。

## 模块地图

| 模块 | 职责 | 关键入口 |
|---|---|---|
| `transport.rs` | 传输 SPI | `MessageIo` |
| `channel.rs` | ch0/分通道帧格式 | `read_control_msg`、`write_frame`、`read_incoming` |
| `handshake.rs` | 2-RTT 握手、签名、恢复票 | `client_handshake`、`server_handshake`、`client_resume` |
| `message.rs` / `control.rs` | control 消息注册与负载 | `ControlMessage`、`CONTROL_MESSAGE_TYPES` |
| `machine.rs` | 会话状态机 | `SessionMachine::handle` |
| `codec.rs` / `capabilities.rs` / `caps.rs` | 编码能力与选择 | `select_codec`、`CodecCap` |
| `media.rs` / `fragment.rs` | 36 B 媒体头、可靠流分片/重组 | `MediaHeader`、`fragment_video`、`VideoReassembler` |
| `video_datagram.rs` | 视频 DATAGRAM 与 XOR FEC | `fragment_video_fec`、`VideoDatagramReassembler` |
| `transfer.rs` | 文件/剪贴板窗口、位图、credit | `FileWindowConfig`、`FileSendCursor`、`FileRecvBuffer` |
| `audio.rs` | 抖动缓冲、RED、时钟漂移 | `JitterBuffer`、`AudioRed` |
| `timing.rs` / `timeouts.rs` | RTT、时钟基线、超时阶段 | `RttEstimator`、`TimeoutPolicy` |
| `tickets.rs` | 恢复票登记/原子换票 | `TicketStore` |

## 数据流

```text
peer bytes
  → MessageIo::recv / recv_control
  → channel framing
  → ControlMessage (JSON) 或 binary frame
  → SessionMachine::handle
  → 状态/应答/业务事件
```

握手阶段只通过控制通道交换 `Hello`/`Challenge`/`Auth`/`Welcome`。多流适配器必须保证 `recv_control` 只读取控制通道，不能把视频/文件 body 交给控制解析器。

## 关键不变量

- 一条 `send` 对应一条完整消息；调用方不得二次分帧。
- control 消息上限 1 MiB；握手上限 128 KiB；单帧上限 16 MiB。
- 媒体头固定 36 B，矩形表最多 27 项。
- 可靠流分片最多 64 片；视频 DATAGRAM 最多 1024 片，FEC 组大小 16。
- 文件上限 8 GiB、8192 块；剪贴板单项 16 MiB、聚合 64 MiB。
- 错误码只增不改，`52` 为保留空洞。

## 相关页面

- [协议规范 1.2.0](protocol.md)
- [分层与传输边界](layering.md)
- [需求规格](requirements.md)
- [实现状态（历史快照）](implementation-status.md)
- [验收矩阵（历史快照）](acceptance-matrix.md)
- [漂移审计（历史快照）](drift-audit.md)
