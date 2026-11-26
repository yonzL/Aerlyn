---
title: Aerlyn Wire 协议规范 1.1.0
version: 1.1.0
wire_proto_version: 400
alpn: aerlyn/4
status: 1.1.0（正式版）
date: 2026-11-21
---

# Aerlyn Wire 协议规范 1.1.0

Aerlyn Wire 是桌面远控/远程桌面场景的二进制会话协议：八个并行平面（control / input / video / file / cursor / audio / feedback / clipboard）共享一条已认证的 QUIC 连接（可降级为 TCP），每个平面独立流、独立优先级、独立背压。本文是 1.1.0 的完整线格式与状态机规范。

**只有本文的数值与布局是规范性的。** 常量名指向 `src/` 中的定义处，便于逐条核对。

---

## 0. 范围、分层与名词

### 0.1 三层职责

| 层 | 内容 | 落点 |
|---|---|---|
| **wire** | 线格式、消息集合与字段、状态机与方向校验、常量与不变量、错误码、超时预算 | `src/` |
| **engine** | 纯逻辑算法：credit 窗口、位图续传、抖动缓冲、DATAGRAM 重组、XOR FEC、优先级策略、RTT/时钟基线 | `src/` |
| **参考实现** | IO 执行（QUIC/TCP 连接、流调度）、编解码器、zstd/SHA-256 计算、屏幕采集与注入、并发模型、指标 | `opsdesk-net` / `opsdesk-service` / `opsdesk-client-core` / `opsdesk-lan` |

规范里出现「执行在参考实现」的条目，描述的是**协议要求的行为**，不是线格式的一部分：私有实现只要满足同样的可观测语义，即视为符合本规范。

### 0.2 名词

| 名词 | 含义 |
|---|---|
| 平面（plane） | 一个逻辑数据面，对应一个 `CH_*` 通道号，多流模式下独占一条 QUIC 双向流 |
| 控制面 | `CH_CONTROL`（ch0），唯一承载 JSON 消息的通道 |
| 消息（message） | 一次 `MessageIo::send` 提交的完整字节串；接收方一次 `recv` 恰好得到一条 |
| 帧（frame） | 平面帧：`9 B 头 + 载荷` |
| 任务（task） | 一次文件/剪贴板传输，用 `task_id` 标识 |
| credit | 接收方授予发送方的「在途块数」额度，单位是块 |
| 票据（ticket） | 会话恢复凭证，一次性 |

---

## 1. 传输契约

### 1.1 ALPN 与协议版本

| 项 | 值 |
|---|---|
| ALPN（QUIC） | `aerlyn/4` |
| ALPN（TCP 降级） | `aerlyn/4-tcp` |
| 线协议主版本 | `proto_version = 400`（与 ALPN 主号 4 绑定，见 `handshake.rs:81` `ALPN_MAJOR = 4`） |

握手消息 `Hello.version` / `Challenge.version` / `Welcome.version` 是 **u16 单值**：双方必须完全相等，不做区间协商。

### 1.2 消息边界契约（`MessageIo`）

协议引擎只依赖下面六个动作（`src/transport.rs:11`）：

| 动作 | 语义 |
|---|---|
| `send(bytes)` | 一次提交**一条完整消息**；实现负责写入与分片，调用方不得二次分帧 |
| `recv()` | 一次返回**一条完整消息**；`Ok(None)` 表示对端正常关闭；半条消息必须报 `UnexpectedEof`，不得静默截断 |
| `recv_control()` | 读取下一条**控制面**消息。多流实现必须只从 control 流读取；单流实现默认退化为 `recv` |
| `recv_timeout(d)` | 带超时读取，超时返回 `WouldBlock`，正常关闭返回 `Ok(None)`；不支持时返回 `Unsupported` |
| `flush()` | 等待此前提交的 `send` 真正写出；默认空实现 |
| `close()` | 关闭传输 |

硬性要求：**多流实现必须实现 `recv_control`**。否则控制面读端会取到视频/文件平面帧，把 9 B 平面头当 `[u32 len]` 解析（典型故障：`bad control frame length`）。仅提供 `recv()` 的实现只允许用于单流模式。

平面级可选能力（同一 trait 家族的 `Transport`）：`recv_channel(channel)` 定点读取某通道、`last_channel()` 报告最近一次读取的通道、`set_stream_priorities()` 应用跨端流权重；不支持时返回 `Unsupported`，调用方必须降级到「应用层排队」而不是失败。

### 1.3 QUIC 基线参数

| 参数 | 值 |
|---|---|
| 双向流上限 | 16 |
| 单向流上限 | 64 |
| keep-alive | 15 s |
| idle timeout | 90 s |
| 单流接收窗口 | 8 MiB |
| 连接接收窗口 | 32 MiB |
| 拥塞控制 | Cubic（默认）；BBR 可经环境变量切换做 A/B |

多流模式（默认）：8 条双向流的第 *i* 条承载 channel *i*。单流兼容模式：全部平面复用同一条流，按平面头里的 channel 字节路由。

连接建立后写平面元数据的方式：发送方在流上先发 1 字节 channel 号（`open_extra`），接收方按该字节把流绑定到平面（`accept_extra`）；channel 号必须 < 8，否则为协议错误。

### 1.4 TCP 降级

TCP 不提供流隔离与 DATAGRAM，因此：

- 视频 DATAGRAM 路径禁用，全部走可靠流切片；
- 单流承载全部平面，按平面头路由；
- 应用三档 QoS（`TcpTier`）：按 RTT 与丢包分档，`RTT ≥ 300 ms` 或 `丢包 ≥ 20‰` → Fair，`RTT ≥ 500 ms` 或 `丢包 ≥ 50‰` → Poor；**向下立刻生效、向上需连续 15 s 无劣化才升档**（步进 3 s，滞回防抖）。
- 每档限制视频帧率/码率与文件块上限：Good 为传输上限（`TCP_MAX_VIDEO_FPS = 30`、`TCP_MAX_VIDEO_KBPS = 8_000`），Fair 为 15 fps / 4 000 kbps / 128 KiB，Poor 为 8 fps / 2 000 kbps / 64 KiB。

