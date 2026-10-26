# Cloudflare Clef 集成研究

研究日期：2026-10-25。仓库基线：`db3e663`，decision-rs 0.4.0。
结论对应 [集成设计](../../specs/cloudflare-clef-design.md) 与 [验证计划](../../specs/cloudflare-clef-verification-plan.md)。本次完成文档、公开参考源码和仓库静态研究；未调用付费推理、未读取凭据、未部署 GPU 服务。下文区分官方事实、代码推导和设计选择。

## 1. 研究结论

推荐把 **Cloudflare 自己托管的 Workers AI Clef** 做成一等 hosted provider，第一阶段支持三款模型的文本和规范化 JSON 决策。共享现有 System One 问题编码、答案领域类型、连接池与限时重试；新增账户路由和 Workers AI 外层协议。不能只添加模型别名，也不需要 Cloudflare 管理面 SDK。

“Cloudflare 自己托管”与“用户自行部署 Cloudflare 开源权重”是两条路径。前者需要 account ID + API token；后者是用户维护的 System One endpoint，不应该要求 Cloudflare 账户。SDK 不负责加载模型、部署 GPU 或训练。

## 2. 模型与部署事实

| 模型 | Workers AI 路径 ID | 规模 | 当前托管上下文 | 每百万输入 tokens 美元 |
|---|---|---|---|---|
| Clef | `@cf/cloudflare/clef` | 27B | 65,536 | 0.240 |
| Clef-flash | `@cf/cloudflare/clef-flash` | 9B | 24,576 | 0.038 |
| Clef-omni | `@cf/cloudflare/clef-omni` | 30B MoE / 3B active | 64,000 | 0.150 |

