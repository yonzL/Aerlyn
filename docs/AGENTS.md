# docs/AGENTS.md

本文件约束在本目录内工作的代理与维护者。它只管文档，不改变协议实现。

## 读者分层

- `README.md`、`guide/`：面向第一次接入的读者，先给可执行步骤，再给边界。
- `development/`：面向改代码的人，给命令、测试层次、发布步骤。
- `architecture/`：面向维护者，给模块职责、数据流、线格式与状态机。
- `design/`：面向决策者，给原则、取舍、版本策略与历史档案。
- `roadmap.md`、`known-issues.md`：面向项目跟踪，状态必须明确。

## 单一事实源

- 线格式、状态机、常量、不变量：`architecture/protocol.md`。
- 从实现反推的需求 ID 与验收条件：`architecture/requirements.md`。
- 历史实现状态与漂移：`architecture/implementation-status.md`、`architecture/drift-audit.md`、`architecture/acceptance-matrix.md`。
- 设计过程与旧版本决策：`design/protocol-v4-archive.md`。

历史文档中的“当前”“未完成”等词是当时快照，不要为了让它们看起来最新而改写结论；需要更新现状时，改 `protocol.md`、`requirements.md` 或新增审计页，并在索引中说明关系。

## 写作规则

- 中文叙述，代码标识、命令、错误信息保留原文。
- 先给结论/步骤，再给边界和反例。
- 每个链接使用相对路径；新页面必须在 `docs/README.md` 与所属目录的 `README.md` 中出现。
- 不写“将来会”“计划支持”除非 `roadmap.md` 有对应条目和状态。
- 不使用模糊量词替代可测条件：写上限、超时、帧数、字节数或命令。
- 代码示例必须能对应当前公开 API；不确定的签名不要猜，引用 `src/` 或测试。

## 路径与测试耦合

`tests/spec_consistency.rs` 通过 `include_str!` 读取固定的规范/需求/审计文档。移动或重命名这些文档时，必须同步：

- `tests/spec_consistency.rs` 的 `include_str!` 路径；
- 文档中的相对链接；
- 根 `README.md` 的文档入口。

## 文档变更流程

1. 确认改的是“当前事实”还是“历史快照”。
2. 改单一事实源，再改索引与入口页。
3. 跑 `cargo test`（至少 `spec_consistency`）与链接检查。
4. 文档-only 变更不移动版本 tag、不改 `proto_version`、不改线格式。
5. 若文档变更伴随协议行为变更，先改代码与测试，再更新规范与版本说明。
