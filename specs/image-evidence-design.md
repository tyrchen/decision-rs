# 图像证据设计

状态：已实现，随 0.7.0 发布；OpenAI 线上已验证，Gemini 待线上验收。日期：2026-11-17。基线：`6bae352` / v0.6.0。
本规格增量扩展 [统一后端设计](unified-decision-backends-design.md) 与 [Gemini Decisions 设计](gemini-decisions-design.md)，只替代下文明确列出的行为。

## 1. 目标与范围

让一次决策的证据在文本之外携带一张或多张图像（PNG、JPEG、WebP），返回与纯文本决策相同的有界答案（predicate / choice / score + confidence）。驱动场景是 Motif 审查渲染后的视频帧：每个问题携带一张 960×540 截图，加上旁白台词与预期画面的简短文本。

首版交付：`Image` 与 `Input` 的图像证据 API、按字节嗅探媒体类型、数量与大小上限、OpenAI 与 Gemini 原生图像请求编码、其余后端在网络调用前失败、证据摘要（digest）、带 metadata 的批处理入口、离线 golden 测试与可选线上冒烟测试。

不在首版：图像 URL / Files API、按问题附图（OpenAI 的 instructions 图像）、音频 / 视频 / PDF、case 文件中的图像、按模态拆分的 usage。

## 2. 官方协议事实（核对日期 2026-11-17）

| 项 | OpenAI Decisions（`POST /v1/decisions`） | Gemini Decisions（`POST /v1beta/decisions`） |
|---|---|---|
| 文本输入 | `input: string` | `input: string` |
| 多模态输入 | `input: [{"role":"user","content":[parts]}]` | `input: [blocks]` |
| 文本部分 | `{"type":"input_text","text":…}` | `{"type":"text","text":…}` |
| 图像部分 | `{"type":"input_image","image_url":"data:<mime>;base64,<b64>","detail"?}` | `{"type":"image","data":"<b64>","mime_type":<mime>,"resolution"?}` |
| 细节档位 | `detail`: `low` / `high` / `auto`（默认）/ `original` | `resolution`: `low` / `medium` / `high` / `ultra_high` |
| 图像格式 | 文档只示例 PNG data URL，未列出格式与上限 | `image/png`、`image/jpeg`、`image/webp` 等 8 种；inline 每请求至多 100 MB |
| 不支持 | `file_id`、音频、文件 | 通用 `application/octet-stream`；MIME 必须与内容一致 |
| usage | `input_tokens`（含缓存）、`input_tokens_details.cached_tokens`、`output_tokens`、`total_tokens`；返回 `model` | `total_input_tokens`、`total_cached_tokens`、`total_output_tokens`（恒 0）、`total_tokens`、`input_tokens_by_modality`；无 `model` |

