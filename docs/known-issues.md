# 已知问题与边界

## 设计边界

- **不提供传输实现**：宿主必须实现 `MessageIo`；QUIC/TCP/内存适配器不在 standalone 内。
- **不提供编码器/文件系统**：视频编解码、压缩、SHA-256、落盘、原子改名由宿主执行。
- **不提供 client/host 驱动**：`send_file_stream`、`download_file`、`serve_file_get` 属于产品层。
- **不提供 UI/设备控制**：输入注入、抓屏、播放、剪贴板操作系统集成由宿主完成。

## 已知风险

1. **控制/数据隔离依赖适配器**：多流实现若未覆盖 `recv_control`，控制解析器可能读到视频/文件 body，表现为 `bad control frame length`。适配器必须只从 control 通道读取。
2. **1.x 不协商版本**：`Hello.version` 单值；混合版本直接拒绝。版本降级/双栈属于 2.0.0 议题。
3. **历史审计页是快照**：`architecture/implementation-status.md`、`drift-audit.md`、`acceptance-matrix.md`、`design/protocol-v4-archive.md` 的“当前/未完成”是当时结论，不是实时状态；当前事实看 `protocol.md` 与代码。
4. **文档与测试路径耦合**：`tests/spec_consistency.rs` 通过 `include_str!` 读取固定路径；移动文档必须同步测试。
5. **DATAGRAM 无重传**：视频 DATAGRAM 丢失由关键帧/FEC 恢复；文件仍走可靠流。
6. **大帧分片上限**：可靠流 64 片，DATAGRAM 1024 片；超出必须由上层拆帧或拒绝。

## 相关页面

- [协议规范](architecture/protocol.md)
- [分层与传输边界](architecture/layering.md)
- [版本策略](design/versioning.md)
- [Roadmap](roadmap.md)
