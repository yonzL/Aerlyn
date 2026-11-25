# 测试与验证

## 测试层次

| 层 | 位置 | 验证 |
|---|---|---|
| 单元 | `src/**` 的 `#[cfg(test)]` | 编码/解码、窗口、边界、状态机 |
| 协议一致性 | `tests/spec_consistency.rs` | 文档条目、消息注册表、错误码、字段表 |
| 平面回环 | `tests/plane_loopback.rs` | 六平面二进制帧 |
| 流程 | `tests/flow*.rs` | 握手、TicketStore、会话流程 |
| 传输替身 | `src/memory.rs` | `MemoryConnection` / `ChoppyConnection` 分帧 |

## 必跑命令

```bash
cargo test --offline
cargo test --test spec_consistency --offline
cargo fmt --check
```

协议一致性测试通过 `include_str!` 读取冻结档案与审计快照：

- `docs/design/protocol-v4-archive.md`（SPEC 基线）
- `docs/architecture/acceptance-matrix.md`
- `docs/architecture/implementation-status.md`
- `docs/architecture/drift-audit.md`
- `docs/architecture/requirements.md`

移动/重命名这些文档时，必须同步 `tests/spec_consistency.rs` 的路径，否则编译失败。

## 加测试的规则

- 新常量：测边界值与拒绝路径。
- 新消息：测 JSON 往返、未知字段/未知 type、方向表。
- 新分片/窗口：测乱序、重复、空洞、超限、关闭。
- 新传输适配器：实现 `MessageIo`，至少测控制隔离、半包 `UnexpectedEof`、`WouldBlock` 与 `flush`。
- 测试不依赖外网、真实编码器或操作系统句柄；真实 I/O 测试放在外部产品仓库。

## 文档检查

除 `spec_consistency` 外，提交前用 Markdown 链接检查器检查相对链接；本仓库不内置脚本。至少确认：

- `docs/README.md` 与各子目录 `README.md` 的链接目标存在；
- 迁移文档后没有残留旧编号路径；
- 锚点不检查，链接目标文件必须存在。
