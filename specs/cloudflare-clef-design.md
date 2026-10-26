# Cloudflare Clef 集成设计

状态：待线上验收（代码与离线验证已完成，尚缺真实服务验收）。日期：2026-10-25。基线：`db3e663` / v0.4.0。
实施结果：[实现审查](cloudflare-clef-review.md)。
依据：[研究记录](../docs/research/cloudflare-clef.md)。验收：[验证计划](cloudflare-clef-verification-plan.md)。
本规格增量扩展 [统一后端设计](unified-decision-backends-design.md)，仅替代其中 provider/model 解析、路由及本规格明确列出的行为；其他原语和安全契约继续有效。

## 1. 目标与范围

让相同的 `Decision`、`Questions`、`Input`、case 和批处理代码能使用 Cloudflare Workers AI 上的 Clef、Clef-flash、Clef-omni，保留规范答案、metadata、有限重试和确定性凭据选择。

首版交付全部三模型的文本/JSON 决策、Cloudflare hosted 构造/配置/env、REST envelope 与错误转换，以及自托管 Clef codec 的明确入口。媒体输入、Gateway 路由、OpenRouter Clef、自动 failover、模型训练、GPU 部署自动化和 tokenizer 均不在首版。三种模型默认都应上线验收；某模型缺少实际访问权限时，标记该模型未验收，不把三模型支持写成完成。

## 2. API 与构造契约

新增 `Provider::Cloudflare`，序列化、Display 和 FromStr 为 `cloudflare`。新增 `CloudflareAccountId` 私有字段 newtype，实现 Debug、TryFrom、FromStr、Serialize 和验证式 Deserialize。SDK 选择严格的 32 位 ASCII 十六进制账户 ID 规则（拒绝空白，不 trim、不自动修正）；这是客户端验证策略，不是从 Run API schema 推出的完整服务端约束。

以下 API 已实现；线上可用性与验证边界见实现审查：

| 接口 | 行为 |
|---|---|
| `Client::cloudflare(account: CloudflareAccountId, token)` | 默认 Clef，token 使用现有 SecretString 转换约定，返回 Result |
| `Client::builder(Provider::Cloudflare).cloudflare_account(account)` | 延续 api_key/model/settings/http/build 链；account setter 返回 builder |
| `Config::cloudflare(account)` | 返回 provider=Cloudflare、model=Default 的非秘密配置 |
| `Endpoint::clef_system_one(url)` | 显式裸 System One + Clef 数值兼容策略，用于用户自托管；返回 Result |

`Client::new(Provider::Cloudflare, token)` 与未填 account 的 builder 返回 `InvalidConfiguration("missing Cloudflare account ID")`，不偷偷读取环境。非 Cloudflare builder 设置 account 在 build 时失败。`Config::new(Cloudflare)` 可建立待填配置，但未补齐时不能构建 client。

Builder 当前恰好五字段。将 provider 字段替换为内部 `HostedSelection`，聚合 provider 与可选账户配置，保持 key/model/http/settings 为原字段；不为了一个 provider 设置复制整个 builder，也不增加第六个独立字段。构建完成后转换成无缺失账户状态的 `Target` 枚举。若实施偏离此组织而产生超过五字段 builder，必须遵守仓库 typed-builder 要求并重新评审依赖。

使用示意：

```rust,ignore
let account = CloudflareAccountId::try_from(account_id.as_str())?;
let client = Client::builder(Provider::Cloudflare)
    .cloudflare_account(account)
    .api_key(token)
    .model(Model::ClefFlash)
    .build()?;
let answer: Triage = client.decide(ticket).await?;
```

不修改 derive 属性、typed answer 或 `ask/decide/run_case` 签名。account 配置属于 client，不属于每个问题或 case。

## 3. 模型解析与路由

| 逻辑 Model | Display / body model | Cloudflare URL 模型部分 |
|---|---|---|
| Default | 解析为 Clef | `@cf/cloudflare/clef` |
| Clef | `clef` | `@cf/cloudflare/clef` |
| ClefFlash | `clef-flash` | `@cf/cloudflare/clef-flash` |
| ClefOmni | `clef-omni` | `@cf/cloudflare/clef-omni` |

