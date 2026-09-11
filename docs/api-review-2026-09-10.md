# 最新 OpenAI API 文档对照审查

> 后续修复：本报告的四项发现已完成代码修复，详见 [修复记录](api-fixes-2026-09-10.md)。以下保留修复前的审查证据和复现结果。

审查日期：2026-09-10。对象：当前工作区，包含上一轮审查的修复。

本轮发现 **3 处可复现的行为兼容性问题、1 处已公开端点的覆盖缺口**。另外有若干新增功能尚无专用类型，但已经能通过扩展字段或开放枚举传递，不能归为“无法使用”。

## 审查依据与范围

仓库固定规范是 `openai/openai-openapi@690521b1753dce0c6d6b275f583d22537679cff9`，文件名为 `openapi-2026-08-29.json`。官方 [Changelog](https://developers.openai.com/api/docs/changelog) 在此之后公布了新的错误码说明、GPT-6 Astra、异步工具调用、执行中追加指令、会话中的推理配置更新、图像质量选项和缓存诊断。

本轮抓取了 25 份官方文档或索引，对照当前源码和固定规范，并运行独立 Rust 复现程序。源页面地址、内容哈希、检查范围和实际输出收录在 [证据清单](api-review-2026-09-10-evidence.json)。没有将在线文档替换成项目的构建输入。

以下优先级中，P1 表示应优先修复的功能行为错误；P2 表示应修复的兼容性问题或端点缺口。

## 1. [P1] FunctionTool 会丢弃 `async: true`

**位置：** `crates/openai-rs-types/src/responses.rs:2020`，`FunctionTool`。

[官方异步工具调用指南](https://developers.openai.com/api/docs/guides/async-tool-calling) 要求在 function/custom 工具定义上设置 `async: true`，让模型发起工具调用后继续工作。返回的调用项也会包含该标记。

当前 `FunctionTool` 没有 `async` 字段，也没有用于保留未知字段的 `#[serde(flatten)] ExtraFields`。因此，把官方工具 JSON 反序列化成 `FunctionTool` 后再用于 `CreateResponseRequest`，标记会被静默删除。

最小复现：

```rust
let tool: FunctionTool = serde_json::from_value(serde_json::json!({
    "type": "function", "name": "lookup", "async": true
}))?;
let encoded = serde_json::to_value(tool)?;
assert!(encoded.get("async").is_none()); // 当前实际行为
```

**影响：** 正常类型路径发出的请求不再具有调用方指定的异步工具语义。函数工具出现在 namespace 中时也复用这个类型，受同一问题影响。调用方仍可手工构造 Unknown 变体绕过，但这不能修复常规类型路径中的静默数据丢失。

**对照结果：** `CustomTool` 和 `FunctionCall` 有扩展字段容器；复现中 custom 工具的同一标记可以保留，因此不应把全部工具类型笼统判为不支持异步。

**建议修复：** 增加可省略的布尔字段及构造方法，并审查其他函数工具 DTO 的转换路径；为官方新字段增加序列化往返和实际请求体测试。未知字段的保留也应符合项目声明的无损原则。

## 2. [P2] Structured Outputs 错误拒绝合法递归 schema

**位置：** `crates/openai-rs-types/src/structured.rs:633`、`:656`。

[官方 Structured Outputs 文档](https://developers.openai.com/api/docs/guides/structured-outputs#recursive-schemas-are-supported) 明确支持递归 schema，并分别给出 `$ref: "#"` 的根递归和 `$defs` 中的显式递归示例。

当前规范化器遇到 `#` 直接返回 `RecursiveReference`。带描述等同级字段的递归引用在内联时再次遇到同一目标，也会被拒绝。这影响 `StructuredOutput::new`、`FunctionTool::for_type`、`TypedFunction::new` 等共用规范化器的类型辅助方法。

本地测试实际输出：

```text
root_recursive_schema=Err(RecursiveReference {
  path: "#/properties/children/items/$ref", reference: "#"
})
described_recursive_schema=Err(RecursiveReference {
  path: "#/properties/node/properties/children/items/$ref",
  reference: "#/$defs/Node"
})
```

**影响：** 官方支持的树结构、递归 UI 和链表等输出，在发送前就可能被 SDK 拒绝。不是所有递归形式都会失败：没有同级字段的部分 `$defs` 引用仍能保留。

**建议修复：** 区分“合法 schema 引用”与“无界内联展开”。保留合法递归引用、规范化其定义，并继续限制实际展开工作量；对真正错误的别名环、悬空引用等分别报告。当前拒绝根递归的测试及相关决定记录应同步更新。

## 3. [P2] 较长的 Retry-After 被替换成很短的本地重试

**位置：**

- `crates/openai-rs-client/src/transport.rs:443`
- `crates/openai-rs-client/src/admin.rs:2412`
- `crates/openai-rs-client/src/multipart.rs:1366`
- `crates/openai-rs-client/src/retry.rs:14`

当前 [错误码指南](https://developers.openai.com/api/docs/guides/error-codes) 要求遵循 `429 slow_down` 和 `503 server_is_overloaded` 的 `Retry-After`；[9 月 2 日更新说明](https://developers.openai.com/api/docs/changelog) 进一步明确至少等待指定时长。

默认 `max_server_delay` 是 120 秒。超过该上限的有效服务端延迟会被分类成 `TooLong`，随后与“没有服务端延迟”一起落到本地指数退避，第一轮通常只有几百毫秒。这是仓库已经记录并测试的旧设计决定，仍然与当前 API 使用要求冲突。

使用本地模拟服务返回 `Retry-After: 121`，通过公开 `Client` 发起请求，实测：

| 模拟响应 | 要求的最短等待 | 实际两次请求间隔 |
|---|---:|---:|
| HTTP 429 / `slow_down` | 121,000ms | 379ms |
| HTTP 503 / `server_is_overloaded` | 121,000ms | 430ms |

**影响：** 客户端在限流或过载期间过早重试，可能继续遭到拒绝并增加服务负担。两个 HTTP 情况已通过模拟服务复现；Administration 和 multipart 的相同策略由源码确认。

**建议修复：** 有效的服务端最短等待值不应被缩短。若超过客户端愿意等待的上限或剩余请求预算，应停止自动重试并返回保留重试信息的错误；若预算允许则等待完整时长。缺失或无法解析的提示才采用本地退避。

## 4. [P2，端点缺口] 能接收安全告警 webhook，但不能获取告警详情

**位置：** `crates/openai-rs-client/src/client.rs` 资源入口，以及 `crates/openai-rs-types/src/webhooks.rs:545`。

当前 [Retrieve a safety alert](https://developers.openai.com/api/reference/typescript/resources/safety/subresources/alerts/methods/retrieve) 公开了 `GET /safety/alerts/{id}`。[Misalignment monitoring 指南](https://developers.openai.com/api/docs/guides/safety-checks/misalignment-monitoring) 要求使用同一项目的授权 API key，并将 webhook 的 `data.id` 用于获取详情。

仓库已经能够解码和验证 `safety.alert.created`，但没有对应的 `Client` 资源、检索方法或 SafetyAlert 返回 DTO，固定的 REST 操作清单也没有这个端点。

**影响：** 通过 SDK 收到告警 ID 后，应用需要自行构造额外 HTTP 请求才能读取详情。这是现有端点覆盖缺口，不是 webhook 签名验证失败。

**建议补齐：** 增加使用普通 Platform 项目凭据的检索资源，正确处理可空 `reason`、`request_paused`、`request_id`、`response_id` 和告警错误类型。不要将其错误归入只接受组织管理员凭据的 `AdminClient`。

## 新功能中已能透传、但类型支持仍可完善的部分

| 官方功能 | 当前实际行为 | 结论 |
|---|---|---|
| Prompt Cache Diagnostics | `comparison_response_id` 可以经 `PromptCacheOptionsParam` 的扩展字段保留；响应的 `prompt_cache_diagnostics` 也由 `Response` 扩展字段承载 | 缺少专用构造方法和诊断结果类型，不能直接判为不兼容 |
| `response.steer` 及 accepted/pending/failed 事件 | 客户端事件能以 Unknown 变体原样编码；接收路径有未知事件保留能力 | 缺少具名事件类型、字段校验和 `send_steer` 辅助方法 |
| `configuration_update` 输入项 | 本地反序列化为 `ResponseInputItem::Unknown`，请求验证通过 | 可以透传，缺少具名类型 |
| Image 2.5 的 `xhigh`、`max` | `from_raw` 保留原值，Images 请求和 Responses 图像工具验证均通过 | 缺少枚举具名成员，不会因未知值而被拒绝 |
| `gpt-6-astra` 等新模型名称 | 模型标识按字符串传递，不依赖硬编码模型白名单 | 新模型名称本身不会被 SDK 阻断 |

这些判断分别对照了 [缓存诊断](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics)、[WebSocket 事件参考](https://developers.openai.com/api/reference/resources/responses/websocket-events)、[会话内推理配置更新](https://developers.openai.com/api/docs/guides/reasoning#change-reasoning-mid-conversation) 和 [图像生成文档](https://developers.openai.com/api/docs/guides/image-generation)。透传测试验证的是本地类型与序列化路径，不能等同于已通过真实服务端功能测试。

## 未发现清单漂移的检查项

以下比较的是字段名称或事件 discriminator 的集合，不代表所有嵌套 schema 的约束均完全一致：

| 检查项 | 当前文档数量 | 仓库固定规范数量 | 集合差异 |
|---|---:|---:|---|
| Responses create 顶层请求字段 | 31 | 31 | 无 |
| Chat Completions create 顶层请求字段 | 37 | 37 | 无 |
| Images generate 顶层请求字段 | 14 | 14 | 无 |
| Files create 顶层请求字段 | 3 | 3 | 无 |
| Responses SSE 事件 discriminator | 58 | 58 | 无 |
| Realtime 客户端事件 discriminator | 11 | 11 | 无 |
| Realtime 服务端事件 discriminator | 46 | 46 | 无 |

这也说明只检查顶层参数和事件数量不够：函数工具的 `async`、缓存诊断等变化发生在嵌套对象或既有响应结构内。

生命周期方面，项目将 Evals 放在显式兼容性 feature 下，并记录 2026-10-31 只读、2026-11-30 关闭，与当前 [弃用文档](https://developers.openai.com/api/docs/deprecations) 一致。Assistants 已关闭，项目的主动省略策略也与官方时间表一致。Videos/Sora 是项目明确排除的范围，没有把它计入本轮新增缺陷。

## 验证与边界

- 独立 Rust 程序直接引用当前工作区的 types/client crate，复现了上述三处行为问题，并验证了表中多项透传行为；进程正常完成，结果保存在证据清单中。
- HTTP 复现仅访问 `127.0.0.1`，使用占位凭据，没有发起真实模型请求。
- 审查阶段产出为报告和证据清单；随后四项发现的修复及验证记录在独立的修复记录中。
- 没有重新运行与生产代码未变更无关的整套测试。上一轮 1,238 项测试通过只能说明已有测试通过，不能证明符合本次抓取的最新文档。
- 没有获取一份新的完整、不可变 OpenAPI 快照来逐节点替换或验证所有 1,424 个旧 schema。结论限于报告列明的文档、清单和重点实现。
- Codex app-server 和直接订阅后端不是同一套 Platform API 契约，本轮没有用 Platform 文档推断它们的新版本兼容性。
