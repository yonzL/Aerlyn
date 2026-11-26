---
id: aerlyn-wire-v4
type: architecture-design
title: Aerlyn Wire 协议 1.0.0（已冻结）
status: 1.0.0                # 2026-11-21 冻结（tag v1.0.0）
date: 2026-11-16
version: 1.0.0                # 接口/功能/修补 = 1.0.0；wire proto_version = 400
supersedes: V3 规范（历史档案，未随 standalone 迁移）
references:
  - docs/44-v3-vs-rustdesk-vnc-rdp.md
  - docs/43-rtc-webrtc-assessment.md
  - docs/40-protocol-review-v2.md
tags: [protocol, quic, codec, av1, hevc, vp9, h264, vp8]
---

> **规范正文见 [`../architecture/protocol.md`](../architecture/protocol.md)。** 本文是 1.0.0 冻结前的设计与决策档案：保留设计过程、上游对比、决策登记与逐轮实测旁注，**不作为线格式权威**。线格式、状态机、常量与不变量一律以 `../architecture/protocol.md` 为准。

# Aerlyn Wire 协议 1.0.0（设计档案）

> **V4 = V3 的直连 QUIC 基线 + 五编码矩阵。**
> **目标：在直连场景胜出或持平 Airhand（0 负为底线，允许平局）。** 传输/通道/握手/会话/文件/剪贴板/音频/安全/错误/背压**沿用 V3 规范（历史档案）**，
> 本文件定义编码协商、码流封装、编解码切换、质量档，以及为不落后补齐的音频同步/多观看者/验收判据。
>
> **分层契约见 [`../architecture/layering.md`](../architecture/layering.md)**：本文件的“wire 保证”由 `aerlyn-wire` 实现；“reference host 要求”由 `opsdesk-service`/`opsdesk-client-core` 实现。`aerlyn-wire` 一个 crate 不等于完整协议。

---

## 0. 目标与继承

| 项 | V4 |
|---|---|
| 网络 | **只做直连**；QUIC + TLS 1.3 默认，TCP 直连兜底；无 relay/NAT/rendezvous（继承 D-V3-18） |
| 帧率 | 客户端只设 `max_fps` 上限；实际 fps 由 host 能力决定（继承 V3 §11.6） |
| 编码 | **5 种性能编码：VP8、VP9、H.264、H.265、AV1**；H.264 是性能托底 |
| 兜底 | **RAW tile（id 5）**：零编码依赖，任何情况下都能出画面；不计入 5 种性能编码 |
| 选择 | 客户端给有序 `codec_pref`；host 按能力取第一个共同项；失败逐个回落 |
| 切换 | 会话内可切 codec，不重建会话；切换后首帧必为 KEY，`frame_id` 连续 |
| 质量 | `LOSSLESS / TEXT / BALANCED / MOTION` 四档（借 VNC 质量级 + RDP AVC444） |
| 硬件 | 硬编/硬解优先，软件回落；编码器初始化失败自动换 codec |

**继承清单**（未在本文件重述的条款按 V3 原文执行）：
§4 承载、§5 版本、§6 caps 框架、§7 通道、§8 分帧、§9 握手/恢复、§10 会话/权限、
§12 输入、§13 光标、§14 音频、§15 文件、§16 剪贴板、§17 测量、§18 背压、§19 错误、§20 安全、§21 演进。

**版本变更**：`proto_version = 400`；ALPN `aerlyn/4` / `aerlyn/4-tcp`。V1/V2/V3（0.1.0/0.2.0/0.3.0）均为未发布 dev 线，0.5.0 不承担向前兼容；v2 适配器降为可选。

**R4 线格式（0.5.0 已发布）**：

```
Hello.version u16                          // 单一版本；无 min/max 范围
Challenge / Welcome                       // 同一服务端航班
```

- R4 已删除 `Hello.version_range`；客户端发送 `ClientConfig.version`，服务端只接受等于自己 `version` 的单值，不做协商。`ClientConfig`/`ServerConfig` 也已是单一 `version` 字段。
- 2 RTT：`Hello`（1）→ `Challenge`+`Welcome`（2）→ `Auth`（3）；客户端收到 `Welcome` 后即可返回会话并发送 `Auth`+首条业务消息，服务端先验 `Auth`，失败则在 `Welcome` 之后补发 `Bye{SignatureInvalid=12}`，不处理业务消息。
- 冻结前不做版本协商；1.0.0 冻结后也不协商。只有 2.0.0 才新增与 1.0.0 的协商，不回溯 0.x。
- `proto_version` 仍为 400；R4 结构（2 RTT、单版本、删 `version_range`）已完成。主验收场景为 LAN；WAN 直连只要求协议可适用，实测延迟/吞吐作为参考数据，不作为冻结门槛。0.5.0 是库版本，wire 仍为 400；未来若改线格式再升 500。

### 0.0 版本、冻结与协商

**当前是 `0.5.0` 开发线，不是冻结版本。** 0.5.0 已提交并打 tag；线格式、消息 ID、字段和默认值在冻结前仍允许改。0.1.0/0.2.0/0.3.0 对应 V1/V2/V3，均为未发布 dev 线，不承担向前兼容。

P0-5 冻结审计摘要见 [实现状态](../architecture/implementation-status.md)：**未达冻结条件**；主场景 LAN-only，WAN 直连只要求协议可适用，延迟/吞吐不作为冻结门槛；历史审计快照记录 53 符合 / 6 部分 / 4 漂移 / 0 缺失，冻结仍需用户批准；收敛路线见 [漂移审计](../architecture/drift-audit.md)。

版本号定义为 **接口字段版本 . 功能实现版本 . 修补版本**（`I.F.P`）：

| 版本 | I | F | P | 线格式 | 说明 |
|---|---:|---:|---:|---|---|
| 0.4.x | 0 | 4 | x | 400 | R1/R3 等兼容补丁 |
| 0.5.0 | 0 | 5 | 0 | 400 | 当前开发线；新增 D21–D23，库版本发布 |
| **1.0.0** | **1** | **0** | **0** | 固定 | 首次冻结；不协商 |
| 2.0.0 | 2 | 0 | 0 | 2000 | 只与 1.0.0 协商 |

- 冻结前不做版本协商；R4 已删除 `version_range`，工作树用 `Hello.version` 单值。
- 1.0.0 冻结线也不做版本协商；只有 2.0.0 才新增与 1.0.0 的协商，不回溯 0.x。
- `proto_version` 跟线格式实际变化走，不强行与 package tag 对齐；0.5.0 保持 400，R4 改线则 500，1.0.0 沿用最终 wire 标识。

冻结为 **1.0.0** 的条件（全部满足才冻结）：

1. `aerlyn-wire`、`opsdesk-service`、`opsdesk-client-core` 的全平面实现完成，单元/集成/回环测试全绿；
2. T1–T22 有可复现证据，且直连场景对 Airhand 为 **0 负、胜或平**；
3. P0 的 R1（TransportConfig）、R3（心跳）、R4（2 RTT 握手结构/单版本）、R2（默认多流）完成并有基准/回归证据；T1–T22 全绿本身不等于冻结；WAN 延迟、吞吐、跨 NAT 时长只作参考，不作门槛；
4. 本档案与代码的一致性审计无未解释漂移（消息、错误码、字段、默认值、方向）；
5. 用户明确批准冻结。

**场景口径（D-V4-21）**：协议设计与质量验收以 **LAN-only** 为主；同一协议栈也适用于 WAN 直连（无 relay 的 P2P QUIC），但 WAN 的通信延迟、吞吐、跨 NAT 保持时长不作为冻结门槛。VPS 实测（R4 `CLIENT_TIMING ready_us=737731/1891305`、16 MiB 上传 40.664 s ≈3.30 Mbps、RTT 177–263 ms）仅作为“协议可在 WAN 运行”的参考数据，不参与 1.0.0 判定。

冻结后的规则：**只增不改**。已发布的错误码、消息 type、通道号、字段语义和二进制布局不得移动；新增能力走新 type / 新 caps 位 / 次版本号。冻结前发现的问题直接改协议，不背兼容包袱；0.5.0 tag 不移动。

**版本降级/回滚**：单版本线内没有协议级降级；混合版本以 `UnsupportedVersion` 拒绝。部署侧采用“版本探针 + 双二进制 + 维护窗口”，回滚靠换回旧二进制，详见 [版本策略](versioning.md)。不要恢复 `version_range` 来实现兼容。

### 0.1 一致性档

| 档 | 必须实现 | 可选 |
|---|---|---|
| **BASE** | H.264 编解码 + RAW 编解码 | 其它 4 种 |
| **MEDIA** | BASE + VP9/AV1 解码；host 编码 H.264 + VP9 或 AV1 之一 | H.265/VP8 |
| **FULL** | 5 种编解码 + RAW | — |

- RAW 是兜底，BASE 也必须实现（实现成本低，避免"无编码器即黑屏"）。
- `MEDIA` 是本项目目标档；`FULL` 是覆盖 Airhand 编码面的扩展档，不等于实战已胜出。
- 声明某档却缺任一必需项 → `UnsupportedCapability`。

### 0.2 竞争目标：直连场景胜出或持平 Airhand

**战场**：直连 QUIC、原生客户端、五编码、协议/可测性、输入/光标/音频/文件/剪贴板、多观看者。
**不比**：relay、NAT 穿透、浏览器控制台、成熟度——这些是非直连或实现维度；按"不考虑非直连"的要求，V4 不把它们列入胜负。

| 维度 | Airhand（直连） | V4 | 判定 |
|---|---|---|---|
| codec 数量 | H.264/H.265/AV1（`session.rs:382-386`） | 5 种 + RAW 兜底 | **V4** |
| codec 参数 | `QualitySettings.codec` + 支持列表（`session.rs:550`） | `CodecCap`（profile/level/chroma/bit depth/hw/temporal）+ 有序 `codec_pref` | **V4** |
| 会话内切换 | 文档未描述 codec 热切换 | `VideoReady` 重协商 + 新流 KEY，`frame_id` 连续 | **V4** |
| 质量档 | `adaptive/priority`（`session.rs`） | `LOSSLESS/TEXT/BALANCED/MOTION` + 4:4:4/10-bit/AV1 无损 | **V4** |
| 帧头 | 24B 媒体头（`architecture.md:20`） | 36B + `hdr_len/rect/frag/layer` | **V4** |
| ack/ABR | 累计 ack + 在途字节 + 跳旧帧（`architecture.md:56/58`） | 同左 + `NetReport`/`HostStats`/SLO | 平 |
| 输入 | 时钟基线 + 过期 release（`architecture.md:56`） | 同左 + `InputStatus` | 平 |
| 光标 | slot/generation + 本地光标（`architecture.md:134`） | generation + 本地合成 | 平 |
| 音频 | Opus + RED + 自适应 jitter + A/V sync（`architecture.md:139-160`） | §5.1 已补 jitter/A-V sync（待实现） | 平（设计，待验） |
| 文件/剪贴板 | bulk lane（2 MiB 帧） | ch3 块级 digest/credit、ch7 多格式/seq/owner | **V4** |
| 多观看者 | 每 viewer 独立账本，慢的跳帧（`architecture.md:64`） | §5.2 已实现 `ViewerLedger`/`ViewerSet` 公平调度模拟（4 viewer） | 平（协议级已验，真实 4 会话待接） |
| 会话恢复 | resume token（`architecture.md:20`） | `ResumeTicket` 300s | 平 |
| 版本/能力 | 版本范围 + Features（`architecture.md:42-50`） | 版本范围 + caps/TLV | 平 |
| 安全（直连） | TLS 1.3 + 应用层身份 | TLS 1.3 + 签名含 host_fp | 平 |
| 可观测性 | ack/统计 | `FrameAck/NetReport/HostStats` + SLO | **V4** |
| 零依赖兜底 | 无 | RAW（id 5） | **V4** |
| 成熟度 | 0.2.3 已发布 | draft，无实现 | Airhand（不计入协议胜负） |

