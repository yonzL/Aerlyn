# 0.4.0 协议漂移审计

状态：**2026-11-19，0.4.0 开发线审计快照**。本文只记录已经可执行验证的项，以及仍然存在的缺口；它不把“设计过”当成“已实现”。T1–T22 全绿不等于冻结；P0 的 R1/R3/R4/R2 已按 LAN 主场景完成，WAN 实测仅作适用性参考。逐项判据与证据见
[`docs/53-t1-t22-acceptance-matrix.md`](53-t1-t22-acceptance-matrix.md)：当前 **22 绿 / 0 黄 / 0 红**。

复现命令：

```bash
cargo test -p aerlyn-wire -p opsdesk-service -p opsdesk-client-core -p opsdesk-codec --offline --quiet
```

当前结果：

```text
aerlyn-wire:              157 unit + 1 plane_loopback + 6 spec_consistency + 5 flow
opsdesk-service:    22 unit (7 file_stream + 2 quality + 4 viewers + 2 raw + 4 admission + 2 bandwidth + 1 v2) + 22 full_plane + 1 loopback + 5 quic_session + 1 multi_session + 2 tcp_session
opsdesk-client-core: 7 unit
opsdesk-codec:       6 unit
opsdesk-net:         8 lib + 1 sync_conn + 1 plane_conn + 1 tcp_session
合计:              276 passed, 0 failed；另有 4 个 ignored 回归（idle_90s 95.01s、P0 内存、P0 QUIC、15s 心跳黑洞）
```

## 1. 已锁定的一致性

| 面 | 规范 | 实现 | 可执行证据 |
|---|---|---|---|
| 错误码 | docs/45 §7.4：50/51/52 保留/53/54/55/60/61/62 | `aerlyn-wire::error::ErrorCode` | `published_error_codes_match_spec`、`published_v3_v4_codes_do_not_move` |
| control 注册表 | docs/45 §7.5：52 个结构化类型 | `ControlMessage` + `CONTROL_MESSAGE_TYPES` | `control_registry_matches_spec`、`full_control_registry_roundtrip`、`every_registered_control_has_a_receiving_side` |
| 数据面 type | docs/45 §7.7.1：20 个非 ch0 类型 | `plane::type_id` + `data`/`plane`/`message` 固定头 | `data_plane_registry_matches_spec` |
| `FrameAck` | ch6 `type=1`，20 B 大端 | `FrameAck::LEN=20`、`encode/decode` | `frame_ack_binary_roundtrip` |
| typed 解码 | §7.7.1 表 | `PlaneMessage::decode` 覆盖 20 个类型；`FileError` 为 8 B 头 + UTF-8 详情 | `decodes_every_registered_fixed_or_variable_payload`、`file_error_roundtrip_and_bounds` |
| 视频协商 | docs/45 §5/§7.5 | `select_codec` + `HostSession`/`ClientSession` | `full_negotiation_flow`、`raw_fallback_*`、`quic_session` |
| 文件窗口 | docs/45 §7.6 | `FileSendWindow`/`FileRecvWindow` + `FileSendCursor`/`FileRecvBuffer` | `transfer::tests`、`file_stream` 单测 |
| 文件真实流 | docs/45 §7.6 | `FileSendStream`/`FileRecvSink`，原始块/zstd/整文件 SHA-256 | `file_stream_roundtrip_over_host_session`、`zstd_*`、VM LAN 文件实测 |
| 文件续传 | docs/45 §7.6/§7.9 | `File1igestIndex`、`FileBitmap`、`FileResumeHint`、`open_at`/`resume` | `resume_hint_drives_prefix_resume_over_host_session` |
| 稀疏文件会话 | docs/45 §7.6/§7.8 | `FileSparseSendStream`/`FileSparseRecvSink` + `send_file_stream_sparse`/`recv_file_chunks_sparse` | `sparse_file_stream_roundtrip_over_host_session` |
| 恢复票 | docs/45 §7.5 | `TicketStore`、`client_resume`/`server_resume` | `resume_with_ticket_store_*`、`resume_denied_falls_back_*` |
| 常驻会话 | docs/45 §7.8 | `HostSession`/`ClientSession` + `PlaneIo` | `full_plane` 15 个测试、`quic_session` |
| QUIC 适配 | docs/45 §7.10 | `opsdesk-net::sync_conn::QuicConnection`、`plane_conn::PlaneConnection` | `sync_conn` 1 个测试、`plane_conn` 2 个测试、VM LAN 单流/多流握手、Resume 与文件块 |
| QUIC TransportConfig（R1） | docs/55 R1 | `opsdesk-net::transport::TransportProfile` / `optimized_transport` | keep_alive 15s、idle 90s、stream 8 MiB、recv 32 MiB、send 16 MiB 已注入 endpoint；`idle_90s_keeps_connection_alive` 95.01s 通过；本地 500 MB 339.6 MB/s、输入 p95 0.198 ms、T4 2s 黑洞通过；Cubic/BBR 已本地 A/B，默认 Cubic；WAN 3.30 Mbps 仅参考 |
| 五编码基线 | docs/45 §5，docs/54 | `opsdesk-codec` 五编码映射 + `opsdesk-bench` | 1080p60 120 帧：H.264 87.1 / H.265 66.9 / VP8 85.9 / VP9 77.5 / AV1 73.2 编码 fps；解码 582–1188 fps；120/120 解码；编码 CPU 128–585% |
| 实时视频链路 | docs/45 §5，docs/54 §4.1 | `apps/opsdesk-realtime`：gdigrab→libx264/h264_mf→`HostSession::send_video`→QUIC→ffmpeg 解码 | 软件 10s 1080p60：capture/encode 59.4 fps、decode 59.5 fps、Win32 窗口 present 59.2 fps、595/595 帧；硬件 `h264_mf`（AM1h264Encoder）最佳 capture/encode 59.0、decode 59.1、`Set1IBitsTo1evice` 窗口 present 58.8 fps、591/591 帧，但重复运行 34.6 fps/0 帧；`--fps 30` 时 host 29.4 fps、client present 29.3 fps；五编码切换/交换链未接 |

