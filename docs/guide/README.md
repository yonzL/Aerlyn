# 接入 Aerlyn Wire

本文给第一次接入的路径：依赖、传输适配、握手、控制面，以及视频/文件/剪贴板/音频的入口。线格式细节见 [协议规范](../architecture/protocol.md)。

## 1. 依赖

```toml
[dependencies]
aerlyn-wire = { git = "ssh://git@github.com/yonzL/Aerlyn.git", tag = "v1.2.1" }
```

本 crate 不提供 socket、线程、编码器、文件系统或 UI。宿主必须提供 `MessageIo`，并按需实现外部 `Transport` 扩展。

## 2. 实现 MessageIo

```rust
use std::io;
use std::time::Duration;
use aerlyn_wire::MessageIo;

struct MyIo { /* socket / stream / test buffer */ }

impl MessageIo for MyIo {
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        // 返回一条完整消息；半条消息必须 UnexpectedEof
        todo!()
    }

    fn recv_control(&mut self) -> io::Result<Option<Vec<u8>>> {
        // 多流实现必须只读控制通道；单流可退化为 recv
        self.recv()
    }

    fn recv_timeout(&mut self, _timeout: Duration)
        -> io::Result<Option<Vec<u8>>> { todo!() }

    fn send(&mut self, _bytes: &[u8]) -> io::Result<()> { todo!() }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
    fn close(&mut self) -> io::Result<()> { Ok(()) }
}
```

关键点：

- `send` 一次一条完整消息，适配器自己加长度前缀。
- `recv` 一次一条完整消息，不能把两个消息合并返回。
- `recv_control` 必须隔离控制通道；否则视频/文件 body 可能被控制解析器误读。
- `recv_timeout` 超时返回 `WouldBlock`，正常关闭返回 `Ok(None)`。
- `flush` 要在返回前排空已提交写入。

## 3. 握手

客户端：

```rust
use aerlyn_wire::{ClientConfig, client_handshake};

let cfg = ClientConfig {
    version: 400,
    public_key: vec![/* 32 B */],
    caps: 0,
    caps_required: 0,
    codec_caps: vec![/* 支持的编码 */],
    actor_host: "workstation".into(),
    expected_host_fp: None,
};
let mut io = MyIo { /* ... */ };
let session = client_handshake(&mut io, &cfg, &signer)?;
```

服务端：

```rust
use aerlyn_wire::{ServerConfig, server_handshake};

let cfg = ServerConfig {
    version: 400,
    host_fp: vec![/* 32 B */],
    nonce: vec![/* 32 B */],
    caps: 0,
    session_id: 1,
    token: vec![/* channel token */],
    resume_ticket: None,
};
let session = server_handshake(&mut io, &cfg, &verifier)?;
```

- 2-RTT 流程：`Hello → Challenge → Auth → Welcome`。
- `Signer` / `Verifier` 由宿主实现；本 crate 只定义 trait。
- `Session` 保存版本、caps、session id、token、host fingerprint 与恢复票。
- 恢复：`client_resume` / `server_resume`；票只允许在 TLS 后首消息使用。

## 4. 控制面

```rust
use aerlyn_wire::channel::{write_control_msg, read_control_msg};
use aerlyn_wire::{ControlMessage, Ping};

write_control_msg(&mut io, &ControlMessage::Ping(Ping {
    seq: 1,
    ts_ms: 0,
}))?;
let msg = read_control_msg(&mut io)?;
```

- 控制消息是 ch0 JSON，上限 1 MiB；握手阶段 128 KiB。
- `ControlMessage` 有 53 个 typed 变体与 `Unknown`；未知 type 保留原始 JSON。
- 用 `SessionMachine` 处理方向、状态、视频生命周期、文件任务、剪贴板序号与 Ping/Pong；不要在各处手写方向表。

## 5. 视频

1. 交换 `VideoConfig` / `VideoReady`，用 `select_codec` 选编码。
2. 每个编码帧构造 36 B `MediaHeader`；矩形表最多 27 项。
3. 可靠流：`fragment_video(&header, &payload)` 切成最多 64 片，逐片 `write_frame`。
4. 接收：`VideoReassembler::push`；缺片用 `SliceRecovery` 生成 `SliceNack`，20 ms 未补齐发 `RequestKeyframe`。
5. 低延迟路径：`video_datagram` 的 1100 B DATAGRAM 分片；可选 XOR FEC（组大小 16）。DATAGRAM 路径不保证送达，丢失由关键帧恢复。

视频编码器、色彩转换、present 不在本 crate。

## 6. 文件

文件平面是双向的：

```text
upload:   FileOffer → FileAccept → FILE_CHUNK(C→H) → FILE_END(C→H) → FileDone(H→C)
download: FileGet   → FileMeta   → FILE_CHUNK(H→C) → FILE_END(H→C) → FileDone(C→H)
```

- `FileOffer.credit` 携带接收端窗口大小；发送端按 `credit × block_size` 控制在途。
- `FileWindowConfig::adaptive(size, rtt_ms, kbps)` 按 BDP 选块大小/credit。
- `FileSendCursor`/`FileRecvBuffer` 负责窗口、位图与续传；压缩、SHA-256、落盘与原子改名由宿主执行。
- 上限：8 GiB、8192 块、单块 4 MiB；下载/上传产品驱动不在本 crate。

## 7. 剪贴板与音频

- 剪贴板：`ClipOffer` / `ClipData` / `ClipAck`；单项 ≤16 MiB，聚合 ≤64 MiB；格式枚举是封闭集合。
- 音频：`AudioPacket` + `JitterBuffer` + `AudioRed`；目标 20–200 ms，支持时钟漂移修正。Opus 编解码与播放由宿主完成。

## 8. 错误与限制

| 层 | 类型 |
|---|---|
| 传输 | `std::io::Error`（`UnexpectedEof` / `WouldBlock`） |
| 握手 | `HandshakeError` |
| 消息 | `MessageError` |
| 分片 | `FragmentError` |
| 文件 | `TransferError` |
| 音频 | `AudioError` |

常见上限：control 1 MiB、握手 128 KiB、单帧 16 MiB、媒体头 36 B、可靠分片 64、DATAGRAM 1024 片、文件 8 GiB、剪贴板 64 MiB。

## 9. 继续阅读

- [协议规范](../architecture/protocol.md)
- [分层与传输边界](../architecture/layering.md)
- [需求规格](../architecture/requirements.md)
- [已知问题](../known-issues.md)