### 1.5 分帧总则

| 位置 | 布局 |
|---|---|
| `CH_CONTROL`（ch0） | `[u32 len][JSON UTF-8]`，`len` 为 JSON 字节数，大端 |
| 其余平面 | `[u8 channel][u16 type][u16 flags][u32 len][payload]`，共 9 B 头（`FRAME_HEADER_LEN = 9`） |

上限：`MAX_CONTROL_LEN = 1 MiB`、`MAX_HANDSHAKE_CONTROL_LEN = 128 KiB`（握手期，任何握手消息超过即断）、`MAX_FRAME_LEN = 16 MiB`。超限必须拒绝，不得截断。

---

## 2. 通道与优先级

### 2.1 通道号

| ch | 名称 | 内容 |
|---|---|---|
| 0 | `CH_CONTROL` | JSON 消息（握手、控制、文件/剪贴板协商、优先级、恢复） |
| 1 | `CH_INPUT` | 输入批量（可靠流或 DATAGRAM，同载荷） |
| 2 | `CH_VIDEO` | 视频帧（可靠流切片 + DATAGRAM 并行） |
| 3 | `CH_FILE` | 文件块与 ack |
| 4 | `CH_CURSOR` | 光标形状/位置 |
| 5 | `CH_AUDIO` | 音频帧 |
| 6 | `CH_FEEDBACK` | FrameAck / NetReport / SliceNack |
| 7 | `CH_CLIPBOARD` | 剪贴板数据块与 ack |

### 2.2 默认权重

`[100, 90, 50, 10, 60, 40, 30, 20]`，顺序同 ch0…ch7（control 最高，file 最低）。权重是相对值，实现可映射为 QUIC 流优先级或应用层排队顺序。

### 2.3 优先级协商

`PlanePriorities`（JSON，双向）：

```json
{ "type": "PlanePriorities", "req_id": 7, "priorities": [100,90,50,10,60,40,30,20], "ack": false }
```

- 数组恒为 8 项，索引即通道号；
- 接收方应用后必须回一条**同 `req_id`**、`ack: true` 的消息；`ack: true` 的消息不再被回复；
- 已认证会话即可协商，不要求额外能力位。

### 2.4 自适应优先级（engine）

`AdaptivePriorities` 以默认权重为基线，按各平面积压字节做指数平滑调整：输入/视频/光标积压时上抬，批量平面（file/clipboard）积压下压。输出恒为 8 项合法权重，且不会把 control 降到非最高档。

### 2.5 背压队列容量

| 平面 | 队列容量（条） | 溢出策略 |
|---|---|---|
| control | 16 | 阻塞等待 |
| input | 4 096 | 丢最旧 |
| video | 1 | 覆盖（总是发最新帧） |
| cursor | 4 | 丢最旧 |

---

## 3. 握手（2 RTT）

### 3.1 时序

```
Client                                  Host
  |-- Hello{version,caps,caps_required,
  |         codec_caps,caps_ext,...} ----->|   RTT 1
  |<---- Challenge{nonce,host_fp,version, |
  |              caps,caps_ext} ----------|
  |<---- Welcome{version,caps,session_id, |
  |              token,host_fp,            |
  |              resume_ticket} -----------|   （Challenge 与 Welcome 同一航班发出）
  |-- Auth{signature} ------------------->|   RTT 2
  |<---- Ok / Error ----------------------|
```

要点：

1. `Challenge` 与 `Welcome` **连续发出**，不等待 `Auth`——Welcome 是乐观应答，`Auth` 验签失败时用 `Bye{code}` 撤销会话（`code = 12` SignatureInvalid）。
2. 因此正常路径只需 2 RTT；失败路径在 RTT 2 之后由 `Bye` 终止。
3. `Challenge.nonce` 是本次握手的一次性随机数；签名原文覆盖 `nonce`、双方 `version`、`caps` 与 `caps_ext`，防止降级与能力篡改。

### 3.2 消息字段

| 消息 | 字段 |
|---|---|
| `Hello` | `version: u16`、`public_key`、`caps: u64`、`caps_required: u64`、`codec_caps: CodecCap[]`、`caps_ext`、`actor_host`、`extensions` |
| `Challenge` | `nonce`、`host_fp`（主机指纹）、`version: u16`、`caps: u64`、`caps_ext` |
| `Auth` | `signature` |
| `Welcome` | `version: u16`、`caps: u64`、`session_id`、`token`、`host_fp`、`resume_ticket` |

### 3.3 版本与能力校验

- `Hello.version != 本端 version` → `Bye{1}`（UnsupportedVersion），不继续；
- `caps_required & !peer_caps != 0` → `Bye{2}`（UnsupportedCapability）；
- `Challenge.host_fp` 与期望指纹不符 → `Bye{11}`（HostFingerprintMismatch）；
- `Auth` 验签失败 → `Bye{12}`；
- `token` 失效/过期 → `Bye{13}`（TokenInvalid）。

### 3.4 扩展 TLV

`extensions` 是 TLV 序列：`tag: u16 | len: u16 | value[len]`，重复 tag、超长、截断、尾随字节都是协议错误。

| tag | 名称 | 值 |
|---|---|---|
| 1 | `EXT_WIRE_VERSION` | 线版本（冗余校验） |
| 2 | `EXT_CAPS` | 扩展能力位图 |

未知 tag 必须保留并跳过（前向兼容）。

### 3.5 子通道握手

主会话建立后，附加流用 `ChannelHello` / `ChannelWelcome` 完成绑定：

- `ChannelHello`（C→H）携带会话 `token` 与目标平面；
- 会话 `token` TTL 60 s，过期即拒绝；
- 超时 1 s 未收到 `ChannelWelcome` 视为失败，关闭该流。

---

## 4. 会话恢复

### 4.1 票据