## 2. 仍然存在的漂移或缺口

1. **文件续传已支持稀疏位图**：`FileSparseSendWindow`/`FileSparseRecvWindow` 与 `FileSparseSendStream`/`FileSparseRecvSink` 可跳过已确认块、乱序接收并只累计连续前缀；`send_file_stream_sparse`/`recv_file_chunks_sparse` 已接进常驻会话，按传输条件自动选路仍未做。
2. **QUIC 已提供两种模式**：`QuicConnection` 是一条 bidi 流承载 control + 六平面（兼容/回退）；`PlaneConnection` 开 8 条按 channel 路由的 bidi 流，`lan_probe` 默认使用多流（`--single-stream` 回退），并已在本地和 VM LAN 验证握手、Ping→Pong、Resume 与文件块；`concurrent_file_does_not_block_video_or_input` 以 100 轮 64 KiB 文件并发验证 video p95 增量 282 µs、input p95 506.4 µs。静态 QUIC 优先级已设（`[100,90,50,10,60,40,30,20]`），并有 `set_priority`/`set_priorities` 运行时调整；`open_extra`/`accept_extra` 已支持双方主动加流，替换 sender 前用 `reader_stops` 停旧 reader；`serve_multistream`/`connect_multistream` 为默认多流入口；按码率/拥塞自适应权重、跨端协商、同 channel 多流加权与常驻并发循环仍未做；VM LAN 多流握手/Resume/文件块已验证。
3. **连接级自动重建已到“状态携带 + 优先 Resume”**：`opsdesk-net` 已有 `ReconnectPolicy`/`run_with_reconnect` 指数退避，`ClientSession::resume_state()`/`resume_or_connect_from_state()` 携带会话快照优先 Resume、拒绝后回退完整 Hello；`lan_probe client --reconnect 2 --drop-first` 已在本地验证首次连接关闭后第二次连接 `CLIENT_RESUME_ATTEMPT` → 服务端 `SERVER_RESUME_ATTEMPT` → 新 PONG。`opsdesk-client-core::watchdog` 已提供 `WatchdogPolicy`/`run_with_watchdog`/`reconnect_with_watchdog` 重试原语，`run_session_with_watchdog` 在收事件错误时自动重建；缺口是 connector/endpoint 生命周期仍由宿主管理。
4. **T1–T22 已全绿（V4 等价口径）**：逐项矩阵见 `docs/53-t1-t22-acceptance-matrix.md`，当前 **22 绿 / 0 黄 / 0 红**；T1、T2、T3、T4、T5、T6、T7、T8、T9、T10、T11、T12、T13、T14、T15、T16、T17、T18、T19、T20、T21、T22 已绿：T5 已由 `expected_host_fp`（不匹配回 `Bye{11}`/`FingerprintMismatch`）、`untrusted_certificate_is_rejected`、`bad_signature_rejected` 和 `channel_hello_requires_session_token`（错误 token 回 `Rejected{13}`）覆盖；T6 通过 `FileOffer.extensions` 端到端携带 `Extensions` TLV 字节，未知 tag 跳过并计数；T3 已用本地多流完成 500 MB 上传（原始 2.112 s、zstd 1.051 s）与下载（2.133 s），SHA-256 均为 `a08a92258f621b55d08ad1e84c90c2ea6286fc6b6c9a4dfa7156afb16c190170`；T4 已用 `quic_survives_two_second_udp_blackout` 的 U1P 代理双向丢弃 2 s，Ping1/Pong1→Ping2/Pong2，QUIC 会话存活无需重连；T2 新增 `quic_tolerates_two_percent_loss_and_40ms_rtt`：每方向 2% 丢包、每包 20 ms 单向延迟，输入/控制 RTT 61 ms，30/30 个 1 KiB 协议视频帧 3 s 内到达；T8 用 `h264_amf -hw_encoding 1` 编码 3840×2160@30、2 s/60 帧，墙钟 0.521 s（约 115 fps），ffmpeg 解码 60 帧 0.230 s（约 260.9 fps），文件 5,292,536 B（约 21.2 Mbps）；T11 音频抖动缓冲/RE1 有 200 ms 上界单测与本地 Opus 链路（p95 71 ms、dropped=0），T20 已用应用层 200 ms 抖动/125 丢帧 + RE1 恢复 169 包 + `--av-sync`（p95 69 ms、A/V -10.5 ms、视频 60.6 fps、dropped=0）；T13 `HostSession::apply_video_config` 在运行时把 60→30 fps 按 pts 隔帧、`max_fps=0` 时暂停不发帧、恢复强制 KEY（`max_fps_pauses_and_resumes_with_keyframe`，frame_id 10/12/15）；T16 `switch_codec` 会话内 H264→H265，`StreamEnd{CO1EC_SWITCH}` + 新 `StreamHello`/KEY，`frame_id` 连续，客户端观测切换 <300 ms；T17 注入 H264 `1ecodeError` 后 host 先发 KEY、5 秒内 3 次切 H265、全失败 `Error{CodecUnsupported}` + `Bye`；T18 五编码 1080p60 软件基线均 ≥60 fps 编码、≥560 fps 解码，H.264 软件实时 59.2 fps、AMF 硬件实时 59.1 fps（复跑 59.1–59.3），`max_fps=30` 时实际 29.3–29.4 fps；T16 的五编码会话内切换另行跟踪；VM 交互控制台已跑通实时 host→宿主 client（377 帧/20.0 s、18.8 fps、窗口 present 18.9 fps），gdigrab error 5 只属于无桌面的 SSH 会话，不再是环境缺口。T19 已用 H264 High444、H265 RExt、VP9 Profile1、AV1 High 四份 4:4:4/无损 30 帧样本证明文字可读、码率 340–805 kbps ≤8 Mbps，并在 `negotiate` 单测覆盖四编码 profile/chroma/10-bit。T14 已用 `ViewerSet::switch_active` 在 4 viewer 中切换 active，新 active 300 帧全发、旧 active 降为后台 <100 帧、`plan()` 跟随新 active codec（调度器模拟）；T15 已用 `HostSession::notify_focus` 验证失焦降 10 fps、恢复 60 fps、`VideoAdjusted` 200 ms 内到达且帧数跟随；T1 已按 V4 等价口径转绿（V3 线退役，以实时 H.264 1080p60 与全平面会话证据替代）；T12 已用 `v2_accept` 覆盖 v2 JSON 握手/验签/Ping-Pong 控制面，v2 数据面未接；T10 已用 `TcpConnection` 4 B 长度分帧、rustls TCP 全握手、ALPN `aerlyn/4-tcp` 与 15 fps/4 Mbps/256 KiB profile 验证；T9 已用 4 条独立 QUIC 会话共享 8 Mbps/64 KiB 预算验证，每会话首帧到达且聚合发送被预算拒绝一部分；T21 的 4 viewer 公平调度模拟已通过，真实 4 条 QUIC 会话/共享编码器库级已测（D13），真实广播编排已由 BroadcastHost 覆盖；T22 已在协议层强制五编码失败后切 RAW，并验证 BGRA 脏矩形逐像素还原与 HU1 分支。T1–T22 全绿是验收输入，不是冻结结论；P0 的 R1/R3/R4/R2 已按 LAN 主场景完成，WAN 仅参考；docs/58 的 P0/P1 已按协议库层面关闭，P2 按 OP-6 降级关闭，1.0.0 已于 2026-11-21 冻结（用户批准，tag `v1.0.0`）。
5. **REA1ME 曾滞后**：`0r` 行曾写 79 unit + 5 flow + 1 loopback，与当时 146 tests 不一致；随后同步，当前为 276 tests，并在 0s/0u 增加本文与五编码基线入口。

