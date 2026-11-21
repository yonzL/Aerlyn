---
type: implementation-status
status: active
date: 2026-11-17
version: 1.0.0
related:
  - docs/45-protocol-spec-v4.md
  - docs/46-v4-implementation-plan.md
  - crates/aerlyn-wire/
  - crates/opsdesk-service/
  - crates/opsdesk-client-core/
---

# Aerlyn Wire 协议库实现状态（1.0.0 已冻结）

> **1.0.0 已冻结（2026-11-21 用户批准）**（tag `v1.0.0`）：线格式、消息、字段和默认值自此不再改；后续改动走 2.0.0。
> 0.1.0/0.2.0/0.3.0 对应 V1/V2/V3，均为未发布 dev 线；0.5.0 不承担向前兼容。
> 冻结为 1.0.0 的条件见 `docs/45-protocol-spec-v4.md` §0.0：全平面库完成、T1–T22 达标、P0 的 R1/R3/R4/R2 完成、文档/实现审计无漂移、用户批准。T1–T22 全绿本身不等于冻结。
> T1–T22 的逐项判据、证据与红黄绿状态见 [`docs/53-t1-t22-acceptance-matrix.md`](53-t1-t22-acceptance-matrix.md)：当前 **22 绿 / 0 黄 / 0 红**。

## 1. 测试基线

```text
cargo test -p aerlyn-wire -p opsdesk-net -p opsdesk-service -p opsdesk-client-core -p opsdesk-codec -p opsdesk-lan --offline
aerlyn-wire:                 174 unit + 1 plane_loopback + 7 spec_consistency + 5 flow
opsdesk-service:      35 unit (7 file_stream + 2 quality + 4 viewers + 2 raw + 4 admission + 2 bandwidth + 1 v2 + 5 abr + 4 media_window + 4 file_stream adaptive)
                      + 1 loopback + 1 multi_session + 30 full_plane + 5 quic_session + 2 tcp_session + 1 multistream_session
opsdesk-client-core:  12 unit (watchdog + heartbeat)
opsdesk-codec:         6 unit (ffmpeg 命令映射/I420/合成帧/CPU 采样)
opsdesk-net:          30 lib (pacer/reconnect/tcp/transport/plane_io/backpressure) + 1 sync_conn
                      + 2 plane_conn + 1 m2_transport + 1 tcp_profile + 1 datagram（另有 1 ignored：idle 95 s）
                       (cargo test -p opsdesk-net --offline)
R1 空闲:               cargo test -p opsdesk-net --test idle --offline -- --ignored
                       idle_90s_keeps_connection_alive = 1 passed in 95.01s（keep_alive=15s / idle=90s）
P0-0 内存基线:         cargo test -p opsdesk-service --test p0_baseline --offline -- --ignored --nocapture
                       {"connect_us":431.1,"first_frame_us":4.0,"input_p95_us":0.5,
                        "file_video_us":118.3,"file_video_frames":200,
                        "viewer_active_sends":600,"viewer_total_sends":870,"transport":"memory"}
P0-0 QUIC 基线:        bash tools/p0.sh（P0_SKIP_IDLE=1 可跳过 95s 空闲）
                       {"connect_us":2721.1,"first_frame_us":99.4,"input_p95_us":85.9,
                        "file_video_us":8105.0,"file_video_frames":200,
                        "viewer_active_sends":600,"viewer_total_sends":870,"transport":"quic-loopback"}
VM LAN 探针:           opsdesk-lan/lan_probe：Win11 192.168.99.244（2026-11-19 控制台会话为 192.168.78.134）真实 QUIC 握手/协商/Ping→Pong
                       本地 + VM 文件平面：307,207 B / 5 块，原始块与 zstd(3) 的 SHA-256 均一致
                       本地断线重建：--reconnect 2 --drop-first 走 CLIENT_RESUME_ATTEMPT/SERVER_RESUME_ATTEMPT 后再次 PONG
合计:                 316 passed, 0 failed；另有 4 个 ignored 回归测试（95s 空闲、P0 内存、P0 QUIC、15s 心跳黑洞）
```

## 2. 已实现

