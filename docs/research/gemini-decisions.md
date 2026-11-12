# Gemini Decisions API 集成研究

研究日期：2026-11-11（API 发布当天）。仓库基线：`3a60c65`，decision-rs 0.5.1。
结论对应 [集成设计](../../specs/gemini-decisions-design.md) 与 [实现审查](../../specs/gemini-decisions-review.md)。本次读取官方 guide、API reference、模型页、OAuth quickstart 与计费页原始 Markdown，并对生产 endpoint 做了错误探针（假 key、无凭据、ADC）；ADC 鉴权已通过，但测试项目预付费余额为 0，尚未拿到真实推理响应。下文区分官方事实、实测事实和设计选择。

## 1. 结论

Gemini Decisions 是与 OpenAI Decisions、Jev、Clef 同类的"类型化问题 → 校准概率"接口，不是规则引擎（不是 DMN/JDM/决策表）。它的问题与答案词汇（`noul` / `choice` / `score`、按 key 的 map、`legend`、`confidence`）与 System One 同源，因此可复用现有 System One 问题编码和答案校验；差异集中在 envelope、鉴权头、usage 字段和错误模型。推荐做成一等 hosted provider `Provider::Gemini`，第一阶段只暴露文本/JSON 输入。

## 2. 官方事实

来源：[Decisions API guide](https://aistudio.google.com/docs/decisions-api?codelanguage=rest)、[API reference](https://aistudio.google.com/api-reference/decisions.md)、[CreateDecision](https://aistudio.google.com/api-reference/decisions/create-decision.md)、[Gemini Decision 模型页](https://aistudio.google.com/docs/models/gemini-decision-1.md)。

| 项 | 事实 |
|---|---|
| Endpoint | `POST https://generativelanguage.googleapis.com/v1beta/decisions`（Beta，仅 Gemini Developer API） |
| 鉴权 | `x-goog-api-key: $GEMINI_API_KEY` |
| 模型 | 仅 `gemini-decision-1`；输入上限 262,144 tokens，输出 0 token |
| 请求 | `model`、`input`（`string` \| `Content` \| `Content[]`）、`questions`（map，1–1,000 个） |
| 问题 | `type` + `instructions`（文本）+ `criteria`；`noul` 的 criteria 可选，为 `{true_description, false_description}`；`choice` 为 `{option_id: description}`（2–255，描述可为空串）；`score` 为非空描述数组（2–255） |
| 答案 | `noul`：`noul` ∈ [0.01, 0.99] + `confidence`；`choice`：`choice` + `probabilities` + `confidence`；`score`：`score`（Σ k·p_k）+ `probabilities`（键为 `"0"`..）+ `legend` + `confidence`。reference 中四者均为 required |
| 响应顶层 | `answers`、`usage`、`prompt_feedback`。**没有 `model`，也没有响应 id** |
| Usage | `total_input_tokens`、`total_output_tokens`（guide：恒为 0）、`total_tokens`、`total_cached_tokens`、`total_thought_tokens`、按模态拆分数组、`grounding_tool_count` |
| 选项排序 | 服务端按 UTF-8 字节序对 choice key 排序，平局取字母序靠前者；不影响按 key 读取 |
| 输入过滤 | guide：HTTP 400，`error.code` 为字符串 `safety` / `prohibited_content` / `blocklist` / `image_safety` / `content_blocked`。reference：200 响应中 `prompt_feedback.block_reason` 有值且 `answers` 为空 |
| 其他限制 | key 为 `[A-Za-z0-9_-]{1,64}`；option key 1–128 字符；JSON 嵌套 100 层；不支持 context caching、Batch、Flex/Priority；超限返回 400 |
| 多模态 | image/audio/video/PDF，inline base64 或 Files API / HTTPS URI |

## 3. 实测事实（2026-11-11）

| 请求 | 结果 |
|---|---|
| `x-goog-api-key: invalid-fixture-key` | **HTTP 400**，`status: INVALID_ARGUMENT`，`details[ErrorInfo].reason = API_KEY_INVALID`，`error.code` 为整数 400 |
| 不带任何凭据 | HTTP 403，`PERMISSION_DENIED`，reason `ACCESS_DENIED` |
| `Authorization: Bearer <api key>` | HTTP 401，`UNAUTHENTICATED`，期望 OAuth access token |
| ADC token（仅 `cloud-platform` scope）+ `x-goog-user-project` | HTTP 403，reason `ACCESS_TOKEN_SCOPE_INSUFFICIENT` |
| gcloud 默认 OAuth client 申请 `generative-language` scope | 登录页 `400 invalid_scope`，需使用项目自有 Desktop OAuth client（`--client-id-file`） |
| ADC token（`cloud-platform` + `generative-language.retriever`）+ `x-goog-user-project` | 鉴权通过；预付费额度耗尽的项目返回 **HTTP 402** `RESOURCE_EXHAUSTED` |

402 错误体只有 `code` / `message` / `status`，没有 `details`；SDK 映射为不可重试的 `QuotaExceeded`。响应头中没有 `x-request-id` 或其他请求 id。由此得到三个对 SDK 有约束力的结论：API key 只能走 `x-goog-api-key`；无效 key 是 400 而不是 401，按现有通用映射会被误报成 `InvalidInput`；Google 错误体的 `code` 是整数，现有字符串 `code` DTO 无法解析。

## 4. 与 SDK 的映射

- 现有 schema 上限（≤64 题、choice ≤64 项、score ≤32 级、key 规则）都是 Gemini 限制的子集，无需改动校验。
- `Kind::Predicate` 直接映射 `noul`，branch 描述映射 `true_description` / `false_description`；与 OpenAI 不同，不需要把带 criteria 的 predicate 改写成 boolean choice。
- `Input` 当前只有文本与规范化 JSON 文本，发送为 `input` 字符串。多模态是模型能力，但不在本 SDK 首版范围。
- 响应不返回模型名，`ResponseMeta.returned_model` 必须能表达"未报告"。不应把请求的 wire model 回填为"服务端返回值"。
- `total_output_tokens` 依文档恒为 0；缺省按 0 处理。成本不由服务端报告，保持 `None`。

## 5. 证据边界

- **数值精度未经真实响应验证**。guide 的 score 示例概率之和为 1.01、期望值 1.13 却报告 1.12，choice 示例中又出现三位小数。可见文档示例至少按两位小数呈现。设计因此采用"每个值独立四舍五入到两位小数"的容差模型，使全部文档示例可解码；拿到真实响应后应据实收紧。
- guide（HTTP 400）与 reference（`prompt_feedback`）对输入拦截的描述冲突，两条路径都需处理。
- 429 区分"日配额耗尽"与"瞬时限流"依据 Google 通用错误模型（`google.rpc.QuotaFailure` 的 `quotaId` 含 `PerDay`、`google.rpc.RetryInfo.retryDelay`），未针对本 API 实测。
- OAuth / ADC：官方 OAuth quickstart 要求项目自有 Desktop OAuth client，scope 为 `cloud-platform` + `generative-language.retriever`，请求带 `Authorization: Bearer` 与 `x-goog-user-project`。实测可通过鉴权。SDK 0.6.0 只支持 API key；ADC 留作后续独立设计（自动刷新 token 需要额外依赖）。
- 计费：Decisions 仅在 Gemini Developer API（AI Studio 计费）上提供，Vertex AI 没有该接口，Vertex 侧 credit 不直接适用。Prepay 余额为 0 时所有请求 402；促销 Cloud Credits 需先充值才能抵扣（[billing](https://aistudio.google.com/docs/billing)）。