**结论**：直连协议维度，V4 设计上 **8 胜 0 负 8 平**（音频/每 viewer 按平局计，§5.1/§5.2 已补设计）；这些都是**设计判据**，未实现、未实测。目标不是每个维度都胜，而是 **0 负、胜或平**；Airhand 的成熟度不在协议胜负内，V4 在 T18–T22 + T1–T15 全绿之前不具备宣称达标的证据。

### 0.3 达标（胜或平）的最后 3 步（2 个设计已补 + 1 个验收）

1. **音频 jitter buffer + A/V sync**（§5.1）：设计已补，待实现；目标是对齐 Airhand，按平局验收。
2. **每 viewer 独立账本**（§5.2）：`ViewerLedger`/`ViewerSet` 已实现并完成 4 viewer 公平调度模拟；目标是慢 viewer 不拖前台，按平局验收；真实 4 条 QUIC 会话与共享编码器待接线。
3. **可测的达标判据**（§6.1）：T18–T22 全绿 + T1–T15 全绿，代表五个战场和基础协议都不落后；允许平局，不要求每维度都胜。

---

## 1. 五编码矩阵

| id | codec | 码流 | 关键帧判定 | 配置数据 | 硬件 | 典型用途 |
|---|---|---|---|---|---|---|
| 0 | **H.264** | Annex B | NAL type 5（IDR） | SPS(7)/PPS(8) | NVENC/QSV/AMF/VideoToolbox/OpenH264 | 兼容基线、低延迟 |
| 1 | **H.265** | Annex B | IRAP（16–21，IDR/CRA/BLA） | VPS(32)/SPS(33)/PPS(34) | NVENC/QSV/AMF/VideoToolbox | 同画质省 30–40% 带宽 |
| 2 | **VP8** | 原始 VP8 帧 | frame tag bit0 = 0 | 无 | libvpx（软） | 最简兜底 |
| 3 | **VP9** | 原始 VP9 帧 | uncompressed header 的 frame_type | 无 | libvpx（软）；部分硬解 | 开源、SVC 时间层 |
| 4 | **AV1** | OBU 流 | KEY_FRAME + show_frame | sequence header OBU | NVENC/QSV/AMF/VideoToolbox/dav1d | 最低码率、时间层、无损 |
| 5 | **RAW**（兜底） | 原始 BGRA/BGR565 矩形 | 每帧独立（KEY=1） | `pixel_format` | 无 | 无编码器、排障、截图级画面 |

> **id 5 RAW 不是第 6 种性能编码**：它不做帧间预测、不做码率控制，只在"5 种都不可用"或显式排障时启用；平时不应被选为默认。

- **编码 id 是本协议私有命名空间**，不复用 RustDesk/Airhand 的编号。
- 所有编码：**无 B 帧、低延迟模式、长 GOP**；关键帧只由 IDR/KEY 触发，不做场景切换自动插帧（除显式请求）。
- `hdr_len`/`layer`/`frame_id`/`pts` 沿用 V3 帧头（§11.2）；`codec` 必须与本表一致。

---

## 2. 能力与协商

### 2.1 `CodecCap`（`VideoCodecs` TLV 的元素）

```
codec u8 | max_level u16 | profiles u32 | chroma u8 | bit_depth u8
max_w u16 | max_h u16 | max_fps u8 | min_kbps u32 | max_kbps u32
flags u8 | temporal_layers u8
```

- `profiles`：按 codec 定义的位图（见 §7）。
- `chroma`：bit0 I420、bit1 I444、bit2 I422、bit3 BGRA/RGB（RAW 用）。
- `bit_depth`：bit0 8-bit、bit1 10-bit、bit2 12-bit。
- `flags`：bit0 HW、bit1 SW、bit2 INTRA_REFRESH、bit3 TEMPORAL_LAYERS、bit4 LOSSLESS、bit5 RAW。
- `flags` 的方向由发送方决定：client 在 `Hello` 里声明**解码**能力，host 在 `Welcome` 里声明**编码**能力。
- `temporal_layers`：1–3；不支持填 1。
- `max_level`：codec 原生 level 码（H.264 51、H.265 153、VP9 51、AV1 13、VP8 0）。
- client 在 `Hello.caps_ext` 里发**解码**能力；host 在 `Welcome.caps_ext` 里发**编码**能力。两边都只声明真实可用的（能初始化的）编码器。

### 2.2 选择顺序

`VideoConfig` 新增/替换字段：

```
VideoConfig{req_id, display, codec_pref [u8], max_fps u16, max_kbps u32,
            active bool, priority u8, scale u16, quality u8,
            quality_mode u8, chroma_pref u8, bit_depth_pref u8, rect?}
```

| 规则 | 条款 |
|---|---|
| 客户端顺序优先 | `codec_pref` 非空 → host **按该顺序**取第一个"双方都支持且满足约束"的 codec（VNC `SetEncodings` 的有序偏好） |
| 空列表 | `codec_pref=[]` → host 用自己的默认排序：`AV1 > H265 > VP9 > H264 > VP8`（Airhand 的效率排序） |
| 约束过滤 | 分辨率/fps/码率/chroma/bit depth 任一不满足 → 跳到下一个 |
| 硬件优先 | 同一 codec 下优先硬编；不可用则软编；`priority=1`（延迟）可要求 H.264 优先 |
| 初始化失败 | 编码器 init 失败 → 自动下一个，发 `VideoAdjusted{reason=ENCODER_INIT, codec}`，不中断会话 |
| 回执 | `VideoReady{req_id, display, codec, profile, level, chroma, bit_depth, width, height, max_fps, max_kbps, hw}` |
| 失败 | 没有任何共同 codec → `Error{CodecUnsupported, need}`，会话不建立视频流 |

`quality_mode`：0 `LOSSLESS`、1 `TEXT`、2 `BALANCED`、3 `MOTION`（§5）。

### 2.3 `CodecConfig`

首次建流、codec 切换、恢复后首帧之前，host 在 control 发：

```
CodecConfig{display, codec, profile, level, chroma, bit_depth, extradata bytes}
```

- H.264：`extradata` = Annex B 的 SPS+PPS。
- H.265：`extradata` = VPS+SPS+PPS。
- AV1：`extradata` = sequence header OBU（可带 metadata OBU）。
- VP8/VP9：空。
- 关键帧仍自带参数集（H.264/H.265 的 SPS/PPS、AV1 的 sequence header），保证晚加入/丢配置也能解。

### 2.4 V4 新增/扩展 control 消息

| type | 方向 | caps | 说明 |
|---|---|---|---|
| `CodecConfig` | H→C | VIEW | profile/level/chroma/bit_depth + extradata |
| `DecodeError` | C→H | VIEW | `{display, codec, frame_id, reason}`，驱动回落 |
| `VideoReady` | H→C | VIEW | 沿用 V3，增加 `level/bit_depth/hw` |
| `VideoAdjusted` | H→C | VIEW | 沿用 V3，reason 增加 `ENCODER_INIT/DECODER_ERROR/CODEC_FALLBACK/QUALITY_MODE` |

---

## 3. 码流封装

| codec | 载荷 | 关键帧要求 | 备注 |
|---|---|---|---|
| H.264 | Annex B access unit | `flags.KEY=1` 且含 IDR；SPS/PPS 在 IDR 前 | 无 B 帧；profile High/Baseline，level 4.1/5.1 |
| H.265 | Annex B access unit | `KEY=1` 且含 IRAP；VPS/SPS/PPS 在 IRAP 前 | Main/Main10；无 B 帧 |
| VP8 | 原始 VP8 帧 | `KEY=1` 且 frame tag bit0=0 | I420 8-bit |
| VP9 | 原始 VP9 帧 | `KEY=1` 且 frame_type=KEY_FRAME | profile 0–3；时间层用 `layer` |
| AV1 | 一个 temporal unit（OBU 序列） | `KEY=1` 且 KEY_FRAME+show_frame=1；sequence header 在前 | Main/High/Professional；时间层用 `layer`；无损 q=0 |
| RAW | 每个 rect 的裸像素（BGRA8 或 BGR565） | 每帧 `KEY=1`（帧独立） | 无预测/无码率控制；`CodecConfig.pixel_format` 指定格式；≤1080p、≤5 fps |

- 大帧按 V3 §11.2 的 256 KiB 切片，`frag_count ≤ 64`。
- `layer` = 时间层 id（0 基本层，1–7 增强层）；VP9/AV1 可发多层，H.264/H.265 只发 0。
- 切换 codec 后 `frame_id` **连续递增**，`pts` 连续；旧流以 `StreamEnd{CODEC_SWITCH}` 结束。
- 解码端必须在 `hdr_len` 处取载荷，忽略不认识的扩展字段（V3 §11.2）。
- RAW 的 rect 载荷按 `rect_count` 顺序拼接：每个 rect 是 `w × h × bytes_per_pixel` 的行主序像素，无 stride padding；单帧仍受 16 MiB 上限（1080p 全屏 BGRA 约 8.3 MB，4K 全屏超限，只能发脏矩形）。

---

## 4. 选择、回落、切换

### 4.1 初始选择

```
client VideoConfig{codec_pref=[AV1,H265,VP9,H264,VP8], ...}
  → host 逐个 intersection + 约束过滤
  → try encoder init（硬→软）
  → 成功: VideoReady + CodecConfig + StreamHello(KEY)
  → 失败: 下一个 codec；全失败: CodecUnsupported
```

### 4.2 运行中切换

触发：client 改 `codec_pref`、host 编码器失能、解码端连续失败、带宽/CPU 变化、质量档变化。

1. host 发旧流 `StreamEnd{CODEC_SWITCH=3}`；client 丢弃旧流排队帧；
2. host 发 `VideoReady`（新 codec）+ `CodecConfig` + 新 `StreamHello`；
3. 新流首帧 `KEY`，`frame_id` 继续；
4. 新 codec 2 s 内失败 → 回退上一个 codec，发 `VideoAdjusted{reason=CODEC_FALLBACK}`；
5. 切换不重建会话，不改 `session_id`，不影响输入/文件/剪贴板平面。

### 4.3 解码失败

`DecodeError{display, codec, frame_id, reason}`（client→host）：
- 首次：host 发 `RequestKeyframe`；
- 5 s 内同一 codec 3 次：host 切到下一个候选 codec；
- 全部失败：`Error{CodecUnsupported}`。

---

## 5. 质量档与格式

| 档 | 默认 codec 顺序 | chroma | 建议 fps | quality | 用途 |
|---|---|---|---|---|---|
| `LOSSLESS` | AV1（q=0）→ H265/H264（q=1） | 4:4:4 → 4:2:0 | 15 | 100 | 截图、文字、设计稿 |
| `TEXT` | AV1/H265/H264 | 4:4:4 优先 | 30 | 90 | IDE、文档、表格 |
| `BALANCED` | AV1/H265/VP9/H264 | 4:2:0 | 60 | 70 | 默认办公 |
| `MOTION` | H264/AV1 | 4:2:0 | 60–120 | 50 | 视频、动画、游戏 |

- 4:4:4 只有双方 `chroma` 都置 I444 且 codec 支持时才用（H.264 High444、H.265 RExt、VP9 Profile1/3、AV1 High/Professional）；否则回 I420。
- 10/12-bit 只在双方 `bit_depth` 支持且内容确需时启用；默认 8-bit。
- 分辨率按 V3 §11.5：host 缩放到 client 请求的窗口设备像素，避免二次缩放。
- `quality` 是 codec QP/CRF 的映射提示；具体映射由实现给出（H.264 QP、H.265 QP、VP9 CQ、AV1 CQ）。
- **LOSSLESS 仍受 `max_kbps` 约束**：无损码率超限时先降 fps（15→10→5），再回 `TEXT` 档，不静默超带宽。
- **RAW 兜底**：5 种性能编码全部不可用（无共同 codec / 编码器 init 失败 / 解码连续失败）时，若双方都置 `RAW`，host 切到 `RAW`：每帧独立 KEY、只发脏矩形、≤5 fps、≤1080p；HUD 显示 `RAW 兜底`。RAW 不参与正常质量档排序，也不作为默认选择。

