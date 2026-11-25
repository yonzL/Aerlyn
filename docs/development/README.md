# 开发环境与构建

## 工具链

- Rust 1.85+。
- 无外部服务、无数据库、无网络依赖。
- 本仓库是 standalone crate，不在 `opsdesk` workspace 内。

## 构建与测试

```bash
cargo test --offline
cargo test --test spec_consistency --offline
cargo fmt --check
cargo doc --no-deps
```

`cargo test` 覆盖 lib、plane、spec、flow 与集成测试；不需要 QUIC/TCP 服务器。

## 目录

```text
src/                协议核心
tests/              spec/flow/plane 集成测试
docs/               本目录
docs/architecture/  协议规范、分层、需求、审计
docs/design/        设计原则、版本策略、历史档案
docs/guide/         接入指南
docs/development/   构建、测试、发布
```

## 常用检查

- 改了常量/上限：同时更新 `architecture/protocol.md` 与 `architecture/requirements.md`。
- 改了文档路径：更新 `tests/spec_consistency.rs` 的 `include_str!`。
- 改了公开 API：更新 `guide/README.md` 与 rustdoc。
- 文档-only 变更：不移动版本 tag，不改 `proto_version`。

## 相关页面

- [测试与验证](testing.md)
- [版本与发布](release.md)
- [架构总览](../architecture/README.md)
