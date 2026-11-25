---
status: draft
scope: aerlyn-wire
basis: 1.0.0 冻结线（tag v1.0.0）；1.1.0 只增量补充双向文件流，见 protocol.md §8
method: 从 public API、类型不变量、编码器/解码器校验和测试反推；不以旧规格中的计划项为依据
---

# Aerlyn Wire 需求规格（代码反推）

> 配套使用说明：[接入指南](../guide/README.md)。本文回答“协议必须满足什么”，接入指南回答“怎么接、怎么调”。

## 1. 这是什么

`aerlyn-wire` 是一个**不持有 socket、不启动线程、不依赖编码器/OS** 的远程桌面协议引擎：

- 定义消息边界、通道分帧、握手、控制消息、媒体头、分片、文件/剪贴板/音频窗口；
- 提供纯逻辑状态机与校验器；
- 通过 `MessageIo` 与真实传输解耦；
- 真实 QUIC/TCP、异步泵、UI、编码器和文件 I/O 在 `opsdesk-*`。

因此“协议库完成”只表示 L0/L1 的行为可被独立验证，不等于产品端到端完成。

### 1.1 受众

| 受众 | 需要什么 |
|---|---|
| 传输实现者 | 实现 `MessageIo`，保证一次一条完整消息 |
| host/client 实现者 | 使用握手、控制消息、媒体/文件窗口和状态机 |
| 审计者 | 用本文件的 ID、证据和验收命令复核 |
| 第二个实现者 | 只依赖公开类型与线格式，不依赖 `opsdesk-*` |

### 1.2 边界

**属于本 crate**

- 线格式与消息注册表；
- 握手转录、签名输入、恢复票校验；
- `SessionMachine` 的方向/状态约束；
- 编码协商、媒体头、可靠分片、DATAGRAM 分片/FEC；
- 文件/剪贴板/音频窗口、位图、RED、抖动缓冲；
- 错误码、超时、优先级等纯策略类型。

**不属于本 crate**

- TLS/QUIC/TCP、连接建立、证书校验；
- 真实编码/解码、像素转换、压缩、SHA-256 计算；
- 文件系统、剪贴板、音频设备、输入注入；
- UI、会话窗口、重连循环；
- 版本协商（0.6.x 线只有单值 `version`；不做区间协商，2.0.0 才设计协商）。

## 2. 不变量

| ID | 不变量 |
|---|---|
| INV-01 | `MessageIo::send` 一次提交一条完整消息；`recv` 一次返回一条完整消息；半条消息必须 `UnexpectedEof`。 |
| INV-02 | ch0 是 JSON 控制消息，其它通道是二进制帧；首字节为 0 只可能是 control。 |
| INV-03 | 所有多字节整数使用网络序；媒体头为 36 B 固定头 + 8 B/矩形。 |
| INV-04 | 未知 control 类型保留原始 JSON；未知二进制 type 由上层计数/忽略，不使解码器崩溃。 |
| INV-05 | 任何长度/数量/索引都必须先校验再分配；不得用对端长度直接预分配无界内存。 |
| INV-06 | 协议层不做隐式降级；降级必须由显式错误码或显式 RAW/FEC 策略表达。 |

## 3. 需求

### 3.1 传输与分帧

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-001 | `aerlyn-wire` 只依赖 `MessageIo` 的 6 个动作：`recv`、`recv_control`（默认退化 `recv`，多流实现只取 ch0）、`recv_timeout`、`send`、`flush`、`close`；不依赖 socket/async 运行时。 | `src/transport.rs`；`tools/check_layering.sh` | `LAYERING_OK`；`MessageIo` 可在 `MemoryConnection`/`ChoppyConnection` 上实现 |
| AW-REQ-002 | `recv_timeout` 超时返回 `WouldBlock`，正常关闭返回 `Ok(None)`；默认实现可返回 `Unsupported`。 | `src/transport.rs` | `memory::tests`、`channel` timeout 测试 |
| AW-REQ-003 | ch0 使用 `[u32 len][JSON]`；长度必须等于实际缓冲减 4，空消息/超限/尾随都拒绝。 | `src/channel.rs::read_control_bytes_limit` | control 长度越界测试 |
| AW-REQ-004 | 非 ch0 帧头固定 9 B：`u8 channel`、`u16 type`、`u16 flags`、`u32 len`，后接 payload；单帧 ≤16 MiB。 | `src/channel.rs::encode_frame`/`parse_frame` | `channel` 单测；分帧 property 测试 |
| AW-REQ-005 | 控制消息普通上限 1 MiB；未鉴权握手阶段单条 control ≤128 KiB，必须在 JSON 解析前拒绝。 | `MAX_CONTROL_LEN`、`MAX_HANDSHAKE_CONTROL_LEN` | `unauthenticated_oversized_hello_is_rejected_before_parse` |
| AW-REQ-006 | 通道号固定 0–7，语义为 control/input/video/file/cursor/audio/feedback/clipboard；未知通道拒绝。 | `src/channel.rs` 常量；`src/plane.rs` | 通道路由/拒绝测试 |