### 5.1 音频 jitter 与 A/V sync（补 Airhand 领先项）

- Opus 10 ms 包，走 QUIC DATAGRAM；每包带前 2 包副本（RED），丢 1–2 包由下一包补齐。
- 接收端 jitter buffer：目标 50 ms，按到达抖动在 20–200 ms 自适应；队列 >200 ms 丢音频，不阻塞视频。
- 时钟基线：包 `pts` 与到达时刻构成相对基线；连续空闲 >5 s 时重锚到最早排队包（`CLOCK_REANCHOR_IDLE_MS=5000`），`JitterStats.clock_reanchors` 计数；同时用到达时刻对 RTP `pts` 做最小二乘斜率估计 `clock_skew_ppm`，按 ppm 从 target 中扣除持续漂移。两者共同防止长会话中基线单向漂移把 target 顶死在 200 ms。10 分钟 60k 包回归要求 playout p95 ≤300 ms，1000 ppm 漂移需被识别。
- **A/V sync**：视频帧与音频包都带 host 采集时钟的 `pts`；开启同步时，视频延迟到与音频对齐（最多 300 ms），HUD 显示 `Sync +N ms`；默认关闭（打字/拖拽优先）。
- 音频 SLO：`SLO-AUDIO-1` 端到端 p95 ≤150 ms（LAN）；`SLO-AUDIO-2` 音画漂移 ≤20 ms（同步开启时）。
- 音频不可用时（无 Opus/设备）会话继续，只关音频平面。

### 5.2 每 viewer 独立账本（补 Airhand 领先项）

- 每个 viewer 独立维护：在途字节、`FrameAck` 基线、`max_fps` 上限、暂停/后台状态；实现见 `opsdesk-service::viewers::ViewerLedger`。
- 共享编码器，但**慢 viewer 只跳过自己的帧**并在下一个 KEY 恢复；不得让一个慢 viewer 把前台 viewer 拖到 5 fps；`ViewerSet::tick` 独立选择接收者。
- 共享编码器取"仍在接收的 viewer 中最高的码率估计"；质量设置合并规则：最高 fps、最高目标码率、最严格的分辨率上限、active viewer 的 codec；实现见 `ViewerSet::plan()`。
- `active` 只是调度提示；后台 viewer 默认 5–10 fps，前台 viewer 的实际 fps 仍由 host 能力决定；`BACKGROUND_MAX_FPS=10`。
- 每 viewer 的 `HostStats` 由各自会话上报；`VideoAdjusted.viewer_id` 尚未加字段，当前共享会话场景用会话 id 区分。
- 当前实现覆盖 4 viewer 账本/公平调度模拟；`ViewerSet::switch_active` 可把 foreground 从 1 切到 2/3/4，旧 active 立即降为后台 10 fps 上限，新的 `plan()` 跟随新 active codec（T14，调度器模拟）。
- R9 基准：`broadcast_host_four_viewers_share_one_encoder` 用 120 帧 1920×1080 合成帧过真实 H.264 编码器（81.6 fps、CPU 125.3%、1,245,234 B），再向 4 个 `HostSession` 分发；1 active 102/120、3 后台各 24/120、`encode_calls=120`。真实采集与 4 条真实 QUIC 会话仍未接。

> 这两节是"胜出或持平 Airhand"的前置：Airhand 已有 jitter/A-V sync 与每 viewer 账本（`architecture.md:64`、`:139-160`）。

---

## 6. 性能目标（按 codec）

| codec | 1080p60 目标码率 | 4K30 目标码率 | 解码端 | 备注 |
|---|---|---|---|---|
| AV1 | ≤ 5–6 Mbps | ≤ 18 Mbps | 硬解优先（RTX 40/Arc/Apple M3+），dav1d 软解 | 效率最高；硬编覆盖较新 GPU |
| H.265 | ≤ 6–7 Mbps | ≤ 20 Mbps | 硬解普遍 | 兼容与效率平衡 |
| VP9 | ≤ 7–8 Mbps | ≤ 25 Mbps | 硬解较普遍；libvpx 软解 | 开源、SVC |
| H.264 | ≤ 7–8 Mbps | ≤ 30 Mbps | 硬解最广；OpenH264 软编 | 兜底基线 |
| VP8 | ≤ 10–12 Mbps | 不推荐 | libvpx 软解 | 最简兜底 |
| RAW | 仅脏矩形；全屏 1080p@5 约 330 Mbps | 不推荐 | 无 | **仅排障/无编码器**，LAN |

- 端到端延迟、输入延迟、恢复时间沿用 V3 §3.1 的 SLO（80/150 ms、50/100 ms、150 ms）。
- T13 已实现：`HostSession::apply_video_config` 运行时按 `max_fps` 用 pts 隔帧；`max_fps=0` 只保持流、停止发帧；恢复发 `VideoReady` 并强制下一帧 KEY。测试 `max_fps_pauses_and_resumes_with_keyframe` 覆盖 60→30→0→60、暂停无帧、恢复非 KEY 拒绝与 KEY 首帧。T15 已实现 `HostSession::notify_focus`：失焦降为 10 fps、恢复回协商上限，并发 `VideoAdjusted{actual_fps}`；测试 `focus_loss_updates_actual_fps_and_throttles_video` 验证 200 ms 内通知与失焦 3 帧只发 1 帧。每种 codec 的 T1 实时基线、切换 T16、解码失败 T17 仍按矩阵逐项验证。
- 目标值是**合同**，需真机实测；没有实测前不得对外承诺码率数字。

### 6.1 对 Airhand 直连的达标判据（胜或平）

| # | 判据 | 通过条件 |
|---|---|---|
| T16 | 会话内编码切换 | 负载中从当前编码切到另一编码；新流以 `KEY` 开始，`frame_id` 连续；切换窗口 ≤ 300 ms，不重建会话 |
| T17 | 解码失败回落 | 注入 `DecodeError` 后 host 先发 IDR；连续失败按 `codec_pref` 回落；回落成功则会话不断，失败才 `Bye` |
| T18 | 五编码矩阵 | VP8/VP9/H264/H265/AV1 各自 1080p60 ≥30 fps（硬编可用时 ≥55 fps），实际 fps ≤ `max_fps` |
| T19 | 4:4:4 / 无损 | H264 High444、H265 RExt、VP9 Profile1、AV1 High 各跑一次 TEXT/LOSSLESS；文字可读、无静默超码率 |
| T20 | 音频同步 | `SLO-AUDIO-1/2` 达标；200 ms 抖动下音频不拖视频，丢包由 RED 补齐 |
| T21 | 每 viewer 公平 | 4 viewer（1 active + 3 后台）下，active 不被后台拖到 ≤10 fps；慢 viewer 只影响自己 |
| T22 | 兜底 | 强制所有 5 种编码器失败，RAW 仍能出画面（≤5 fps、仅脏矩形）；HUD 显示兜底 |

> 2026-11-18 T16 已满足（协议级）：`HostSession::switch_codec` 在会话内从 H264 切到 H265，先发旧流 `StreamEnd{CODEC_SWITCH=3}`，再发新 `VideoReady`/`CodecConfig`/`StreamHello`；新流首帧 KEY，`frame_id` 连续（1..5→6），`session_id` 不变。测试 `codec_switches_mid_stream_with_continuous_frame_ids` 观测切换开始到新 KEY 的客户端耗时 <300 ms。
>
> 2026-11-18 T19 已满足（编码/协商层）：H264 High444 `yuv444p`、H265 RExt `yuv444p10le`、VP9 Profile1 `yuv444p`、AV1 High `yuv444p` 各 30 帧 LOSSLESS；码率 340–805 kbps ≤8 Mbps；文字图四宫格逐字可读。`negotiate` 的 `QualityMode::Lossless/Text` 首选 I444 + 10-bit，四编码 profile/chroma/bit-depth 由单测锁定；运行时编码器切档仍由调用方执行。
>
> 2026-11-18 T2：`quic_tolerates_two_percent_loss_and_40ms_rtt` 用测试内 UdpProxy 每方向丢弃 2/100 个 UDP 报文，并对每包加 20 ms 单向延迟；客户端发 `InputBatch` 后收到 `Ping`，输入/控制 RTT 61 ms，30/30 视频帧在 3 s 内到达，2% 丢包由 QUIC 重传恢复。T2 转绿；代理模拟而非 OS netem，输入延迟用控制 RTT 近似。
>
> 2026-11-18 T22 已满足（协议/像素层）：`all_codecs_failing_falls_back_to_raw_dirty_rects` 先把 H264/H265/VP8/VP9/AV1 全部标记为 3 次失败，再切 RAW；`send_raw_frame` 只发送 Header.rects 对应的 BGRA 像素，测试逐像素还原两个脏矩形，每帧 KEY、≤1920×1080；app 在 `ready.codec==Raw` 时打印 `CLIENT_HUD RAW 兜底`。真实编码器全部故障与桌面 HUD 截图仍未做。

> 2026-11-18 T3：`lan_probe` 本地多流完成 500 MB 上传与下载；上传原始 2.112 s、zstd(3) 1.051 s，下载 `recv-file` 2.133 s；落盘 SHA-256 均为 `a08a92258f621b55d08ad1e84c90c2ea6286fc6b6c9a4dfa7156afb16c190170`。VM 500 MB 与中断续传仍未跑，不作为本项判据。

> 2026-11-18 T4：`watchdog_resumes_real_quic_after_two_second_outage` 在真实 QUIC 上关闭首连接，`run_session_with_watchdog` 等待 2 s 后新开连接并 `resume_with_store`，客户端收到恢复后的 Ping；`quic_survives_two_second_udp_blackout` 进一步用测试内 UdpProxy 双向丢弃全部 UDP 报文 2 s 后恢复，Ping1/Pong1→Ping2/Pong2，QUIC 会话存活无需重连。中断由 UDP 代理制造，不是物理拔网线；T4 已绿。
>
> 2026-11-18 T10：`opsdesk-net::tcp_conn::TcpConnection` 以 4 B 长度前缀分帧；rustls TCP 测试使用 ALPN `aerlyn/4-tcp` 跑完整 `HostSession`/`ClientSession` 握手，协商 H264、`max_fps=30`、`max_kbps=8000`。`TcpProfile` 固定视频帧 ≤256 KiB、fps ≤30、码率 ≤8 Mbps、音频关闭、文件 credit=4；`HostSession.tcp` + `clamp_tcp_selected` 在协商后强制压到 30/8000，`send_audio_fmt/frame` 直接拒绝；测试以 60 fps/8000 kbps 请求并断言收到 30/8000，另覆盖 128 KiB 未鉴权 Hello 读限。测试打印 `CLIENT_HUD transport=TCP`。这是显式 TCP 传输，UDP→TCP 自动切换与桌面 overlay 未做；T10 已绿。

> 2026-11-18 T8：本机 `h264_amf -hw_encoding 1` 完成 3840×2160@30、2 s/60 帧编码，墙钟 0.521 s（约 115 fps），文件 5,292,536 B（约 21.2 Mbps）；ffmpeg 解码 60 帧 0.230 s（约 260.9 fps）。4K 实时会话/HUD 未接，T8 只证明离线硬件编解码。

