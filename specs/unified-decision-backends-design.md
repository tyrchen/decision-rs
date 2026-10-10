# 统一决策后端设计

状态：已实现，目标版本 0.3。日期：2026 年 10 月 9 日。测试结果、实现细节与在线验证边界见 [验证记录](./unified-decision-backends-verification.md)。

将 jev-typed 扩展为统一的 typed decision SDK，支持 OpenAI Decisions、OpenRouter Jev、TypeSafe Jev 与自托管 System One。业务代码声明一次问题，用同一种调用方式得到同一种答案；provider 选择仅出现在 client 构造或配置处。内部负责模型解析、协议编译、答案关联、校验和错误转换。

本设计取代 [原设计](./jev-sdk-design.md) 中关于公共 wire 类型、surface、默认模型、optional 答案与透传字段的契约。允许直接移除旧 API，不增加 deprecated 别名。包名 `jev-typed`、Rust 导入名 `jev` 和 workspace 划分保持不变。研究依据见 [OpenAI Decisions 集成研究](../docs/research/openai-decisions.md)。

## 目标与范围

必须支持 typed derive、动态问题、文本与 JSON 证据、三种决策原语、显式拒答、响应 metadata、YAML 配置、case 文件、共享连接池、自托管 endpoint、限时重试与有界并发。切换后端不得要求重写 decision struct、判断 wire type、处理答案列表或手动调整模型命名空间。

第一版不提供图像、音频、工具调用、Responses API 模拟决策、自动跨供应商 failover 或在线阈值校准。它们不能作为完成本版本的前置条件。多个模型的准确率、概率校准、原生 confidence 与缺失可选统计字段不保证一致；SDK 统一访问契约，并明确表示不可用的数据。

## 用户调用契约

下面是已实现的 API 示例。其他后端只需替换构造语句，业务部分完全相同。

```rust
use jev::{Choice, ChoiceOptions, Client, Decision, Predicate, Probability};

#[derive(Debug, Decision)]
struct Triage {
    /// Is the customer reporting a defect?
    #[jev(yes = "Reports broken product behavior.",
          no = "Asks a question or requests a feature.")]
    is_bug: Predicate,
    /// Which team should handle the ticket?
    team: Choice<Team>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceOptions)]
enum Team {
    /// Payments, invoices, and refunds.
    Billing,
    /// Product failures and technical support.
    Technical,
    /// Requests outside the other categories.
    Other,
}

async fn triage(key: String, ticket: &str) -> Result<Triage, jev::Error> {
    let client = Client::openai(key)?;
    let answer: Triage = client.decide(ticket).await?;
    let gate = Probability::try_from(0.9)?;
    if answer.is_bug.is_yes(gate) {
        let _team_for_automatic_routing = answer.team.accept(gate);
    }
    Ok(answer)
}
```

入口约定如下，所有 fallible 构造及调用返回 `Result`：

| API | 契约 |
|---|---|
| `Client::new(provider, key)` | 明确选择 hosted provider，使用该 provider 的默认模型 |
| `Client::openai/openrouter/typesafe(key)` | `new` 的同构便利入口 |
| `Client::builder(provider)` | 必填 key，可配置 model、共享 HttpClient 和 Settings；`build()` 一次校验并冻结 |
| `Client::from_config(config, key)` | 配置不含秘密，构造时一次解析默认模型 |
| `Client::from_env()` | 依照下述确定性规则解析 provider、key 和 model |
| `Client::for_endpoint(endpoint, auth, model)` | 自定义完整 endpoint；协议由 Endpoint 构造器声明 |
| `decide::<D>(input)` | 返回 `D`；拒答的必需字段产生命名错误 |
| `decide_with::<D>(RequestOptions)` | 返回 `Decided<D> { value, meta }` |
| `ask(input, &questions)` | 返回规范 `DecisionResponse`，保留逐题拒答 |
| `run_case(&case)` | 使用同一发送路径，返回带 case ID 的规范响应 |
| `decide_all` / `run_cases` | 返回 `Result<Vec<Result<Output, Error>>, Error>`；外层报告非法 batch 参数，内层保持逐项结果与输入顺序 |

不再提供能把已绑定密钥的 client 改到另一 provider 的 `with_provider`。调用方新建另一个 client，可以共享同一个 `HttpClient`。这是 SDK 建立的 reqwest 连接池包装：固定关闭重定向与隐式重试，允许配置连接池参数，通过 clone 共享。`Client::http()` 返回可克隆的包装；其只读 reqwest 引用可用于应用的其他请求。移除注入任意裸 reqwest Client 的入口，因为无法检查它的 redirect/retry 策略。Client clone 共享不可变配置、连接池与并发许可；不通过内部可变状态切换后端。

