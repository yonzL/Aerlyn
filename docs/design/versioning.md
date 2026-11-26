# 版本策略

Aerlyn Wire 有两个独立版本概念：**库版本**（Cargo package）与 **wire 版本**（`proto_version`）。

## 当前版本

| 项目 | 值 |
|---|---|
| crate 版本 | `1.2.1` |
| wire `proto_version` | `400` |
| ALPN | `aerlyn/4` / `aerlyn/4-tcp` |
| 冻结基线 | `v1.0.0` |
| 当前协议标签 | `v1.2.1` |

1.2.1 是 1.2.0 功能线的补丁版本：只修 Clippy 与文档路径，不改 wire 版本号、消息 ID 或字段；`400` 仍表示 1.x 线格式。1.2.0 的功能集见发布历史。

## 1.x 规则

- **不做版本协商**。`Hello.version` / `Welcome.version` 是单值；不同版本直接拒绝。
- 1.x 内的兼容变更走补丁版本；需要新线格式时提升次版本并更新 wire 版本。
- 只有 2.0.0 才考虑与 1.0.0 的协商/降级。
- 已发布 tag 不移动；修复走新提交、新 tag。

## 库版本与 wire 版本的关系

```text
库版本 1.2.1  ──  package / API / 文档
wire 400      ──  线格式 / 消息 ID / 字段
```

两者不必同步递增。文档-only、测试-only、注释-only 变更不提升任何版本。改变字段语义、常量或状态机时，必须同时提升库版本并说明 wire 是否变化。

## 发布与依赖

- standalone 仓库：`git@github.com:yonzL/Aerlyn.git`。
- 产品通过 Git tag 依赖，不通过本地路径：
  ```toml
  aerlyn-wire = { git = "ssh://git@github.com/yonzL/Aerlyn.git", tag = "v1.2.1" }
  ```
- 发布新 tag 后，宿主更新依赖并刷新 `Cargo.lock`。
- `v1.0.0` 是冻结基线，不重写历史；`v1.2.1` 是当前协议版本。

## 文档变更

纯文档重组、索引调整、链接修复不改变版本号，也不移动 tag。若文档修正伴随协议行为变化，先改代码/测试，再更新规范与 `design/versioning.md`。