### 3.2 握手、身份与恢复

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-010 | 0.6.x 使用单值 `version`，双方必须相等；不做版本区间协商。 | `ClientConfig.version`、`ServerConfig.version`、`handshake` | 版本不等返回 `VersionMismatch` |
| AW-REQ-011 | 握手为 Hello →（Challenge+Welcome）→ Auth；Challenge/Welcome 可同航班发出，Auth 通过后才接受业务消息。 | `src/handshake.rs:229-317` | 握手/签名失败测试 |
| AW-REQ-012 | `nonce` 与 `host_fp` 必须各 32 B；`caps_required` 必须是 host `caps` 的子集。 | `require_32`、`server_handshake` | 32 B/required 缺失测试 |
| AW-REQ-013 | 签名转录固定为 `nonce(32) ‖ u16be alpn_major ‖ u16be version ‖ host_fp(32) ‖ u64be caps ‖ u64be caps_required ‖ sha256(caps_ext) ‖ public_key`。 | `src/handshake.rs::transcript` | `transcript_covers_alpn_caps_required_and_ext_hash` |
| AW-REQ-014 | 签名/验签通过注入的 `Signer`/`Verifier` 完成；协议 crate 不绑定 ed25519 或 TLS 实现。 | `src/handshake.rs:17-23` | Fake/Echo signer 测试 |
| AW-REQ-015 | `expected_host_fp` 存在时必须与 Challenge 的 `host_fp` 相等；不等发 `Bye` 并失败。 | `client_handshake` | `FingerprintMismatch` 测试 |
| AW-REQ-016 | `Hello.caps_ext`/`Welcome.extensions` 必须携带非空 TLV：`wire_version=1`、`caps=2`；转录哈希覆盖实际字节。 | `handshake_extensions`、`EXT_WIRE_VERSION`/`EXT_CAPS` | `handshake_extensions_are_non_empty_and_roundtrip` |
| AW-REQ-017 | 恢复票有 TTL 且只能原子赎回一次；旧票重放失败；换票失败不消耗旧票。 | `TicketStore::{issue,register,redeem,prune}` | `tickets.rs` 5 个单测 |
| AW-REQ-018 | `Resume` 只允许在 TLS 后首消息；失败回 `ResumeResult{ok:false}`，调用方可改走完整 Hello。 | `client_resume`、`server_resume_with_validator` | 恢复/拒绝测试 |
| AW-REQ-019 | `ChannelHello` 的 token 有 60 s TTL，同一 channel 幂等回 `ChannelWelcome`；错误 token/TTL 回 `Rejected{TokenInvalid=13}`。 | `timeouts.rs`、`machine.rs` | `channel_token_expires_after_60_seconds` |

### 3.3 控制消息

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-020 | `ControlMessage` 有 53 个已结构化类型；未知 type 保留 `Unknown{type_name,raw}`，不得反序列化失败。 | `src/message.rs::ControlMessage`/`control_dispatch!` | control registry 往返/未知类型测试 |
| AW-REQ-021 | JSON 使用 `type` 字段分派；`to_json` 生成同一 tag；未知类型原样回写。 | `message.rs` | `full_control_registry_roundtrip` |
| AW-REQ-022 | control 类型表、`type_name`、分派由同一张表生成；新增类型不得只改一处。 | `control_dispatch!` | `control_registry_matches_spec` |
| AW-REQ-023 | 状态机必须按方向表拒绝错误方向；`Unknown` 在状态机中静默忽略，不得 panic。 | `src/machine.rs`、`direction_allowed` | `machine` 方向/Unknown 测试 |