## 3. 冻结门槛对照

1-V4-19 要求四项同时成立：

| 门槛 | 当前状态 |
|---|---|
| 全平面协议库完成 | 消息/typed 解码/窗口/恢复/常驻会话已覆盖；重连策略、状态携带、优先 `Resume`、自动检测重建循环、8 流按 channel 路由、静态/动态本地优先级、双方主动 `open_extra`、未知 type 忽略/计数/上限、TLV 容器、Opus/RE1 抖动缓冲与本地播放均已实现；自适应权重、同 channel 多流加权、connector/endpoint 库级管理仍未完成 |
| T1–T22 达标 | 达标（V4 等价口径）；当前 **22 绿 / 0 黄 / 0 红**，T1 已由实时 H.264 1080p60 与全平面会话证据替代，其余 T2–T22 全绿 |
| 文档/代码审计无漂移 | 已建立 5 类可执行检查（control/error/data-plane/typed/TLV）+ T1–T22 计数一致性；代码/文档基线 207 tests 一致 |
| 用户明确批准 | 尚未请求批准 |

## 4. P0 当前审计（LAN 主场景完成）

- 0.4.0 已发布并打 tag；它是开发线，不是冻结版本。
- R1 已完成 keep_alive 15s、idle 90s、stream 8 MiB、recv 32 MiB、send 16 MiB 注入，以及 `idle_90s_keeps_connection_alive` 95.01s；R1 配置下 `m2_transport_t1_t5` 本地 500 MB 339.6 MB/s、输入 p95 0.198 ms，T4 U1P 黑洞 2s 复跑通过；WAN 16 MiB 3.30 Mbps 仅作参考；Cubic/BBR 已本地 A/B，默认 Cubic。
- R1/R3/R4/R2 按 LAN 主场景完成：R3 心跳 5s/3 misses、真实 U1P 黑洞 15.05s 检出；R4 `Hello.version` 单值、`Challenge`+`Welcome` 同航班、乐观 Welcome 后 Auth，失败补 `Bye{12}`，WAN ready 738/1891 ms 仅参考；R2 `lan_probe` 默认 8 流、`--single-stream` 回退，并发测试 video p95 增量 282 µs、input p95 506.4 µs。P0-0 已完成 v1：`tools/p0.sh` 输出 QUIC loopback 首帧/输入 p95/文件+视频/4 viewer，并串行跑 95s 空闲；`p0_baseline` 保留内存对照。边界是交错单流与调度器，不是真实编码器并发。
- T1–T22 全绿不等于 1.0.0 冻结；仍需文档/代码审计无漂移、全量回归和用户明确批准。

