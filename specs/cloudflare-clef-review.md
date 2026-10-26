# Cloudflare Clef 实现审查

日期：2026-10-25。基线：`db3e663`，workspace 0.4.0；分支 `feat/cloudflare-clef`。
状态：代码实现与离线验证完成；Cloudflare hosted 验收待凭据，不能称为线上已验证。
对应 [设计](cloudflare-clef-design.md)、[验证计划](cloudflare-clef-verification-plan.md) 和 [研究](../docs/research/cloudflare-clef.md)。

## 实现结果

- 新增 Cloudflare provider、经过验证且 Debug 脱敏的账户 ID、Clef/Flash/Omni 内置模型与短名/完整路径别名。其他 provider 的默认模型不变。
- 构造器、五字段 builder、YAML 与确定性 env 选择支持账户配置；配置中不保存 token，其他 provider 拒绝账户字段，显式 null 拒绝。
- 新增私有 `target.rs`：固定 endpoint 与账户级目标分开；每次解析产生不可变 ResolvedCall，URL 与 body model 同步，clone 继续共享 transport/许可。
- Cloudflare REST adapter 解包 result，共用原 System One 编码和答案转换。成功标志、错误数组、已知 detail 字段与大小预算严格检查；不接受裸 hosted 响应，不补造缺失 usage。
- Clef 专用 NumericPolicy 处理参考实现四位舍入，保留原始概率；旧 Jev/OpenAI 数值验证不放宽。原生 confidence 独立保留。
- Cloudflare 429/3036 不重试，容量类 429 复用既有 deadline/retry；错误只保留安全 code，cf-ray 作为 x-request-id 缺省时的诊断 fallback。
- 新增 `routing_model` metadata 与裸 `Endpoint::clef_system_one`；自托管不要求 Cloudflare token，不自动猜测协议。
- 补齐 quickstart、根/crate README、thresholds、自托管示例、独立三模型 ignored smoke 与 `make test-live-cloudflare`。旧 provider 的 live 命令改成完整测试名，避免 `case_1` 等模糊过滤误选新付费测试。

没有新增依赖、feature 或 case 版本，也没有修改 derive 宏、Cargo manifests 或 lockfile。短名 `clef*` 现在是保留的 hosted 模型别名；原来把这些短名当自托管 Custom 的调用方，应使用 `Cloudflare/clef` 等 checkpoint ID 或另一个服务器别名。

## 实施细节与规格对应

`execute` 负责输入/模型选择，再把 ResolvedCall 交给私有 `execute_call`。测试只在模块内部将已解析 URL 的 origin 替换成本地 mock，保留生产路径与相同发送/解码逻辑；没有公开 hosted base URL override。生产路由另有三模型及 Default 的精确 URL 测试。

数值测试包含 64 项均为 0.0156 的回归、边界外拒绝、score 舍入误差界及 proptest 生成的 2–64 项分布。测试 fixture 明确为 synthetic，不是线上抓包。模型、账户、env/YAML、重定向凭据保护、response caps、队列 deadline、并发模型选择、case/batch 顺序均有覆盖；已有 wire 测试继续保护 typed refusal、重试取消、慢流、非法答案和原 provider 行为。

`make test-no-retry` 现在同时运行 library 与 wire 测试，覆盖 Cloudflare 私有测试在 retry feature 关闭时的行为。新增 `make docs` 生成并打开 SDK rustdoc。共享并发逻辑没有引入新的任务或可变模型状态。

按仓库最新稳定版要求，将工具链 pin 从 1.99.0 更新为 [Rust 1.99.1](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/)；edition 2024 与 MSRV 1.99 不变。该补丁版的官方发布日期为 2026-10-15。HTTP 继续使用现有 reqwest 0.13.5；没有借此次整合升级依赖图。

## 验证结果

以下最终检查在 Rust 1.99.1、本机 macOS arm64 上完成：

| 检查 | 结果 |
|---|---|
| `make build` | workspace / all-features 通过 |
| `make test` | nextest 176 项全部通过，6 项在线测试 ignored；cargo test 同样通过，另有 119 项 doctest 通过 |
| `make lint` | nightly fmt check、全 target clippy -D warnings、boundary pedantic/unwrap/indexing/panic/expect lints 全部通过 |
| `make test-no-retry` | 83 项 library 测试及 57 项 wire 测试通过 |
| `make feature-check` | 37 种 feature 组合全部通过；无 dev-dependency 遮蔽 |
| `make test-concurrency` | 跨 clone semaphore/deadline 测试连续 100 次通过 |
| 拒答测试定向复核 | 两个协议 case 各重复 10 次，通过且未复现 leaky-process 提示 |
| `make docs` | 生成并打开 SDK rustdoc 成功，无文档警告；新增 API 页面检查通过 |
| `make secret-check` | staged patch 扫描通过，无秘密泄露 |
| 文档与 diff | 本地链接、code fence、git diff --check、AGENTS/CLAUDE byte parity 通过 |

一次 nextest 全量运行将既有 `test_should_collect_required_refusals_and_preserve_outcomes::case_2` 标记为 passed/leaky；后续独立 10 次复跑未复现，cargo test 也通过。未推测根因或以压低 nextest 检查掩盖该提示。

依赖/lockfile/许可策略/打包配置均未改变，因此未运行 cargo audit/deny 或 publish dry-run；本 PR 不发布 crate。按用户明确选择，本次先完成实现与 PR，不运行真实付费 API；环境中也没有 `CLOUDFLARE_API_TOKEN`、`CLOUDFLARE_ACCOUNT_ID`。没有部署 GPU 服务或运行业务准确率基准，不将 mock、参考源码或 SDK 编译成功替代这些证据。

## 合并/发布前的线上验收

凭据由本地环境或 secret manager 提供后运行 `make test-live-cloudflare`。当前 smoke 对每个模型执行三种输入（英文、中文、JSON），再做一次模型 override 与默认恢复，共 15 次请求；正常情况下不多于 45 次 HTTP attempts（默认有限重试），可能产生费用。不得把 API token、账户 ID 或原始敏感输入提交到 fixture/日志。

记录三模型的实际 result/envelope、返回 model、usage、分布精度及权限结果，再将设计状态改为 Implemented。若真实服务与文档不同，先更新证据、adapter 和回归用例，再关闭这项验收。长输入截断、模型阈值校准、用户自托管 GPU 与 Omni serving 能力继续属于显式部署/应用验证边界。
