# OpenAI Decisions API 集成研究

核对日期：2026 年 10 月 9 日。OpenAI Decisions 与 jev-typed 的三种决策原语高度匹配，适合接入现有 typed decision 能力。建议将 SDK 的公共模型改为与供应商无关的决策模型，在内部编译不同的请求协议并校验答案；只增加 provider 和 endpoint 无法完成集成。

## 官方能力与适用边界

OpenAI 于 2026 年 10 月 6 日发布 Decisions beta。当前支持 `gpt-6-luna`，调用 `POST https://api.openai.com/v1/decisions`。官方所称约 10 倍速度提升是相对 Responses API 的产品描述，不能推导为相对 Jev 的性能保证。[发布记录](https://developers.openai.com/api/docs/changelog)、[Decisions 指南](https://developers.openai.com/api/docs/guides/decisions)。

请求包含共享 `input` 和命名的 `questions`；答案按问题返回。三种问题分别是条件成立概率 `predicate`、无序分类 `choice`、有序等级的连续评分 `score`。同一请求适合相互独立的问题；依赖前一个答案的问题应分成不同请求。[Decisions 指南](https://developers.openai.com/api/docs/guides/decisions)。

API 返回选择和评分的概率分布，并分别提供 `confidence`；拒答是单个问题的结果，其他问题仍可能成功。Choice 可以使用字符串或布尔值，布尔值与同名字符串是不同值。概率列表与问题列表都需要转换成项目内部结构。[HTTP API reference](https://developers.openai.com/api/reference/resources/decisions/methods/create)。

基础 input 价格为每百万 token 0.10 美元；Decisions 不收取 output 和缓存读写费用，地区与长上下文加价仍适用。成本计算不能直接复用模型在 Responses API 下的价格。[Decisions 价格说明](https://developers.openai.com/api/docs/guides/decisions#pricing-and-availability)。

## 与 0.2 实现的对应关系

本节记录研究时的 0.2 状态；0.3 的当前契约见统一设计与迁移文档。

| 项目现状 | OpenAI 对应 | 集成影响 |
|---|---|---|
| `State::Text` | 字符串 `input` | 可直接发送 |
| `State::Json` | 作为文本证据的 JSON | 需定义一致的序列化契约 |
| 按 key 保存的 `Questions` | 有 `name` 的问题列表 | 保留 key 与声明顺序 |
| `Noul` | `predicate`，或保留双分支描述的布尔 `choice` | 两条内部编码路径归一为同一种答案 |
| Choice 的 criteria map | `choices` 中的 value 与 description | 字符串 key 无需改变 |
| Score 的 criteria list | `levels` 中的 label 与 description | 增加稳定等级 key，按下标关联概率 |
| 按 key 保存的 `Answers` | 有 `name` 的答案列表 | 校验后转为规范答案 |
| 概率 map | 概率列表 | 按选项 key 或等级下标转换 |

上述现状来自 [question.rs](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/question.rs)、[state.rs](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/state.rs)、[answer.rs](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/answer.rs)、[wire.rs](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/wire.rs)；OpenAI 字段来自 [HTTP API reference](https://developers.openai.com/api/reference/resources/decisions/methods/create)。

## 代码中需要改变的边界

[Provider](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/provider.rs) 目前只管理 endpoint、surface 和模型 ID；[Client](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/client.rs) 总是序列化同一种 `DecisionRequest`，再反序列化同一种 `Response`。这适用于两个 Jev surface，但 OpenAI 不共享这个协议。

[Config](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/config.rs) 的全局默认模型是 Jev；只添加 OpenAI provider 会让默认配置产生错误模型组合。[Model](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/model.rs) 的 `Latest` 与 `wire_id(Surface)` 也带有 Jev 假设。模型解析应在 client 建立时结合 provider 完成，并在发起请求之前拒绝已知不兼容组合。

[Guidance](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/guidance.rs) 可以是字符串、对象或数组。公共问题描述需要收敛为明确文本，避免同一个结构化 criteria 在不同后端被解释成不同协议。`Input::json` 则应保留为有明确序列化规则的业务证据入口。

[ScoreLevels](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev/src/typed.rs) 目前仅提供等级描述。为每个等级保留稳定 key 和描述后，可以生成 OpenAI label，同时保持 Jev 的等级顺序。[derive](https://github.com/tyrchen/jev-typed/blob/0008e9be684a999e8d4cc4e30980890b9a88ce95/crates/jev-macros/src/decision.rs) 的 optional 当前仅意味着容忍缺失答案，不代表拒答；新设计应明确区分这些状态。

## 关键设计判断

普通 predicate 直接使用 OpenAI predicate。有显式 yes/no criteria 时，建议内部生成值为布尔 `true` 与 `false` 的 choice，完整携带两个 description，然后取 `P(true)` 作为公共 Predicate。它保留两条标准，但与原生 predicate 的统计行为并不保证相同，应分别评估。

所有后端共享同一个输入准备、问题 schema、答案校验、typed decoding 和错误模型。请求编码与初步响应解码是协议专属部分。现有连接池、deadline、重试和 bounded batching 可以复用；使用内部 enum 分派足够，不需要动态 async trait 或 actor。

统一调用方式不等于统一统计表现。`confidence` 不应被替换成选中项概率，缺失分布也不应被补成确定性答案。阈值需用应用标注数据重新评估。[官方答案解释](https://developers.openai.com/api/docs/guides/decisions#interpret-the-answers)。

## 实施前需要保留的限制

图像文档存在冲突：指南要求 inline base64 data URL，HTTP reference 当前允许公网 URL。第一版统一 SDK 限定为文本与 JSON 证据，避免将未核实的图像 URL 行为写进公共承诺。[图像指南](https://developers.openai.com/api/docs/guides/decisions#check-an-image-for-visible-damage)、[HTTP reference](https://developers.openai.com/api/reference/resources/decisions/methods/create)。

模型页面只列出 `gpt-6-luna`，未提供可依赖的日期 snapshot ID。固定这个模型字符串不能被描述为固定模型权重；SDK 应记录实际返回模型和自身版本。[GPT-6 Luna 模型页面](https://developers.openai.com/api/docs/models/gpt-6-luna#snapshots)。

OpenAI 的 429 既可能是速率限制，也可能是余额或额度耗尽。统一错误解析需利用 error code 区分，并防止对额度耗尽自动重试。[官方错误说明](https://developers.openai.com/api/docs/guides/error-codes)。

## 建议结论

采用允许 breaking changes 的 0.3 设计：公共 `Predicate`、`Choice<T>`、`Score<T>` 与 `Outcome<T>`；provider 在构造或配置边界选择；内部协议适配器负责 wire 差异。移除普通 API 中的 `Surface`、透传 trace 与不明确的全局默认模型。保留包名 `jev-typed` 和导入名 `jev`，将产品定位扩展为 typed decision SDK。

具体契约、迁移和验收标准见 [统一决策后端设计](../../specs/unified-decision-backends-design.md)。
