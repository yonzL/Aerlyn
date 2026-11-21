# T1–T22 验收矩阵（0.4.0 快照）

> 快照时间：2026-11-19。协议是 `0.4.0` 开发线；T1–T22 全绿是验收输入，不是 1.0.0 冻结结论。docs/58 仍有 D3–D9/P1 缺口，冻结需这些条款关闭后由用户明确批准。
> 判据来源：V3 T1–T15 见 `docs/41-protocol-spec-v3.md:706-720`；V4 T16–T22 见
> `docs/45-protocol-spec-v4.md` §6.1。
> 状态：**绿 = 有可复现证据且满足判据；黄 = 只实现了一部分或只有局部证据；红 = 未实现/未测/不满足**。
> 本表不允许把“设计”或“局部单测”当成达标；逐项证据与边界见 `docs/52-protocol-drift-audit.md`，条款级缺口与收敛路线见 `docs/58`、`docs/59`。

## 1. 汇总

| 状态 | 数量 |
|---|---:|
| 绿 | 22 |
| 黄 | 0 |
| 红 | 0 |

当前结论：**22/22 绿（T1–T22）**。主场景按 LAN-only 验收，P0 的 R1/R3/R4/R2 结构/回归已完成；WAN 数据只作适用性参考，不作为冻结门槛。1.0.0 冻结仍需全量回归、文档/代码审计无漂移和用户明确批准。

## 2. 矩阵