| 模块 | 内容 |
|---|---|
| `codec.rs` | Codec 0..5（H264/H265/VP8/VP9/AV1/RAW）、CodecCap、profile/chroma/bit-depth 选择、`select_codec`、RAW 回落 |
| `capabilities.rs` | RustDesk 形状 `SupportedEncoding` / `SupportedDecoding` / `CodecAbility` |
| `settings.rs` | Airhand 形状 `QualitySettings`，多 viewer 合并 |
| `media.rs` | 36B `MediaHeader` 编解码、rect、frag、`decode_any` 保留未知 codec、extradata 校验 |
| `fragment.rs` | 视频帧 256 KiB 切片（`frag_count≤64`）、`LAST_FRAG` 规则、乱序/重复/缺口重组；`HostSession::send_video_rects` 自动切片，`ClientSession::recv_video_frame` 走 `VideoReassembler` |
| `quality.rs` | LOSSLESS / TEXT / BALANCED / MOTION，码率/fps/chroma 映射 |
| `message.rs` | 握手 + 视频 control + 53 个已结构化 control 类型（`CONTROL_MESSAGE_TYPES`）；未知 type 原样保留 |
| `control.rs` | ErrorReply / VideoStop / RequestKeyframe / Display / Input / File / Clip / Audio / HostStats / Ping/Pong / Resume / ChannelHello / SessionInfo 等 |
| `plane.rs` | StreamHello/StreamEnd/FileEnd/FileChunkZstd/FileResumeHint/CursorShapeHeader/CursorReset/AudioFmt/NetReport/SliceNack 固定头 |
| `ext.rs` | 通用 TLV `Extensions`：最多 64 项/4096 B、已知 tag 去重、未知 tag 跳过并计数、截断/尾随/超限校验；尚未嵌入现有消息 |
| `transfer.rs` | 文件 1 MiB 块 / credit 8、剪贴板 256 KiB 块 / credit 4、严格 offset 顺序、重复块幂等、ack 窗口；`FileSendCursor`/`ClipSendCursor` 内存源切块，`FileRecvBuffer`/`ClipRecvBuffer` 落地/重复校验/`assemble`；`FileBitmap` LSB-first 位图，`validate_file_offer/accept` 校验摘要长度、位图尾位与块数；`FileSparseSendWindow`/`FileSparseRecvWindow` 支持跳过已确认块、乱序接收和连续前缀 ack |
| `audio.rs` | `JitterBuffer`（20–200 ms 目标、EWMA、乱序/丢包跳队）、`AudioRed`（≤3 包/≤64 KiB）、`AudioPacket`；4 个单测覆盖乱序、200 ms 上界、RED 恢复、坏形状 |
| `payload.rs` | `CursorShape`（BGRA/PNG/mask）、`FileChunk`、`FileChunkZstd`、`ClipData`、`AudioFrame` 的变长体编解码与边界校验 |
| `tests/plane_loopback.rs` | input/cursor/file/clipboard/audio/feedback 六平面在内存连接上的通道/头/体往返 |
| `tests/spec_consistency.rs` | 可执行规格检查：§7.5 control 类型、§7.5.1 关键 control JSON 字段、§7.4 错误码、§7.7.1 二进制 type/channel/固定头长度与代码常量一致；`FrameAck` 固定 20 B |
| `opsdesk-service/file_stream.rs` | 真实文件顺序读源/临时落盘：`FileSendStream` 按 `FileSendWindow` 读块并增量 SHA-256（含空文件、原始块/zstd 块），`FileRecvSink` 顺序写临时文件、解压 zstd、`FileEnd.sha256` 校验通过后原子改名；摘要不符删除半成品；`FileDigestIndex` 计算块级 SHA-256 列表/根摘要，`resume_accept` 生成位图与首个缺失偏移；`FileSendStream::open_from_accept` 按位图首个缺失块重读/重哈希前缀，`FileRecvSink::resume` 从已预分配临时文件续写，`detach` 保留半成品；`FileSparseSendStream`/`FileSparseRecvSink` 已把稀疏窗口接到真实文件 I/O，支持乱序落盘和按位图跳过 |
| `machine.rs` | `Side`、`SessionMachine`、control 方向表、视频/文件/剪贴板/音频状态、Ping→Pong、错误状态；`ResumeSnapshot`/`note_frame`/`note_input`/`accept_resume`/`restore_resume` 续接恢复状态 |
| `handshake.rs` | 单版本 `Hello.version`（不协商）、2 RTT `Challenge`+`Welcome`、七段签名转录（nonce/alpn_major/version/host_fp/caps/caps_required/sha256(caps_ext)）、nonce/host_fp 32 B 校验、`Welcome.resume_ticket` 透传、`client_resume`/`server_resume` 与成功换票 |
| `tickets.rs` | 服务端票据登记表：session 绑定、nonce/过期/已用校验、原子换票、`prune_expired`、`issue` 自动清理过期票、`prune` 显式清理 |
| `channel.rs` | ch0 JSON、ch1–7 二进制帧、8 通道、1 MiB control / 16 MiB frame 上限；`read_incoming` 混合读取 |
| `plane_io.rs` | `PlaneIo` 常驻收发器：control 自动过状态机并回写答复，frame 直接返回；`PlaneEvent`；`recv_decoded` 返回 `DecodedPlaneEvent` 并走 `PlaneMessage` 解码 |
| `plane_message.rs` | `PlaneMessage::decode(&Frame)`：按 `channel + type` 统一解码 20 个数据面类型，校验固定头与变长体；`FileError` 为 8 B 头 + UTF-8 详情 |
| `opsdesk-net/sync_conn.rs` | `QuicConnection` 把一条 QUIC bidi 流包装成 `opsdesk-net::Transport`：u32 长度前缀、64 MiB 上限、`open_async`/`accept_async` 与同步 `open`/`accept`、`Handle::block_on` 阻塞边界；单流兼容模式承载 control + 六平面 |
| `opsdesk-net/plane_conn.rs` | `PlaneConnection`：同一 QUIC 连接上 8 条 bidi 流按 channel 路由，客户端先写零长度握手帧、服务端收满 8 条；后台读取线程汇聚到 mpsc；`open_extra`/`accept_extra` 支持双方主动加流；替换 sender 前用 `reader_stops` 的 `AtomicBool` 停旧 reader；静态/动态优先级；仍实现 `opsdesk-net::Transport` |
| `opsdesk-net/transport.rs` | R1：`TransportProfile` 定制 keep_alive 15s、idle 90s、stream 8 MiB、receive 32 MiB、send 16 MiB、`max_concurrent_bidi_streams=16`、`max_concurrent_uni_streams=64`；`optimized_transport()` 注入所有 QUIC endpoint；`idle_90s_keeps_connection_alive` 已过 95s；R1 配置下 `m2_transport_t1_t5` 本地 500 MB 339.6 MB/s、输入 p95 0.198 ms，T4 UDP 黑洞 2s 复跑通过；拥塞控制默认 Cubic，BBR 可经 `AERLYN_CONGESTION=bbr` A/B（本地 BBR T1 967.7 fps / 500 MB 315.6 MB/s，低于 Cubic，故不默认）；WAN 16 MiB 40.664 s ≈3.30 Mbps 作为参考，不作冻结门槛 |
| `tests/sync_conn.rs` | 真实 quinn 连接上的长度分帧、双向消息往返与关闭；验证同步适配层 |
| `tests/plane_conn.rs` | 8 条按平面流上的 channel 路由、静态/动态优先级与 `open_extra`/`accept_extra` 测试：ch1/ch3/ch5 发送、ch2 回包，服务端/客户端各主动加流并验证接管；D14 旧 reader 停投递无独立回归测试；`serve_multistream`/`connect_multistream` 多流入口测试已加 |
| `opsdesk-service/tests/quic_session.rs` | 真实 QUIC 上 `HostSession::accept` + `ClientSession::connect` 的完整握手/VideoConfig/VideoReady/CodecConfig |
| `opsdesk-lan/src/bin/lan_probe.rs` | 真实 LAN 探针：同一二进制可作 server/client/send-file；Win11 VM `192.168.99.244`（2026-11-19 控制台为 `192.168.78.134`）上完成握手/协商、Ping→Pong，以及原始块/zstd(3) 文件传输（307,207 B / 5 块 / SHA-256 一致） |
| `opsdesk-codec` | ffmpeg 进程后端：`EncoderConfig`/`Frame`/`VideoEncoder`/`VideoDecoder`；H264/H265/VP8/VP9/AV1 五编码映射；`apps/opsdesk-bench` 输出 JSONL + 表格；1080p60 软件基线见 `docs/54` |
| `apps/opsdesk-realtime` | 实时 H.264 本地链路：`ffmpeg gdigrab → libx264/h264_mf → HostSession::send_video → QUIC → ffmpeg low_delay 解码`；软件 10 秒 1080p60 capture/encode 59.4、decode 59.5、Win32 窗口 present 59.2 fps、首帧 105 ms、595/595 帧无丢帧；硬件 `h264_mf`（AMDh264Encoder）最佳一次 capture/encode 59.0、decode 59.1、`SetDIBitsToDevice` 窗口 present 58.8 fps、591/591 帧，但重复运行出现 34.6 fps / 0 帧，硬件路径不稳定；`--fps 30` 时 `select_codec` 返回 `max_fps=30`，host 以 29.4 fps 采集/发送（294 帧），窗口 present 29.3 fps；`--present window` 走 Win32 DIB，`memory` 仍可回归 |
| `error.rs` | 50/51/52 保留/53/54/55/60/61/62；只增不改单测 |
| `opsdesk-service` | `serve` 一次性视频流程；`HostSession::accept` 常驻会话，协商后收发 control/六平面帧；typed 方法覆盖视频流生命周期、文件块/文件尾/ack、光标形状/位置、剪贴板、音频；`send_video` 自动记录 `frame_id`，`recv` 自动记录输入 `seq`，`send_next_video` 自动递增；`send_file_stream`/`send_file_stream_zstd`/`send_file_stream_adaptive`/`send_file_stream_from` 按窗口驱动 `FileSendStream` 并在全部 ack 后发 `FileEnd`，`from` 从 `FileResumeHint` 偏移用 `open_at` 续发；`FileSparseSendStream`/`FileSparseRecvSink` 提供按位图跳块/乱序落盘的真实文件驱动；`send_file_stream_sparse` 已把稀疏发送窗口接进常驻会话；`recv_file_chunks`/`recv_file_chunks_zstd` 对原始/zstd 块回调、回 ack 并返回 `FileEnd`；`send_host_stats`/`send_input_status` 补齐 HostStats 与 InputStatus 的 typed 下发，`send_clip_ack`/`send_cursor_reset`/`send_audio_fmt` 补齐剪贴板 ack、光标复位与音频格式 typed 下发；`recv_decoded` 提供 `DecodedPlaneEvent` 统一解码入口；`HostSession::accept_with_store` 用 `TicketStore::issue` 签发首票；`HostSession::resume` 用 `ResumeSnapshot` 恢复并校验 `ResumeAhead`；`HostSession::resume_with_store` 用 `TicketStore::redeem` 原子换票并拒绝旧票重放；`HostSession::resume_or_accept` 在票据拒绝后同连接回退完整 `Hello` |
| `opsdesk-client-core` | `connect` 一次性视频流程；`ClientSession::connect` 常驻会话，协商后收发 control/六平面帧；typed 方法覆盖输入、文件块/文件尾/ack、光标位置、剪贴板、音频；`send_input`/`send_ack`/`recv` 自动记录输入 seq 与视频帧号，`send_next_input` 自动递增；`recv_file_chunks`/`recv_file_chunks_zstd`/`recv_file_chunks_sparse` 对原始/zstd/稀疏块回调、回 ack 并返回 `FileEnd`；`ClientSession::resume` 沿用上一会话的 `VideoReady`/`CodecConfig` 与序号；`ClientSession::resume_or_connect` 在 `ResumeDenied` 后同连接回退 `connect`；`send_stream_hello`/`send_stream_end`/`send_net_report`/`send_slice_nack`/`request_keyframe` 补齐客户端视频流生命周期、反馈平面与关键帧请求 typed API；`send_clip_ack` 补齐客户端剪贴板确认；`recv_decoded` 提供 `DecodedPlaneEvent` 统一解码入口；`resume_state()`/`resume_or_connect_from_state()` 提供断线重建所需的会话快照与 Resume/完整 Hello 入口；`watchdog::WatchdogPolicy`/`run_with_watchdog`/`reconnect_with_watchdog` 提供指数退避重试原语，`run_session_with_watchdog` 在收事件错误时自动重建并继续 |
| `tests/full_plane.rs` | 一次协商后同连接交换视频帧、输入、剪贴板、文件块/文件尾、HostStats、StreamHello/StreamEnd；`file_stream_roundtrip_over_host_session` 用 `send_file_stream` + `recv_file_chunks` 在 HostSession/ClientSession 上跑真实文件与窗口/ack；`zstd_file_stream_roundtrip_over_host_session` 用 `send_file_stream_zstd` + `recv_file_chunks_zstd`；`sparse_file_stream_roundtrip_over_host_session` 预置块 1/3，用 `send_file_stream_sparse` + `recv_file_chunks_sparse` 跳过已确认块并乱序落盘；`host_recv_file_chunks_drives_client_sender` / `host_recv_file_chunks_zstd_drives_client_sender` 反向验证 host 接收驱动；`file_resume_hint_crosses_the_session_api` 验证 ch3 `type=6` 的 typed API；`resume_hint_drives_prefix_resume_over_host_session` 先落两块并 `detach`，再用提示偏移续发/续写并校验整文件；另用恢复构造器验证 `frame_id` 与输入 seq 从上一会话续接、host 拒绝超前值；`TicketStore` 路径验证首票签发、旧票换新票与重放拒绝；`resume_denied_falls_back_to_full_hello_on_same_connection` 用错误 nonce 触发拒绝并完成同连接回退；`raw_fallback_negotiates_and_streams_over_session` 用仅有 RAW 的 caps 强制协商到 BGRA8/5fps，并验证 64 B RAW 帧头与载荷；`client_stream_lifecycle_and_feedback_typed_api` 验证客户端 `StreamHello`/`StreamEnd`、`NetReport`/`SliceNack` typed API 与 host 解码，并覆盖 host `HostStats`/`InputStatus` 下发与客户端 `RequestKeyframe` 请求；`six_plane_typed_helpers_roundtrip` 验证 HostSession 的 `ClipAck`/`CursorReset`/`AudioFmt` 下发与客户端 `ClipAck` 回传 |