> 2026-11-18 T6：`FileOffer.extensions` 已承载 `Extensions::encode` 的 TLV 字节，`file_offer_carries_tlv_extensions_end_to_end` 验证 JSON 往返后已知 tag 保留、未知 tag 跳过并计数；未知 control/二进制 type 的跳过与 1024 连续上限仍生效。
>
> 2026-11-18 音频：`aerlyn-wire::audio::JitterBuffer`/`AudioRed` 已实现并有 200 ms 上界单测；`opsdesk-realtime` 本地 10 s H.264+Opus 链路实测 p95 71 ms、A/V 漂移 -9.5 ms、dropped=0。T11 因此转绿。T20 随后用应用层 200 ms 抖动 + 125 个丢帧 + RED 恢复 169 包 + `--av-sync` 门控复测：视频 60.6 fps、p95 69 ms、A/V -10.5 ms、dropped=0，转绿；注入是应用层调度，不是 OS netem。
>
> 2026-11-18 T17 已满足：`HostSession::handle_decode_error` 注入 H264 `DecodeError` 后先发 KEY（frame_id 1/2），5 秒内第 3 次按 `codec_pref` 切到 H265，发 `VideoReady`/`CodecConfig` 与新 KEY（frame_id 3），会话不断；全失败发 `Error{CodecUnsupported}` + `Bye`。测试见 `opsdesk-service/tests/full_plane.rs`。
>
> 2026-11-18 T18 已满足：`opsdesk-codec` + `opsdesk-bench` 五编码 1080p60 120 帧基线，编码 66.9–87.1 fps、解码 582–1132 fps、编码 CPU 128–585%（见 opsDesk 历史基准记录，未随 standalone 迁移）。`apps/opsdesk-realtime` H.264 全链路 10 秒 1080p60：软件 capture/encode 59.4、decode 59.5、Win32 present 59.2 fps、595/595 帧；硬件 `h264_amf` capture/encode 60.4、decode 59.3、present 59.1 fps、606/606 帧，复跑 59.1–59.3；`h264_mf` 作为不稳定对照保留。`--fps 30` 已验证 `max_fps=30`（host 29.4、client present 29.3），实际 fps 不超过上限。T16 的会话内五编码切换、VM 交互桌面抓屏不在 T18 判据内。

> **达标的定义（胜或平）**：T18–T22 全绿 + V3 的 T1–T15 全绿。它保证每个战场不落后；允许平局，不要求每维度都胜；任一判据红 = 未达标。这些是**验收判据**，不是当前实测结论。
>
> 目标口径统一为“直连场景胜出或持平（0 负；T18–T22 + T1–T15 全绿）”，不再写“全胜”或“每维度都胜”。

---

## 7. 注册表

### 7.1 codec id

| id | codec |
|---|---|
| 0 | H.264 |
| 1 | H.265 |
| 2 | VP8 |
| 3 | VP9 |
| 4 | AV1 |
| 5 | RAW（兜底，不计入 5 种性能编码） |
| 6–255 | 保留 |

### 7.2 profile 位图

| codec | bit0 | bit1 | bit2 | bit3 |
|---|---|---|---|---|
| H.264 | Baseline (66) | Main (77) | High (100) | High444 (244) |
| H.265 | Main (1) | Main10 (2) | RExt (4) | — |
| VP8 | Profile0 | — | — | — |
| VP9 | Profile0 | Profile1 | Profile2 | Profile3 |
| AV1 | Main (0) | High (1) | Professional (2) | — |
| RAW | BGRA8 | BGR565 | — | — |

### 7.3 caps 快速位（替换 V3 的 12–16）

| bit | 名称 | 含义 |
|---|---|---|
| 12 | `VIDEO_H264` | 支持 H.264 |
| 13 | `VIDEO_H265` | 支持 H.265 |
| 14 | `VIDEO_VP8` | 支持 VP8 |
| 15 | `VIDEO_VP9` | 支持 VP9 |
| 16 | `VIDEO_AV1` | 支持 AV1 |
| 34 | `VIDEO_RAW` | 支持 RAW 兜底（34 原为保留位） |

其余 caps 位沿用 V3；`VIDEO_JPEG` 不再定义。实现侧用 `aerlyn_wire::caps` 的 `VIDEO_*` 常量与 `VideoCaps`（`Hello/Challenge/Welcome::video_caps()`）避免散落裸位；未知位必须原样保留。

### 7.4 错误码（V3 已发布 + V4 新增）

| code | 名称 | class | 说明 |
|---|---|---|---|
| 50 | `CodecUnsupported` | 2 | 沿用 V3，无共同 codec |
| 51 | `VideoStalled` | 1 | 沿用 V3，视频停滞 |
| 52 | — | — | 保留，不复用；避免与 V3 已发布语义冲突 |
| 53 | `EncoderInitFailed` | 1 | host 编码器初始化失败 |
| 54 | `DecoderError` | 1 | client 解码失败 |
| 55 | `CodecFallback` | 0 | 已自动回退 |
| 60 | `InjectFailed` | 1 | 沿用 V3，部分输入注入失败 |
| 61 | `NoDesktop` | 1 | 沿用 V3，无可用桌面 |
| 62 | `UipiBlocked` | 2 | 沿用 V3，UIPI 拒绝 |

码段规则保持 V3“只增不改”：V3 已发布的 50/51/60/61/62 不移动；V4 新增 53/54/55；52 保留为空。实现以 `crates/aerlyn-wire/src/error.rs` 为准，并由 `published_v3_v4_codes_do_not_move` 单测锁定。

`VideoAdjusted.reason` 增加：`ENCODER_INIT`、`DECODER_ERROR`、`CODEC_FALLBACK`、`QUALITY_MODE`。

### 7.5 control 消息注册表（0.5）

这是 0.5 线的完整 control 注册表；`crates/aerlyn-wire/src/message.rs` 的 `CONTROL_MESSAGE_TYPES` 与本节由 `spec_consistency` 测试锁定。冻结后只增不改。

| type | 方向 | caps | 答复 / 说明 |
|---|---|---|---|
| `Hello` / `Auth` / `Resume` / `ChannelHello` | C→H | — | `Challenge` / `Welcome` / `ResumeResult` / `ChannelWelcome` |
| `Challenge` / `Welcome` / `ResumeResult` / `ChannelWelcome` / `Rejected` | H→C | — | 握手、恢复或通道拒绝 |
| `Bye` | 双向 | — | 关闭原因 |
| `VideoConfig` / `VideoStop` / `RequestKeyframe` | C→H | VIEW | `VideoReady` / `Ok` |
| `VideoReady` / `CodecConfig` / `VideoAdjusted` | H→C | VIEW | 协商、码流参数、调整 |
| `DecodeError` | C→H | VIEW | 驱动关键帧/回落 |
| `DisplayList` / `DisplayChanged` | H→C | MULTI_MONITOR | 显示器列表/变化 |
| `InputConfig` | C→H | INPUT | `Ok` |
| `InputState` / `InputStatus` | H→C | INPUT | 输入状态/注入回执 |
| `FileOffer` / `FileGet` / `FileList` / `FileCancel` | C→H | FILE_* | `FileAccept` / `FileMeta` / `FileListing` / `Ok` |
| `FileAccept` / `FileMeta` / `FileListing` / `FileDone` / `FileError` | H→C | FILE_* | 文件任务答复 |
| `FileMkdir` / `FileDelete` / `FileRename` | C→H | FILE_PUT | `Ok` / `Error` |
| `ClipPull` | C→H | CLIPBOARD_TEXT | `ClipOffer` |
| `ClipOffer` / `ClipAccept` / `ClipDone` | 双向 | CLIPBOARD_* | 剪贴板所有权与数据 |
| `Screenshot` | C→H | SCREENSHOT | `Saved{path}` |
| `Saved` | H→C | SCREENSHOT | 截图落盘路径 |
| `SendCad` | C→H | SAS | `Ok` |
| `AudioStart` / `AudioStop` | C→H | AUDIO_OPUS | `Ok` |
| `PreviewSubscription` | C→H | PREVIEW | 逐 peer 预览预算请求 |
| `SessionInfo` / `SessionEnded` / `ControlOwner` | H→C | SHARE_SESSION | 会话元数据/结束/控制者 |
| `PrivacyState` | H→C | PRIVACY | 隐私屏权威状态（单调 revision） |
| `ShareRequest` | C→H | SHARE_SESSION | 请求切换 share 模式 |
| `HostStats` | H→C | STATS | 实际 fps/编码耗时/丢弃 |
| `Ping` / `Pong` | 双向 | HEARTBEAT | `Pong` |
| `PlanePriorities` | 双向 | PRIORITY | 接收方回 `ack=true` |
| `Ok` / `Error` | 双向 | — | 通用答复/错误 |

文件浏览/下载请求的线上形状：

```
FileGet    { task_id u32, dir String, name String, offset u64 = 0 }
FileList   { task_id u32, dir String, offset u32 = 0, limit u32 = 500, show_hidden bool = false }
FileCancel { task_id u32, reason String = "" }
```

`FileGet`/`FileList`/`FileCancel` 只能由客户端发往主机；`task_id` 由客户端生成，主机在 `FileMeta`/`FileListing`/`Ok` 中原样回显。`FileList.limit` 必须在 `1..=2000`，缺省 500；超限或 0 由状态机以 `ProtocolError` 拒绝。`FileCancel` 成功回 `Ok{req_id=task_id}`，未知任务回 `UnknownTask`；取消不保证已写盘分块回滚，接收端按临时文件清理。`FileGet` 从 `offset` 处继续下载，任务大小在 `FileMeta` 到达前未知，允许为 0。

#### 7.5.1 关键 control JSON 字段

以下字段是 0.5 线 JSON 的稳定名称；`spec_consistency::control_field_registry_matches_spec`
同时检查本表与 Rust 结构体字段，防止 `w/h` 与 `width/height` 一类漂移。

| type | JSON 字段 |
|---|---|
| `Hello` | `version`, `public_key`, `caps`, `caps_required`, `codec_caps`, `caps_ext`, `actor_host`, `extensions` |
| `Welcome` | `version`, `caps`, `session_id`, `token`, `host_fp`, `resume_ticket`, `extensions` |
| `VideoConfig` | `req_id`, `display`, `codec_pref`, `max_fps`, `max_kbps`, `active`, `priority`, `scale`, `quality`, `quality_mode`, `chroma_pref`, `bit_depth_pref`, `extensions` |
| `VideoReady` | `req_id`, `display`, `codec`, `profile`, `level`, `chroma`, `bit_depth`, `width`, `height`, `max_fps`, `max_kbps`, `hw` |
| `FileOffer` | `task_id`, `dir`, `name`, `size`, `sha256`, `block_size`, `credit`, `block_sha_root`, `mtime_ms`, `conflict`, `compress`, `resume`, `extensions` |
| `FileAccept` | `task_id`, `resume_offset`, `received_bitmap`, `block_sha256`, `final_name`, `error`, `extensions` |
| `FileGet` | `task_id`, `dir`, `name`, `offset` |
| `FileList` | `task_id`, `dir`, `offset`, `limit`, `show_hidden` |
| `FileCancel` | `task_id`, `reason` |

恢复票不是独立消息，随 `Welcome.resume_ticket` 或 `ResumeResult.ticket` 下发：

```
ResumeTicket { id u64, nonce [16], exp_ms u64 }
Resume       { ticket_id u64, nonce [16], last_frame_id u32, last_input_seq u32 }
ResumeResult { ok bool, code u16, need u64, ticket ResumeTicket?, session_id u64 }
ChannelHello { channel u8, session_id u64, token [32] }
```

`Resume` 只允许作为 TLS 后首消息；进入业务状态后再收到 `Resume` 按 `ProtocolError` 处理。

`ResumeTicket` 的校验顺序固定为“id/nonce 匹配 → 未过期”，`exp_ms` 与 `now_ms` 必须使用同一时钟；TTL 为 300 s，成功恢复后必须换发新票，旧票立即作废。`Welcome.resume_ticket` 缺省为 `None`，表示本次会话不签发恢复票；服务端可由 `ServerConfig.resume_ticket` 注入一张票，并同时写入双方 `Session`。当前库已固化票据结构、校验、`client_resume`/`server_resume`、服务端 `TicketStore` 登记表与成功换票；`redeem` 先校验 session/nonce/过期/已用，再原子签发新票并标记旧票已用。票据拒绝时服务端写 `ResumeResult{ok:false}` 并返回 `ResumeRejected`，客户端收到 `ResumeDenied`；两侧分别由 `HostSession::resume_or_accept` 与 `ClientSession::resume_or_connect` 在同一 TLS 连接回退完整 `Hello`/`connect`，TLS/IO/协议错误不回退。恢复后的流状态由 §7.9 的 `ResumeSnapshot` 续接；`HostSession::resume`/`ClientSession::resume` 已接线；`HostSession::accept_with_store` 由 `TicketStore::issue` 签发首票，`HostSession::resume_with_store` 由 `TicketStore::redeem` 原子换票，旧票重放返回 `AlreadyUsed`。`issue` 在签发前自动调用 `prune_expired`，只清过期票；TTL 内已用票保留，以便重放仍返回 `AlreadyUsed` 而不是 `Unknown`。后台定时清理与 TTL 续期策略不在协议线格式内，由会话层实现。

