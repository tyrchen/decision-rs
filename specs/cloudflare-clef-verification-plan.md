# Cloudflare Clef 验证计划

状态：离线验证已执行，线上验收待凭据。日期：2026-10-25。
对应 [设计](cloudflare-clef-design.md) 与 [研究](../docs/research/cloudflare-clef.md)。本文件保留验收要求；实际执行结果见 [实现审查](cloudflare-clef-review.md)。

## 1. 证据分层

| 层级 | 能证明什么 | 不能证明什么 |
|---|---|---|
| 官方文档/固定 reference 源码 | 协议意图、参考格式和精度 | hosted 实际 payload、可用性或 SLA |
| 手工/参考代码导出的 fixture | codec、边界和错误分类 | 真实服务采用同版本实现 |
| wiremock 确定性测试 | URL/body/auth、限时、并发、重试 | 实际 token 权限和模型质量 |
| opt-in 三模型 live smoke | 当日账户的端到端协议兼容 | 长期模型稳定性、业务准确率 |
| 应用标注集 | 目标场景的质量、校准和成本比较 | 任意任务上的通用优势 |

所有 fixture 标注来源、日期与 model/revision。手工 fixture 明确标记 synthetic；线上 fixture 只使用无隐私合成输入，删除 token、账户和关联真实用户的 ID。缺凭据时只报告未运行，不能将 skip 计为 pass。

## 2. 模型、账户与配置测试

使用现有 rstest 参数化；测试名采用 `test_should_` 前缀，避免全局修改进程 env，沿用可注入 environment reader。

- Cloudflare 默认解析 Clef；三模型短名/完整 @cf 路径别名的 FromStr、Display、serde roundtrip。
- 原三 provider 默认模型不变；其与三 Clef 变体全部不兼容组合本地失败。
- Cloudflare 拒绝 Custom、Jev、Luna；Custom 包装已知短名不能绕过验证；`Cloudflare/clef` 在裸自托管下保持 Custom。
- 账户 32 位 ASCII hex 成功；空、31/33 字节、空白、Unicode、斜线、百分号、query、控制字符失败；Serde 不能绕过 newtype 验证。
- builder/new/from_config/from_env 的缺账户错误一致；非 Cloudflare 携带账户失败；直接构造不读取 env。
- 唯一 Cloudflare token + account 推断成功；account 单独不推断；多 provider token 歧义；通用 key 无 provider 歧义。
- 显式 Cloudflare 的通用 key 优先，不跨 provider fallback；空白 token/account/model 失败；显式其他 provider 忽略 Cloudflare 环境。
- YAML 有效配置与 roundtrip；unknown/duplicate 字段、显式 null/非法账户、缺账户、非 Cloudflare 账户、秘密 token 字段拒绝。覆盖 flatten 后未知字段仍拒绝，不能只测独立 DTO。

## 3. 请求与路由测试

生产 hosted 域名不能被公开 override。URL 解析器使用纯函数精确断言完整 Cloudflare URL；本地 wiremock 通过 crate-private 单元测试组装相同 Target/ResolvedCall 或测试内部 HTTP 边界，不能为测试加入公开 hosted base_url 开关。

1. 三模型分别断言 route 使用 `@cf/cloudflare/<name>`，body model 为短名，Bearer token 及 Content-Type 正确。
2. 同一次请求携带无 criteria Predicate、有 true/false criteria Predicate、Choice、Score；JSON 输入是与旧 provider 完全相同的规范字符串。
3. 单次 Clef→Flash→Omni override 同时改 route/body，下一次无 override 恢复默认；retry 不改变选择。
4. clones 并发不同模型，路由无交叉污染，仍共享 max_inflight 与连接池。
5. ask、typed decide、run_case、batch 使用同一发送路径；批结果顺序不变，case 版本不变。
6. 无效账户、模型、schema、超字节上限在零 HTTP 请求下失败。
7. 302 redirect 不跟随，不向目标泄露 token；日志和 Debug 不出现 token、输入、完整账户 URL。
8. 自托管完整 `/v1/systemone` 路径保留，model=`Cloudflare/clef`；不读取 Cloudflare env，不期待 result 外层；HTTP 非 loopback 拒绝。

## 4. 响应与数值测试

Cloudflare outer envelope 覆盖：完整成功、只含 result、success=false、success=null、非布尔 success、errors 非空/错误类型/超 32 项、null/missing result、裸 answers、HTML、重复 JSON key、超深与超体积。错误体与成功体都必须遵守读取和解析预算。可忽略扩展字段，不可忽略非法已知字段。

Inner result 覆盖三类型和两个 predicate 形式；按问题名重排，拒绝漏题、多题、未知选项、kind 不符、非法 legend、缺 model/usage。缺可选概率统计沿用公共 Unavailable 表达；live smoke 则检查三模型承诺的实际完整分布。必需 typed 字段 refusal 仍返回含 metadata 的 Refused，Outcome 则显式保留拒答。

数值回归矩阵：

| 用例 | 预期 |
|---|---|
| 64 个 choice 概率均为 0.0156 | Clef 接受，保留总和 0.9984，不重新归一化 |
| 分布偏差恰好 Nε，以及超过 Nε+δ | 前者接受，后者拒绝 |
| 高精度 normalized 概率 | Clef 与现有后端都接受 |
| 缺项、重复项、负值、>1、非有限数 | 拒绝，不能靠容差修复 |
| 从真实分布四舍五入出的 2..32 级 score | 符合推导误差界且 score 在合法区间时接受 |
| 在 score 误差界外构造的错误期望 | 拒绝 |
| 近似相同概率导致 choice 舍入后并列 | 接受合法 selected key |
| confidence 不等于 selected probability | 分别保留；accept 与 accept_confidence 仍取不同统计 |
| 同一超旧容差 fixture 送原 Jev/OpenAI backend | 维持原拒绝行为，无全局放宽 |