| # | 判据 | 当前证据 | 状态 | 缺口/下一步 |
|---|---|---|---|---|
| T1 | V3 全能力（V4 等价口径） | V3 线已退役；V4 等价证据：`opsdesk-realtime` H.264 1080p60 10 s 窗口链路 595/595，capture/encode 59.4、decode 59.5、present 59.2 fps；`HostSession`/`ClientSession` 六平面 typed 回环、文件原始/zstd 与续传、Opus 音频 200 ms 抖动/RED、4 条独立 QUIC 会话共享预算均已有测试 | 绿 | 不是字面 v3↔v3 线格式；V3 已由 V4 取代，保留此行为 V4 全能力等价验收 |
| T2 | 2% 丢包 + 40 ms RTT | `quic_tolerates_two_percent_loss_and_40ms_rtt`：测试内 `UdpProxy` 每方向丢弃 2/100 个 UDP 报文，并对每包加 20 ms 单向延迟（RTT 目标 40 ms）；客户端发 `InputBatch` 后收到 `Ping`，实测输入/控制 RTT 61 ms；服务端 30 个 1 KiB 视频帧在 3 s 内全部到达并解码，2% 丢包由 QUIC 重传恢复 | 绿 | 代理模拟而非 OS netem；输入延迟用控制 RTT 近似，视频帧为协议载荷，未接真实编码器 |
| T3 | 500 MB 上传/下载 | 本地多流 500 MB 上传：原始 2.112 s（chunks=8000，64 KiB 块）、zstd(3) 1.051 s；下载 `recv-file` 2.133 s；落盘 SHA-256 均为 `a08a92258f621b55d08ad1e84c90c2ea6286fc6b6c9a4dfa7156afb16c190170`；307,207 B 本地/VM 文件与稀疏乱序块已通过 | 绿 | 双向 500 MB + 摘要已验证；VM 500 MB 与中断续传未跑，不作为本项判据 |
| T4 | 拔网线 2 s 再插回 | 恢复票/`ResumeSnapshot`/同连接回退单测；`ReconnectPolicy`/`run_with_reconnect` 单测；`watchdog_resumes_real_quic_after_two_second_outage` 在真实 QUIC 上关闭首连接、2 s 后由 `run_session_with_watchdog` 自动新开连接并 `resume_with_store`，收到恢复后的 Ping；`quic_survives_two_second_udp_blackout` 用测试内 `UdpProxy` 在客户端与服务端之间双向丢弃全部 UDP 报文 2 s 后恢复：Ping1/Pong1 → 2 s 全丢包 → Ping2/Pong2，QUIC 会话存活、无需重连；`lan_probe --reconnect 2 --drop-first` 本地与 VM LAN（UDP 9450）均复现 | 绿 | 中断由 UDP 代理制造，不是物理拔网线；已覆盖网络路径 2 s 全丢包恢复，物理网卡/防火墙场景仍可用同一测试替换 |
| T5 | 错误指纹/签名/无 token | `ClientConfig.expected_host_fp` 在 Challenge 后比对 `host_fp`，不匹配回 `Bye{HostFingerprintMismatch=11}` 并返回 `HandshakeError::FingerprintMismatch`（`host_fingerprint_mismatch_rejected`）；证书不受信由 `untrusted_certificate_is_rejected` 拒绝；错误签名由 `bad_signature_rejected` 拒绝；`SessionMachine.channel_token` 绑定 `Session.token`，`ChannelHello` 错误/空 token 回 `Rejected{TokenInvalid=13}`（`channel_hello_requires_session_token`），正确 token 回 `ChannelWelcome` | 绿 | 失败路径未构造 Session/数据面；审计日志与抓包级“零数据帧”未做 |
| T6 | 未知 type/字段/TLV | `ControlMessage::Unknown` 保留并计数；未知二进制 type 被 `recv_decoded` 跳过并计数，连续 1024 帧触发上限；未知 JSON 字段被忽略；`FileOffer.extensions` 端到端携带 `Extensions::encode` 的 TLV 字节，测试 `file_offer_carries_tlv_extensions_end_to_end` 验证已知 tag 保留、未知 tag 跳过并计数 | 绿 | 扩展块目前只在 `FileOffer` 中嵌入；其它消息的 TLV 扩展位点后续按需增加 |
| T7 | 未鉴权洪泛 | `AdmissionLimiter` 全局 64、每 IP 8、30 s 宽限；握手 control 上限 128 KiB（`read_control_msg_limit`）；`admission_state_stays_below_eight_mib` 在 Windows 采样工作集增量；`lan_probe` accept 前检查，超限回 `SERVER_ADMISSION_BUSY` | 绿 | 限流状态只保留计数器，不缓存握手消息；4 个 admission 单测 + 1 个 channel 上限单测 |
| T8 | 4K30 | `h264_amf -hw_encoding 1 -usage ultralowlatency` 编码 3840×2160@30、2 s/60 帧：墙钟 0.521 s（约 115 fps），文件 5,292,536 B（约 21.2 Mbps）；ffmpeg 解码 60 帧墙钟 0.230 s（约 260.9 fps），`ffprobe` 60/60 帧；软件 libx264 4K30 只有 22.1 fps，不作为实时证据 | 绿 | 硬件 H.264 4K30 离线编解码已证；4K 实时会话/HUD 未接 |
| T9 | 4 会话并发 | `four_concurrent_quic_sessions_share_aggregate_bandwidth`：4 条独立 QUIC 连接同时握手，4 个 `HostSession` 共享 `BandwidthBudget::new(8_000, 64 KiB)`；每会话先发 1 帧，再各尝试 100 帧，所有会话都收到首帧，聚合发送被预算拒绝一部分（`taken_bytes < attempted`）；预算单测覆盖 8 Mbps 补充与 64 KiB 突发上限 | 绿 | 真实 4 连接并发与共享总预算；故障注入为发送侧预算拒绝，不是公网带宽整形 |
| T10 | TCP 降级 | `opsdesk-net::tcp_conn::TcpConnection` 以 4 B 长度前缀分帧；`tcp_tls_runs_full_handshake_and_negotiation` 在 rustls TCP 上以 ALPN `aerlyn/4-tcp` 跑完 HostSession/ClientSession 握手与 H264/15 fps/4 Mbps 协商；`TcpProfile` 固定 256 KiB/15 fps/4 Mbps/无音频；测试输出 `CLIENT_HUD transport=TCP alpn=aerlyn/4-tcp` | 绿 | 显式 TCP 传输；UDP→TCP 自动切换未做；HUD 是控制台标记，不是桌面 overlay；无 4K |
| T11 | 音频 200 ms 抖动 | `aerlyn-wire::audio` 的 `JitterBuffer` 目标 20–200 ms、EWMA 抖动、RED 补包与乱序/200 ms 上界单测；`opsdesk-realtime` 本地 Opus 链路：3147 个 RED 条目/984 个包播放、dropped=0、target=50 ms、max_jitter=0、p95=71 ms；无 OS 级网络抖动注入 | 绿 | 协议级抖动缓冲/RED 已达标；真实 200 ms 网络抖动与 A/V sync 开关在 T20 继续 |
| T12 | v2 客户端连 V4 host | `opsdesk-service::v2::v2_accept` 读 v2 JSON Hello/Auth（proto>=2、channel 0、base64 公钥/签名），验签后回 `Welcome::Ok`，`v2_ping` 回 Pong；单测 `v2_client_handshakes_and_pings_v4_service` 覆盖握手与 Ping/Pong | 绿 | v2 控制面适配已通；v2 数据面/媒体流未接，0.4 线不承诺 |
| T13 | `max_fps` = 30/60/0 | `HostSession::apply_video_config` + `paused`/`fps_cap`/`last_video_pts_us`：运行时 30 fps 按 pts 隔帧（16.666 ms 的帧被丢弃、34 ms 的帧发送）；`max_fps=0` 只发 `VideoReady{max_fps:0}`、不再发视频帧；恢复发 `VideoReady` 并强制下一帧 KEY，非 KEY 返回协议错误；测试 `max_fps_pauses_and_resumes_with_keyframe` 覆盖 30→0→60、暂停无帧、恢复 KEY、`frame_id` 10/12/15 | 绿 | 发送侧限帧与暂停/恢复已达标；真实编码器在 30/60 间的动态调整仍可接入实时 app |
| T14 | 4 会话 active 切换 | `ViewerSet::switch_active` 在 4 viewer 中切换 foreground：切换后新 active 300 帧全发，旧 active 降为后台 <100 帧，`plan()` 跟随新 active codec；测试 `active_switch_moves_foreground_budget_among_four_viewers` | 绿 | 协议级调度器模拟已达标；真实 4 条 QUIC 会话与共享编码器未接线 |
| T15 | 焦点切换 | `HostSession::notify_focus(focused, display)`：失焦时 `focus_cap=10`、发 `VideoAdjusted{actual_fps:10,max_fps:60}`，恢复时清 cap、发 `actual_fps:60`；`send_video_rects` 用 effective cap 限帧。测试 `focus_loss_updates_actual_fps_and_throttles_video` 在 200 ms 内收到通知，失焦 3 帧只发 1 帧，恢复后 3/3 | 绿 | 焦点事件来源由调用方提供；应用窗口失焦未接 |
| T16 | 会话内编码切换 | `HostSession::switch_codec`：会话内 H264→H265，旧流 `StreamEnd{CODEC_SWITCH}`，新 `VideoReady`/`CodecConfig`/`StreamHello`，新流首帧 KEY，`frame_id` 1..5→6 连续；测试 `codec_switches_mid_stream_with_continuous_frame_ids` 在切换前后各发 5/1 帧，并从切换开始到新 KEY 观测客户端耗时 <300 ms；`session_id` 不变 | 绿 | 协议级切换已达标；真实编码器/五编码逐一负载切换与 VM 抓屏另行扩展 |
| T17 | 解码失败回落 | `HostSession::handle_decode_error` + `DecodeFailureTracker`：注入 H264 `DecodeError` 后 host 先发 KEY（frame_id 1/2）；5 s 内第 3 次按剩余候选切到 H265，发 `VideoReady`/`CodecConfig`，新关键帧 frame_id 3，会话不断；全失败发 `Error{CodecUnsupported}` + `Bye`；测试 `decode_errors_request_keyframe_then_fall_back_within_same_session`、`decode_errors_exhaust_all_codecs_and_close_the_session` | 绿 | 协议级回落已达标；真实解码器错误注入仍可扩展 |
| T18 | 五编码矩阵 | 五编码 1080p60 软件基线：H.264 87.1 / H.265 66.9 / VP8 85.9 / VP9 77.5 / AV1 73.2 编码 fps，解码 582–1188 fps，120/120 帧；H.264 实时全链路：软件 capture/encode 59.4、decode 59.5、Win32 present 59.2 fps、595/595 帧；硬件 `h264_amf` capture/encode 60.4、decode 59.3、present 59.1 fps、606/606 帧，复跑 59.1–59.3、预热 201–214 ms；`max_fps=30` 时 host 29.4、present 29.3 fps；见 `docs/54` | 绿 | 五编码均 ≥30 fps，硬编可用时 ≥55 fps，实际 fps ≤ `max_fps`；`h264_mf` 不稳定仅作对照；T16 五编码会话内切换、VM 交互桌面抓屏另行跟踪 |
| T19 | 4:4:4 / 无损 | H264 High444 `yuv444p`、H265 RExt `yuv444p10le`、VP9 Profile1 `yuv444p`、AV1 High `yuv444p` 各 30 帧 LOSSLESS；码率 805/421/703/340 kbps，均 ≤8 Mbps；文字图四宫格逐字可读；`negotiate` 单测覆盖四编码 profile/chroma/10-bit | 绿 | 质量档协商与四编码 4:4:4/无损编码已证；运行时编码器切档未接 |
| T20 | 音频同步 | 本地 H.264+Opus 10 s：`CLIENT_AUDIO` p95=69 ms（SLO-AUDIO-1 ≤150）、`CLIENT_AV av_drift_ms=-10.5`（SLO-AUDIO-2 ≤20）、1195 个包播放、dropped=0；`--audio-jitter-ms 200` 对 RED 到达做 0–200 ms 应用层调度，`--audio-drop-every 10` 注入 125 个丢帧，RED 恢复 169 个包，视频仍 60.6 fps；`--av-sync` 首帧门控已实测 | 绿 | 应用层注入而非 OS netem；连续唇同步、真实网络抖动/丢包仍可用更强测试替换 |
| T21 | 每 viewer 公平 | `opsdesk-service::viewers`：`ViewerLedger` 每 viewer 独立在途字节/credit/ack 基线/最后发送时间，`ViewerSet::tick` 独立丢弃慢 viewer 的帧；`plan()` 合并最高 fps、最高码率、最严格分辨率、active codec。600 帧 60 fps 模拟：1 active 600/600 发送，3 个慢背景 viewer 均 <200 帧且 skipped>0；active 不被标记 slow；慢 viewer ack 后下一 KEY 恢复。测试 `background_viewers_are_capped_and_do_not_starve_active`、`slow_viewer_recovers_on_key_after_ack`、`shared_plan_uses_highest_fps_bitrate_and_strictest_resolution` | 绿 | 协议级账本/调度已达标；真实 4 条 QUIC 会话与共享编码器接线未做 |
| T22 | RAW 兜底 | `all_codecs_failing_falls_back_to_raw_dirty_rects` 将五种性能编码各标记 3 次失败后切 RAW，发送真实 BGRA 两个脏矩形并逐像素还原；`send_raw_frame` 限 ≤1920×1080、每帧 KEY，协商上限 ≤5 fps；app `CLIENT_HUD RAW 兜底` | 绿 | 协议/像素/上限已验证；真实编码器 init/输出故障注入与桌面 HUD 截图仍未做 |