T5 身份校验：客户端 `ClientConfig.expected_host_fp` 可选；设置后必须在 `Challenge.host_fp` 到达时比对，不匹配回 `Bye{HostFingerprintMismatch=11}` 并终止握手。TLS 证书校验仍由传输层负责，`untrusted_certificate_is_rejected` 覆盖证书不受信。握手完成后，主机把 `Session.token` 绑定到 `SessionMachine.channel_token`；`ChannelHello.token` 必须逐字节一致，否则回 `Rejected{TokenInvalid=13}`，不建立该 channel。

### 7.6 文件/剪贴板字节窗口（0.5）

`crates/aerlyn-wire/src/transfer.rs` 把 ch3/ch7 的块大小、credit 与按 offset 收块固化成纯逻辑；它不读文件、不压缩、不算 SHA-256。真实文件顺序读源、临时落盘与整文件 SHA-256 在 `crates/opsdesk-service/src/file_stream.rs`，由会话层驱动。

| 项 | 文件 ch3 | 剪贴板 ch7 |
|---|---|---|
| 默认块 | 1 MiB（新传输可自适应 64 KiB–4 MiB） | 256 KiB |
| 块范围 | 64 KiB–4 MiB | 固定 256 KiB |
| 初始 credit | 8（自适应 2–64） | 4 |
| 单项上限 | 8 GiB | 16 MiB |
| 聚合上限 | — | 64 MiB |
| 块数上限 | 8192 | 64 |
| 结束标记 | `FileEnd{sha256[32]}` | `ClipDone{seq}` |

- 发送方只有 `acked_upto` 之后的块算在途；`in_flight >= peer_credit` 时停读源，形成真背压。
- 接收方只接受 `offset == received_upto` 的块；重复块回同一 ack、不重复推进；`offset > received_upto` 回 `OutOfOrder{expected}`，`offset < received_upto` 且跨边界回 `Overlap`。
- 文件块必须是完整块，只有最后一块可以短；块级 SHA-256 与 `FileEnd.sha256` 的校验由落盘侧完成。空文件允许不发 `FileChunk`，直接发 `FileEnd`，摘要为空输入的 SHA-256。
- `FileBitmap` 固定 LSB-first：块 `i` 对应 `byte[i/8] & (1 << (i%8))`；未使用的尾位必须为 0。`validate_file_offer` 校验块参数与摘要长度，`validate_file_accept` 校验 `resume_offset`、位图长度/尾位和 `block_sha256[]` 的块数与每项 32 B。
- `FileDigestIndex` 在服务层计算块级 SHA-256 列表与根摘要：`root = sha256(concat(block_sha256[i]))`；`resume_accept` 用接收方已有字节生成 `FileAccept` 的位图与首个缺失偏移。`FileOffer.block_sha_root` 与 `FileAccept.block_sha256[]` 的线上长度分别受 32 B/块与 512 KiB 上限约束。
- `FileResumeHint{task_id, resume_offset}` 是 ch3 `type=6` 的 12 B 头；接收方在块偏移失配时回该提示，不中断任务。它只表达“从哪个未压缩偏移继续”，不替代 `FileAccept` 的位图协商。
- 剪贴板 `ClipDataHeader{seq,format,offset}` 的 `seq` 必须与 `ClipAccept` 一致；`ClipAck` 同时带 `received_upto` 与 `credit`。
- 剪贴板格式值域固定为 `0=text`、`1=rtf`、`2=html`、`3=png`、`4=dib`；未知值拒绝。`ClipOffer` 的 `items[].size` 单项 1 B–16 MiB，且 `sum(items[].size) ≤ 64 MiB`；`ClipItem::validate`/`ClipOffer::validate` 在 `SessionMachine` 入口执行。
- 文件窗口可由 `FileWindowConfig::adaptive(size, rtt_ms, available_kbps)` 计算：BDP=`kbps*rtt/8`，块大小按 4 KiB 对齐夹在 64 KiB–4 MiB，credit 夹在 2–64；`FileSendStream::open_adaptive`/`HostSession::send_file_stream_adaptive_window` 是新传输入口，旧固定参数入口保留。`FileOffer.credit` 由发送方携带自适应值（缺省 8），接收方必须用它构建 `FileRecvWindow` 并在后续 `FileAck.credit` 中继续广告，不能被 `DEFAULT_FILE_CREDIT` 覆盖。
- `FileSendCursor`/`ClipSendCursor` 用一段内存源驱动发送窗口并产出 `FileChunk`/`ClipData`；`FileRecvBuffer`/`ClipRecvBuffer` 按 offset 落地、重复块校验字节一致后幂等 ack，收齐后 `assemble()` 还原源。
- `FileSendStream` 按窗口顺序读真实文件并增量算 SHA-256，`next_chunk_zstd(level)` 输出 `FileChunkZstdPayload`；`FileRecvSink` 顺序写临时文件，`accept_zstd` 解压后按未压缩长度推进，`FileEnd.sha256` 匹配后才原子改名为最终文件，摘要不符删除半成品。
- 自适应发送用 `next_chunk_adaptive(level)`：压缩后体积必须 `< 原块 90%`（`ZSTD_MIN_RATIO_PERCENT=90`）才发 `FileChunkZstd`，否则回退 `FileChunk` 原始块；`HostSession::send_file_stream_adaptive` 按块选择两种消息，接收端同一状态机接受两种块。执行在 `opsdesk-service/src/file_stream.rs`；`aerlyn-wire` 只提供 `FileChunk`/`FileChunkZstdPayload` 的线格式与长度校验。阈值是发送策略，不改变线格式。
- `FileSendStream::open_from_accept` 用 `validate_file_accept` 的位图找第一个缺失块，从该偏移重读并重哈希前缀；`open_at` 允许从任意块边界恢复，`FileEnd` 仍覆盖整文件。`FileRecvSink::resume` 要求临时文件已按 `size` 预分配，重哈希前缀后从同一偏移续写；`detach` 保留临时文件供下一次恢复。
- `FileSparseSendWindow`/`FileSparseRecvWindow` 提供真正的位图调度：发送端跳过 `acked` 块、允许在途块乱序，接收端可接收任意顺序块，`ack` 只推进连续前缀。`opsdesk-service` 的 `FileSparseSendStream`/`FileSparseRecvSink` 把这两个窗口接到真实文件 I/O；`FileSparseSendStream::finish` 在全部块确认后重新读整文件算 SHA-256，`FileSparseRecvSink::finish` 校验摘要后原子改名。
- 发送端仍保留顺序 `FileSendStream` 作为流式默认路径；`FileSparse*` 用于已知位图的恢复/乱序调度。`FileResumeHint` 已由 `HostSession`/`ClientSession` 的 typed API 发送；`opsdesk-net` 已提供 `ReconnectPolicy`/`run_with_reconnect` 重连策略与 `opsdesk-client-core::watchdog` 重试原语，`lan_probe client --reconnect N` 已能对同一服务端自动重试并优先 `Resume`；自动检测断线的宿主循环与按平面循环仍未接。

### 7.7 变长载荷

固定头后的体由 `payload.rs` 统一校验；头本身仍由 `plane.rs` / `data.rs` 编解码。任何“头合法、体长度不对”的帧在进入会话层前就被拒绝。

| 消息 | 头 | 体 | 规则 |
|---|---|---|---|
| `CursorShape` | `CursorShapeHeader` 15 B | 光标像素/压缩数据 | `fmt=0` BGRA8，体长必须 `w*h*4`；`fmt=1` PNG，非空且 ≤1 MiB；`fmt=2` mask，体长必须 `2*ceil(w/8)*h`；`w/h` 均为 1–1024 |
| `FileChunk` | `offset` 8 B | 文件块 | 体长 1 B–4 MiB；offset 由窗口层校验 |
| `FileChunkZstd` | `FileChunkZstd` 12 B | zstd 压缩块 | 压缩体 1 B–4 MiB；`uncompressed_len` 1–4 MiB |
| `FileResumeHint` | 12 B（`task_id` + `resume_offset`） | 无 | ch3 `type=6`；偏移为未压缩字节偏移 |
| `ClipData` | `ClipDataHeader` 10 B | 剪贴板块 | 体长 1 B–256 KiB；`seq/format/offset` 由窗口层校验 |
| `AudioFrame` | `AudioHeader` 12 B | 音频包 | 体长 1 B–64 KiB；Opus 打包由音频层负责 |

- 所有解码器拒绝短头、空体、超上限和未知光标格式；不做静默截断。
- `FileChunkZstd` 的压缩/解压已由 `opsdesk-service/file_stream.rs` 提供；`FileBitmap`、`validate_file_offer/accept` 与 `FileResumeHint` 已由 `aerlyn-wire` 提供线格式与边界校验，块级摘要索引由 `FileDigestIndex` 计算；`FileSendStream::open_from_accept` 与 `FileRecvSink::resume` 支持连续前缀恢复，`FileSparseSendWindow`/`FileSparseRecvWindow` 与 `FileSparseSendStream`/`FileSparseRecvSink` 支持按位图乱序调度；会话层自动选路仍未接。
- `FrameAck` 是 ch6 `type=1` 的固定 20 B 头（`ack_frame_id u32 + recv_ts_us u64 + decode_done_us u32 + queue_depth u16 + dropped u16`），不是 JSON；`NetReport` 仍是 ch6 固定头。`SliceNack` 为 12 B 固定头加 0–16 个 `u64` 扩展字（总长 12–132 B，8 字节对齐），首字覆盖片 0–63，后续字依次覆盖 64–1023；旧 12 B 编码仍合法。

#### 7.7.1 二进制 type 注册表

每个非 ch0 帧按 `channel + type` 分派；固定头长度必须与 `aerlyn-wire::plane`/`aerlyn-wire::data`/`aerlyn-wire::message` 的实现一致。新增 type 只能追加，不得复用旧编号。

| channel | type | 名称 | 固定头 |
|---|---|---|---|
| 1 | 1 | `InputBatch` | 变长 |
| 2 | 0 | `StreamHello` | 14 B |
| 2 | 1 | `VideoFrame` | 36 B |
| 2 | 2 | `StreamEnd` | 1 B |
| 3 | 1 | `FileChunk` | 8 B |
| 3 | 2 | `FileAck` | 10 B |
| 3 | 3 | `FileEnd` | 32 B |
| 3 | 4 | `FileError` | 8 B + UTF-8 |
| 3 | 5 | `FileChunkZstd` | 12 B |
| 3 | 6 | `FileResumeHint` | 12 B |
| 4 | 1 | `CursorShape` | 15 B |
| 4 | 2 | `CursorReset` | 2 B |
| 4 | 3 | `CursorPos` | 15 B |
| 5 | 1 | `AudioFmt` | 7 B |
| 5 | 2 | `AudioFrame` | 12 B |
| 6 | 1 | `FrameAck` | 20 B |
| 6 | 2 | `NetReport` | 18 B |
| 6 | 3 | `SliceNack` | 12 B + 8n（n≤16） |
| 7 | 1 | `ClipData` | 10 B |
| 7 | 2 | `ClipAck` | 10 B |

`aerlyn-wire::PlaneMessage::decode(&Frame)` 是上表的统一 typed 解码入口：它按
`channel + type` 校验固定头并解码变长体，覆盖全部 20 个类型。
`FileError` 的文件数据面体是 `task_id:u32 | code:u16 | detail_len:u16 | UTF-8 detail`，
头 8 B，详情上限 4096 B；控制面的 `FileErrorInfo` 仍是 JSON，两者不混用。
`PlaneIo::recv_decoded` 对未知二进制 type 忽略并递增 `unknown_frames`，对
`ControlMessage::Unknown` 保留并递增 `unknown_controls`，连接不因未知 type 断开；
未知 control 与未知二进制帧各自连续达到 1024 条时返回 `InvalidData`，已知消息会
清零连续计数，防止未知消息占满读取循环。