## 3. 尚未实现（按冻结条件）

- **control 注册表缺口**：53 个类型已结构化并接入 `SessionMachine`；`ResumeTicket` 能从 `ServerConfig` 经 `Welcome` 透传到双方 `Session`，并有 id/nonce/过期校验、`TicketStore` 登记/原子换票/旧票作废与 `client_resume`/`server_resume`。
- **数据面缺口**：`FileSendWindow` / `FileRecvWindow` 已实现块序与 credit，`FileChunk` / `FileChunkZstd` / `FileResumeHint` / `CursorShape` / `ClipData` / `AudioFrame` 已有线格式与边界校验；`file_stream.rs` 已有真实文件顺序读源、zstd、临时落盘、整文件 SHA-256、`FileDigestIndex`/`resume_accept`、`open_from_accept`/`resume` 连续前缀恢复；六平面已有内存回环；`FileResumeHint` 已能驱动 `send_file_stream_from` + `FileRecvSink::resume` 的连续前缀恢复，稀疏窗口与 `FileSparseSendStream`/`FileSparseRecvSink` 已支持按位图乱序调度，`send_file_stream_sparse`/`recv_file_chunks_sparse` 已接进常驻会话；`opsdesk-net` 已提供 `ReconnectPolicy`/`run_with_reconnect` 退避重试，`ClientSession::resume_state()`/`resume_or_connect_from_state()` 可携带快照优先 `Resume`；`lan_probe --reconnect 2 --drop-first` 已验证首次连接关闭后第二次连接走 `CLIENT_RESUME_ATTEMPT`/`SERVER_RESUME_ATTEMPT` 并再次 `PONG`。`watchdog` 已含自动检测/重建循环；仍缺 endpoint/connector 的库级管理、会话层自动选路、真实 Opus 打包；`StreamHello` 会话内切换已由 T16 覆盖。
- **全平面编排（部分）**：`HostSession`/`ClientSession` 已提供协商后的常驻 control + 六平面 typed 收发，文件平面另有 `send_file_stream`/`recv_file_chunks` 顺序驱动，并有输入/剪贴板/文件/HostStats/流生命周期回环测试；`QuicConnection` 已在真实 QUIC 上验证完整握手/协商，`PlaneConnection` 已提供 8 条按 channel 路由的 bidi 流，并设静态优先级 `[100,90,50,10,60,40,30,20]`，`set_priority`/`set_priorities` 可动态调整；LAN 探针自 0.5.0 起默认 8 流（`DEFAULT_MULTISTREAM=true`，`--single-stream` 回退兼容），本地与 Win11 VM LAN 均已验证握手、Ping→Pong、断线 Resume 与文件块传输；本地并发测试 100 轮 64 KiB 文件 + input + video，video p95 168.4→450.4 µs（增量 282 µs）、input p95 506.4 µs。动态权重已接 `PlanePriorities` 控制面；还没有服务端主动开流、常驻并发循环、服务端输入注入/光标渲染的应用实现；客户端 Opus 播放已接本地链路，但缺网络抖动/丢包注入与 A/V sync 开关。
- **输入面**：`InputBatch` 结构已有，但没有 server 注入、sequence/InputStatus 完整闭环。
- **续传/恢复**：`ResumeTicket` 300s、`Resume/ResumeResult`、票据透传与校验、`TicketStore` 登记/原子换票/旧票作废、`client_resume`/`server_resume` 已有；`ResumeSnapshot` 已能续接 `frame_id`/输入 seq/文件落盘偏移/剪贴板 seq/音频开关，并在 `accept_resume` 拒绝超前值；`HostSession::resume`/`ClientSession::resume` 已接线并有集成测试；`HostSession::accept_with_store`/`resume_with_store` 已接 `TicketStore` 首票签发与原子换票，旧票重放返回 `AlreadyUsed`；TTL 清理已由 `TicketStore::issue` 自动调用 `prune_expired`（保留 TTL 内已用票以识别重放）；后台定时周期清理与真实媒体在途窗口仍未续接；`resume_or_accept`/`resume_or_connect` 的回退路径已实现并通过错误 nonce 集成测试。
- **验收**：T1、T2、T3、T4、T5、T6、T7、T8、T9、T10、T11、T12、T13、T14、T15、T16、T17、T18、T19、T20、T21、T22 已绿；T1 以 V4 等价口径转绿：V3 线退役，实时 H.264 1080p60 595/595、present 59.2 fps，六平面/文件/音频/多会话证据分别由 HostSession/ClientSession 测试覆盖；T5 已由 `expected_host_fp`（不匹配回 `Bye{11}`/`FingerprintMismatch`）、`untrusted_certificate_is_rejected`、`bad_signature_rejected` 和 `channel_hello_requires_session_token`（错误 token 回 `Rejected{13}`）覆盖；T6 已用 `FileOffer.extensions` 端到端携带 `Extensions` TLV 字节，未知 tag 跳过并计数，且现有未知 control/二进制 type 跳过与 1024 连续上限仍在；T8 用 `h264_amf -hw_encoding 1` 编码 3840×2160@30、2 s/60 帧，墙钟 0.521 s（约 115 fps），ffmpeg 解码 60 帧 0.230 s（约 260.9 fps），文件 5,292,536 B（约 21.2 Mbps）；T11 音频抖动缓冲/RED 已有单测与本地 Opus 链路（p95 71 ms、dropped=0），T20 音频同步已用应用层 200 ms 抖动/125 个丢帧 + RED 恢复 169 包 + `--av-sync` 门控验证（p95 69 ms、A/V -10.5 ms、视频 60.6 fps、dropped=0）；T13 由 `HostSession::apply_video_config` 验证运行时 30 fps 隔帧、`max_fps=0` 暂停无帧、恢复强制 KEY（`frame_id` 10/12/15，非 KEY 被拒绝）；T14 `ViewerSet::switch_active` 在 4 viewer 中切换 active，切换后新 active 300 帧全发、旧 active 降为后台 <100 帧，`plan()` 跟随新 active codec（调度器模拟；T9 已有 4 条独立 QUIC 会话，D13 库级一次编码/多 recipients 已测，真实广播编排已由 BroadcastHost 覆盖）；T15 `HostSession::notify_focus` 失焦降为 10 fps、恢复回 60 fps，并发 `VideoAdjusted`；测试 `focus_loss_updates_actual_fps_and_throttles_video` 在 200 ms 内收到通知，失焦 3 帧只发 1 帧，恢复后 3/3；T16 `HostSession::switch_codec` 会话内 H264→H265，旧流 `StreamEnd{CODEC_SWITCH}`，新流 `StreamHello` + KEY，`frame_id` 1..5→6 连续，客户端观测切换 <300 ms；T17 注入 H264 `DecodeError` 后 host 先发 KEY（frame_id 1/2），5 秒内第 3 次切到 H265 并发 `VideoReady`/`CodecConfig`，新 KEY frame_id 3；全失败发 `Error{CodecUnsupported}` + `Bye`。T18 五编码 1080p60 软件基线均 ≥60 fps 编码、≥560 fps 解码；`opsdesk-realtime` H.264 10s 1080p60 软件链路 capture/encode 59.4、decode 59.5、present 59.2 fps，硬件 `h264_amf` capture/encode 60.4、decode 59.3、present 59.1 fps（复跑 59.1–59.3）；`--fps 30` 时 host 29.4、present 29.3 fps，不超过 `max_fps`。`h264_mf` 仍作为不稳定对照；T16 五编码会话内切换另行跟踪；VM 交互控制台已验证 18.8 fps 实时 host→宿主 client，gdigrab error 5 仅属于无桌面的 SSH 会话。T19 已用 H264 High444、H265 RExt、VP9 Profile1、AV1 High 四份 4:4:4/无损 30 帧样本证明文字可读、码率 340–805 kbps ≤8 Mbps，并在 `negotiate` 单测覆盖四编码 profile/chroma/10-bit；T21 的 `ViewerSet` 在 600 帧模拟中让 active viewer 600/600 发送、3 个慢背景 viewer 均 <200 帧且只丢自己的帧，慢 viewer ack 后下一 KEY 恢复，`plan()` 合并最高 fps/最高码率/最严格分辨率/active codec。T22 已用 `all_codecs_failing_falls_back_to_raw_dirty_rects` 强制五种性能编码失败后切 RAW，`send_raw_frame` 发送真实 BGRA 两个脏矩形并逐像素还原，每帧 KEY、≤1920×1080，app 在 `ready.codec==Raw` 时打印 `CLIENT_HUD RAW 兜底`；T10 已用 `opsdesk-net::tcp_conn::TcpConnection` 的 4 B 长度分帧与 rustls TCP 全握手验证：ALPN `aerlyn/4-tcp`、H264/30 fps/8 Mbps 协商、`TcpProfile` 256 KiB/无音频；T12 已用 `opsdesk-service::v2::v2_accept` 读 v2 JSON Hello/Auth、验签后回 `Welcome::Ok`，`v2_ping` 回 Pong，覆盖 v2 客户端控制面连接 V4 host；T7 已用 `AdmissionLimiter` 全局 64、每 IP 8、30 s 宽限和 128 KiB 握手消息上限，`channel::read_control_msg_limit` 由握手路径调用，`lan_probe` accept 前拒绝超限连接，4 个 admission 单测与 1 个 channel 上限单测覆盖；T3 已用 `lan_probe` 本地多流完成 500 MB 上传（原始 2.112 s、zstd 1.051 s）与下载（2.133 s），落盘 SHA-256 均为 `a08a92258f621b55d08ad1e84c90c2ea6286fc6b6c9a4dfa7156afb16c190170`。T4 已用 `watchdog_resumes_real_quic_after_two_second_outage` 在真实 QUIC 上关闭首连接、2 s 后由 watchdog 新开连接并 `resume_with_store` 收到恢复后的 Ping；另新增 `quic_survives_two_second_udp_blackout`，在客户端与服务端之间用 UDP 代理双向丢弃全部报文 2 s，Ping1/Pong1→2 s 全丢包→Ping2/Pong2，QUIC 会话存活、无需重连，因此 T4 转绿。T2 新增 `quic_tolerates_two_percent_loss_and_40ms_rtt`：UDP 代理每方向丢弃 2/100 报文、每包 20 ms 单向延迟，输入/控制 RTT 61 ms，30/30 个 1 KiB 协议视频帧在 3 s 内到达，因此 T2 转绿；边界是代理模拟、视频帧未接真实编码器。当前矩阵 22 绿 / 0 黄 / 0 红；T9 已用 4 条独立 QUIC 会话验证共享预算，T21 的真实 4 viewer D13 库级一次编码/多 recipients 已测，真实广播编排已由 BroadcastHost 覆盖。