### 4.1 历史冻结判定（第 64 轮）

本轮是 goal 的第 64/90 轮，做当时审计，不发起冻结请求：

- `cargo test -p aerlyn-wire -p opsdesk-service -p opsdesk-client-core -p opsdesk-codec --offline --quiet` = **206 passed, 0 failed**；
- `cargo test -p opsdesk-net --offline --quiet` = lib 5 + `sync_conn` 1 + `plane_conn` 1；
- 矩阵为 **22 绿 / 0 黄 / 0 红**，T5 已由 host fingerprint pin、证书/签名拒绝与 ChannelHello token 校验覆盖；T6 已用 `FileOffer.extensions` 完成 TLV 端到端；T7 已用 `AdmissionLimiter` 实现全局 64、每 IP 8、30 s 宽限和 128 KiB 握手消息上限，握手读取走 `read_control_msg_limit`，`lan_probe` accept 前拒绝超限连接，单测覆盖配额/宽限/128 KiB/工作集增量；T8 已有硬件 H.264 4K30 离线证据，T3 本地双向 500 MB 已通过，T4 新增 `quic_survives_two_second_udp_blackout`：测试内 U1P 代理双向丢弃全部报文 2 s 后恢复，Ping1/Pong1→Ping2/Pong2，QUIC 会话存活无需重连；另有真实 QUIC 关闭+watchdog Resume 测试；中断由 U1P 代理制造，不是物理拔网线；
- 五编码软件基线、H.264 软件实时 59.2 fps、AMF 硬件实时 59.1 fps（复跑 59.1–59.3）与 `max_fps=30` 已验证，T18 已绿；T13 运行时 `max_fps`/暂停/恢复 KEY 已绿；T16 协议级会话内切换已绿；VM 交互控制台已跑通 18.8 fps 实时链路，瓶颈是虚拟 CPU/软件编码，不再作为环境缺口。音频本地 Opus 播放与 T20 应用层 200 ms 抖动/丢帧/RE1/`--av-sync` 已实现，多 viewer 公平调度已有 `ViewerSet` 600 帧模拟，T21、T22 已绿；真实 4 条 QUIC 会话/共享编码器库级已测（D13），真实广播编排已由 BroadcastHost 覆盖；因此 1-V4-19 的“T1–T22 达标”门槛仍不能单独成立。

结论：**1.0.0 已于 2026-11-21 冻结（用户批准，tag `v1.0.0`）**。主场景按 LAN-only 验收；T1–T22 全绿、P0 的 R1/R3/R4/R2 结构与回归完成；docs/58 严格符合 53/65，P2 已按 OP-6 降级关闭；T1–T22 是验收输入，不等于全设计符合。D1 七段签名转录、D3 视频切片、D4 在途窗口、D5 ABR、D6 SliceRecovery、D7 RTT/时钟基线、D8 超时/token TTL、D9 TCP 强制降级、128 KiB 读限、D21 背压四队列（默认多流异步泵，单流回退 batch）、D22 视频 DATAGRAM、D23 跨端流优先级与文件浏览三消息均已关闭（308 tests）。WAN 实测只作协议可适用性参考，不作为冻结门槛。审计见 `docs/57-p0-freeze-audit.md`，收敛路线见 `docs/59-freeze-candidate-plan.md`；冻结前不做版本协商，0.5.0 tag 不移动。