#### 7.7.2 TLV 扩展容器

`aerlyn-wire::ext::Extensions` 定义通用扩展块：

```text
u16 count
repeat count:
  u16 tag
  u16 len
  u8  value[len]
```

约束：最多 64 项、编码后最多 4096 B、已知 tag 不得重复。`decode(data, known)`
只保留 `known` 中的 tag，其余 tag 跳过并返回跳过数量；截断、尾随、超限都报错。
0.5 线已在 `Hello`、`Welcome`、`VideoConfig`、`FileAccept` 与 `FileOffer` 上提供
`extensions` 字段（JSON 数组，元素为 `{tag,value}`）；其余消息保持无扩展位。
握手实际写入 `Tag 1 = wire_version(u16)` 与 `Tag 2 = caps(u64)`：客户端填
`Hello.caps_ext`/`Hello.extensions`，服务端在 `Challenge` 复制 `caps_ext`，并在
`Welcome.extensions` 回写同一组条目；因此握手转录中的 `sha256(caps_ext)` 不再是常量。
旧端遇到不认识的 tag 必须在容器解码时跳过并计数，已知字段照常解析。后续扩展字段
必须复用该容器，并在消息层把跳过计数上报到 `unknown_frames`/指标。

### 7.8 常驻会话编排

协议允许两种驱动方式，线上格式相同：

| 方式 | 入口 | 用途 |
|---|---|---|
| 一次性 | `serve` / `connect` | 协议联调、最小示例：协商 → 一帧视频 → 一个 ack 后返回 |
| 常驻 | `HostSession::accept` / `ClientSession::connect` | 真实会话：握手与协商只做一次，之后 control 与六平面共用连接 |

常驻会话的规则：

1. 握手与 `VideoConfig → VideoReady + CodecConfig` 只在会话建立阶段发生；
2. 之后 ch0 承载 control，ch1–7 承载 input/video/file/cursor/audio/feedback/clipboard；
3. control 必须先交给 `SessionMachine`：方向、状态、文件任务去重、Clip seq、Ping→Pong 由状态机判定，产生的答复立即回写；
4. 二进制帧不经过 control 状态机，由平面层按 `channel + type_id + flags + len` 分派；
5. 同一连接上的消息顺序即发送顺序；跨平面并发、每平面独立流和 credit 由会话层/传输层实现，不在本层承诺；
6. 任一平面解码失败只影响该平面，control 会话保持；会话级致命错误用 `Bye`/`Error` 结束。

当前库提供 `HostSession::send_control/send_frame/send_video/send_stream_hello/send_stream_end/send_host_stats/send_input_status/send_ack/send_file_chunk/send_file_chunk_zstd/send_file_end/send_file_ack/send_cursor_shape/send_cursor_reset/send_cursor_pos/send_clip/send_clip_ack/send_audio_fmt/send_audio_frame/recv/recv_decoded`，以及
`ClientSession::send_control/send_frame/send_ack/send_input/send_file_chunk/send_file_chunk_zstd/send_file_end/send_file_ack/send_cursor_pos/send_clip/send_clip_ack/send_audio_frame/send_stream_hello/send_stream_end/send_net_report/send_slice_nack/request_keyframe/recv/recv_decoded`。
`recv_decoded` 返回 `DecodedPlaneEvent::{Control,Plane}`，数据面帧统一走
`PlaneMessage` typed 解码；需要原始字节或自定义缓冲时仍可用 `recv`。
文件平面另有 `HostSession::send_file_stream` / `send_file_stream_zstd` / `send_file_stream_adaptive` / `send_file_stream_from` /
`send_file_stream_sparse`
和 `HostSession::recv_file_chunks` / `recv_file_chunks_zstd`，以及
`ClientSession::recv_file_chunks` / `recv_file_chunks_zstd` / `recv_file_chunks_sparse`：发送端按窗口连续发块、收 `FileAck`、
全部确认后发 `FileEnd`；接收端对每个块调回调、回 `FileAck`，返回 `FileEnd`。原始块与 zstd 块
使用各自的驱动；`send_file_stream_from` 从接收方 `FileResumeHint` 的偏移重建发送流，
`FileRecvSink::resume` 从预分配临时文件续写。`FileSparseSendStream`/`FileSparseRecvSink`
是服务层的稀疏驱动：按 `FileBitmap` 跳过已确认块、接收任意顺序块，`FileEnd` 仍校验整文件摘要；
`send_file_stream_sparse` / `recv_file_chunks_sparse` 已把稀疏窗口接进常驻会话，
测试先预置块 1/3，再让发送端跳过它们、接收端乱序落盘。
`tests/full_plane.rs` 验证一次协商后同连接交换输入、剪贴板、文件块/文件尾、`HostStats` 和
`StreamHello/StreamEnd`；`file_stream_roundtrip_over_host_session` 与 `zstd_file_stream_roundtrip_over_host_session`
用真实文件、窗口 ack、原始/zstd 块与 `FileEnd.sha256` 在 `HostSession`/`ClientSession` 上跑完整文件传输；
`host_recv_file_chunks_drives_client_sender` / `host_recv_file_chunks_zstd_drives_client_sender`
反向验证 host 接收驱动；`sparse_file_stream_roundtrip_over_host_session` 先预置块 1/3，
host 用 `send_file_stream_sparse` 跳过已确认块，client 用 `recv_file_chunks_sparse` 乱序接收并校验整文件；
`resume_hint_drives_prefix_resume_over_host_session` 先落两个块并 `detach`，
客户端发 `FileResumeHint`，host 用 `send_file_stream_from` 从偏移续发，客户端 `FileRecvSink::resume`
完成剩余块并校验整文件摘要。
按平面拆分的并发循环、连接断开后的自动 `Resume` 重建、输入注入/光标/音频高层 API 仍属会话层，不在 `aerlyn-wire` 协议核心内；重连退避策略与完整 `Hello` 重试已由 `opsdesk-net`/`lan_probe` 提供；
`FileResumeHint` 已由 `file_resume_hint_crosses_the_session_api` 与上述恢复测试验证会话 API。
RAW 兜底由 `raw_fallback_negotiates_and_streams_over_session` 验证：双方仅提供 RAW 时协商到
BGRA8 / 5 fps，会话发送 64 B RAW 帧并能按 `MediaHeader.codec() == Raw` 解出。
`client_stream_lifecycle_and_feedback_typed_api` 验证客户端发送 `StreamHello`、`StreamEnd`、
`NetReport`、`SliceNack`，host 在同一连接上按 channel/type 解码；同一测试还验证
`HostStats`、`InputStatus` 的 typed 下发与 `RequestKeyframe` 的 typed 请求。
`six_plane_typed_helpers_roundtrip` 验证 `ClipAck`、`CursorReset`、`AudioFmt` 的 host 下发
与客户端 `ClipAck` 回传。

### 7.9 恢复后的状态续接

`Resume` 只携带 `last_frame_id` 与 `last_input_seq`；服务端必须在 TLS 恢复协商成功后
用本地 `SessionMachine` 校验这两个值不能超前，再按快照续接：

| 字段 | 含义 | 续接规则 |
|---|---|---|
| `last_frame_id` | 客户端已收到的最后一个视频 `frame_id` | host 从 `last_frame_id + 1` 继续；首帧必须 KEY |
| `last_input_seq` | host 已应用的最后一批输入序号 | client 从 `last_input_seq + 1` 继续；旧序号可重放但不得倒退 |
| `files[task_id].received_upto` | 文件已落盘偏移 | 用 `FileAccept.resume_offset` 重开窗口；未确认发送块从该偏移重发 |
| `clip_seq` | 当前剪贴板序号 | 未完成项作废，重新 `ClipOffer`，不复用旧 seq |
| `audio` | 音频流是否开启 | 重新 `AudioStart`，不续接旧 jitter buffer |

实现约束：

- `SessionMachine::note_frame/note_input` 由会话层在收发时调用；`send_video` 和
  `send_input` 已自动记录，`recv` 对视频帧/输入帧解析后自动记录；
- `accept_resume` 拒绝 `last_frame_id` 或 `last_input_seq` 超前，返回
  `MachineError::ResumeAhead`；`restore_resume` 恢复文件偏移、剪贴板序号、音频开关与视频描述；
- `HostSession::resume`/`ClientSession::resume` 负责恢复握手、重建 `PlaneIo`、恢复快照与序号；`HostSession::resume` 必须在返回前拒绝超前值；
- 媒体在途帧、文件未确认发送块不进入快照，由媒体层按帧号/`FileAccept` 重新建立；
- 恢复后的 `frame_id` 必须继续递增，不得回绕到旧值；输入序号同理。

`ResumeSnapshot` 是本库对“恢复后续接”的最小状态定义；服务端/客户端恢复构造器、
真实媒体在途窗口和文件重传仍由会话层实现。

### 7.10 QUIC 同步适配

`aerlyn-wire` 会话是同步状态机，quinn 是异步传输。`opsdesk-net::sync_conn::QuicConnection`
把一条 QUIC 双向流包装成 `opsdesk-net::Transport`：

| 项 | 规则 |
|---|---|
| 线上分帧 | 每条消息前加 `u32` 大端长度；`send` 先写长度再写体，`recv` 先读长度再读体 |
| 上限 | 单条消息 64 MiB，与 `opsdesk-net::write_msg/read_msg` 一致 |
| 流模式 | 推荐默认：`PlaneConnection` 在同一个 QUIC 连接上开 8 条双向流，第 `ch` 条流承载 channel `ch`；单流兼容模式：一条双向流承载 control + 六平面；两种模式都保留 `PlaneIo` 的 `channel/type/flags/len` 头 |
| 构造 | async 上下文用 `open_async`/`accept_async`；同步线程用 `open`/`accept` |
| 阻塞边界 | `send/recv` 内部 `Handle::block_on`；必须在 tokio worker 之外调用，否则会 panic |
| EOF | 长度前缀读到 0 字节返回 `Ok(None)`；读到部分长度或体返回 `UnexpectedEof` |
| 关闭 | `close` 调用 `SendStream::finish` 并关闭 QUIC 连接 |

#### 7.10.1 `MessageIo` 硬契约

`MessageIo` 不是裸字节流接口，而是**消息边界接口**。这是类型/文档层面的硬约定，不是建议：

- `send(bytes)` 一次必须且只能提交**一条完整消息**；
- `recv()` 一次必须返回**一条完整消息**，不得返回半条，也不得把两条合并；
- 底层是 QUIC/TCP 字节流时，由该 `MessageIo` 实现自行完成长度前缀、`read_exact` 和重组；调用方不得再按字节切片二次分帧；
- `recv_control()` 只取控制面（ch0）的下一条消息；默认实现退化为 `recv()`。按平面多流实现必须覆写它，控制面读端不得从多通道汇聚队列取消息，否则会把 ch2/其它平面的帧头当 ch0 长度前缀解析（0.6.0 起的硬契约，见 `crates/aerlyn-wire/src/transport.rs` 与 `crates/opsdesk-net/src/plane_conn.rs` 的 `recv_control`）；
- `recv()` 返回 `Ok(None)` 表示对端正常关闭；读到半条长度前缀/半条消息必须返回 `UnexpectedEof`，不得静默截断或死循环；
- 违反上述任一条属于传输实现缺陷，不是协议可容忍的输入；`channel.rs` 的 `len + 4 != buf.len()` 是在检查该契约是否被遵守，而不是替代实现方的分帧。

`QuicConnection`（§7.10）与 `PlaneConnection`（按平面流）都必须在实现内部满足该契约；`MemoryConnection` 因一次 `send` 天然对应一次 `recv`，只用于内存回环测试，不能作为字节流实现的样板。