`FromStr` 同时接受短名与各自完整 `@cf/cloudflare/...`，先匹配已知别名，后尝试 ModelId。不要扩大 ModelId 通用字符集来接纳任意 `@` 路径。`Cloudflare/clef` 等 Hugging Face checkpoint ID 保持 Custom，专供自托管，不注册为 hosted 内置别名。

Cloudflare 首版仅允许这三个内置模型及 Default；Custom、Jev、Luna 均本地 UnsupportedModel。原三 provider 拒绝 Clef 内置变体。Custom 包装已知短名仍不能绕过校验。该限制是刻意的：任意 Workers AI 模型不一定说 System One 协议。扩展私有模型必须另有 route/body-selector 契约，不能仅透传字符串。

所有旧 provider 的默认值保持不变。`model.resolve` 中 `Some(_) => Jev113` 改为穷举，防止新 provider 意外落到 Jev。

内部设计：

- `Target::Fixed(Endpoint)` 保持原三 provider/custom 的完整固定 endpoint。
- `Target::Cloudflare(CloudflareAccountId)` 保存账户；请求路径只能使用固定 `https://api.cloudflare.com/client/v4` 与内部模型表构造。
- 每次 execute 获得一个不可变 `ResolvedCall`，包含 logical model、body model、endpoint/backend 与数值策略；body 和 URL 来自同一次解析。
- `RequestOptions.model` 覆盖必须同时改变 URL 和 body，重试固定复用同一已解析请求。禁止修改共享 client、覆盖 host、account 或 auth。
- 同一 client 的不同 clone 可并发调用 Clef 与 Flash，仍共享连接池和 semaphore；不能把每模型 route 变成新 transport。

现有 `wire_model` metadata 表示 body 中的精确 model，Cloudflare 为短名。新增可选 `routing_model: Option<String>` 记录完整路径模型 ID；其他后端为 None。它不是 URL，不包含账户。继续分别保存 requested/resolved/returned model，不能把请求短名伪装成响应值。当前别名不承诺固定权重。

## 4. 配置与凭据

Config 增加 `cloudflare_account_id: Option<CloudflareAccountId>`，YAML 名称为 `cloudflareAccountId`，None 不序列化。它对其他 provider 必须缺省；Cloudflare 必填。显式 null 和空白值拒绝，缺省才表示 None。为 from_yaml_str、from_config、builder 使用同一非秘密配置校验函数；严格未知字段、重复 key、解析预算规则保持。

```yaml
provider: cloudflare
cloudflareAccountId: "0123456789abcdef0123456789abcdef"
model: clef-flash
timeoutMs: 10000
retry:
  maxAttempts: 3
  initialBackoffMs: 200
  maxBackoffMs: 2000
```

这是形状示例，账户字符串不是可用账户。token 不入 YAML；`Client::from_config(config, token)` 不访问 env。

新增 provider key `CLOUDFLARE_API_TOKEN` 与配套 `CLOUDFLARE_ACCOUNT_ID`。保留 `DECISION_PROVIDER/API_KEY/MODEL`：

1. 显式 provider=cloudflare 时，通用 key 优先，否则读取 Cloudflare token；必须读取并验证账户。
2. 未指定 provider 时，Cloudflare token 与原三家 key 一样参与唯一 provider 推断；账户 ID 单独不参与推断。多个 key 即 AmbiguousProvider，即使某家配置不完整也不能自动跳过。
3. 唯一 token 属于 Cloudflare 但缺账户，返回账户配置错误；不能 fallback。无 provider + 通用 key 继续 AmbiguousProvider。
4. 被选中的空白 key/account/model 立即失败；自动推断时出现无效 provider key 继续遵循现有 fail-fast 规则。显式选择其他 provider 不读取 Cloudflare 账户与 token。
5. 不新增 `CLOUDFLARE_AUTH_TOKEN` 隐式别名；官方示例采用的变量名由应用自行映射，避免多 key 优先级歧义。

## 5. 协议层复用

新增 `backend/cloudflare.rs`，增加 `Backend::Cloudflare`；新增裸 Clef endpoint 对应明确的内部 codec/profile，而非猜测 host 或 model 文本。共享 `jev::prepare`；将 decode 拆成共享的 System One inner-result 解码与可选择的数值校验策略。Cloudflare adapter 只负责 envelope/错误与 profile，禁止复制三类答案转换。