```rust
ResumeTicket { id: u64, nonce: [16]u8, exp_ms: u64 }
```

- TTL 300 s；
- **一次性**：成功换票后旧票立即作废，重放返回「已使用」。

### 4.2 恢复消息

| 消息 | 方向 | 字段 |
|---|---|---|
| `Resume` | C→H | `ticket_id: u64`、`nonce`、`last_frame_id: u32`、`last_input_seq: u32` |
| `ResumeResult` | H→C | `ok: bool`、`code: u16`、`need: u64`、`ticket`（新票）、`session_id` |

### 4.3 续接状态

主机侧维护每会话的续接表：最近视频 `frame_id`、最近输入 `seq`、每个文件 `task_id` 的 `received_upto`、剪贴板 `seq`、音频序号。恢复成功后：

- 视频从主机下一个 IDR 继续，客户端丢弃 `frame_id ≤ last_frame_id` 的帧与分片；
- 输入按 `seq` 去重，`seq ≤ last_input_seq` 的事件不重复注入；
- 文件用 `received_upto` 续传，配合 `FileResumeHint` 让发送方跳过已确认区间；
- 剪贴板按 `seq` 续传。

失败码：`21` ResumeExpired、`35` ResumeMetaMismatch（票据与会话元数据不匹配）。恢复失败后客户端回到完整握手。

---

## 5. 能力与编码协商

### 5.1 能力位

`caps` 为 `u64` 位图。本规范定义视频相关位：

| 位 | 常量 | 含义 |
|---|---|---|
| 12 | `VIDEO_H264` | H.264 |
| 13 | `VIDEO_H265` | H.265 / HEVC |
| 14 | `VIDEO_VP8` | VP8 |
| 15 | `VIDEO_VP9` | VP9 |
| 16 | `VIDEO_AV1` | AV1 |
| 34 | `VIDEO_RAW` | 未压缩 BGRA 回退 |

功能能力位与 `version` 独立，本规范定义：

| 位 | 常量 | 含义 |
|---|---|---|
| 40 | `FEATURE_PRIVACY` | 隐私屏状态机与本地输入阻断 |
| 41 | `FEATURE_DISPLAY_MODES` | 显示模式枚举/切换 |
| 42 | `FEATURE_TEXT_INPUT` | 长文本注入 |
| 43 | `FEATURE_PREVIEW` | 逐 peer 预览订阅 |
| 44 | `FEATURE_RESOLUTION_PREF` | 按 `display_key` 持久化分辨率偏好 |
| 45 | `FEATURE_FPS15` | 15 fps 档 |
| 46 | `FEATURE_BANDWIDTH` | 逐 peer 带宽预算 |
| 47 | `FEATURE_FILE_BROWSER` | 文件浏览 |
| 48 | `FEATURE_FILE_CLIPBOARD` | 文件剪贴板 |

其余位由实现按产品策略分配并在同一字段协商；**未知位必须原样保留**，不得因存在未知位而拒绝会话。旧端忽略未实现的功能位，新端只打开已实现的功能位；功能位不得用来替代协议版本判断。

`DisplayInfo.display_key` 是跨会话稳定的 64 位显示器身份，`0` 表示未知；它不随 `id`/分辨率/坐标变化而改变。旧端缺省按 `0` 处理。

### 5.2 编解码能力描述

`CodecCap` 描述单个编码器的可用形态：

| 字段组 | 位图 | 位值 |
|---|---|---|
| 色度格式 | chroma | `I420=bit0`、`I444=bit1`、`I422=bit2`、`BGRA=bit3` |
| 位深 | bit_depth | `8=bit0`、`10=bit1`、`12=bit2` |
| 特性 | flags | `HW=bit0`、`SW=bit1`、`INTRA_REFRESH=bit2`、`TEMPORAL_LAYERS=bit3`、`LOSSLESS=bit4`、`RAW=bit5` |

### 5.3 协商流程

1. 客户端在 `Hello.codec_caps` 中列出全部可解码组合；
2. 主机选一个双方都支持、且采集/编码路径可用的组合，用 `VideoConfig` 下发（含 `req_id`）；
3. 客户端确认后回 `VideoReady`（同 `req_id`，携带实际 `width`/`height`）；
4. 主机随后发送以 `KEY` 标志开头的视频帧；中途更换编码器时重发 `VideoConfig`，旧帧在客户端按 `codec_id` 丢弃。

运行中质量变化用 `VideoAdjusted`（方向 H→C）通知码率/帧率档位；解码失败用 `DecodeError`（C→H）上报，主机据此降档或换编码器（`55` CodecFallback）。

### 5.4 相关消息

| 消息 | 方向 | 关键字段 |
|---|---|---|
| `VideoConfig` | C→H | `req_id`、`codec`、`width`、`height`、`fps`、`bitrate_kbps` |
| `VideoReady` | H→C | `req_id`（回显）、`width`、`height` |
| `VideoAdjusted` | H→C | 调整后的档位 |
| `VideoStop` | C→H | 停止推流 |
| `CodecConfig` | H→C | 编解码器私有初始化数据（SPS/PPS、VPS、extradata） |
| `DecodeError` | C→H | 失败原因 |

---

## 6. 媒体平面（ch2）

### 6.1 帧头（36 B，大端）

| 偏移 | 宽度 | 字段 | 说明 |
|---|---|---|---|
| 0 | 1 | `fmt` | 恒为 1 |
| 1 | 1 | `flags` | 见 6.2 |
| 2 | 1 | `codec_id` | 编码器标识 |
| 3 | 1 | `hdr_len` | `36 + 8 * rect_count`，上限 255 |
| 4 | 4 | `frame_id` | 单调递增，恢复与 ack 的基准 |
| 8 | 4 | `request_id` | 回应 `VideoConfig`/`RequestKeyframe` 的 `req_id`，无则 0 |
| 12 | 2 | `frag_idx` | 分片序号，从 0 起 |
| 14 | 2 | `frag_count` | 本帧分片总数 |
| 16 | 8 | `pts_us` | 采集时间戳（µs） |
| 24 | 4 | `capture_ms` | 采集耗时 |
| 28 | 4 | `encode_ms` | 编码耗时 |
| 32 | 2 | `rect_count` | 脏矩形数，≤ `MAX_RECTS = 27` |
| 34 | 2 | `reserved` | 0 |