用 proptest 从归一化正向量生成 2..64 选项分布，四位舍入，验证允许误差且不改写概率；score 用 2..32 级。含 total_tokens 加法溢出、负/非有限 cost、missing usage；不把缺失统计补零。

## 5. 错误与生命周期测试

- 401/403/402 与 429/3036、429/3040、未知 429；code 数字和合法数字串，两种表示等价。
- 3036 与 3040 同时存在时 quota 优先；message 文本含“quota”但 code 未识别不改变归类。
- 非 JSON 429 仍 RateLimited；非 JSON 503 仍按 HTTP 归类；已知字段非法不得泄露原文。
- HTTP 200 success=false 不重试、不返回业务拒答；404、408、400/413/422 不重试。
- 503→成功、429→成功走现有次数限制；Retry-After 秒数与日期、超剩余 deadline、禁 retry feature。
- 队列等待、响应慢流、backoff 累计到同一 deadline；共享 semaphore 限制跨模型 calls。
- x-request-id 优先，缺省用 cf-ray；缺两者为 None；过长/控制字符/非 UTF-8 失败；body id 单独保留。
- requested/resolved/wire/routing/returned model、attempts、usage、latency 对应同一请求；cost 缺省为 None，不本地填价格。

## 6. live smoke 与质量评估

新增专用 ignored `test_should_accept_cloudflare_clef_protocol`，参数覆盖三模型，明确要求 `CLOUDFLARE_API_TOKEN` 和 `CLOUDFLARE_ACCOUNT_ID`，不借用通用 provider 的两参数构造。使用合成英文和中文短输入、三种原语、有/无 predicate criteria、JSON 证据；每个模型检查答案结构、完整选项分布、概率边界、元数据、非空 returned model 和有效 usage。预测值不写成精确断言。

复用同 client 做一次模型 override 再恢复，确认真实 route/body 兼容。64 题/64 选项压力主要在本地测，线上只在明确预算下采样；不要通过冲击配额验证 429。记录日期、SDK commit、模型别名、返回名称、请求次数与 reported usage，若 cost 缺失明确标记。参考格式或 local smoke 不能替代 hosted smoke。

增加 Makefile `test-live-cloudflare`，仅执行专用 ignored 测试，避免为测 Cloudflare 调用其他付费 provider；沿用现有 test-live 全量语义。不添加 shell 脚本，不把真实推理接入默认 CI。

业务评估另由应用侧提供有标签、许可清晰的数据：固定 train/calibration/test 划分，覆盖中文、多选项、模糊/域外输入、长文本和关键事实位于末尾的情形。比较现有生产模型、Clef、Flash、Omni；记录分类混淆矩阵、false positive/negative、Brier/ECE、自动化覆盖率及其风险、p50/p95、失败/重试率、token 用量。冷/热连接和不同服务地区分开；不将官方吞吐基准直接用作生产 SLA。

每个模型独立选择阈值；测试集不得用于调阈值。由应用风险要求定义接受界限，没有标签和风险目标时不虚构准确率 gate。协议支持可以验收，默认生产替换不能据此自动批准。

## 7. 实现后的验证命令与完成标准

先跑相关 codec/config/route 单元与 wire 测试，再使用已存在 Makefile：

```text
make build
make test
make lint
make test-no-retry
make feature-check
```

这些覆盖仓库要求的 cargo build/test、nightly fmt 检查、clippy -D warnings；`make lint` 还包括 boundary pedantic lint。若修改格式，先执行 `cargo +nightly fmt --all` 再检查 diff。feature-check 防止新 provider 意外依赖 yaml/derive/batch；只在确有 deadline/共享 semaphore 改动时额外跑 `make test-concurrency`。

仅依赖/lockfile/供应链/发布打包变化时补 `make audit`；实际发布时执行现有 publish-check。无新依赖的 adapter 不以机械执行供应链检查代替协议验证。绝不 cargo clean。

完成标准：上述确定性矩阵通过，原 provider 回归无变化，三模型 live smoke 证据齐全，公共 API/examples/docs 完成，研究中的未确认项有实际记录或清晰限定。自托管 GPU 运行若未验证必须单列，不能写成已部署。届时新增 `{feature}-verification` 或 review 记录并更新 specs/index，记录命令、结果、失败/跳过原因与剩余限制。

## 8. 初始研究与规格交付检查（历史记录）

本次仅研究与规格，不执行上述实现 gates。交付前检查本地链接、两个 index、Markdown code fence 与 diff 空白；AGENTS.md/CLAUDE.md 未修改。Rust 源码、Cargo、示例、toolchain 均未改变，因此依据仓库文档专项规则跳过 build/test/clippy/audit；这不表示拟议实现已经通过测试。

本次实际检查结果：三个新增文档及两个 index 的本地链接、code fence、行尾空白检查通过；`git diff --check` 通过；AGENTS.md 与 CLAUDE.md 仍 byte-identical。另以固定随机种子的 12,600 组归一化分布检查设计中的舍入误差公式，全部满足界限；这是公式 sanity check，不是 Rust 实现测试或模型推理测试。
