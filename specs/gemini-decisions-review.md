# Gemini Decisions 实现审查

状态：离线验证通过，随 0.6.0 发布；线上验收待完成。日期：2026-11-11。分支：`feat/gemini-decisions`（基线 `3a60c65`）。
对应 [设计](gemini-decisions-design.md) 与 [研究记录](../docs/research/gemini-decisions.md)。

## 1. 实现摘要

| 模块 | 变更 |
|---|---|
| `provider.rs` / `model.rs` / `target.rs` | `Provider::Gemini`、`Model::GeminiDecision1`、固定 endpoint `/v1beta/decisions` |
| `transport.rs` | `Auth::validate` 与协议感知的 `Auth::header(backend)`（Gemini → `x-goog-api-key`，穷举匹配）；`Endpoint::gemini_decisions`；无 `Retry-After` 头时使用错误体中的延迟 |
| `backend/jev.rs` | 抽出 `questions`（criteria 字段名泛型化）与 `answers`，供 Jev / System One / Clef / Gemini 共享 |
| `backend/gemini.rs` | 请求编码、envelope / usage / `prompt_feedback` 解码、Google 错误摘要（`ErrorInfo` / `QuotaFailure` / `RetryInfo`，按 `@type` 采信） |
| `backend/mod.rs` | `ErrorSummary` 统一错误映射；`NumericPolicy` 以半单位 `h` 统一 Standard / Clef / Gemini |
| `metadata.rs` | `returned_model: Option<String>`（破坏性变更） |
| `client.rs` | `Client::gemini`；env 推断加入 `GEMINI_API_KEY` |
| 文档 / 自动化 | README（两份）、quickstart、研究记录、规格与索引；`make test-live-gemini` |

与设计无偏离。

## 2. 验证结果

| 门禁 | 结果 |
|---|---|
| `make build` | 通过 |
| `make test`（nextest + cargo test） | nextest 224/224；lib 131、wire 64、derive 4、cases 5、macros 20、doctest 121，全部通过；live 11 项按设计忽略 |
| `make lint`（nightly fmt、clippy `-D warnings`、pedantic + unwrap/expect/indexing/panic 边界 lint） | 通过 |
| `make test-no-retry` | lib 130、wire 58 通过 |
| `make feature-check`（37 种 feature 组合） | 通过 |
| `make test-concurrency`（100 次压力） | 100/100 |
| `cargo doc` with `-D warnings` | 通过 |

`cargo audit` / `cargo deny check` 未运行：本次没有新增或升级依赖，`Cargo.lock` 未变。

新增覆盖：

- guide 中的多问题响应、score 示例（质量 1.01）逐字作为 fixture 解码；同一 score 示例在 Standard 策略下被拒绝，证明容差只作用于 Gemini。
- 生产 endpoint 对假 key 的 400 `API_KEY_INVALID` 响应原样作为 fixture，映射为不可重试的 `Authentication`，且 Debug/Display 不含远端 message。
- 真实 HTTP（wiremock）：hosted 与自定义 endpoint 均只发送 `x-goog-api-key`，不发送 `Authorization`；body 与文档形状 JSON 语义一致；`RetryInfo` `0s` 触发一次重试，`60s` 超出 deadline 立即 `Timeout`，不重放请求。
- `prompt_feedback` 拦截、usage 缺失/溢出、答案缺失/多余/类型不符/越界、legend 不完整、重复键、HTML 响应均被拒绝。
- protobuf duration 解析的边界用例；env 推断、歧义、跨 provider 模型拒绝、非 ASCII / 含空格 key 拒绝。

## 3. 未完成项

1. **线上验收**：ADC 已通过鉴权，但测试项目 AI Studio 预付费余额为 0，生产 endpoint 返回 402（实测错误体已作为 fixture，映射为 `QuotaExceeded`）。充值或改用 Postpay 后运行 `make test-live-gemini`，并记录一份真实响应，用于确认数值精度（并据此收紧 `NumericPolicy::Gemini`）、`prompt_feedback` 与 400 两种拦截路径，以及 429 的 `QuotaFailure` / `RetryInfo` 形状。
2. **ADC / OAuth**：不在 0.6.0 范围，见研究记录第 5 节。

版本：`returned_model` 类型变更属于破坏性变更，工作区版本升至 0.6.0；CHANGELOG 由 `make release`（git-cliff）生成。