紧随其后每个矩形 8 B：`x u16 | y u16 | w u16 | h u16`。`rect_count = 0` 表示整帧。

### 6.2 flags 位

| 位 | 名称 | 含义 |
|---|---|---|
| 0 | `KEY` | 关键帧/IDR |
| 1 | `CURSOR_EXCLUDED` | 采集时已排除光标 |
| 2 | `LAST_FRAG` | 本帧最后一个分片 |
| 3 | `INTRA_REFRESH` | 帧内刷新（非 IDR 的渐进恢复） |
| 4 | `REGION` | 仅含脏矩形区域 |
| 5–7 | `LAYER_SHIFT`/`LAYER_MASK` | 时域层号 |

同一 `frame_id` 的 `hdr_len`、`frag_count`、`codec_id` 必须一致；不一致即为协议错误。

### 6.3 可靠流切片

编码帧按 **256 KiB** 切片，单片上限 `MAX_FRAME_LEN` 约束下全帧 ≤ 64 片，每片一个平面帧（`type = 1` `VIDEO_FRAME`），`flags` 带 `LAST_FRAG` 标记尾片。可靠流保证不丢片，重组只按 `frag_idx` 顺序拼接。

### 6.4 DATAGRAM 承载

QUIC DATAGRAM 路径并行存在，用于低延迟：一帧切成 ≤ `MAX_VIDEO_FRAGMENTS = 1024` 个数据报，单报 ≤ MTU 1100 B（头 20 B + 载荷 1080 B），因此单帧上限约 1.05 MiB。

数据报头（20 B，大端）：

| 偏移 | 宽度 | 字段 |
|---|---|---|
| 0 | 4 | `frame_id` |
| 4 | 2 | `frag_index` |
| 6 | 2 | `frag_count` |
| 8 | 2 | `flags`（`KEY=bit0`、`PARITY=bit1`） |
| 10 | 1 | `codec` |
| 11 | 1 | `reserved` |
| 12 | 8 | `pts_us` |

规则：

- `frag_count == 0` 视为 1 片；`frag_index ≥ frag_count` 或 `frag_count > 1024` → 整报丢弃；
- 同一帧内 `frag_count`、`flags`、`codec`、`pts_us` 不一致 → 丢弃该帧（`FrameMismatch`）；
- 重复的相同分片幂等忽略；同一 `frag_index` 内容不同 → 丢弃整帧（`DuplicateConflict`）；
- 丢片通过 `SliceNack` 上报，主机按需重发或整帧重发 IDR。

### 6.5 XOR FEC

每 16 片（`FEC_GROUP = 16`）生成 1 个校验片：`flags` 置 `PARITY`，载荷为该组已到片逐字节 XOR（组内片长不一致时按最长片对齐、缺位视为 0）。尾组不足 16 片时不生成校验片。

恢复条件：某组内**恰好缺 1 片**时可用校验片逐字节还原。缺 2 片及以上无法恢复，只能等重传或等 IDR。校验片自身丢失不影响正确性。

### 6.6 重组与解码

- 重组器支持 8 帧并发（`BTreeMap<frame_id, PendingFrame>`），按 `frame_id` 递增交付；
- 一帧收齐后按 `frame_id` 顺序交给解码器；跳号时等待 `SliceNack` 补齐或等下一个 `KEY`；
- 解码器不接受非 `KEY` 起始的流。

### 6.7 关键帧请求

`RequestKeyframe`（C→H，含 `req_id`）要求主机尽快发 IDR；主机在新帧的 `request_id` 回显该 `req_id`。以下情形客户端必须主动请求：解码器报错、丢片无法恢复、恢复会话后、切换编码器后。

### 6.8 未压缩回退

`VIDEO_RAW` 能力位协商成功时，`codec_id` 标记为 RAW，帧载荷为 BGRA 像素（`w * h * 4`），按矩形下发时数据为各矩形拼接。RAW 只用于极小分辨率或调试。

---

## 7. 输入与光标

### 7.1 输入批量

`ch1` `type = 1`（`INPUT_BATCH`）；可靠流与 DATAGRAM 共用同一载荷。

| 偏移 | 宽度 | 字段 |
|---|---|---|
| 0 | 4 | `seq`（单调递增，恢复去重基准） |
| 4 | 8 | `ts_us`（客户端采集时刻） |
| 12 | 1 | `monitor`（显示器序号） |
| 13 | 1 | `count`（事件数，≤ `MAX_INPUT_EVENTS = 16`） |
| 14 | … | 事件序列 |

### 7.2 事件编码

每个事件以 `kind: u8 | flags: u8` 开头，随后是定长或定长+变长字段（全大端）：

| kind | 事件 | 后续字段 | 长度 |
|---|---|---|---|
| 1 | Move | `x i32`、`y i32` | 10 |
| 2 | MoveRel | `dx i32`、`dy i32` | 10 |
| 3 | Button | `down u8`（flags 位）、`button u8`、`x i32`、`y i32` | 11 |
| 4 | Wheel | `delta i32`、`pan i32`、`x i32`、`y i32` | 18 |
| 5 | Key | `down u8`（flags 位）、`vk u16`、`scan u32`、`chr u32`、`mods u16` | 14 |
| 6 | Text | `len u8`、`utf8[len]`（截断到 255 B） | 3+len |
| 7 | Touch | `id u8`、`phase u8`、`x u16`、`y u16` | 8 |

鼠标按下/抬起的 `down` 存在 `flags` 字节里（bit0），`button` 字段紧跟其后。`mods` 为修饰键位图。`Text` 用于 IME 合成结果，按 UTF-8 边界截断。

