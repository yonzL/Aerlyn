# 分层与传输边界

本文说明 `aerlyn-wire` 与传输适配器、宿主应用之间的契约。它不是线格式规范；线格式见 [protocol.md](protocol.md)。

## 四层模型

| 层 | 位置 | 负责 | 不负责 |
|---|---|---|---|
| L0 wire | `src/channel.rs`、`src/message.rs`、`src/control.rs`、`src/plane.rs` | 长度前缀、JSON control、二进制头、TLV、常量 | socket、线程、重试 |
| L1 engine | `src/handshake.rs`、`src/machine.rs`、`src/fragment.rs`、`src/transfer.rs`、`src/audio.rs` | 握手、状态机、分片/重组、窗口、纯算法 | I/O 阻塞、编码器、文件系统 |
| L2 transport | 外部适配器（参考：`opsdesk-net`） | `MessageIo`、流/通道映射、DATAGRAM、异步写泵、TCP 降级 | 消息语义、状态机 |
| L3 host | 外部产品（参考：`opsdesk-service` / `opsdesk-client-core`） | 会话编排、编码器、文件 IO、压缩、UI | 线格式重定义 |

## MessageIo 契约

```rust
pub trait MessageIo: Send {
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>>;
    fn recv_control(&mut self) -> io::Result<Option<Vec<u8>>>;
    fn recv_timeout(&mut self, timeout: Duration)
        -> io::Result<Option<Vec<u8>>>;
    fn send(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
    fn close(&mut self) -> io::Result<()>;
}
```

语义：

- `send` 一次提交一条完整消息；字节流实现自行加长度前缀。
- `recv` 一次返回一条完整消息；半条消息必须返回 `UnexpectedEof`，不得静默拼接。
- `recv_control` 只能读取控制通道；单流实现可默认退化为 `recv`。多流实现必须覆盖，否则视频/文件 body 可能被控制解析器误读。
- `recv_timeout` 超时返回 `WouldBlock`；正常关闭返回 `Ok(None)`。
- `flush` 等待已提交的写入真正落地；没有异步写的实现可返回 `Ok(())`。
- `close` 幂等；关闭后 `send` 应返回错误。

## 可选 Transport 扩展

异步视频、通道提示、stream 优先级与 QUIC DATAGRAM 不属于 `aerlyn-wire` 的核心 trait。参考适配器在外部定义：

```rust
pub trait Transport: MessageIo {
    fn recv_channel(&mut self) -> io::Result<Option<(u8, Vec<u8>)>>;
    fn last_channel(&self) -> Option<u8>;
    fn supports_async_video(&self) -> bool;
    fn send_video_batch(&mut self, frames: Vec<Vec<u8>>) -> io::Result<()>;
    fn is_tcp(&self) -> bool;
    fn set_stream_priorities(&mut self, p: [i32; 8]) -> io::Result<()>;
    fn stream_priorities(&self) -> Option<[i32; 8]>;
    fn send_datagram(&mut self, data: &[u8]) -> io::Result<()>;
    fn recv_datagram(&mut self) -> io::Result<Option<Vec<u8>>>;
    fn max_datagram_size(&self) -> usize;
}
```

`aerlyn-wire` 只依赖 `MessageIo`；它不知道 TCP、异步 writer、优先级或 DATAGRAM 的存在。

## 适配器必须保证

1. **控制/数据隔离**：`recv_control` 不返回 ch2/ch3 等数据 body。
2. **消息边界**：按协议长度前缀重组；短读/半包不得当成完整消息。
3. **背压/关闭**：`flush` 在返回前排空已提交数据；`close` 先停读、再停写、最后关闭底层连接。
4. **错误映射**：底层错误映射为 `io::Error`；不要吞掉 `UnexpectedEof`。
5. **不重写语义**：分片、窗口、credit、状态转换由协议引擎决定，传输层不得自行改变。

## 常见反模式

- 让 `recv()` 返回任意通道的 body，然后在握手处“猜”当前是哪一帧。
- 在传输层重新实现 `SessionMachine` 的方向表。
- 把编码器/文件 SHA/压缩放进 `aerlyn-wire`。
- 把 `MessageIo` 的 `send` 当成“字节流写入”，调用方再切半条消息。

## 参考实现边界

- `opsdesk-net`：QUIC/TCP、通道映射、异步视频泵、`Transport`。
- `opsdesk-service` / `opsdesk-client-core`：host/client 编排、文件流、编码器、音频播放。
- standalone `aerlyn` 不包含上述产品层；只包含本 crate 与文档。