请求仍为 `model/state/questions`，不套 `input` 或 `options` 外层。JSON 输入仍通过 `Input::json` 排序并文本化，避免 provider 切换改变证据语义。保持最多 64 题、64 choice options、32 score levels、64 字节 key、4096 字节 guidance，以及 input/request/response 的 1/2/4 MiB 硬上限。无需为了 Cloudflare 的媒体额度放大 SDK 限制。

成功响应：先通过现有有界、重复 key 检查的 `json::parse`，再提取 `result`。只接受对象型 result；`success` 缺省可接受，存在时必须是 true；`errors` 缺省可接受，存在时必须为空数组；字段类型错误、false、非空 errors、null result、裸 answers 均失败。已知 `messages` 若存在需为有界数组，不用于日志；未知扩展字段允许但受整体解析预算限制。外层数组最多 32 项；已知 error code 仅接收非负 u32 数字或 1–16 字节 ASCII 数字串，message 最多 4096 字节，其他未知字符串沿用 parser 4096 字节上限。

上述规则根据当前两种官方 envelope 示例制定，真实 fixture 是发布 gate。如果实际服务使用第三种外层，先更新证据与规格，不增加“任意找 answers”的容错。

Inner result 的 model、answers、usage 必填；usage 缺失不能补零。output_tokens 为实际报告值，不能因为价格表无输出费用强行改成零。继续检查原始 schema 的答案 ID、kind、选择项、legend、数值范围；不合格响应产生 Protocol。显式 refusal 保留现有语义，但不承诺 Clef 总会提供 refusal 或把低概率推断为 refusal。

## 6. 概率精度与 confidence

使用私有 `NumericPolicy`：旧后端保留当前规则；Cloudflare 与 `clef_system_one` 使用四位小数可解释误差。支持更高精度输出，不要求每个概率恰好四位小数。

令 N 为选项数、ε=0.00005、浮点裕量 δ=1e-9。所有概率仍须有限且在 [0,1]，键集合必须完整且唯一。Clef 分布要求 `abs(sum(p)-1) <= N*ε+δ`。不接受无限制 normalization，不改写 `Distribution::Reported` 中的 p。

Score 等级为 0..N-1，`score` 仍须在区间内；用未重归一化的报告概率计算 `Σ(i*p_i)`，要求与报告 score 的差不超过 `ε*(1 + N*(N-1)/2)+δ`。这同时覆盖每项舍入与 score 自身舍入，避免旧实现除以质量总和引入额外偏差。运算 N 已限制至 32，但实现仍使用安全的计数转换/算术。

Choice 的 selected 必须是允许的 key；不强制要求舍入后唯一 argmax，允许真实概率近似相同时的并列。confidence 保留原生值，不合成、不等同于 selected probability；即使参考实现相等，也不能推广到 SGLang 或 hosted。旧后端容差不随本变更扩大。

## 7. 错误、重试与观测

在现有 `Transport::attempt` 有界读取后，调用 backend 的 HTTP 错误分类；不要新建发送循环。Cloudflare 解析 `errors[]`，不依赖自然语言 message。优先按认证 HTTP 状态归类；429 再识别额度 code；无法解析错误体时仍可依据 HTTP 状态返回脱敏错误。

| 条件 | 错误 | 重试 |
|---|---|---|
| HTTP 401 / 403 | Authentication / PermissionDenied | 否 |
| HTTP 402，或 429 且任一合法 code=3036 | QuotaExceeded | 否 |
| 429/3040 或其他未知 429 | RateLimited | 现有 Retry-After/deadline 策略 |
| 400 / 413 / 422 | InvalidInput | 否 |
| 404 或其他非 transient HTTP | Api，安全 code + request ID | 否 |
| 500 / 502 / 503 / 504 | Api | 现有有限重试 |
| 408/3007/3008 | Api | 首版不自动重试，避免重复计费 |
| HTTP 2xx 但 envelope 表示失败 | Protocol | 否 |
| 非法 JSON、答案、usage | Protocol | 否 |