### 7.3 注入语义

- 注入端按 `seq` 顺序执行，`seq` 回退即丢弃；
- `count = 0` 的批量是合法的心跳式空批，接收方不得报错；
- 事件里坐标超出当前显示器边界时按实现策略裁剪到边界内，不得注入失败。

### 7.4 光标

| 消息 | `type` | 布局 |
|---|---|---|
| `CURSOR_SHAPE` | 1 | 15 B 头 `cursor_id u32\|generation u16\|hx i16\|hy i16\|width u16\|height u16\|format u8` + 形状数据 |
| `CURSOR_RESET` | 2 | 2 B（`generation u16`），表示回到默认光标 |
| `CURSOR_POS` | 3 | 15 B `x i32\|y i32\|shape_id u32\|generation u16\|visible u8` |
| `CURSOR_STATE` | 4 | 22 B `x i32\|y i32\|semantic u16\|render_mode u8\|hidden_reason u8\|visual_offset_x i16\|visual_offset_y i16\|display_id u8\|flags u8\|seq u32` |

`CURSOR_STATE` 是语义光标；位图 `CURSOR_SHAPE`/`CURSOR_POS` 仍是 fallback。`semantic` 为 0…28 的内置形状（0 default、1 arrow、2 ibeam、…、28 no-drop），未知值原样保留；`render_mode` 为 0 legacy/1 separate/2 embedded/3 hidden/4 unknown；`hidden_reason` 为 0 none/1 system-hidden/2 system-suppressed/3 no-pointing-device/4 sampling-failed/5 secure-desktop-pending/255 unknown；`flags` bit0 position-valid、bit1 visible。能力位 `FEATURE_SEMANTIC_CURSOR = 1<<49`。

形状 `format`：`0` BGRA8（数据长 `w*h*4`）、`1` PNG（≤ 1 MiB）、`2` 掩码（`2 * ceil(w/8) * h`）。`w`/`h` 取值 1…1024。形状数据总长 ≤ `MAX_CURSOR_BYTES = 1 MiB`；`generation` 递增，客户端丢弃旧代形状。

### 7.5 时钟与 RTT 基线

- `RttEstimator`：以采样最小值作基线，均值用 EWMA（α = 1/8）平滑；
- `ClockBaseline`：以 `min(recv_time − pts)` 为基线，允许 1 ms/s 的时钟向上漂移（`MAX_ADJUST_US_PER_SEC = 1000`），超出即重锚；
- 二者供抖动缓冲与超时判定使用，属于 engine 层，不入线格式。

---

## 8. 文件平面（ch3）

### 8.1 消息与类型

| `type` | 名称 | 方向 | 布局 |
|---|---|---|---|
| 1 | `FILE_CHUNK` | 双向（上传 C→H，下载 H→C） | 8 B `offset u64` + 块数据（≤ 4 MiB） |
| 2 | `FILE_ACK` | 反向于当前块流 | 10 B `received_upto u64\|credit u16` |
| 3 | `FILE_END` | 同 `FILE_CHUNK` 方向 | 32 B `sha256[32]` |
| 4 | `FILE_ERROR` | 双向 | 8 B `task_id u32\|code u16\|detail_len u16` + UTF-8 说明（≤ 4 096 B） |
| 5 | `FILE_CHUNK_ZSTD` | 同 `FILE_CHUNK` 方向 | 12 B `offset u64\|uncompressed_len u32` + zstd 数据 |
| 6 | `FILE_RESUME_HINT` | 反向于当前块流 | 12 B `task_id u32\|resume_offset u64` |

协商类消息（`FileOffer`/`FileAccept`/`FileDone`/`FileCancel`/`FileGet`/`FileList`/`FileListing`/`FileMeta`/`FileMkdir`/`FileRename`/`FileDelete`）走 ch0。

### 8.2 窗口参数

| 参数 | 值 |
|---|---|
| 默认块大小 | 1 MiB（`DEFAULT_FILE_BLOCK`） |
| 块大小范围 | 64 KiB – 4 MiB |
| 默认 credit | 8 块（`DEFAULT_FILE_CREDIT`） |
| credit 范围 | 2 – 64 |
| 单文件上限 | 8 GiB |
| 块数上限 | 8 192 |
| 位图上限 | 1 024 B |

### 8.3 credit 协商

`FileOffer` 携带接收方视角的 `block_size` 与 `credit`：

- **发送方必须采纳 `FileOffer.credit`** 作为初始在途额度；忽略它会在第一条 ack 后被固定窗口覆盖，自适应窗口失效；
- 自适应算法（engine）：`bdp = 可用带宽 × max(RTT, 1 ms) / 8`，`block = clamp(bdp / 8, 64 KiB, 4 MiB)` 并按 4 KiB 对齐，`credit = clamp(ceil(bdp / block), 2, 64)`；
- 接收方在每条 `FILE_ACK` 里回带当前 `credit`，发送方据此更新窗口；`credit = 0` 表示暂停发送直到下一次 ack。

### 8.4 位图与续传

接收方按 `received_upto` 声明连续前缀；缺块用位图表达，**LSB-first**：块 *i* 对应 `byte[i / 8] & (1 << (i % 8))`。`FileResumeHint` 让接收方在恢复会话时主动声明已有前缀，发送方从该偏移继续。

约束：块偏移必须单调、不得跨越文件尾、不得超出 `MAX_FILE_BYTES`；越界为协议错误。

### 8.5 摘要

- 每块可选携带块级 SHA-256（用于乱序/压缩路径校验）；
- `FILE_END` 携带整文件 SHA-256（空文件为 SHA-256 of empty）；
- 摘要计算与校验的执行在参考实现；wire 只约束字段长度与顺序，以及「`FILE_END` 之前不得宣告完成」；
- 校验失败 → `34` ShaMismatch。

### 8.6 压缩

