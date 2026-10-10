# 统一决策后端实现与验证

日期：2026 年 10 月 9 日。目标版本：0.3。设计依据：[统一决策后端设计](./unified-decision-backends-design.md)。

## 实现结果

OpenAI、OpenRouter 与 TypeSafe 使用相同的 `Client`、`Input`、`Decision`、动态 `Questions`、`Predicate`、`Choice`、`Score` 和 `Outcome`。自托管 endpoint 使用相同发送与解码路径。业务 decision struct 无供应商字段；模型解析、Jev map 与 OpenAI list 协议、布尔 criteria、答案关联及统计校验均在私有模块中完成。旧 surface、wire、State/Guidance、Noul 与 optional 字段已删除，没有兼容别名。

`accept` 使用选中项概率，原生 confidence 有独立入口。Score 的 nearest 使用均值舍入，拒答用 Outcome 或带 metadata 的错误表示。缺失统计为 Unavailable；不伪造概率、token 或费用。JSON 证据排序后编码成有界文本，文本证据保持原文。配置、case v2、示例、README 与迁移说明已同步更新；完整清单见 [0.3 迁移指南](../docs/migration-v0.3.md)。

## 最终设计细化

- Client 使用 `Arc<ClientInner>` 共享整份不可变绑定，包括认证、模型、transport 与 semaphore；clone 不复制密钥。不同 client 共享连接池时仍各自持有并发限制。
- 自定义 endpoint 的 `for_endpoint_with` 入口允许显式共享 HttpClient 与 Settings，协议和完整 URL 由 validated Endpoint 一次确定。
- 除关闭 redirects 和 reqwest 内部重试，还显式关闭所有自动解压方式，避免下游 feature 合并改变响应字节限制。
- Retry-After 同时支持秒数与 HTTP date。服务器要求的等待不会被指数退避上限缩短；整体 deadline 不足时直接超时。发送后断连不自动重放。
- Choice 的 typed 解码验证完整 key 集合，允许等价动态 schema 重排无序选项；Score levels 保持顺序约束。加权采样不会选择尾部概率为零的选项。
- 外层 case 文档使用深度 40、节点 1000000 的独立预算，单条 Input 继续使用深度 32、节点 10000。YAML 在 parser 层拒绝重复 key、anchors、aliases、merge keys 和 includes。
- JSON 启用 `float_roundtrip`，修复 property test 发现的浮点序列化往返偏差；最小复现 seed 保存在 proptest-regressions 中。

这些细化不增加业务调用分支。依赖选择与官方来源见 [runtime dependency review](../docs/research/decision-runtime-dependencies.md)。

## 确定性验证

| 检查 | 结果 |
|---|---|
| `make build` | 两个 crate、全部 features 构建通过 |
| `make lint` | nightly rustfmt、所有 targets/examples 的 clippy，以及 library boundary pedantic/panic/indexing/unwrap/expect lint 通过 |
| `make test` | nextest 与 cargo test 均通过；108 个常规测试、113 个 doctests；3 个 live 测试默认 ignored |
| `make test-no-retry` | 56 个 HTTP 协议测试通过；无 retry feature 时 deadline 仍生效 |
| `make feature-check` | 37 个生产 feature 组合通过，移除 dev dependencies 后检查 |
| `make audit` | cargo audit 无已知漏洞；cargo deny advisories、bans、licenses、sources 均通过 |
| `cargo doc --workspace --no-deps --all-features` | 两个 crate 的 API 文档生成通过 |
| `make publish-check` | 两个 0.3.0 包 dry-run 打包与编译通过；未发布 |
| `make secret-check` | staged patch 扫描通过；输出脱敏 |
| 文档与 diff 检查 | 本地 Markdown 链接、索引、代码围栏与 whitespace 检查通过 |

cargo deny 仍报告既有 policy 中未被使用的 license allowlist 项及 syn 主版本重复，均为非阻断提示。没有为了消除提示而改变允许策略。未运行 Miri、性能 benchmark 或 fuzz campaign；本次没有引入 unsafe，实现的重点验证是公共 API、协议归一、拒答、资源边界与异步生命周期。

### 关键覆盖

HTTP fixtures 检查 OpenAI、Jev 与 System One 请求、同一业务类型的等价结果、响应 name 的完整唯一集合、duplicate JSON key、错类型及概率分布/score 不变量。还检查模型家族不匹配零 HTTP、显式 provider 密钥隔离、响应 metadata/accounting、401/403/402、额度类 429 与可重试限流、5xx、两种 Retry-After、redirect、实际响应字节 cap、clone 并发许可、排队 deadline、取消与发送后断连。

宏验证包含 12 个 compile-fail fixtures，覆盖重复问题/levels、超过题数上限、非法字段、Option 和嵌套 Outcome；手写非法 schema 返回缓存的 SchemaError。Case 测试验证 JSON/YAML 等价、严格 version 2、未知字段、重复 ID 与 parser 预算；batch 保序并保留逐项错误。Property tests 检查名称关联和概率分布；回归 seed 随源码保留。

## 在线验证与边界

使用显式 provider 和对应环境密钥，OpenAI 与 OpenRouter 的 ignored hosted smoke 均通过。每个请求同时包含普通 Predicate、双 criteria Predicate、Choice 与 Score；验证远端接受请求、四个 Outcome 都有答案，并成功归一 provider/model metadata。没有使用固定概率或类别作为真实模型的稳定断言。

TypeSafe 未配置 `TYPESAFE_API_KEY`，因此其 System One 路径已通过离线 HTTP fixtures，但没有声称在线兼容性已验证。设置该密钥后可运行 `cargo test -p jev-typed --test live -- --ignored case_3`。运行全部在线测试使用 `make test-live`，需三个专用 provider 密钥。

未提供业务标注集，因此未验证真实模型的误报/漏报、概率校准、阈值可迁移性或 p50/p95。这些是模型与应用的评估结果，不能由统一 SDK 或一次 smoke 推导。统一的是调用、答案含义与错误契约；供应商统计缺失和拒答仍明确可见。