上下文、规模分别来自 [Clef 模型页](https://developers.cloudflare.com/workers-ai/models/clef/)、[Flash 模型页](https://developers.cloudflare.com/workers-ai/models/clef-flash/)、[Omni 模型页](https://developers.cloudflare.com/workers-ai/models/clef-omni/)；费率与 [2026-10-20 更新的价格表](https://developers.cloudflare.com/workers-ai/platform/pricing/)交叉核对。模型页说明无输出 token 费用；这不意味着输入免费。价格是研究快照，不应写进 SDK 的 reported cost。

[发布文章](https://blog.cloudflare.com/clef-decision-models/)最初介绍两款模型；不能据此遗漏后来的 Omni，也不能把发布期 Flash 的 64K 描述当成当前 hosted 限制。公开模型卡标记 Apache-2.0；模型采用 backbone + joint schema head，一次 forward 对给定选项输出 logits，再逐题 softmax。它不是通过生成 JSON 文本实现分类。来源：[Clef 模型卡](https://huggingface.co/Cloudflare/clef)、[Flash 模型卡](https://huggingface.co/Cloudflare/clef-flash)、[Omni 模型卡](https://huggingface.co/Cloudflare/clef-omni)。

三者均能处理文本；Clef/Flash 有视觉能力，Omni 另支持音频。SDK 当前 `Input` 只有文本与 JSON 文本化，第一阶段不暴露这些媒体扩展。这是 SDK 范围选择，不是模型能力限制。

默认建议 Clef，理由是先保持通用决策质量取向；Flash 供显式成本/延迟选择，Omni 供实验及未来媒体接入。这个排序是设计判断，不是跨任务准确率保证。官方基准也存在不同模型胜出的任务；发布者基准不能替代业务标注集、中文输入和生产网络测量。

## 3. REST 协议及证据边界

请求发送到 `POST https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/{model_name}`，使用 Bearer API token；body 本身就是模型输入。Clef 的 body `model` 是短名 `clef`，URL 则使用完整命名空间。Flash/Omni 同理；不能把完整 `@cf/...` 填入 body，或只更换 body 不更换 URL。来源：[Run API](https://developers.cloudflare.com/api/resources/ai/methods/run/)与三款模型页。

模型输入使用 `state` 与按 ID 索引的 `questions`；`noul`、`choice`、`score` 与仓库现有 codec 对应。官方允许每次 1–64 题，问题 ID 至多 100 字符且可含点；SDK 现有 64 字节 ASCII 字母/数字/下划线/连字符是安全子集，无须扩大公共 schema。

[REST 入门文档](https://developers.cloudflare.com/workers-ai/get-started/rest-api/)展示 `result/success/errors/messages` 外层；模型页展示的是内部模型结果，不能混用 Worker binding 返回值与 REST 返回值。Run API 的示例又只列出 `result`，没有明确要求 `success`。因此设计支持两种有文档依据的**封装形式**：`result` 必须存在；`success` 若存在必须为 true，`errors` 若存在必须为空。不把裸 System One 响应自动当作 hosted 成功。真实 Clef REST fixture 仍需在线确认。

鉴权使用控制台 Workers AI token 模板。入门页建议手工 token 配 Read + Edit，Run API 权限栏则列出至少一项 Read/Write；这一文档差异应记录并以实际账户最小权限测试确认，不声称已测出最小 scope。Account ID 不等于 token；不支持 Global API Key + email 旧鉴权。

## 4. 参考源码揭示的兼容细节

读取并固定了 Cloudflare 官方发布的 [joint_schema_model.py](https://huggingface.co/Cloudflare/clef/blob/a30162b99d68a481d8bd75d38df069a0cf83df40/joint_schema_model.py)，revision `a30162b99d68a481d8bd75d38df069a0cf83df40`，文件 SHA-256 `0e304cf7c6500e8bb59bef7e2afd2c6373f82596dfb3b57d1aa93c175e2dc3a3`。

`systemone_answer` 的 noul 是 true 的概率；choice 取最高概率选项；score 是从零开始的等级期望。参考实现把概率、confidence 和 score 四舍五入到四位小数，usage 明确报告 input tokens 与零 output tokens。此处是参考代码行为，不能断言线上服务运行相同代码或相同舍入策略。

由此可推导兼容风险：仓库 `backend/mod.rs::distribution` 的固定 `0.001` 总和容差可能拒绝合法舍入结果。例如 64 个等概率选项，各自 `0.015625` 舍入成 `0.0156`，总和 `0.9984`，偏差 `0.0016`。应使用限定于 Clef 的、按选项数推导的误差边界，并保留原始报告概率，不偷偷重归一化。

自托管时不能只加载 backbone。Hugging Face 自动生成的通用 chat/vLLM 部署片段不证明 joint head 正确工作。[SGLang 官方 Clef cookbook](https://docs.sglang.io/cookbook/autoregressive/Cloudflare/clef)明确使用 `/v1/systemone`，支持在 0.5.21 之后合入，提供固定 nightly `0.5.22.dev20261009+g37ae292e6f` 和 `dev-clef` 镜像。部署前应固定实际 image digest，而非把浮动 tag 当稳定版本。

该 cookbook 覆盖 Clef/Flash；其有效上下文默认至多 16,384，且 confidence 公式可能不同于参考实现。不能继承 hosted 上下文或假定同权重不同 serving stack 的 confidence 等价；也不能据此宣称 Omni 已在同一路径验证。Omni 模型卡有独立参考加载说明，本次未做运行验证。

## 5. 运行限制、数据和错误

当前模型页说明过长文本 state 会被服务端截断，为问题/媒体留出空间。仓库的字节 cap 不是 tokenizer cap，不能承诺未截断；第一阶段不假造 `truncated=false`、不按字符数估算精确 token。需要完整证据的调用方必须自行按部署 tokenizer 分块或预检，模型别名更新后重新验证。

[Workers AI 错误表](https://developers.cloudflare.com/workers-ai/platform/errors/)区分 429/3036（免费额度耗尽）与 429/3040（暂时容量不足）。当前 SDK 所有非 OpenAI 429 都映射 RateLimited，会对确定性额度耗尽进行无意义重试。新 adapter 必须先识别结构化 code，再复用 retry 分类。403 模型权限/套餐错误不能伪装成模型回答中的 refusal。

[Limits](https://developers.cloudflare.com/workers-ai/platform/limits/)按任务、模型与计费方式定义配额；不能直接承诺 Clef 永远具有某个固定 RPM。现有 semaphore 约束并发，并不等于账户级每分钟限流；应用应按实际 quota 调节吞吐。

[Data usage](https://developers.cloudflare.com/workers-ai/platform/data-usage/)说明标准无状态推理的内容保留和不用于训练承诺，同时列出 stateful 功能、Gateway 日志/缓存与运营 metadata 例外。集成默认直连 Workers AI，不额外开启 Gateway 缓存或记录证据；不将此概括为任意配置下绝对零存储。

## 6. 仓库契合点与缺口

| 现有位置 | 静态核查结果 | 整合影响 |
|---|---|---|
| `provider.rs` / `model.rs` | 三个 provider；模型 ID 字符集不接受 `@` | 新增 Cloudflare 与三个已知别名；不要全局放宽任意 ID |
| `client.rs::build/execute` | endpoint 在构造时冻结，单次 override 仅解析 body model | 必须按调用解析 route + body model，并保持 clone 并发安全 |
| `backend/jev.rs` | 三类问题和答案已兼容参考 System One 格式 | 共享 codec，外加 Cloudflare envelope |
| `backend/mod.rs` | 错误仅解析 `error` 对象；分布固定容差 | 适配 `errors[]` 数值 code 与定向舍入规则 |
| `transport.rs` | 单一限时、限并发、有界读取路径；只取 x-request-id | 保持路径；增加 Cloudflare cf-ray 诊断 fallback |
| `config.rs` | 严格 YAML，provider 必选，无账户字段 | 增加非秘密账户配置并统一入口校验 |
| `Input` / Questions / derive / cases | 文本证据、规范化 JSON、稳定问题 ID | 无须改变业务类型、宏与 case 文件版本 |
| `tests/live.rs` | 只覆盖原三家，忽略测试显式 opt-in | 独立三模型 smoke，不能假定通用两参数构造足够 |

先读了现有 [依赖研究](decision-runtime-dependencies.md) 和 [统一后端研究](openai-decisions.md)。重新核对 [reqwest 当前文档](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html)，其最新页显示 0.13.5，现有 workspace 0.13 范围足够，仍可禁用 redirect 与内部 retry。建议零新增 Rust 依赖、零新增 feature；不用 Cloudflare SDK、Python runtime、tokenizer 或配置框架来完成这个 HTTP adapter。本次不更新 Cargo 或 toolchain。

## 7. 尚需实施阶段验证

1. 三模型真实 REST `result`、usage、返回 model 命名及错误样本；成功标志是否始终存在。
2. hosted 概率精度、完整分布、64 选项边界；不能用 reference fixture 冒充线上录制。
3. 实际 token scopes、账户模型可用性、429 code 与 Retry-After；不要为采样主动耗尽额度。
4. 中文/英文业务集的错误率、校准、延迟、自动化覆盖率；当前没有“优于现有 provider”的实测结论。
5. 自托管部署的版本/digest、实际上下文、confidence 及 Omni 支持状态。

以上不阻碍完成设计，但阻止把设计或 mock 通过称为 hosted 集成已验证。