`FILE_CHUNK_ZSTD` 载荷为 zstd 压缩块（默认 level 3），`uncompressed_len` 必须落在 1…4 MiB。压缩后不小于原块 90%（`ZSTD_MIN_RATIO_PERCENT = 90`）时应发送未压缩的 `FILE_CHUNK`，避免负收益。是否压缩由发送方决定，接收方必须两条路径都支持。

### 8.7 错误

`30` FileTooLarge、`31` DiskFull、`32` FileLocked、`33` PathInvalid、`34` ShaMismatch、`35` ResumeMetaMismatch。错误携带 `task_id`，接收方据此终止该任务并释放窗口。

### 8.6 上传与下载

**上传（push，控制端发文件给主机）**

`FileOffer`(C→H) → `FileAccept`(H→C) → `FILE_CHUNK`(C→H) / `FILE_ACK`(H→C) → `FILE_END`(C→H) → `FileDone`(H→C)。

**下载（pull，主机发文件给控制端）**

`FileGet`(C→H) → `FileMeta`(H→C) → `FILE_CHUNK`(H→C) / `FILE_ACK`(C→H) → `FILE_END`(H→C) → `FileDone`(C→H)。

两条流共用 ch3 的 `FILE_CHUNK`/`FILE_ACK`/`FILE_END`；方向由 `FileOffer` 或 `FileGet` 决定，控制面不得只按消息名判断方向。

---

## 9. 剪贴板平面（ch7）

### 9.1 格式

| 值 | 格式 |
|---|---|
| 0 | text |
| 1 | rtf |
| 2 | html |
| 3 | png |
| 4 | dib |

### 9.2 消息

| 位置 | 名称 | 布局 |
|---|---|---|
| ch7 `type=1` | `CLIP_DATA` | 10 B `seq u32\|format u16\|offset u32` + 数据（≤ 256 KiB） |
| ch7 `type=2` | `CLIP_ACK` | 10 B `seq u32\|received_upto u32\|credit u16` |
| ch0 | `ClipOffer`/`ClipPull`/`ClipAccept`/`ClipDone` | JSON 协商 |

### 9.3 约束

- 单条目 1 B – 16 MiB（`MAX_CLIP_BYTES`）；
- 默认块 256 KiB（`DEFAULT_CLIP_BLOCK`），credit 4（`DEFAULT_CLIP_CREDIT`）；
- **聚合预算 ≤ 64 MiB**：同一会话内所有未完成条目之和超限必须拒绝（`70` ClipboardTooLarge）；
- `seq` 单调，接收方丢弃旧 `seq`；`ClipAck.received_upto` 语义同文件平面（连续前缀）。

---

## 10. 音频平面（ch5）

### 10.1 参数

| 参数 | 值 |
|---|---|
| 采样率 | 48 000 Hz |
| 帧长 | 10 ms |
| 编码 | Opus（默认） |
| RED 冗余 | 最多 3 个前序包（`MAX_RED_PACKETS`） |
| 抖动缓冲目标 | 50 ms（`DEFAULT_TARGET_MS`） |
| 抖动缓冲范围 | 20 – 200 ms |
| 单帧上限 | 64 KiB（`MAX_AUDIO_FRAME`） |

### 10.2 消息

| `type` | 名称 | 布局 |
|---|---|---|
| 1 | `AUDIO_FMT` | 7 B `codec u8\|rate u32\|channels u8\|frame_ms u8` |
| 2 | `AUDIO_FRAME` | 12 B `seq u32\|pts_us u64` + 载荷 |

### 10.3 抖动缓冲

- 到达间隔用 EWMA 平滑（到达 0.875 / 历史 0.125）；
- 目标延迟 = `clamp(平滑抖动 × 系数, 20 ms, 200 ms)`，初始 50 ms；
- 平均到达间隔**连续超过 5 s**（`CLOCK_REANCHOR_IDLE_MS`）触发重锚：重置首包时间、首包 pts、下一 `seq`，抖动归零，目标延迟回 50 ms，计数 `clock_reanchors += 1`；
- `clock_skew_ppm` 用最小二乘拟合到达时刻与 `pts_us` 的斜率，供长时间播放的漂移补偿；
- 统计字段：`received / played / dropped_late / duplicates / recovered_red / target_ms / max_jitter_ms / playout_delay_ms / clock_reanchors / clock_skew_ppm`。

---

## 11. 反馈平面（ch6）

| `type` | 名称 | 布局 |
|---|---|---|
| 1 | `FRAME_ACK` | 20 B `ack_frame_id u32\|recv_ts_us u64\|decode_done_us u32\|queue_depth u16\|dropped u16` |
| 2 | `NET_REPORT` | 18 B `rtt_us u32\|loss_ppm u32\|recv_bps u64\|queue_ms u16` |
| 3 | `SLICE_NACK` | 12 B `frame_id u32\|missing_bitmap u64` + 0–15 个 u64 扩展（12 – 132 B） |

### 11.1 FrameAck

客户端每解出一帧回一条 `FrameAck`。主机据此释放在途窗口，并计算端到端时延（`now − recv_ts_us`）与解码耗时（`decode_done_us`）。

在途窗口（参考实现）：32 帧 / 4 MiB；满窗时主机丢帧并置强制关键帧。**客户端必须持续回 ack**，否则窗口不会释放——这是协议层的强制义务，不是可选优化。

### 11.2 SliceNack

`missing_bitmap` 首字覆盖片 0–63，后续字依次覆盖到片 1023（`MAX_WORDS = 16`，最高 132 B）。缺失位置 1、收到位置 0。超过 1024 片的帧不需要 nack（本就超限）。

### 11.3 NetReport

`rtt_us` / `loss_ppm` 驱动主机侧码率、文件块与 TCP 分档；`recv_bps` 为客户端实测下行速率；`queue_ms` 为接收队列排队时长，用于自适应优先级。

---

## 12. 控制消息注册表（ch0）

### 12.1 编码

控制面载荷是 UTF-8 JSON，消息身份由 JSON 内的 `type` 字符串决定（serde tag），**没有数字类型号**。注册表共 54 项：