## 4. P0 路线状态（当前）

T1–T22 全绿只证明验收闭环，不等于可以冻结 1.0.0。当前按 P0 路线推进：

| 阶段 | 版本 | 状态 |
|---|---|---|
| P0-0 基准工具 | — | **完成 v1**：`tools/p0.sh` 跑 QUIC loopback JSON（首帧 99.4 µs、输入 p95 85.9 µs、文件+视频 200 帧 8.11 ms、4 viewer 600/870）与 95.01 s 空闲；`p0_baseline` 保留内存对照。边界：文件+视频是交错单流、4 viewer 是调度器、真实编码器负载未接 |
| P0-1 R1 TransportConfig | 0.4.1 | **完成（LAN 主场景）**：keep_alive 15s / idle 90s / 8 MiB stream / 32 MiB recv / 16 MiB send 已注入；95.01s 空闲、本地 500 MB 339.6 MB/s、输入 p95 0.198 ms、T4 UDP 黑洞 2s 复跑均通过；WAN 16 MiB 3.30 Mbps 仅作参考；Cubic/BBR 已本地 A/B，默认 Cubic |
| P0-2 R3 心跳 | 0.4.2 | 完成：`Heartbeat` 5s/3 misses、15s 检出单测、`ClientSession::heartbeat_poll/on_pong`；`heartbeat_detects_15s_udp_blackhole` 真实 QUIC/UDP 代理全丢包 15.05s 检出 |
| P0-3 R4 2 RTT 握手 | 0.5.0-rc | **结构完成**：已删除 `Hello.version_range`、`negotiate_version`，`Hello.version` 与 `ClientConfig.version`/`ServerConfig.version` 均为单值；服务端 `Challenge`+`Welcome` 同航班发出，客户端收到 Welcome 即发 `Auth`（可带首条业务消息），服务端先验 Auth、失败补 `Bye{12}`；成功路径 3 帧。WAN ready 738/1891 ms（RTT 177–263 ms）仅作参考，不作冻结门槛；wire 仍 400，0.5.0 为库版本发布，不改 wire；未来线格式变化再升 500 |
| P0-4 R2 默认 8 平面多流 | 0.5.0 | 完成：`PlaneConnection` 默认 8 流、`--single-stream` 回退；`concurrent_file_does_not_block_video_or_input` 100 轮 64 KiB file + input + video，video p95 168.4→450.4 µs（增量 282 µs）、input p95 506.4 µs；默认多流握手/Ping 已本地 smoke，VM 多流此前已验证；WAN 不再作为出口判据 |
| P0-5 审计 + 1.0.0 冻结 | 1.0.0 | **1.0.0 已冻结（2026-11-21 用户批准，tag `v1.0.0`）**：见 `docs/57-p0-freeze-audit.md` 与 `docs/59-freeze-candidate-plan.md`；T1–T22 22/0/0、316 tests + 4 ignored、审计通过；P0/P1 已按协议库层面关闭（D1–D23 全部关闭；D21 背压队列在默认多流异步泵上生效，单流回退显式 batch），P2 按 OP-6 降级关闭。2026-11-21 用户批准冻结 1.0.0；OP-1/OP-6/OP-11/OP-13 已修复/复测并随 1.0.0 冻结 |