`RequestOptions::new(input)` 返回 Result，只支持 `.model(model)`，用于同 provider 的单次覆盖；不能覆盖 provider、endpoint 或 credentials。`decide_with` 的问题始终来自 `D`，动态问题仅通过 `ask` 传入。移除 `raw`，避免把规范结果误称为供应商原始响应。

## Provider 与模型解析

公共 `Provider` 是非穷尽枚举，包含 `OpenAi`、`OpenRouter`、`TypeSafe`。公共 `Model` 包含 `Default`、`Jev113`、`JevLatest`、`Gpt6Luna`、`Custom(ModelId)`；`Default` 默认选择 provider 对应的模型。`Latest` 的含糊别名和全局 `DEFAULT_MODEL` 被移除。

| Provider | 默认 Model | 实际 wire model | 内部协议与默认 endpoint |
|---|---|---|---|
| OpenAi | Gpt6Luna | `gpt-6-luna` | OpenAI Decisions，`https://api.openai.com/v1/decisions` |
| OpenRouter | Jev113 | `typesafe/jev-1.13` | Jev Decisions，`https://openrouter.ai/api/alpha/decisions` |
| TypeSafe | Jev113 | `jev-1.13.0` | System One，`https://api.typesafe.ai/v1/systemone` |

Client 建立后保存 `ResolvedBackend`，包括目标、协议、已解析模型和限制。已知模型家族不匹配立即返回 `Error::UnsupportedModel`，不发 HTTP。JevLatest 在 OpenRouter 编码为 `~typesafe/jev-latest`，在 TypeSafe 编码为 `jev-latest`；OpenAi 拒绝这两个 Jev 变体。绝不把显式 Jev 模型偷偷换成 Luna。

`Model::Custom` 原样发送，验证 ID 为 1 到 128 字节，字符仅允许 ASCII 字母、数字及 `._:/~+-`。这是扩展入口，不承诺远端接受自定义模型；它也不绕过 SDK 输入和响应限制。自托管 endpoint 必须使用显式 custom model，不使用 hosted 默认模型。单次模型覆盖使用相同解析器，不重复准备输入、不持久修改 client。

`Model::FromStr` 将 `default` 解析为 Default，`gpt-6-luna` 为 Gpt6Luna，`jev-1.13`、`jev-1.13.0`、`typesafe/jev-1.13` 为 Jev113，`jev-latest`、`~typesafe/jev-latest` 为 JevLatest；其余验证后成为 Custom。Display 对应输出 `default`、`gpt-6-luna`、`jev-1.13`、`jev-latest` 或原始 custom ID。直接构造的 Custom 若使用已知内置 ID，在解析时拒绝，要求使用对应变体，避免绕过家族校验。HTTP 路径和模型命名空间是内部知识。