```
AudioStart AudioStop Auth Bye Challenge ChannelHello ChannelWelcome
ClipAccept ClipDone ClipOffer ClipPull CodecConfig ControlOwner DecodeError
DisplayChanged DisplayList Error FileAccept FileCancel FileDelete FileDone
FileError FileGet FileList FileListing FileMeta FileMkdir FileOffer FileRename
Hello HostStats InputConfig InputState InputStatus Ok Ping PlanePriorities Pong PrivacyState
Rejected RequestKeyframe Resume ResumeResult Saved Screenshot SendCad
SessionEnded SessionInfo ShareRequest VideoAdjusted VideoConfig VideoReady
VideoStop Welcome
```

结构性说明：`Error` 的载荷结构体名为 `ErrorReply`，`Ok` 为 `OkReply`，`FileError` 为 `FileErrorInfo`。**未知 `type` 必须保留并交给上层**：`SessionMachine` 将原始 JSON 放入有界队列（最多 64 条，超出丢弃最旧），应用可用 `take_unknown()` 取走；不得断开会话——新增类型只追加，不重排、不复用旧名。

### 12.2 方向表

接收方视角（`Host` 表示该消息由客户端发出、主机接收）：

| 方向 | 消息 |
|---|---|
| C→H | Hello, Auth, Resume, ChannelHello, VideoConfig, VideoStop, RequestKeyframe, InputConfig, DecodeError, FileOffer, FileGet, FileList, FileCancel, ClipPull, AudioStart, AudioStop, Screenshot, SendCad, FileMkdir, FileDelete, FileRename, ShareRequest |
| H→C | Challenge, Welcome, ResumeResult, ChannelWelcome, Rejected, VideoReady, CodecConfig, VideoAdjusted, DisplayList, DisplayChanged, InputState, InputStatus, HostStats, Saved, SessionInfo, SessionEnded, ControlOwner, PrivacyState, FileAccept, FileMeta, FileListing |
| 双向 | Bye, Error, Ok, Ping, Pong, PlanePriorities, ClipOffer, ClipAccept, ClipDone, FileDone, FileError |

表中未列出的名字一律视为非法方向并报 `WrongDirection`。

### 12.3 关键字段约束

| 消息 | 约束 |
|---|---|
| `Hello` | `version` 为单值；`caps_required` 必须是 `caps` 的「需求」而非「声明」 |
| `Welcome` | 必须携带 `session_id`、`token` 与 `resume_ticket` |
| `VideoConfig` / `VideoReady` | `req_id` 必须成对回显；`VideoReady` 还须回 `width`/`height` |
| `PlanePriorities` | `priorities` 长度恒为 8；`ack: true` 必须回显同一 `req_id` |
| `FileOffer` | `block_size` 64 KiB – 4 MiB；`credit` 2 – 64，接收方值必须被采纳 |
| `FileGet` | `dir` 与 `name` 必填，`offset` 为续传起点；仅 C→H；host 回 `FileMeta` 后启动下载块流 |
| `FileList` | `limit` 取值 1 – 2 000，缺省 500；仅 C→H；越界报「FileList.limit must be 1..=2000」 |
| `FileCancel` | 携带 `task_id`；未知任务报 `UnknownTask` |
| `Resume` | `ticket_id` 必须对应未过期票据；`nonce` 必须与票据一致 |
| `Bye` | 携带错误码，服务端主动关闭时最先发出 |

### 12.4 目录浏览

`FileList`（C→H，`path` + `limit`）→ `FileListing`（H→C，条目数组 + 是否还有更多）。条目含名字、类型、大小、修改时间。`FileGet` 用于请求一个文件的内容下载：host 先回 `FileMeta`，随后按 `offset` 发送 `FILE_CHUNK`；条目元数据浏览应使用 `FileList`。`FileList` 分页由客户端按 `limit` 自行循环，协议不提供游标。

---

### 12.3 PrivacyState

`PrivacyState` 是主机权威、单调 revision 的隐私屏状态：`{revision u64, state u8, overlay_active bool, reason String, local_unlock_revision u64}`。`state` 为 0 off / 1 on / 2 unavailable；`revision` 必须严格递增，客户端丢弃旧 revision；`local_unlock_revision` 记录客户端本地已关闭的最近 host revision，只影响本地 overlay，不回传主机。`reason` ≤ 256 B。

## 13. 错误码

错误码为 `u16`，`Error`/`Bye` 的载荷是 `ErrorInfo{ code: u16, class: u8, need: u64, retry_after_ms: u32 }`。`need` 是「需要补的能力位」，`retry_after_ms` 是建议重试间隔。

| code | 名称 | 类别 |
|---|---|---|
| 0 | Unspecified | Fatal |
| 1 | UnsupportedVersion | NeedsUser |
| 2 | UnsupportedCapability | NeedsUser |
| 3 | Unauthorized | NeedsUser |
| 4 | PolicyDenied | NeedsUser |
| 5 | Busy | Fatal |
| 6 | SessionGone | RetryableTransport |
| 7 | Timeout | Fatal |
| 8 | ProtocolError | Fatal |
| 9 | ShuttingDown | RetryableTransport |
| 10 | UnknownKey | NeedsUser |
| 11 | HostFingerprintMismatch | Fatal |
| 12 | SignatureInvalid | Fatal |
| 13 | TokenInvalid | Fatal |
| 20 | SessionTakenOver | Fatal |
| 21 | ResumeExpired | RetryableTransport |
| 30 | FileTooLarge | Fatal |
| 31 | DiskFull | NeedsUser |
| 32 | FileLocked | Fatal |
| 33 | PathInvalid | Fatal |
| 34 | ShaMismatch | NeedsUser |
| 35 | ResumeMetaMismatch | Fatal |
| 50 | CodecUnsupported | NeedsUser |
| 51 | VideoStalled | Fatal |
| 52 | （保留，不得复用） | — |
| 53 | EncoderInitFailed | Fatal |
| 54 | DecoderError | Fatal |
| 55 | CodecFallback | RetryableTransport |
| 60 | InjectFailed | Fatal |
| 61 | NoDesktop | Fatal |
| 62 | UipiBlocked | NeedsUser |
| 70 | ClipboardTooLarge | NeedsUser |
| 71 | ClipboardBusy | Fatal |
| 80 | AudioUnsupported | NeedsUser |