### 3.4 编码协商与视频

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-030 | 编码 id 固定：H264=0、H265=1、VP8=2、VP9=3、AV1=4、RAW=5；未知 id 保留在媒体头但解码器拒绝。 | `src/codec.rs::Codec`、`MediaHeader::decode_any` | codec registry/媒体头未知 codec 测试 |
| AW-REQ-031 | 客户端按 `codec_pref` 顺序选择；空列表用默认 `AV1 > H265 > VP9 > H264 > VP8 > RAW`；五种性能编码都不行时，双方有 RAW 才回落 RAW。 | `select_codec` | `default_prefers_av1`、`client_order_wins`、RAW 回落测试 |
| AW-REQ-032 | 协商必须同时满足 profile、chroma、bit depth、分辨率、fps、码率约束；profile 取交集最高位，chroma 优先 I420→I444→I422→BGRA，位深 8→10→12。 | `select_from_pair`/`choose_chroma`/`choose_bit_depth` | 约束矩阵测试 |
| AW-REQ-033 | 媒体头 36 B：`fmt=1`、flags、codec_id、hdr_len、frame_id、request_id、frag_idx/count、pts_us、capture_ms、encode_ms、rect_count、reserved；`hdr_len=36+8N`，N≤27。 | `src/media.rs` | `MediaHeader` 编解码/rect 上限测试 |
| AW-REQ-034 | 媒体 flags 定义 KEY/CURSOR_EXCLUDED/LAST_FRAG/INTRA_REFRESH/REGION 与 3-bit layer；未知 codec 可用 `decode_any` 保留。 | `frame_flags`、`MediaHeader::layer` | flags/layer/rect 测试 |
| AW-REQ-035 | 可靠视频单帧 ≤16 MiB，按 256 KiB 切片，最多 64 片；`fragment_video` 覆盖 frag_idx/count 并设置 LAST_FRAG。 | `src/fragment.rs` | `fragment`/`VideoReassembler` 测试 |
| AW-REQ-036 | 重组器同时跟踪 ≤8 个未完成帧，允许跨帧乱序，重复片幂等，元数据冲突拒绝；`missing_words_for` 支持大帧缺片。 | `VideoReassembler`、`MAX_PENDING_FRAMES` | `tracks_multiple_frames_concurrently`、property 测试 |
| AW-REQ-037 | 可靠流缺片后 20 ms 未补齐发一次 `RequestKeyframe`；`SliceNack` 固定 12 B + 0–16 个 8 B 扩展字，总长 ≤132 B，旧 12 B 兼容。 | `SliceRecovery`、`plane::SliceNack` | `recovery_nacks_then_requests_keyframe_after_20ms`、扩展 NACK 测试 |
| AW-REQ-038 | DATAGRAM 视频使用 20 B 头、1100 B MTU、1080 B 载荷、最多 1024 片；支持 KEY/PARITY 标志与每组 16 片的 XOR FEC（每组恢复 1 丢片）。 | `src/video_datagram.rs` | 大帧 NACK、FEC 恢复测试 |

### 3.5 文件与剪贴板

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-040 | 文件 ≤8 GiB、≤8192 块；块 64 KiB–4 MiB、credit≥1；默认 1 MiB×8；自适应按 BDP 计算块/credit。 | `FileWindowConfig`、`adaptive` | `adaptive_file_window_scales_with_bdp` |
| AW-REQ-041 | `FileOffer.credit` 是窗口契约；接收端必须用它建窗口并继续在 ACK 广告同一 credit，不能固定 8。 | `validate_file_offer`、`FileRecvWindow::ack` | `file_offer_carries_credit_into_window`、`ack_keeps_receiver_credit` |
| AW-REQ-042 | 发送窗口只允许 `peer_credit-in_flight` 个块；ACK 推进 `acked_upto`；跨边界/超尾/空洞/重复分别处理。 | `FileSendWindow`/`FileRecvWindow` | transfer 单测 |
| AW-REQ-043 | 位图 LSB-first，尾位必须为 0；支持连续前缀与稀疏块；`count`/`is_full` 为 O(1)，`first_missing` 按字节扫描。 | `FileBitmap` | 位图/边界测试 |
| AW-REQ-044 | 文件结束用 `FileEnd.sha256`（32 B）；zstd 块用 12 B 头 + 压缩体，压缩体与未压缩长度均受 4 MiB 限制。 | `plane::FileEnd`/`FileChunkZstd`、`payload` | 文件流/A-B 测试 |
| AW-REQ-045 | 剪贴板格式固定 0=TEXT、1=RTF、2=HTML、3=PNG、4=DIB；单项 1 B–16 MiB，聚合 ≤64 MiB；未知格式拒绝。 | `control::clip_format`、`ClipItem::validate`、`ClipOffer::validate` | `clipboard_format_domain_is_explicit`、聚合预算测试 |
| AW-REQ-046 | 剪贴板块 256 KiB，`ClipDataHeader` 10 B；发送/接收游标按 seq/offset/credit 推进。 | `ClipSendCursor`/`ClipRecvBuffer` | clipboard roundtrip 测试 |