`gpt-6-luna` 目前没有已确认的日期 snapshot；不能承诺默认模型固定权重。Metadata 同时记录逻辑 model、请求 wire model、响应 model 和 SDK 版本，供评估追溯。[官方模型页面](https://developers.openai.com/api/docs/models/gpt-6-luna#snapshots)。

### 环境与 YAML

保留 `JEV_PROVIDER`、`JEV_API_KEY`、`JEV_MODEL`；provider key 包括 `OPENAI_API_KEY`、`OPENROUTER_API_KEY`、`TYPESAFE_API_KEY`。环境规则：

1. 有 `JEV_PROVIDER` 时，使用非空 `JEV_API_KEY`，否则使用所选 provider key；不读取其他 provider key 作为 fallback。
2. 无 provider 时，仅一个非空 provider key 可以自动推断 provider；零个报 MissingCredentials，多个报 AmbiguousProvider。
3. 无 provider 但存在非空 `JEV_API_KEY` 报 AmbiguousProvider，不猜测通用密钥的归属。
4. 出现的空白 key、无效 provider 或 model 立即报配置错误，不跳过后尝试另一个密钥。
5. 未配置 model 使用 Default；配置 model 走相同解析和兼容性校验。

YAML 使用 camelCase、`deny_unknown_fields`，只包含非秘密设置：

```yaml
provider: openai
model: default
timeoutMs: 10000
retry:
  maxAttempts: 3
  initialBackoffMs: 200
  maxBackoffMs: 2000
thresholds:
  bugYes: 0.9
  routeProbability: 0.8
```

Config 必须明确 provider，不使用全局默认。Settings 聚合 deadline、retry、limits、并发上限与 thresholds，并统一复用于 builder/config。deadline 为 1 ms 到 120 s；maxAttempts 为 1 到 10；initialBackoff 为 1 ms 到 maxBackoff，maxBackoff 不超过 30 s。配置全在构造时校验。若实现的 builder 实际超过五个字段，使用 typed-builder；不要为三个必要构造参数引入 builder 依赖。

### 自定义 endpoint

移除 `Surface`、`with_surface` 和 `with_api_base`。Endpoint 私有字段由 `Endpoint::system_one(url)`、`Endpoint::jev_decisions(url)`、`Endpoint::openai_decisions(url)` 创建；url 是完整 endpoint，不通过 `set_path` 覆盖调用方路径。

Auth 是 `ApiKey(SecretString)` 或 `None`；None 仅限显式 custom endpoint。Hosted 构造器不接受无认证模式。自定义 endpoint 允许 HTTPS，及 IP 字面量 loopback 的 HTTP；拒绝 userinfo、fragment、query 和其他 scheme。禁止自动重定向，避免凭据转发；自定义地址本身是应用的可信部署配置，不从请求 input 中取得。测试使用相同 custom endpoint 机制，不新增测试专属路径覆盖。

## 公共领域模型

### 输入与问题

`Input` 替代 `State`，存储一次验证的文本证据。`TryFrom<&str/String>` 校验 UTF-8 字节数；`Input::json(&T)` 接受对象或数组，将 JSON object key 递归排序、array 保持顺序，生成 compact JSON 文本。所有后端读取完全相同的文本：Jev 的 `state` 是字符串，OpenAI 的 `input` 是字符串。

`decide/ask/RequestOptions::new` 接受 `I: IntoInput`；这个 sealed trait 的 `try_into_input(self) -> Result<Input, Error>` 支持 &str、String、Input 和 &Input，不给未校验字符串提供 infallible From。已准备输入使用共享不可变文本，借用转换不重新解析 JSON。JSON 文本化是明确的 breaking change，避免同一个业务对象随 provider 改变证据编码。Json scalar、超过深度或大小限制的值被拒绝。Input 的 Debug 仅显示类型和长度。

移除可为任意 JSON 的 `Guidance`。问题 instructions 和选项 descriptions 使用私有字段的 `GuidanceText`，通过 TryFrom 在边界验证；允许自然语言、换行和制表符，拒绝其余控制字符。schema 构造完成后不可变。

公共原语改为 `Predicate`、`Choice<T>`、`Score<T>`，去除 Jev 的 Noul 名称。Question 是字段私有的公共只读结构，Kind 是内部枚举；调用方通过 builder 与 derive 建立 schema，不直接构造可绕过验证的枚举字段。Questions 的公共 iter/get 返回只读视图。

Choice 保留稳定字符串 key 和描述；Score 每个 level 同时保存稳定 key 和描述。两者 key 默认由 enum variant 转成 snake_case，均支持 `#[jev(key = "...")]`。`ScoreLevels::levels()` 改为有序的 `LevelSpec`，包含 key 与 description；声明顺序仍是评分尺度，下标从零开始。

### derive 与动态问题

保留 `#[derive(Decision, ChoiceOptions, ScoreLevels)]`、`#[jev(...)]` 和 doc comment 作为模型 instructions 的规则。Decision 字段支持 Predicate、Choice、Score 及各自的 `Outcome<T>`。移除 `Option<T>` 字段和 `#[jev(optional)]`：缺失答案是协议错误，拒答是显式状态。

动态 builder 提供 `predicate(key, instructions)`、`predicate_with(key, instructions, yes, no)`、`choice(key, instructions, options)`、`score(key, instructions, levels)`；build 返回已验证 Questions。动态 Score 的 LevelSpec 与 derive 共用类型，不另外解释 label。

`Decision::questions()` 保持静态缓存；宏在编译期验证字面量。移除公共 `from_validated_entries` 逃生口：生成代码通过同一个安全 schema builder 构造缓存，首次初始化保存 `Result<Questions, SchemaError>`；trait 改为 `questions() -> Result<&'static Questions, Error>`。SchemaError 是可 Clone 的非秘密诊断类型，缓存失败通过克隆转为 Error::InvalidSchema。宏的编译期检查提升诊断质量，运行时构造维持安全契约，失败不 panic。

编译期错误覆盖重复问题/选项/等级 key、空文档、越界长度或数量、携带数据的 variant、不支持的字段类型、只给 yes 或 no、Outcome 嵌套、Option 字段。手写 trait 实现仍经过发送前的公共 schema 校验，不被宏专用优化绕过。

### 概率与接受条件

`Probability` 是字段私有的公共 f64 newtype，`TryFrom<f64>` 仅接受有限的 `0..=1`，提供 `get()`；所有响应概率、confidence、阈值和 sample roll 使用它。Thresholds 从 YAML f64 构造时完成验证。

Predicate 提供 `probability()`、`is_yes(threshold)`、`is_no(threshold)`、`decide(yes_threshold, no_threshold)`。若两个阈值同时满足，decide 返回 Unsure，不能依赖分支顺序。Choice 保留 typed value，Score 保留有限的连续 raw score、nearest、normalized、at_least；nearest 是舍入后的等级，不是最大概率等级，恰好半级时向上舍入，必须在文档中区分。涉及手写选项/等级 trait 映射的 nearest、at_least 和 sample 返回 `Result<_, Error>`；不一致的 key/index/from_key/from_index 产生 Error::Protocol 的结构化 InvalidMapping reason，不 panic。derive 保证这些映射一致。

Choice 和 Score 暴露原生 `confidence() -> Option<Probability>` 与概率分布访问。分布内部用 `Reported(ValidatedDistribution)` / `Unavailable` 表示，既不补零，也不造 one-hot；`probability_of` 在 unavailable 时返回 None。

`Choice::accept(threshold)` 改为根据选中项的概率接受；该概率不可用时返回 None。这是有意的 breaking change，使常见路由门限基于具体事件。需要原生 confidence 的调用方显式读取 confidence 或使用 `accept_confidence(threshold)`；SDK 不把两者替换。Score 的 confident 只判断原生 confidence，absence 返回 false。保留 weighted sampling，改为 `sample(roll) -> Result<T, Error>`；分布不可用时报告 StatisticsUnavailable，不静默回退到选中项。

每个发布模型需要独立评估阈值。切换 provider 改变配置与统计结果，但不改变访问方法和返回类型。[官方阈值建议](https://developers.openai.com/api/docs/guides/decisions#interpret-the-answers)。

### 拒答与规范答案

`Answer` 是非穷尽枚举，包含 Predicate、Choice、Score、Refusal；不透传未知 wire JSON 为 Other。`Outcome<T>` 包含 `Answered(T)` 与 `Refused(Refusal)`，Refusal 只包含问题 key，不伪造服务器未提供的理由。

动态响应保留每个 Refusal。Typed 普通字段拒答时返回 `Error::Refused`，携带非空的拒答 key 集合和 `ResponseMeta`；收集所有必需字段拒答后返回。Outcome 字段则成功构造为 Refused，其余字段照常解码。协议错误始终使整次调用失败，即使字段使用 Outcome，也不转成业务拒答。

Jev 没有拒答时自然全部是 Answered；若远端显式返回 refusal，使用同一结果。低概率、未知类型、缺失答案和 HTTP 403 都不等于 Refusal。

`Decision::from_answers(&Answers)` 改为返回 `Result<Self, DecodeError>`。DecodeError 只表示 Refused（非空 key 集合）或 InvalidMapping（schema 与手写 trait 不一致），不需要供应商或 metadata。宏先收集普通字段的拒答，再逐字段调用 checked accessors；Outcome 字段不进入拒答错误集合。Client 在边界将 Refused 加上本次 ResponseMeta 变成 Error::Refused，InvalidMapping 变成 Error::Protocol；确保 decoder 不依赖全局 metadata，也没有在错误发生后丢失本次 usage。

## 内部架构与执行顺序

```text
公共 Client 和 Decision schema
    ↓ 输入与 schema 校验、模型解析
PreparedDecision
    ↓ Backend::prepare
PreparedRequest 和 ResponsePlan
    ↓ 共享 Transport：认证、deadline、重试、读取上限
供应商 JSON
    ↓ Backend::decode
候选规范答案
    ↓ validate_response against ResponsePlan
DecisionResponse
    ↓ Decision::from_answers
业务 struct 或命名错误
```

内部 Backend 使用封闭 enum，分派到 Jev Decisions、System One、OpenAI Decisions 适配器；不引入 async-trait、dyn backend、Actor 或锁。后端适配器只做同步准备与解码，共享 Transport 是唯一异步 HTTP 层。新供应商只需新增解析规则和编码/解码实现，公共领域类型不携带 provider 分支。

PreparedDecision 持有已验证 Input、Questions 引用、已解析模型。PreparedRequest 固定 URL、认证方式、JSON body 和 ResponsePlan；body 使用有界 writer 只序列化一次，复用可克隆的非 streaming reqwest Request。若 request 无法克隆，在发送前返回内部构造错误；不得 panic。重试不重新读取环境、改模型或重建 schema。

ResponsePlan 为每题记录领域 kind、wire kind、predicate 编码策略、合法选项/等级集合及声明顺序。解码依照 plan，不只看服务器 type；这使由布尔 choice 编码的 Predicate 能安全还原。

## 协议适配规则

### Jev 两种协议

继续支持仓库现有 Jev 请求/响应形状，provider 的默认 endpoint 保持不变。发送固定的文本 state；Predicate 编码为 noul，分支描述保留 criteria.true/false；Choice 编码为 criteria map；Score 发送有序 descriptions。

两种协议使用私有 DTO，共享字段复用同一个 Jev codec。地图答案转换到规范 Answer；可选 confidence、probabilities、legend 的 absence 被保留。公开 schema 与 Answer 不直接 derive 供应商 Deserialize。

普通 API 移除 session_id、user、trace；不自动把字段转换成其他平台的 safety_identifier。这些可选供应商字段不属于本版本统一契约。Tracing 仅在 SDK 本地记录可靠请求 metadata；要求远端专有字段的应用需在升级时明确调整集成。

### OpenAI Decisions

默认请求仅包含 model、字符串 input、questions 数组。每题必填 name，值为规范问题 key。普通 Predicate 编码为 predicate，读取 probability；带 yes/no 描述的 Predicate 编码为布尔 choice，两条 description 完整保留，规范概率只取 value 为布尔 true 的 probability，不用选中项 confidence，不把字符串 "true" 当布尔 true。

领域 Choice 只发送字符串 value 与对应 description。Score levels 发送稳定 key 作为 label，criteria 文本作为 description；读取 value 下标关联等级，返回 label 必须与该下标的发送 label 相符。读取原生 continuous score，不重新计算替代服务器值。[OpenAI 字段规范](https://developers.openai.com/api/reference/resources/decisions/methods/create)。

所有 name 都按名称关联，最后按 schema 顺序排列。无 name、null name、未知/重复 name、缺失题目、错误 wire kind 均是协议错误；不使用位置 fallback。OpenAI 的答案顺序保证不能代替名称校验。

布尔 Predicate 的 response plan 要求 choice、概率 value 均为布尔值，且完整覆盖 true/false。它没有原生 predicate 的概率校准保证；实现需用独立 fixture 和 live 评估验证该策略，不把 choice confidence 放入 Predicate。

## 统一响应校验

所有协议在产生公共响应前运行同一套 schema 关联校验：

1. 答案数量不超过问题数；名称集合与请求完全相同，顺序归一为声明顺序。Refusal 占用正常题目位置。
2. 每个已回答题目的领域 kind 与 schema 一致；未知 wire type 直接报 UnsupportedAnswerType，不尝试猜测。
3. 所有 probability/confidence 有限且在 0 到 1；已报告的分布必须完整覆盖合法选项或等级，拒绝未知项与重复项。
4. 分布之和与 1 的偏差不超过 `PROBABILITY_SUM_TOLERANCE = 0.001`。范围越界不因容差而接受；通过后的原始概率保留，不静默重标定。
5. Choice 的选中值属于选项集合；不要求它是概率最大项，避免对未被官方承诺的行为增加限制。
6. Score 处于 `0..=(levels-1)`；存在分布时，score 与概率加权均值差不得超过 `SCORE_MEAN_TOLERANCE = 0.001 * max(1, levels-1)`。按有效概率质量计算均值以隔离总和的舍入误差，不修改返回值。
7. OpenAI 必须提供其协议必填的 confidence 和分布；Jev 缺失可选字段成为 Unavailable，有字段但为空或不完整则报错。Jev legend 可缺失，存在时下标必须合法且完整；展示标准来自请求 schema。
8. DTO 对象容忍新增未知属性，以便上游加字段；kind、名称、选项和数量不容忍变化。JSON duplicate object key 必须在解析时拒绝，不能由 Value 覆盖后丢失证据。

校验失败是 Error::Protocol，携带问题 key 和结构化 reason，不附带输入、criteria、答案原始 body 或模型写出的任意文本。传输与协议成功后才执行 typed decoder；自定义 Decision 实现通过 checked accessors 解码。

## Metadata 与错误处理

`DecisionResponse` 包含规范 Answers 和 `ResponseMeta`。Meta 保存目标 provider（custom endpoint 为 None）、requested/resolved/returned model、可选 request ID、SDK 版本、Usage、耗时与 attempts。Usage 的 token counters 使用非负 u64；缓存/推理细分真正缺失时为 None，不能补成 0；完整 total_tokens 不存在时用 checked addition 得到。

费用仅保留供应商报告的可选非负有限 `reported_cost_usd`；OpenAI 未报告费用则 None。SDK 不嵌入动态价表，也不把 None 解释为免费。`request_id` 仅来自已知的响应 header（OpenAI x-request-id；Jev 若不存在则 None）；body 中的 id 单独保留为可选 `response_id`。不把 model response ID 偷换成 request ID；二者分别限制为 256 字节。

Error 使用 thiserror，保留 source 的传输错误。稳定类别包括 InvalidInput、InvalidSchema、InvalidConfiguration、AmbiguousProvider、MissingCredentials、UnsupportedModel、Authentication、PermissionDenied、QuotaExceeded、RateLimited、Timeout、Transport、Protocol、Refused、StatisticsUnavailable；未归类 HTTP 错误为 Api，保留 status 与可选 code/request ID。具体非法值与任意服务器文本不进入 Display。

协议专属 error DTO 将不同字段映射为这些类别。OpenAI 的 `credit_balance_exhausted`、`organization_spend_limit_exceeded`、`project_spend_limit_exceeded`、`organization_usage_limit_exceeded` 均映射 QuotaExceeded 且不可重试；其 error.type 为 `insufficient_quota` 时同样处理，兼容旧服务的同名 code。普通 429 和 `slow_down` 是 RateLimited。Jev 的 402 映射 QuotaExceeded。未知 code 只依据已知 HTTP 状态处理，并保留 bounded、经过控制字符转义的 code 供诊断，不记录未审查的 error.message。[OpenAI 错误语义](https://developers.openai.com/api/docs/guides/error-codes)。

## 资源限制与可靠性

以下是 SDK 产品限制，不宣称为各远端模型上限：

| 对象 | 限制 |
|---|---|
| input 文本及最终 JSON 证据 | 每项 1 MiB，不能为空白 |
| JSON 证据深度 | 32 层；object/array 总节点最多 10000 |
| 单个 JSON 字符串/字段名 | 字符串 256 KiB，字段名 256 字节 |
| 问题数 | 1 到 64 |
| key | 1 到 64 字节，ASCII 字母/数字/下划线/连字符 |
| 单条 instructions / description | 1 到 4096 UTF-8 字节 |
| Choice / Score 数量 | 2 到 64 / 2 到 32 |
| 最终 JSON 请求 body | 2 MiB |
| HTTP 响应 body | 4 MiB，按实际读取字节计数 |
| API key | 1 到 4096 字节，不允许空白和控制字符 |
| 响应 model / request ID / error code | 256 / 256 / 128 字节 |
| 响应 JSON 深度 / 节点 / 未建模字符串 | 32 / 10000 / 4096 字节 |
| case 文件 / 数量 | 16 MiB / 10000，每项仍受 input/schema 限制 |
| batch 长度 / 并发数 | 最多 10000 项 / 1 到 128 |
| 同 client 及其 clone 的在途请求 | 默认 64，配置范围 1 到 128 |

输入上限可以通过 validated Limits 向下调整，不能提高硬上限。非空文本允许自然语言及正常空格；使用 checked arithmetic 统计序列化长度、节点数和 token 总数。Input::json 的有界 serializer 在构建整个 Value 前限制写出长度与深度，避免先分配超大对象再拒绝；解析外部 case 文件则先限制文件读取和 JSON/YAML 嵌套，禁止展开无界 alias。

Case 文件的外层结构采用独立预算：深度 40、节点 1000000；YAML 另限制事件 2000000、单文档与累计 scalar/comment 字节，不允许 anchors、aliases、merge keys 或 includes。每项证据进入 Input 时仍执行深度 32、节点 10000 等更严格的限制，避免把单条响应的预算误用于整个批量文件。

删除 bytes/4 的模型上下文猜测。统一 byte cap 不承诺等于 tokenizer；远端 context 超限返回结构化 InvalidInput。JSON 请求已经准备好后开始网络总 deadline，默认 10 秒，覆盖连接、发送、完整读取、全部重试与 backoff。准备阶段受同步大小上限约束，不把它宣传为可中断的网络 deadline。

Tokio time 和 sync 无论 retry feature 是否启用都必须启用；无 retry 仍有真正 deadline。共享 Semaphore 限制同 client 及其 clone 的在途请求；等待许可包含在网络总 deadline 内，许可在每次发送前获取，完成读取后释放，backoff 不占许可。每个新 client 有独立许可池，即使共享 HttpClient；不承诺全应用的速率治理。

重试默认最多三次，间隔 200 ms 起；指数退避的计算值封顶 2 秒，有效 Retry-After 优先且不被封顶缩短，若超过剩余总 deadline 则返回 Timeout，不提前重试。只有连接建立失败、明确的瞬时限流与 500/502/503/504 可重试；发送后断连或单次读取超时不自动重放，避免无法确认是否计费的请求。达到总 deadline 后不再重试。

不自动重试认证、额度耗尽、解析/校验失败或拒答；不自动切换供应商。关闭 reqwest 自带的应用级重试，确保 attempts 只由 SDK 计算。batch 使用现有 stream bounded concurrency，不逐项无限 spawn；用户取消后 drop 未完成 future，不做后台 detached 请求。

密钥使用 SecretString 与 sensitive Authorization header；禁止重定向。Client/Input/Refusal/Error 的 Debug 不包含密钥、input 或描述。Tracing 仅记录 provider、模型、题数、attempt、latency、token、request ID 与校验类别；移除固定 confidence 小于 0.5 就 warn 的策略，SDK 不替用户决定业务门限。继续 forbid unsafe_code。

## Cases 与迁移

Case 文件升级为 `version: 2`，使用 `input`、`predicate`、Choice options、Score levels 的规范格式；每个 options/levels 元素包含 key 和 description。拒绝缺失或其他 version，不自动解释 version 1。YAML/JSON 加载复用公共 schema builder，case 不包含 provider；同一 CaseSet 可以分别交给不同 client。

```yaml
version: 2
cases:
  - id: ticket-001
    input:
      subject: Duplicate charge
      body: Please refund the second payment.
    questions:
      refund:
        type: predicate
        instructions: Is the customer requesting a refund?
        criteria:
          yes: Explicitly requests money back.
          no: Asks about an invoice without requesting money back.
      team:
        type: choice
        instructions: Which team should handle the ticket?
        options:
          - key: billing
            description: Payments and refunds.
          - key: other
            description: Requests outside billing.
      urgency:
        type: score
        instructions: How urgent is the ticket?
        levels:
          - key: low
            description: Can wait for ordinary support.
          - key: high
            description: Immediate assistance is needed.
```

criteria 在 predicate 上是成对的可选 yes/no；Choice 不接受 criteria，Score 不接受 criteria 或独立数字 value。input 字符串保持原文，对象/数组经过 Input::json 规则。加载后 case ID/name 上限 256 字节，ID 唯一且为非空 ASCII 字母/数字/下划线/连字符，name 允许自然语言但拒绝异常控制字符；其他字段使用上述 input/schema 限制，未知字段拒绝。

| 旧 API | 新 API 或迁移动作 |
|---|---|
| Noul / noul / noul_with | Predicate / predicate / predicate_with |
| State / Guidance JSON | Input；证据通过 Input::json，guidance 改成明确文本 |
| Client::new(key) / with_provider / with_http | provider 便利入口或 Client::new(provider, key)；连接池通过 HttpClient 共享 |
| Model::Latest / DEFAULT_MODEL | JevLatest / provider-aware Model::Default |
| Surface / with_surface / with_api_base | hosted 自动解析，或显式完整 Endpoint |
| raw / wire Response | ask / 规范 DecisionResponse |
| optional / Option 决策字段 | Outcome 字段保留拒答；缺失答案为错误 |
| Choice::accept 基于 confidence | accept 基于选中项概率，或显式 accept_confidence |
| 手写 ScoreLevels descriptions | LevelSpec key 与 description |
| Decision::from_answers 返回 Error | 返回 DecodeError；Client 负责附加响应 metadata |
| session_id / user / trace | 删除远端透传；应用保留本地 tracing 上下文 |
| version 1 case 文件 | 显式改为 version 2，更新字段与 levels/options |

实施时同步更新 README、各 crate 文档、quickstart、primitives、thresholds、cases、所有 examples 与 trybuild fixtures。原设计标记为历史；不留双重 schema、旧别名或未使用代码。Release notes 明确 accept 语义、JSON 文本化与拒答迁移。

## 实现组织

| 文件或模块 | 职责 |
|---|---|
| provider.rs / model.rs | 逻辑 provider/model 与解析验证 |
| backend/mod.rs | ResolvedBackend、ResponsePlan、封闭分派 |
| backend/jev.rs / backend/openai.rs | 私有 DTO 和协议编解码 |
| transport.rs | 唯一 HTTP/认证/deadline/retry/读取实现 |
| input.rs / question.rs | 私有字段的规范输入与 schema |
| answer.rs / typed.rs | 校验后的规范答案、Outcome 与 typed decoding |
| metadata.rs / error.rs | 规范 metadata 与稳定错误类别 |
| client.rs | 公共 API 编排，不包含供应商 JSON 字段 |
| cases.rs / batch.rs | 规范 case 加载和有界执行，共享发送链路 |
| jev-macros | 规范 schema/decoder 的 compile-time 生成 |

原 wire.rs、state.rs、guidance.rs 被对应职责替换并删除。沿用已有 reqwest、serde、thiserror、secrecy、Tokio、indexmap；优先不增加 OpenAI SDK 依赖。若实施引入新 crate，先查当前官方用法、维护状态和安全记录，再更新 lockfile 与审计；本设计本身不更新依赖版本。

## 验证与完成标准

用同一份 Triage schema、相同 Input 和等价 fixture，通过三个 hosted provider 以及自定义 System One endpoint，断言同一种 typed 结果和规范答案顺序。测试只断言归一契约，不要求真实模型给出完全相同数字。

必须覆盖：

- 每种协议的 golden request；普通 Predicate 与双 criteria Predicate；布尔 true 对字符串 true 的区分；Choice key 与 Score label/index 的往返。
- 默认/显式/单次模型解析、错误家族零 HTTP 请求、Custom 原样发送、多个环境 key 的歧义和无跨供应商密钥 fallback。
- 正常/混合拒答、Outcome 字段、必需字段拒答集合、缺失/重复/未知/null name、错 kind、duplicate JSON key。
- NaN/无限/越界概率、分布缺项/未知项/错误总和、Score 越界及加权均值不一致、Jev 可选统计缺失与 OpenAI 必填字段缺失。
- accept 与 accept_confidence 使用不同数值的 fixture；sample 在无统计时出错；Score nearest 与众数不同的 fixture。
- 429 限流对额度耗尽、402、401/403、各 5xx、Retry-After、整体 timeout、retry feature 关闭时 deadline、取消不继续发送、发送后断连不重放。
- request/response/input/case 各字节与深度上限，checked counters，Debug 与 tracing 的输入/密钥脱敏，redirect 不转发 Authorization。
- 宏的错误诊断、手写 Decision 非法 schema、case version 2 的 YAML/JSON 等价、有界 batch 保序与逐项错误。

使用 rstest 做协议矩阵，proptest 检查字段关联、顺序与分布不变量，wiremock 检查真实 HTTP 交互。测试命名沿用 `test_should_`。Live smoke 标记 ignore；针对每个 hosted provider 验证请求可接受、返回可归一，以及双 criteria Predicate 的实际 boolean choice 支持。Live 不用固定数值当稳定断言。

实施后的 Makefile 必须保留并按新模块更新 build/test/lint/feature-check/audit/test-live/publish-check，不增加独立 shell automation。Rust 变更运行完整 build、test、nightly fmt、clippy gate，关键边界追加 pedantic 与 panic/indexing/unwrap lint；依赖、lockfile 或发布内容变更运行 audit/deny。宏与 feature matrix 通过后再判定可发布。

在有授权密钥的环境运行 hosted smoke，并用应用标注集比较概率阈值下的误报、漏报、拒答率、覆盖率、p50/p95 耗时和 usage。没有密钥时离线协议测试可完成，但不得把 beta 实际请求兼容性和真实校准描述为已验证。研究与设计文档完成不代表 SDK 已实现。

版本完成条件：三个 hosted 后端共享全部公共调用与类型；所有确定性差异封装在私有适配器；公开字段不存在 wire map/list 或 surface；拒答和数据缺失没有被伪装；旧实现与文档已移除或迁移；上述 gates 和明确的 live 状态可供复核。