来源：[OpenAI Decisions 指南](https://developers.openai.com/api/docs/guides/decisions#check-an-image-for-visible-damage)、[OpenAI create decision reference](https://developers.openai.com/api/reference/resources/decisions/methods/create)、[Gemini Decisions 指南 · Input / Images](https://aistudio.google.com/docs/decisions-api#media)、[Gemini CreateDecision reference](https://aistudio.google.com/api-reference/decisions/create-decision.md)。

## 3. 公开 API

| 接口 | 行为 |
|---|---|
| `Image::try_from(Vec<u8>)` / `Image::try_from(&[u8])` | 按魔数嗅探 PNG（`89 50 4E 47 0D 0A 1A 0A`）、JPEG（`FF D8 FF`）、WebP（`RIFF····WEBP`）；空、未知格式、超过 `MAX_IMAGE_BYTES` 返回 `InvalidInput`。构造时计算 SHA-256 |
| `Image::with_detail(ImageDetail)` | `Auto`（默认，不发送字段）、`Low`、`High`；分别映射 OpenAI `detail` 与 Gemini `resolution` |
| `Image::media_type()` / `detail()` / `as_bytes()` / `byte_len()` / `digest()` | 只读访问；`Debug` 只输出类型、字节数与摘要前缀，不输出内容 |
| `ImageMediaType` | `Png` / `Jpeg` / `Webp`，`as_str()` 返回 MIME；调用方无法声明媒体类型 |
| `Input::with_image(image)` / `Input::with_images(iter)` | 在已校验文本之后追加图像；校验数量 ≤ `MAX_IMAGES`，总字节 ≤ `MAX_TOTAL_IMAGE_BYTES` |
| `Input::images()` / `Input::digest()` | 图像切片（按追加顺序）；覆盖文本与全部图像的 `Digest` |
| `Digest` | 32 字节 SHA-256，`as_bytes()`，`Display` 为小写十六进制 |
| `Provider::accepts_images()` | `OpenAi`、`Gemini` 为 true |
| `Error::ImagesUnsupported { provider }` | 不支持图像的协议在请求编码前返回；不发起网络调用 |
| `Limits::image_bytes` | 每次调用的原始图像总字节，默认等于硬上限 16 MiB，只能下调 |
| `decide_all_with`（`batch` feature） | 与 `decide_all` 相同的有界并发，但每项返回 `Decided<D>`，保留每次调用的 `ResponseMeta` |

上限常量：`MAX_IMAGES = 16`，`MAX_IMAGE_BYTES = 8 MiB`，`MAX_TOTAL_IMAGE_BYTES = 16 MiB`。960×540 截图通常为数百 KB，留有余量；16 MiB 原始字节 base64 后约 21.4 MiB，远低于 Gemini 的 100 MB inline 上限。

文本仍为必填且非空（≤ 1 MiB），图像总是附加在文本之后。纯文本用法完全源码兼容：`Input::try_from(&str)`、`Input::json`、`IntoInput` 与 `as_str()` 不变；`Input` 为 `#[non_exhaustive]`，内部表示改变不影响调用方。`Error` 与 `Limits` 均为 `#[non_exhaustive]`，新增变体与字段不破坏兼容性，因此本版本为非破坏性 minor（0.7.0），无需迁移文档。

```rust
let frame = Image::try_from(std::fs::read("frame-0042.png")?)?.with_detail(ImageDetail::Low);
let input = Input::try_from("Narration: \"Revenue doubled.\"\nIntended picture: a rising bar chart.")?
    .with_image(frame)?;
let review = client.decide_with::<FrameReview>(RequestOptions::new(&input)?).await?;
tracing::info!(input_tokens = review.meta.usage.input_tokens, digest = %input.digest());
```

## 4. 协议编码

- 无图像时，两个后端的请求体与 0.6.0 字节一致（`input` 为字符串），保持既有 golden。
- OpenAI 有图像时：`input` 为单条 user 消息，`content` 依次为一个 `input_text` 与每张图像的 `input_image`；`image_url` 为 `data:<mime>;base64,<标准 base64>`，`detail` 仅在非 `Auto` 时输出。
- Gemini 有图像时：`input` 为 block 数组，依次为一个 `text` block 与每张图像的 `image` block（`data` 为标准 base64，`mime_type` 为嗅探结果，`resolution` 仅在非 `Auto` 时输出）。
- base64 通过 `serde_json::Serializer::collect_str` 与 `Base64Display` 流式写入请求缓冲，不额外分配整段 base64 字符串。
- 请求体上限：`Limits::request_bytes` 继续约束非图像部分；对带图像的输入，编码上限为 `request_bytes + Σ(base64 长度 + 64)`（checked 运算），图像本身由 `Limits::image_bytes` 约束。
- Jev、System One、Cloudflare Clef（两种协议）以及任何其他协议：客户端在编码前检查 `Backend::accepts_images()`，返回 `ImagesUnsupported { provider }`；`Backend::prepare` 本身也对这些协议拒绝图像输入（`provider: None`），任何调用路径都不会静默丢弃图像。

## 5. 摘要与缓存键

SDK 本身不做缓存、去重或幂等键；case ID 由调用方提供。为让调用方的缓存键涵盖图像，`Input::digest()` 对以下带长度前缀的帧计算 SHA-256：

```
"decision-rs/input/v1" ‖ u64be(len(text)) ‖ text ‖ u64be(n)
  ‖ for each image: u8(media type tag) ‖ u8(detail tag) ‖ sha256(image bytes)
```

相同文本、相同图像字节、相同顺序与细节档位才得到相同摘要；文本相同而图像不同的输入必然不同（排除 SHA-256 碰撞）。`Input` 的 `PartialEq` 同样比较图像摘要与细节档位。新增依赖 `sha2`（RustCrypto）与 `base64`（后者已在依赖图中）。

## 6. Token 用量与模型

每次调用的用量已经由 `ResponseMeta` 暴露（`decide_with` 的 `Decided::meta`、`ask` 的 `DecisionResponse::meta`、`Error::Refused::meta`）：

| 字段 | OpenAI | Gemini |
|---|---|---|
| `usage.input_tokens` | `input_tokens`（含缓存与图像） | `total_input_tokens`（含图像） |
| `usage.output_tokens` | `output_tokens` | `total_output_tokens`，缺省 0 |
| `usage.cached_tokens` | `input_tokens_details.cached_tokens` | `total_cached_tokens` |
| `returned_model` | 响应 `model` | `None`（协议不返回） |
| `wire_model` | 实际发送的模型 ID | 实际发送的模型 ID |

缺口只有批处理：`decide_all` 丢弃 metadata。新增 `decide_all_with` 返回 `Vec<Result<Decided<D>, Error>>`，与 `decide_all` 共享校验与并发实现。成本仍不本地估算。

## 7. 验证

- 单元：嗅探（三种格式、空、未知、截断 WebP、超限）、数量与总字节上限、摘要稳定性与区分性、`Debug` 不泄露内容、`Limits::image_bytes` 配置校验。
- 编码 golden：OpenAI 与 Gemini 带图像请求体（含 `detail` / `resolution`）完整 JSON 比对；无图像请求体保持原 golden。
- 客户端：wiremock 验证 OpenAI、Gemini 带图像请求到达服务端；Jev、System One、Cloudflare 在零请求的情况下返回 `ImagesUnsupported`；配置的 `image_bytes` 生效。
- 线上（`#[ignore]`）：`make test-live-images` 以仓库内 96×54 纯红测试帧分别询问 OpenAI 与 Gemini，断言答案与 usage；默认测试不访问网络。2026-11-17 OpenAI 实测通过（`detail: low`，304 input tokens，0 output tokens，返回模型 `gpt-6-luna`）；Gemini 因本机无 `GEMINI_API_KEY` 待验收。
- 示例：`examples/10_image_evidence.rs` 读取一张截图并输出答案、摘要与用量。