R1 当前证据：`cargo test -p opsdesk-net --lib` = 11 passed；`idle_90s_keeps_connection_alive` = 1 passed in 95.01s；五包全量 **316 passed / 4 ignored**。R1 已完成 LAN 主场景；0.5.0 已发布，0.4.1 不再单独发布。

R6/R8/R9/R10 当前证据（0.5.0 后工作树，未改 tag）：

- R6 音频时钟：`JitterBuffer` 在空闲 >5s 时 `reanchor()` 重置基线，`JitterStats.clock_reanchors` 计数；新增到达时间对 RTP 时间戳的最小二乘拟合 `clock_skew_ppm`，按估计 ppm 从目标窗口扣除持续漂移；`ten_minute_playout_delay_stays_bounded` 用 60k 包验证 p95 ≤300ms，`clock_skew_is_estimated_and_removed_from_target` 验证 1000 ppm 被识别；audio 7 passed。
- R8 TCP 三档：`TcpQos` 在 loss/RTT 超阈时降 Good→Fair→Poor，3s 降档、15s 升档，`clamp()` 压到 30/8000/256KiB、15/4000/128KiB、8/2000/64KiB；控制器 4 tests。`HostSession::on_net_report` 在 TCP 会话上按 `NetReport` 调用并回 `VideoAdjusted`。
- R9 4 viewer：`broadcast_host_four_viewers_share_one_encoder` 用 120 帧 1920×1080 合成帧过真实 H.264 ffmpeg 编码器（`FFMPEG=...` 时 81.6 fps、CPU 125.3%、1,245,234 B），再向 4 个 HostSession 分发：1 active 102/120、3 background 各 24/120、encode_calls=120；关掉在途窗口以隔离编码/分发。当前本机 `where ffmpeg` 无结果，测试会走 `R9_ENCODER_SKIP` 回退；上述真实数字来自配置过 ffmpeg 的运行，真实采集/4 条 QUIC/端到端 present 仍未接。
- R10 动态权重：`AdaptivePriorities::observe(backlog)` 提升 input/video/cursor、压低 bulk，`PlaneIo::adapt_priorities` 在传输支持时下发；`HostSession::on_net_report` 在 `queue_ms>0` 时按队列压力调用，默认拥塞驱动入口已接。
- R5 文件自适应：`lan_probe` 默认 `open_adaptive`（rtt 50ms / 100Mbps 缺省），`--fixed-file` 回退 64 KiB。本地同机 16 MiB A/B（rtt 200ms/100Mbps 窗口计算，上传/下载两个方向）：
  - 上传：固定 64 KiB/256 块 **469ms**；自适应 311296 B/54 块、credit 9 **169ms**。
  - 下载：固定 64 KiB/256 块 **465ms**；自适应 311296 B/54 块、credit 9 **171ms**。
  - 三份落盘文件 SHA-256 均为 `080acf35…19643e`。
  - `FileOffer.credit` 已由发送方携带，接收端用其构建 `FileRecvWindow` 并在 ACK 中继续广告；此前接收端固定 `DEFAULT_FILE_CREDIT=8` 会把自适应 credit 覆盖掉，现已修复并有 `file_offer_carries_credit_into_window`/`ack_keeps_receiver_credit` 单测。