按平面模式（0.5.0 起的推荐默认）由 `opsdesk-net::plane_conn::PlaneConnection` 提供：客户端开 8 条 bidi 流，
每条先写一个零长度握手帧让服务端可见；`send` 按消息首字节 channel 路由，`recv` 由每流一个
后台读取线程汇聚到 mpsc。服务端 `accept` 收满 8 条流。它是 `opsdesk-net::Transport` 的另一实现，
不改变 `PlaneIo`/会话 API。每条发送流按 channel 设静态 QUIC 优先级
`[100,90,50,10,60,40,30,20]`（control/input/video/file/cursor/audio/feedback/clipboard），
`priorities()` 可读回；`set_priority(channel, priority)`/`set_priorities([i32;8])`
可在运行时调整本地发送权重（不跨端协商）。`open_extra(channel)` 可主动为某 channel 再开一条
bidi 流，首帧一个字节携带 channel，对端 `accept_extra()` 接收并改走新流；双方都可发起。
关闭时 join 读取线程，所有读取线程退出后 `recv` 返回 `Ok(None)`。当前限制：额外流需应用显式
accept，同一 channel 的多条流不做加权调度，也没有按码率/拥塞自适应权重；`lan_probe`
自 0.5.0 起默认使用该模式，`--single-stream` 回退单流兼容模式。

`tests/sync_conn.rs` 验证真实 QUIC 上的长度分帧与双向收发；
`tests/plane_conn.rs` 验证 8 条按平面流上的 channel 路由、静态/动态优先级、双向
`open_extra`/`accept_extra` 后新流接管 channel，以及 64 KiB 文件并发时 video p95 增量
282 µs、input p95 506.4 µs；
`opsdesk-service/tests/quic_session.rs` 在 QUIC 上跑完 `HostSession::accept` /
`ClientSession::connect` 的完整握手/编解码协商。

断线重建分三层：`opsdesk-net::reconnect::ReconnectPolicy`/`run_with_reconnect` 负责传输层指数退避；
`ClientSession::resume_state()` 摘出 `session/ready/codec_config/frame_id/input_seq`，
`resume_or_connect_from_state()` 用新连接优先 `Resume`，`ResumeDenied` 才回退完整 `Hello`；
`opsdesk-client-core::watchdog` 提供 `WatchdogPolicy`/`run_with_watchdog`、
`ClientSession::reconnect_with_watchdog` 与 `run_session_with_watchdog`：后者在
`recv_decoded` 返回错误时自动取出 `ClientResumeState`，由宿主提供的 connector
重建会话并继续收事件。
`lan_probe client --reconnect N --drop-first` 已验证：首次连接关闭后，第二次连接自动调用
`resume_or_connect_from_state`，优先 `Resume`；服务端用共享 `TicketStore` + `ResumeSnapshot`
接受并换票。`run_session_with_watchdog` 已把“检测 IO 错误 → Resume 重建 → 继续收事件”
做成库级循环；connector 与 endpoint 生命周期仍由宿主提供，重建周期上限等于
`WatchdogPolicy::max_attempts`。

2026-11-17 已在 Win11 VM（`192.168.99.244:9443`）用 `opsdesk-lan` 的 `lan_probe`
完成真实 LAN 验证：客户端拿到 `session_id=42`、H.264、1920×1080，服务端收到
`Ping` 并回 `Pong`。单流兼容模式验证了握手、协商、状态机与双向 control；
默认 8 流模式已在本地与同一 VM LAN 上验证握手、Ping→Pong、断线 Resume
和文件块传输（5 块 / 307,207 B / SHA-256 一致）。静态/动态本地优先级和双向 `open_extra` 已做；按码率/拥塞的自适应权重、跨端协商与常驻并发循环仍未做。

同一探针也已验证文件平面：

- 本地 QUIC：`307,207 B`、`64 KiB` 块、5 块，原始块与 zstd(level=3) 两种模式；
  `FileEnd.sha256` 通过，落盘文件与源文件 SHA-256 均为
  `630c13d6c735e7f0dc6dac7a5201fe7ee11e99f68f2b68bedfa12ca13fa47987`。
- VM 真机：`192.168.99.244:9444`（9443 被既有探针占用），zstd(level=3) 模式，
  5 块，VM 上 `Get-FileHash` 与源文件 SHA-256 一致。
- VM 需为测试 UDP 端口放行防火墙；本次额外放行 UDP 9444。探针二进制用
  `-C target-feature=+crt-static` 构建，避免 VM 缺 `VCRUNTIME140.dll`。

复现命令：

```bash
# 服务端（VM）
lan_probe.exe server --bind 0.0.0.0:9444 --cert server.crt --key server.key \
  --out recv_zstd.bin --size 307207 --zstd

# 客户端（本机）
lan_probe.exe send-file --path lan_src.bin --block 65536 --zstd --level 3 \
  --addr 192.168.99.244:9444 --ca ca.crt --server-name aerlyn.test
```

这验证了单流兼容模式下的文件读源、窗口 ack、zstd 压缩/解压、整文件摘要与落盘。
每平面一条流、control 与媒体分流、以及 QUIC 流级 credit 仍是后续传输层工作，
不改变本节的单流兼容语义。

---

### 7.11 RTT 与输入时钟基线

- `RttEstimator`：`base_ms = min(sample)`，`avg_ms` 用整数 EWMA（α=1/8）；由本地 Ping 发送时刻与 Pong 到达时刻采样，`Pong.rtt_ms` 上报本端最近平滑值，不冒充对端测量。
- `ClockBaseline`：`baseline = min(recv_ts_us − ts_us)`；更快样本立即更新，向上漂移每秒最多 1 ms；输入相对延迟为 `max(0, recv−ts−baseline)`。
- `HostSession::observe_input_clock`（`opsdesk-service`）记录并返回输入延迟；`ClientSession::heartbeat_on_pong`（`opsdesk-client-core`）把本地测得的 RTT 送入 `SessionMachine`。
- 典型值：LAN 输入 p95 0.198 ms；T2 代理 RTT 61 ms；D7 单测覆盖最快样本、漂移上限、Pong 上报与心跳本地 RTT。

### 7.12 超时与 token TTL

| 阶段 | 默认预算 |
|---|---:|
| QUIC 建连 | 3 s |
| TLS | 3 s |
| `Hello → Challenge` | 2 s |
| `Challenge → Auth` | 2 s |
| `Auth → Welcome` | 3 s |
| `ChannelHello → ChannelWelcome` | 1 s |
| 单条 control 请求 | 5 s |
| token TTL | 60 s |

- `TimeoutPolicy` 暴露上述 7 个阶段预算与 `token_ttl_ms`；`Deadline` 负责绝对期限与剩余时间，传输/宿主循环在到期时中断阻塞读。
- `Session.token_issued_ms` 记录 token 签发时刻；`SessionMachine::set_channel_token_at` 绑定签发时刻，`set_now_ms` 提供判定时钟。`ChannelHello` 在 token 错误或超过 60 s 时回 `Rejected{TokenInvalid}`；同 channel 重发是幂等重发 `ChannelWelcome`。
- 超时是协议常量与判定器，不在阻塞 `MessageIo::recv` 内自行抢占；调用方必须把 `Deadline` 交给传输层或使用带读超时的适配器。

### 7.13 QUIC 流并发配置

0.5.0 的 QUIC 传输基线（`opsdesk-net::transport::TransportProfile`）固定：

| 参数 | 值 | 说明 |
|---|---:|---|
| `max_concurrent_bidi_streams` | 16 | control、文件、额外双向流；8 平面默认占 8 条 |
| `max_concurrent_uni_streams` | 64 | 视频/音频/文件的单向流余量 |
| `keep_alive_interval` | 15 s | 保持 NAT 映射 |
| `max_idle_timeout` | 90 s | 空闲回收上限 |
| `stream_receive_window` | 8 MiB | 单流大帧/文件块 |
| `receive_window` | 32 MiB | 连接级接收窗口 |

这些值只影响本端 QUIC 栈，不改变线格式。规范中的旧值 30 s idle / BBR 默认已由 0.5.0 的 LAN 主场景决策覆盖：idle 90 s、Cubic 默认、BBR 仅环境变量 A/B。

默认会话入口为多流：`opsdesk_service::serve_multistream` 与 `opsdesk_client_core::connect_multistream` 在同一个 QUIC 连接上建立 8 条按平面 bidi 流；`serve`/`connect` 保留为单流兼容入口。

`open_extra`/`accept_extra` 替换同一 channel 的发送流前，必须先把旧读取线程的 `stop` 置位；旧 reader 在读取前后检查该标志，不再向 channel 队列投递。该行为避免同 channel 双投递，但没有独立的重复投递回归测试。

### 7.14 视频 DATAGRAM 分片

视频默认走可靠 `CH_VIDEO` 流；低延迟/可丢场景可显式选择 DATAGRAM。它不替代可靠流，也不进入 `media_in_flight` 窗口。

| 参数 | 值 | 说明 |
|---|---:|---|
| `VIDEO_DATAGRAM_MTU` | 1100 B | 低于通用 datagram 上限 1200 B，留协议余量 |
| `VIDEO_DATAGRAM_HEADER` | 20 B | 固定头 |
| `VIDEO_DATAGRAM_PAYLOAD` | 1080 B | 每片最大负载 |
| `MAX_VIDEO_FRAGMENTS` | 1024 | 单帧最多片数 |
| `VIDEO_DATAGRAM_KEY` | bit 0 | `flags` 的 KEY 标志 |
| `VIDEO_DATAGRAM_PARITY` | bit 1 | parity 片标志 |
| `VIDEO_DATAGRAM_FEC_GROUP` | 16 | 每 16 个数据片 1 个 parity |

固定头（大端）：

| 字段 | 宽度 | 说明 |
|---|---:|---|
| `frame_id` | 4 B | 视频帧 id |
| `frag_index` | 2 B | 从 0 开始 |
| `frag_count` | 2 B | 1..=1024 |
| `flags` | 2 B | bit0=KEY；bit1=PARITY；其余保留 0 |
| `codec` | 1 B | `Codec::id()` |
| `reserved` | 1 B | 0 |
| `pts_us` | 8 B | 采集时间 |

规则：

- 空帧也发 1 片；每片 `header+payload ≤ 1100`。
- 接收端按 `frame_id` 重组，允许乱序与重复；同一片重复必须字节相同，否则丢弃整帧并报冲突。
- 一帧的所有片必须 `frag_count`、`codec`、`pts_us`、KEY 标志一致；不一致丢弃整帧。
- DATAGRAM 不重传；`SliceNack.missing_bitmap` 首字覆盖片 0–63，`extra: Vec<u64>` 最多再覆盖到片 1023，因此大帧缺片可精确 NACK；20 ms 仍未补齐时发 `RequestKeyframe`。TCP 降级路径禁用视频 datagram。
- 可选 XOR FEC：每 16 个数据片追加 1 个 parity 片，`flags` 的 bit1 标记 parity，`frag_index` 为 group 序号、`reserved` 为 group 大小；接收端每组最多恢复 1 个丢片，未恢复的组仍走 SliceNack/Keyframe。`HostSession::send_video_datagram_fec` 为显式入口，默认仍不发 parity。
- 发送端 `HostSession::send_video_datagram`，接收端 `ClientSession::recv_video_datagram` + `VideoDatagramReassembler`；`recv_video_datagram_with_recovery` 可把缺片转入 `SliceRecovery`。realtime 示例仍默认走可靠流，属应用接线。

### 7.15 跨端流优先级

`PlanePriorities` 是双向 control 消息：

| 字段 | 类型 | 说明 |
|---|---|---|
| `req_id` | u32 | 请求 id，ack 必须原样返回 |
| `priorities` | `[i32;8]` | 索引 0–7 对应 control/input/video/file/cursor/audio/feedback/clipboard；数值越大越优先 |
| `ack` | bool | 发起方 false；接收方应用后回 true |

规则：