### 3.6 音频、反馈与策略

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-050 | 音频 48 kHz、10 ms/帧；抖动目标 20–200 ms，默认 50 ms；RED 最多 3 包、≤64 KiB；空闲 5 s 可重锚；慢漂移用 `clock_skew_ppm` + 最小二乘估计，target 扣除 skew（见 051）。 | `src/audio.rs` | audio 7 个测试（含 10 分钟 p95） |
| AW-REQ-051 | 音频慢漂移用到达时间对 RTP pts 的斜率估计，target 扣除 skew；斜率/ppm 不替代重锚。 | `clock_skew_ppm`、最小二乘、corrected delay | `clock_skew_is_estimated_and_removed_from_target` |
| AW-REQ-052 | `FrameAck` 线上固定 20 B（累计 ack_frame_id、recv_ts_us、decode_done_us、queue_depth、dropped），不是 JSON。 | `FrameAck::{encode,decode}` | FrameAck roundtrip |
| AW-REQ-053 | 反馈面含 `NetReport`、`SliceNack`、`RequestKeyframe`、`HostStats`；`PlanePriorities` 请求/ack 双向，默认权重 `[100,90,50,10,60,40,30,20]`。 | `plane.rs`、`control.rs`、`machine.rs` | priorities ack、net report 测试 |
| AW-REQ-054 | 7 段超时固定：连接 3 s、TLS 3 s、Hello→Challenge 2 s、Challenge→Auth 2 s、Auth→Welcome 3 s、ChannelHello 1 s、普通 control 5 s；token TTL 60 s；恢复票 300 s。 | `src/timeouts.rs`、`control.rs` | timeout 单测 |

### 3.7 错误与安全

| ID | 需求 | 代码证据 | 验收 |
|---|---|---|---|
| AW-REQ-060 | 错误码只增不改；码段 0–29 通用/会话、30–49 文件、50–69 视频/输入、70–89 剪贴板/音频；52 保留。 | `src/error.rs` | `published_error_codes_match_spec` |
| AW-REQ-061 | 错误必须带 class/need/retry_after_ms 语义；可由 `ErrorInfo` 平铺为 `Error` control 消息。 | `ErrorInfo`、`ErrorReply` | error reply 往返 |
| AW-REQ-062 | 协议层不替代 TLS；签名只证明持有 `public_key`，host 指纹可选固定；未鉴权消息有 128 KiB 上限。 | handshake、128 KiB 读限 | 签名/证书/上限测试 |
| AW-REQ-063 | 未知扩展 tag 跳过并计数；已知 tag 重复/超限/TLV 尾随拒绝。 | `Extensions` | TLV 测试 |

## 4. 非目标

- 不实现 QUIC/TCP 握手、证书校验、重连循环；
- 不实现编码器/解码器、像素格式转换、FEC 之外的信道编码；
- 不实现文件系统、压缩、SHA-256 计算、原子 rename；
- 不实现 UI、剪贴板/音频设备、输入注入；
- 不把 `opsdesk-service` 的参考编排当成协议强制行为；
- 不在 0.5.0 线做版本协商或跨大版本兼容。

## 5. 追溯

| 层 | 代码入口 |
|---|---|
| 传输边界 | `src/transport.rs`、`src/channel.rs`、`src/memory.rs` |
| 握手/恢复 | `src/handshake.rs`、`src/tickets.rs`、`src/timeouts.rs` |
| 控制/状态机 | `src/message.rs`、`src/control.rs`、`src/machine.rs` |
| 视频 | `src/codec.rs`、`src/media.rs`、`src/fragment.rs`、`src/video_datagram.rs` |
| 文件/剪贴板 | `src/transfer.rs`、`src/control.rs`、`src/payload.rs` |
| 音频/反馈 | `src/audio.rs`、`src/plane.rs`、`src/timing.rs` |

验收命令：

```bash
cargo test -p aerlyn-wire --offline
cargo test -p aerlyn-wire -p opsdesk-net -p opsdesk-service \
  -p opsdesk-client-core -p opsdesk-lan --offline
bash tools/check_layering.sh
bash tools/p0_audit.sh
```

通过只说明本文件列出的可测条款成立；不代表 1.0.0 冻结或产品端到端完成。