- WAN 文件：在本机 `wan_proxy` 100 ms 单程 UDP 延迟代理（200 ms RTT）上跑 16 MiB 下载：固定 64 KiB/credit 8 **8731 ms**，自适应 311296 B/credit 9 **3743 ms**，SHA-256 一致。地理 WAN 尚未复跑；`lan_probe` 接收端必须用同一 `--file-rtt-ms/--file-kbps` 计算块大小，否则 `FileRecvSink` 会报 `bad block boundary`。
- 文件收尾：下载方向已用 FileEnd 后 Ping 完成确认修复；`PlaneConnection::close` 先 join writer 再关 QUIC。

### 4.1 详细工程下一步（历史清单）

1. 真实 LAN 握手/协商/Ping→Pong 已在 Win11 VM（此前 `192.168.99.244:9443`；2026-11-19 控制台为 `192.168.78.134`）通过；文件平面原始块与 zstd 已在本地和 VM（UDP 9444）验证，`307,207 B` / 5 块 / SHA-256 一致。`--multistream` 已在本地与 VM LAN（UDP 9450/9451）验证断线 Resume 与文件块传输；动态权重已接 `PlanePriorities`；下一步补服务端主动开流与常驻并发循环。
2. `HostSession::accept_with_store`/`resume_with_store` 已覆盖首票与换票；恢复拒绝后同连接回退完整 `Hello` 已由 `HostSession::resume_or_accept`/`ClientSession::resume_or_connect` 覆盖；`TicketStore::issue` 已自动调用 `prune_expired`，后台定时清理尚未接。
3. `FileResumeHint` 已接到 `HostSession::send_file_stream_from` + `FileRecvSink::resume` 的连续前缀恢复，并有集成测试；`FileSparseSendWindow`/`FileSparseRecvWindow` 与 `FileSparseSendStream`/`FileSparseRecvSink` 已支持按位图乱序调度，并有真实文件单测与会话级 `sparse_file_stream_roundtrip_over_host_session`；`ReconnectPolicy`/`run_with_reconnect`、`ClientSession::resume_state()`/`resume_or_connect_from_state()` 与 `lan_probe --reconnect 2 --drop-first` 已覆盖“断线后新连接优先 Resume、拒绝回退完整 Hello、服务端共享 TicketStore/ResumeSnapshot 换票”的路径；`watchdog` 已提供 `run_session_with_watchdog` 自动检测/重建循环；剩余的是 connector/endpoint 管理和把稀疏驱动接进自动选路。
4. `apps/opsdesk-realtime` 已把 H.264 本地全链路跑到 10 秒 1080p60：软件 capture/encode 59.4、decode 59.5、窗口 present 59.2 fps；硬件 `h264_amf` capture/encode 60.4、decode 59.3、present 59.1 fps（复跑 59.1–59.3），T18 已满足；`--fps 30` 已验证 host 29.4 fps、client present 29.3 fps。T16 协议级会话内切换已完成；T13 的运行时 `max_fps` 限帧、暂停与恢复首帧 KEY 已由 `max_fps_pauses_and_resumes_with_keyframe` 验证；T20 音频已接本地 Opus 链路并完成应用层 200 ms 抖动/丢帧注入与 `--av-sync` 门控（p95 69 ms、A/V -10.5 ms、视频 60.6 fps、RED 恢复 169 包、dropped=0）；`h264_mf` 不稳定路径仅保留对照；VM 交互控制台已于 2026-11-19 跑通实时 host→宿主 client（377 帧/20.0 s、18.8 fps、窗口 present 18.9 fps），gdigrab error 5 只出现在无桌面的 SSH 会话，不再是环境缺口。
5. 按平面拆分常驻循环与并发调度：control 常驻、video/input/feedback 并发、file/cursor/audio/clipboard 按需开流。
6. 给每个平面加内存回环集成测试，再跑 T1–T22。
7. 审计 docs/45 与代码的消息/错误码/字段/方向，清掉未解释漂移；P0 结构/回归已完成，WAN 数据仅作参考；OP-1/OP-6/OP-11/OP-13 已修复并复测，待请求用户复审 1.0.0 冻结。

## 5. 已知边界

- `Session` 的 role 还没有从握手结果传入；service/client 当前固定 `Role::Control`，等 share/session 消息落地后再接角色协商。
- `SessionMachine` 目前是单连接 control 状态机，不负责 QUIC 流生命周期、重传、credit 和 codec 实际切换。
- 所有“完成”仅指库内逻辑与单测，不代表真机端到端平面已通。
