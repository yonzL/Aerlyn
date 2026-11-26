# 版本与发布

## 版本历史

| 版本 | 状态 | 说明 |
|---|---|---|
| `v1.0.0` | 冻结 | 首个 standalone 冻结基线 |
| `v1.1.0` | 历史 | 文件平面上传/下载双向流语义 |
| `v1.2.0` | 历史 | 功能位、未知消息、display_key、语义光标、隐私态、预览订阅、golden bytes |
| `v1.2.1` | 当前 | Clippy 清零；文档路径修正；无 wire/API 语义变化 |
| wire `400` | 当前 | 1.x 线格式；1.x 不协商 |

## 发布前检查

```bash
cargo test --offline
cargo test --test spec_consistency --offline
cargo fmt --check
cargo doc --no-deps
```

文档-only 变更不升级版本、不移动 tag。协议行为变化必须：

1. 改代码与测试；
2. 更新 `architecture/protocol.md` 与 `architecture/requirements.md`；
3. 更新 `design/versioning.md` 的版本表；
4. 新建提交与 annotated tag。

## Tag 规则

- 已发布 tag 不移动、不重写。
- standalone 仓库只保留当前需要的 tag；历史 0.x tag 不迁入。
- 产品通过 Git tag 依赖：
  ```toml
  aerlyn-wire = { git = "ssh://git@github.com/yonzL/Aerlyn.git", tag = "v1.2.1" }
  ```
- 发布后由宿主刷新 `Cargo.lock`。

## 远端

- SSH：`git@github.com:yonzL/Aerlyn.git`。
- HTTPS 在当前网络下可能超时；发布使用 SSH。
- push 前确认工作树干净、tag 指向预期 commit。
