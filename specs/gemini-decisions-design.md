# Gemini Decisions 集成设计

状态：已实现，随 0.6.0 发布，待线上验收。日期：2026-11-11。基线：`3a60c65` / v0.5.1。
依据：[研究记录](../docs/research/gemini-decisions.md)。实施结果：[实现审查](gemini-decisions-review.md)。
本规格增量扩展 [统一后端设计](unified-decision-backends-design.md) 与 [Cloudflare Clef 设计](cloudflare-clef-design.md)，只替代下文明确列出的行为。

## 1. 目标与范围

让相同的 `Decision`、`Questions`、`Input`、case 与批处理代码通过 Google Gemini Decisions API（`gemini-decision-1`）运行，保留规范答案、metadata、有限重试和确定性凭据选择。

首版交付：hosted provider、构造/配置/env、自定义 endpoint 协议、请求编码、响应与错误转换、线上冒烟测试。不在首版：多模态输入（image/audio/video/PDF）、Files API、OAuth、Vertex AI、按模态 usage 明细。

## 2. 公开 API

| 接口 | 行为 |
|---|---|
| `Provider::Gemini` | 序列化 / Display / FromStr 为 `gemini`；env key 为 `GEMINI_API_KEY` |
| `Model::GeminiDecision1` | `gemini-decision-1`；Gemini 的 Default；其他 provider 本地 `UnsupportedModel` |
| `Client::gemini(key)` | 等价于 `Client::new(Provider::Gemini, key)` |
| `Endpoint::gemini_decisions(url)` | 自定义 URL + Gemini 协议（如代理或网关） |
| `ResponseMeta::returned_model` | **破坏性变更**：`String` → `Option<String>`；Gemini 为 `None`，其余协议为 `Some` |

`Model::Custom` 在 Gemini 上透传（为后续预览模型保留），但包装已知内置名仍被拒绝。`DECISION_PROVIDER=gemini` 与无 provider 时的唯一 key 推断沿用现有规则，`GEMINI_API_KEY` 与其他 provider key 同时存在时报 `AmbiguousProvider`。

`returned_model` 改为 `Option` 而不是回填请求的 wire model：字段语义是"服务端实际返回"，伪造值会让调用方误以为经过了服务端确认。0.x 版本按 CLAUDE.md 直接变更，不走弃用流程；下个版本号应为 0.6.0。

## 3. 协议设计

### 3.1 鉴权

`Auth::ApiKey` 改为按协议选择凭据头：`Backend::Gemini` 发送 `x-goog-api-key: <key>`，其余协议保持 `Authorization: Bearer <key>`；分支穷举，新增协议必须显式选择。key 校验与协议解耦（`Auth::validate`），并收紧为 1–4096 字节的可见 ASCII——与原先"拒绝空白/控制字符 + HeaderValue 编码"的合集等价，只是非 ASCII key 现在统一报 `API key bounds`。

### 3.2 请求

复用 System One 问题编码：`jev::questions` 对 predicate criteria 的字段名泛型化，Jev 使用 `{"true","false"}`，Gemini 使用 `{"true_description","false_description"}`。顶层字段为 `model` / `input` / `questions`，保持声明顺序。

### 3.3 响应

- 先检查 `prompt_feedback.block_reason`：有值即返回 `InvalidInput("upstream content filter blocked input")`，不要求 `answers` / `usage`。
- `usage` 必需，`total_input_tokens` 必需；`total_output_tokens` 缺省为 0（文档保证）；`total_tokens` 缺省时用 checked add 计算；`total_cached_tokens` → `cached_tokens`，`total_thought_tokens` → `reasoning_tokens`；成本恒为 `None`。
- 答案复用 `jev::answers`：答案集合必须与 schema 完全一致，按声明顺序重排，choice 分布按 schema 选项顺序重排，score 的 `legend` 必须覆盖全部索引。
- `model` 与 `id` 均为 `None`，未知顶层字段（如 `input_tokens_by_modality`）忽略。

### 3.4 数值容差

新增 `NumericPolicy::Gemini`。`NumericPolicy` 统一用"最后一位小数的半个单位" `h` 表达：Standard 无舍入假设；Clef `h = 0.00005`；Gemini `h = 0.005`。分布质量容差为 `n·h`，score 容差为 `h·(1 + max(max+1)/2)`。Gemini 取两位小数是因为官方 score 示例（质量 1.01、期望值 1.13 报告为 1.12）在 Standard 下会被拒绝；该容差仍能拒绝缺项、未知选项、越界和明显失衡的分布。拿到真实响应样本后应依实际精度收紧。

### 3.5 错误

`Backend::http_error` 改为先由各协议生成 `ErrorSummary { code, quota, invalid_key, blocked, retry_after_secs }`，再统一映射状态码。Gemini 摘要只读取有界 JSON 中的结构化字段，从不保留远端 message：

| 条件 | 结果 |
|---|---|
| 400 且 `ErrorInfo.reason = API_KEY_INVALID` | `Authentication`（实测无效 key 为 400） |
| 401 / 403 | `Authentication` / `PermissionDenied` |
| 400 且字符串 `code` 属于五个过滤码 | `InvalidInput("upstream content filter blocked input")` |
| 429 且 `QuotaFailure.violations[].quotaId` 含 `PerDay` | `QuotaExceeded`（不重试） |
| 429 其他 | `RateLimited`，`retry_after_secs` 优先取 `Retry-After` 头，其次取 `RetryInfo.retryDelay`（向上取整） |
| 其余 | 原有映射；`Api.code` 取字符串过滤码或 `status`（如 `UNAVAILABLE`） |

details 仅在 `@type` 为对应 `type.googleapis.com/google.rpc.*` 时采信，数组上限 32。transport 在没有 `Retry-After` 头时使用错误中的 `retry_after_secs` 作为等待时间，仍受总 deadline 约束（等待超过剩余时间直接 `Timeout`）。

## 4. 验收标准

1. 官方 guide 中全部响应示例可解码；无效 key 的实测响应映射为 `Authentication`。
2. hosted 请求只带 `x-goog-api-key`，不带 `Authorization`；URL 为 `/v1beta/decisions`；body 与文档形状逐字节等价（JSON 语义）。
3. 现有协议行为不变：全部既有单测、wire、doctest、feature powerset 与 lint 门禁通过。
4. 线上：`make test-live-gemini` 以 `GEMINI_API_KEY` 跑通 typed 冒烟，`returned_model` 为 `None`。

## 5. 风险

- 数值精度、429 细分与 `prompt_feedback` 路径未经真实响应验证（见研究记录第 5 节）。
- API 处于 v1beta，字段可能变化；解码对未知字段宽松、对答案集合与类型严格。