确定性额度错误优先于同一 errors 数组中的容量错误。首版无需给所有业务错误增加诊断字段；现有 Api 保留数字 code 字符串，其他变体保持原公共 API。禁止将远端错误 message、body、token 或账户 URL 输出到日志。

Cloudflare 请求使用 x-request-id，缺省时取 cf-ray；两者均走 256 字节、无控制字符检查，存在但非法则 Protocol，不跳过。cf-ray 是关联诊断标识，不等同于 body id，不用于幂等。只对 Cloudflare 使用该 fallback；保留 body id 单独字段。

队列等待、HTTP、读取和 backoff 继续共享一次 deadline；禁重试 feature 仍限时。429 retry 可能产生额外请求，连接失败以外的未知网络中断不擅自重试。Semaphore 是进程内并发控制，不承诺账户全局 RPM 控制。

## 8. 自托管与未来多模态

自托管使用 `Endpoint::clef_system_one("http://127.0.0.1:30000/v1/systemone")` + `Auth::None` + Custom `Cloudflare/clef`。该入口与 system_one 同一裸 envelope，区别仅为明确选择 Clef 数值策略；高精度 SGLang 输出也能使用。现有 `Endpoint::system_one` 保持旧策略，不能悄悄放宽所有 Jev 服务。

自托管 endpoint 继续要求显式 Custom model，不使用 hosted Default；不读取 Cloudflare token、不提取 Workers AI result。完整 endpoint 保留原路径；只允许 HTTPS 或 literal-loopback HTTP，保持无 userinfo/query/fragment、无 redirect。远程生产服务应由部署方提供 HTTPS 和鉴权。SDK 不接收请求 input 动态生成 endpoint。

正式接入文档只推荐有 joint head 支持的 System One serving 路线，区分 Clef/Flash 已有 cookbook 与 Omni 尚需部署验收。不要把普通 chat-completions endpoint 当成替代品。

未来多模态另写规格，必须先处理 Input 表达、媒体类型/解码体积/像素/时长限制、上传期限与按 backend 的能力拒绝；不能把 base64 混入当前文本 state 冒充媒体支持。本次不添加占位字段或空实现。

## 9. 模块改动与实施顺序

| 阶段 | 文件/边界 | 完成条件 |
|---|---|---|
| 1 | provider.rs、model.rs、新账户类型与 lib.rs 导出 | 全部别名/不兼容矩阵/账户校验有确定性测试 |
| 2 | client.rs、config.rs、metadata.rs | Target/ResolvedCall、每次 override、env/YAML 一次统一解析；五字段 builder 约束保持 |
| 3 | backend/cloudflare.rs、jev.rs、backend/mod.rs | 共用 inner codec、严格 envelope、局部数值策略、错误映射 |
| 4 | transport.rs、tests/wire.rs、tests/live.rs | 单发送路径、cf-ray、并发路由与三模型 smoke |
| 5 | README、crate README、quickstart、thresholds、自托管示例、相关 indexes | 文档明确短名/路径 ID、账户配置、截断与原生 confidence；发布 gate 满足 |

阶段可分 commits，但未完成全部首版验收不能将 spec 改为 Implemented。现有 case 格式与宏无需改动；除修复真实回归外不扩大范围。不新增 Rust 依赖或 feature；如确需变更，先补依赖研究并执行 supply-chain gates。

## 10. 方案取舍与发布风险

仅添加 Custom model 无法处理 `@`、账户 URL、外层结果与单次覆盖；拒绝。通用 Cloudflare 管理 SDK 对小协议增加依赖和第二套 HTTP 生命周期；拒绝。经 Worker proxy 转 System One 可以由用户部署，但不能作为 SDK 原生支持的前提。Gateway/OpenRouter 可另行接入，但不复用未验证的 Decisions 路由。

最重要的风险是远端截断与概率校准，而非 JSON 解码成功。必须在 quickstart/thresholds 明确：字节限制不保证上下文完整；默认模型是可更新别名；不同模型/serving stack 必须独立校准自动执行阈值。SDK 不推测 token 数，不伪造精度、账单或截断标志。真实三模型 smoke 与业务质量评估分开报告，绝不把通过协议测试当成准确率保证。