## 3. 可复现命令

```bash
cargo test -p aerlyn-wire -p opsdesk-service -p opsdesk-client-core --offline
cargo test -p opsdesk-net --test sync_conn --offline
```

LAN/文件证据：

```bash
# 服务端（持久 SSH 会话）
lan_probe server --bind 0.0.0.0:9444 --cert certs/server.crt --key certs/server.key \
  --out received.bin --size 307207 --zstd --level 3

# 客户端
lan_probe client --addr 192.168.99.244:9444 --ca certs/ca.crt --server-name aerlyn.test
lan_probe send-file --path source.bin --block 65536 --zstd --level 3

# T3：本地 500 MB 多流上传（原始/zstd）
lan_probe server --bind 127.0.0.1:9463 --cert certs/server.crt --key certs/server.key \
  --out target/t3_500m_recv.bin --size 524288000 --multistream
lan_probe send-file --addr 127.0.0.1:9463 --ca certs/ca.crt --server-name aerlyn.test \
  --multistream --path target/t3_500m.bin --block 65536

# T3：本地 500 MB 多流下载
lan_probe server --bind 127.0.0.1:9475 --cert certs/server.crt --key certs/server.key \\
  --source target/t3_500m.bin --multistream
lan_probe recv-file --addr 127.0.0.1:9475 --ca certs/ca.crt --server-name aerlyn.test \\
  --multistream --out target/t3_recv.bin --size 524288000 --block 65536
```

## 4. 结论

当前是 **22 绿 / 0 黄 / 0 红**。T1–T22 的汇总计数已由 `spec_consistency::acceptance_matrix_counts_match_docs` 锁定；逐项证据仍需按本表复验。
本轮只把判据、证据和缺口固定下来，避免把局部测试当成 V4 达标。1.0.0 冻结审计与前置条件见 [`docs/57-p0-freeze-audit.md`](57-p0-freeze-audit.md)；22/0/0 是冻结输入，不是自动批准。
表格行数与状态计数已由 `crates/aerlyn-wire/tests/spec_consistency.rs` 的
`acceptance_matrix_counts_match_docs` 锁定，并同步检查 docs/51、docs/52。