类别 id：`0 = RetryableTransport`（可原地重连/续传）、`1 = Retryable`、`2 = NeedsUser`（需要用户介入，如换版本、补权限）、`3 = Fatal`。

---

## 14. 超时预算

| 阶段 | 超时 |
|---|---|
| QUIC 建连 | 3 s |
| TLS 握手 | 3 s |
| Hello → Challenge | 2 s |
| Challenge → Auth | 2 s |
| Auth → Welcome | 3 s |
| ChannelHello → ChannelWelcome | 1 s |
| 单条 control 请求（应答类） | 5 s |
| 会话 token 有效期 | 60 s |
| 恢复票有效期 | 300 s |

超时后按对应错误码终止：握手阶段用 `Bye`，会话内用 `Error{7}`（Timeout）。任何超时都不得导致「静默继续」——必须显式失败或显式重试。

---

## 15. 不变量

| 编号 | 不变量 |
|---|---|
| INV-1 | 一次 `send` 一条消息、一次 `recv` 一条消息；半条消息报 `UnexpectedEof`，禁止二次分帧 |
| INV-2 | 多流模式下控制面只从 ch0 读取；控制读端不得收到任何非 ch0 字节 |
| INV-3 | 未知 control `type` 必须保留并交给上层（有界队列），未知 TLV tag 必须保留并跳过；两者都不得导致断开 |
| INV-4 | 帧头 `len` 与实际载荷长度一致；超 `MAX_FRAME_LEN`/`MAX_CONTROL_LEN` 直接拒绝 |
| INV-5 | credit 只能由接收方声明；发送方不得超出在途额度 |
| INV-6 | 文件/剪贴板位图为 LSB-first，块 *i* ↔ `byte[i/8] & (1 << (i%8))` |
| INV-7 | `version` 为单值，双方不等即 `Bye{1}`，不做区间协商 |
| INV-8 | 同一 `frame_id` 的分片元数据（`frag_count`/`codec`/`hdr_len`）必须一致 |
| INV-9 | 方向表强制：非法方向报 `WrongDirection`，不得默默丢弃 |
| INV-10 | `PlanePriorities.priorities` 恒 8 项；`ack` 必须回显 `req_id` |
| INV-11 | 恢复票据一次性；重放必须拒绝并保留原会话状态 |
| INV-12 | 任何超时都必须显式失败或显式重试，不得静默继续 |

---

## 附录 A 平面 `type` 注册表

| 通道 | `type` | 名称 |
|---|---|---|
| ch1 input | 1 | `INPUT_BATCH` |
| ch2 video | 0 / 1 / 2 | `STREAM_HELLO` / `VIDEO_FRAME` / `STREAM_END` |
| ch3 file | 1 / 2 / 3 / 4 / 5 / 6 | `FILE_CHUNK` / `FILE_ACK` / `FILE_END` / `FILE_ERROR` / `FILE_CHUNK_ZSTD` / `FILE_RESUME_HINT` |
| ch4 cursor | 1 / 2 / 3 / 4 | `CURSOR_SHAPE` / `CURSOR_RESET` / `CURSOR_POS` / `CURSOR_STATE` |
| ch5 audio | 1 / 2 | `AUDIO_FMT` / `AUDIO_FRAME` |
| ch6 feedback | 1 / 2 / 3 | `FRAME_ACK` / `NET_REPORT` / `SLICE_NACK` |
| ch7 clipboard | 1 / 2 | `CLIP_DATA` / `CLIP_ACK` |

新增 `type` 只能追加，不得重排或复用。

## 附录 B 字节序、单位与计数

- 全部整数字段**大端**；
- 时间统一微秒（`*_us`），除显式标注 `_ms` 的耗时字段；
- 长度字段单位是字节；块/片计数单位是个；
- 字符串一律 UTF-8，长度前缀单位为字节；
- JSON 字段名大小写敏感，未知字段必须忽略（除 `type`）。

## 附录 C 握手扩展 TLV

`tag: u16 | len: u16 | value[len]`，全部大端。已知 tag 见 3.4；重复 tag、`len` 超出剩余字节、尾随残余字节均为协议错误。未知 tag 跳过并保留。

## 附录 D 编解码器私有数据

`CodecConfig`（H→C）承载编解码器初始化数据：H.264 为 SPS/PPS，H.265 为 VPS/SPS/PPS，AV1 为 sequence header，VP8/VP9 为最小初始化。客户端必须在收到 `CodecConfig` 后、收到下一个 `KEY` 帧前完成解码器初始化；初始化失败用 `DecodeError` 上报，主机侧对应 `54`/`55`。

## 附录 E 实现检查表

实现声称符合本规范时，至少能逐条给出证据：

1. 多流模式下 `recv_control` 只取 ch0，且在压力下不串道；
2. 握手 2 RTT 成立，`Auth` 失败能在 `Bye{12}` 后收回会话；
3. `FileOffer.credit` 被发送方采纳，自适应窗口在首条 ack 后不被覆盖；
4. 位图续传在乱序、丢块、重连三种场景下都能续；
5. DATAGRAM 丢 1 片能用 FEC 恢复、丢 2 片能整帧重发；
6. 音频 5 s 空闲后重锚，`clock_skew_ppm` 有值；
7. `PlanePriorities` 双向 ack 成对，`AdaptivePriorities` 不把 control 降档；
8. 所有超时按 §14 的预算触发并有日志；
9. 错误码表 33 项可往返映射（`code → 名称 → code`）；
10. 未知 `type` / 未知 TLV 不引起断开。