- 发起方发 `ack=false`；接收方 `SessionMachine` 产生同 `req_id`/`priorities` 的 `ack=true`，不重复应答 ack。
- `PlaneIo::send_control` 与 `recv` 在传输支持时调用 `Transport::set_stream_priorities`；`Unsupported` 表示该传输没有可调度的多流，静默忽略。
- `PlaneConnection` 将其映射到 `set_priorities`；单流/TCP 传输不实现，消息仍可安全通过。
- 默认数组为 `[100,90,50,10,60,40,30,20]`，与 `PlaneConnection::default` 一致。

### 7.16 `Transport` 超时与最后 channel

`Transport` 在消息边界契约之外提供：

- `recv_timeout(Duration)`：最多等待指定时间；超时返回 `ErrorKind::WouldBlock`，正常关闭返回 `Ok(None)`；默认实现返回 `Unsupported`。
- `last_channel() -> Option<u8>`：最近一次成功 `recv`/`recv_channel`/`recv_timeout` 返回消息的 channel；没有消息时为 `None`。单流实现可返回 `None`。
- 多流实现必须让三者在同一连接上保持一致；超时不得吞掉已经入队但未返回的消息。

### 7.17 多会话共享编码广播

`BroadcastHost` 是 `opsdesk-service/src/broadcast.rs` 提供的服务层协调器，不是新的线格式：

- 持有一组已完成协商的 `HostSession`，按会话索引建立 `ViewerSet`；第一个会话为前台，其余为后台。
- `broadcast_video` 先取 `ViewerSet::plan()`，再调用一次编码器闭包；`SharedVideoFrame.payload` 只产生一次。
- 每个 recipient 的 `ViewerId` 直接映射为会话索引；各会话仍走自己的 `HostSession::send_video`、在途窗口和 `FrameAck`。
- 编码前只对实际 `recipients` 校验 codec：与共享 plan 不一致的 background recipient 本帧跳过，不阻塞前台；前台 plan codec 由 active viewer 决定，避免把不可解码负载发给 viewer。
- 每会话 fps/码率由 `ViewerSet` 的 `ViewerConfig` 合并；帧级跳过由各 viewer 的 `ViewerLedger` 独立决定。

### 7.18 TCP 三档 QoS（R8）

`opsdesk-net/src/tcp_conn.rs` 的 `TcpQos` 在 TCP 降级线上提供三档策略，不是新线格式：

| 档 | 视频 fps | 视频 kbps | 单帧上限 | 进入条件 |
|---|---|---|---|---|
| Good | 30 | 8000 | 256 KiB | 默认；loss <20‰ 且 RTT <300 ms |
| Fair | 15 | 4000 | 128 KiB | loss ≥20‰ 或 RTT ≥300 ms |
| Poor | 8 | 2000 | 64 KiB | loss ≥50‰ 或 RTT ≥500 ms |

- 观察周期 `TCP_TIER_STEP_MS=3000`；降档立即，升档需连续稳定 `TCP_TIER_UPGRADE_MS=15000`。
- `observe(rtt_ms, loss_per_mille, now_ms)` 返回是否变化；`clamp()` 把协商结果压到当前档上限。
- `HostSession::on_net_report` 在 TCP 会话上以 `NetReport.rtt_us/loss_ppm` 调用 `TcpQos::observe`，切换档位后压 `selected` 并回 `VideoAdjusted{reason=CONGESTION}`；非 TCP 会话不走该路径。

### 7.19 动态流权重（R10）

`AdaptivePriorities` 在静态 `PlanePriorities` 基础上按发送 backlog 调整权重，不改变 control 消息形状：

- 基数组 `[100,90,50,10,60,40,30,20]`；backlog 越大，input/video/cursor 权重上调，file/audio/clipboard 下调。
- `observe(backlog_bytes)` 用 EMA 平滑后返回新数组；`PlaneIo::adapt_priorities(backlog)` 在传输支持时调用 `set_stream_priorities`，否则只返回计算值。
- 对端仍通过 `PlanePriorities` + ack 获知权重；动态权重必须有上限/下限，避免控制面饿死。
- `HostSession::on_net_report` 在 `queue_ms > 0` 时把队列压力折算成 8 平面 backlog 并调用 `adapt_priorities`；这是默认拥塞驱动入口。真实链路调度效果仍需实测。

## 8. 与 V3 的差异

| # | V3 | V4 |
|---|---|---|
| 1 | JPEG 基线 + H264/H265/VP9/AV1 预留 | **五编码全部定义**；JPEG 不做流式编码 |
| 2 | `VideoCodecs` 简单条目 | `CodecCap`（profile/level/chroma/bit depth/hw/temporal layers） |
| 3 | 单个 `codecs [u8]` | 有序 `codec_pref [u8]` + `quality_mode` |
| 4 | codec 切换靠重建流 | `VideoReady` 重协商 + `StreamEnd{CODEC_SWITCH}`，`frame_id` 连续 |
| 5 | 无 `CodecConfig` | 新增，承载 SPS/PPS/VPS/AV1 seq header |
| 6 | 无 `DecodeError` | 新增，驱动回落 |
| 7 | 单值 `proto_version` | 单值 `Hello.version`；冻结前不做版本协商，2.0.0 才与 1.0.0 协商 |
| 8 | ALPN `aerlyn/3` | `aerlyn/4`；V3 draft 退役；v2 适配器不变 |
| 9 | caps 12–16 = JPEG/H264/H265/VP9/AV1 | 12–16 = H264/H265/VP8/VP9/AV1 |
| 10 | 无质量档 | `LOSSLESS/TEXT/BALANCED/MOTION`（借 VNC 质量级/RDP 4:4:4） |
| 11 | 无零依赖兜底 | **RAW（id 5）**：5 种都不可用时的最后兜底，≤1080p/≤5fps/仅脏矩形 |

**迁移**：V4 端点不实现 `aerlyn/3`；v2 客户端仍走 V3 §22 的适配器；首次升级的证书/TOFU 处理同 V3 §22。

---

## 9. 决策登记

| 编号 | 决策 | 状态 |
|---|---|---|
| D-V4-01 | 五编码：VP8/VP9/H264/H265/AV1；JPEG 不做流式 | 〔定〕 |
| D-V4-02 | codec id 0..4，私有命名空间 | 〔定〕 |
| D-V4-03 | 客户端有序 `codec_pref`；空则 host 默认排序 | 〔定〕 |
| D-V4-04 | 硬件优先、软件回落；init 失败换 codec | 〔定〕 |
| D-V4-05 | `CodecConfig` 承载 extradata；关键帧自带参数集 | 〔定〕 |
| D-V4-06 | 会话内切换，`frame_id` 连续，新流首帧 KEY | 〔定〕 |
| D-V4-07 | profile/level/chroma/bit depth 全部协商 | 〔定〕 |
| D-V4-08 | 四档质量；LOSSLESS 优先 AV1 q=0 | 〔定〕 |
| D-V4-09 | 版本范围 + ALPN `aerlyn/4` | 〔定〕 |
| D-V4-10 | 传输/通道/会话/文件等沿用 V3 | 〔定〕 |
| D-V4-11 | V3 draft 退役；v2 适配器保留 | 〔待批准〕 |
| D-V4-12 | codec 码率目标按 §6，实测前不对外承诺 | 〔定〕 |
| D-V4-13 | **RAW（id 5）作为零依赖兜底**，5 种都不可用时启用；≤1080p/≤5fps/仅脏矩形 | 〔定〕 |
| D-V4-14 | 音频 jitter 20–200 ms 自适应 + RED + 可选 A/V sync（≤300 ms 视频延迟） | 〔定〕 |
| D-V4-15 | 每 viewer 独立账本；慢 viewer 只跳过自己，不拖前台 | 〔定〕 |
| D-V4-16 | 直连目标为“胜出或持平 Airhand”：0 负为底线，允许平局；T18–T22 + T1–T15 全绿 = 达标 | 〔定〕 |
| D-V4-17 | 不比较 relay/NAT/浏览器/成熟度（非直连或非协议维度） | 〔定〕 |
| D-V4-18 | 目标由“全胜”改为“胜出或持平”（2026-11-17 用户指令）；音频/多 viewer 按平局计，设计比分 8 胜 0 负 8 平 | 〔定〕 |
| D-V4-19 | 0.5.0 是开发线；1.0.0 才冻结，冻结前不做版本协商。冻结条件：全平面库完成 + T1–T22 达标 + P0 的 R1/R3/R4/R2 结构/回归完成 + 审计无漂移 + 用户批准；WAN 延迟/吞吐仅参考；冻结后只增不改 | 〔定〕 |
| D-V4-20 | 常驻会话为规范驱动方式：握手/协商一次，control 过状态机，六平面帧共用连接；一次性 `serve/connect` 仅作联调示例 | 〔定〕 |
| D-V4-21 | 场景口径：设计与质量验收以 LAN-only 为主；同一协议栈适用于 WAN 直连，但通信延迟/吞吐/跨 NAT 时长不作冻结门槛（2026-11-19 用户指令） | 〔定〕 |

### 需要你确认的 4 件事

1. **BASE 档只强制 H.264**（推荐）：其他 4 种按端点能力声明，避免小端点被迫全实现。
2. **4:4:4 只在双方支持时启用**（推荐）：默认 I420，文字场景可开 I444。
3. **AV1 无损作为 LOSSLESS 首选**（推荐）：AV1 不可用时退 H265/H264 q=1。
4. **H.265/AV1 的专利/许可**：只影响分发，不影响协议；若你要求"零专利风险"，默认排序改成 `AV1 > VP9 > H264 > VP8`，H265 仅显式开启。

---

## 附录 A：`CodecConfig.extradata` 格式

| codec | extradata |
|---|---|
| H.264 | `00 00 00 01` + SPS + `00 00 00 01` + PPS（Annex B） |
| H.265 | `00 00 00 01` + VPS + SPS + PPS（Annex B） |
| AV1 | sequence header OBU（`obu_type=1`），可带 metadata OBU |
| RAW | `{pixel_format u8(0=BGRA8,1=BGR565), bpp u8}` |
| VP8/VP9 | 空 |

关键帧内仍重复参数集：H.264 的 IDR 前带 SPS/PPS，H.265 的 IRAP 前带 VPS/SPS/PPS，AV1 的 KEY_FRAME 前带 sequence header。

## 附录 B：与参考实现的映射

| V4 | RustDesk | Airhand | FreeRDP/RDP | VNC |
|---|---|---|---|---|
| H.264 | `h264s`（`message.proto:30`） | `Codec::H264=0`（`session.rs:382`） | AVC444/AVC420 | — |
| H.265 | `h265s`（`:31`） | `Codec::H265=1`（`:384`） | HEVC（RDPGFX 新版） | — |
| VP8 | `vp8s`（`:32`） | — | — | — |
| VP9 | `vp9s`（`:27`） | — | — | — |
| AV1 | `av1s`（`:33`） | `Codec::AV1=2`（`:386`） | — | — |
| 码流选择 | `SupportedEncoding/Decoding` + `negotiated_codec` | `QualitySettings.codec` + host 支持列表 | caps 版本×flags | `SetEncodings` 有序列表 |
| 质量档 | `ImageQuality` + `custom_image_quality` | `QualitySettings{adaptive,priority}` | 动态画质/AVC444 | `QUALITY_LEVEL_0..9` |

## 附录 C：取各家所长（编码面）

| 来源 | 采纳 |
|---|---|
| RustDesk | 五编码 oneof、`SupportedEncoding/Decoding`、`custom_fps` 上限、多帧封装（V4 用 `layer` 预留） |
| Airhand | AV1>HEVC>H264 的效率排序、硬件优先、低延迟无 B 帧、长 GOP、`QualitySettings.fps` |
| RDP | AVC444 的 4:4:4、RDPGFX caps、ack/QoE、auto-reconnect |
| VNC | 有序编码偏好、质量/压缩级、cursor 伪编码、无损回退 |
| V3 | 直连 QUIC/TLS1.3、帧头/ack/ABR、多平面、文件块级续传、resume |

**不采纳**：JPEG 流式编码（V4 用五编码取代）、RDP 的动态虚拟通道全套、VNC 的请求-响应拉流、WebRTC/`rtc` 基础承载（SCTP RTO 1s）。
