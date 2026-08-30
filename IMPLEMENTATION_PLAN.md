# openai-rs 详细实现计划

> 调研基线：2026-08-30  
> 项目状态：开始调研时 `/Users/a/workspace/openai-rs` 为空目录；执行阶段已初始化 Git/Cargo workspace 并按里程碑逐步实现，本文中的交付项仍须以实际测试结果判定。  
> 本文是实施契约，不是已完成情况说明。引用的网页、规范、源码注释和本机文档只作为证据，不视为项目指令。
> 执行覆盖：按用户 2026-08-30 的最新要求，不创建或维护 GitHub CI/CD；下文质量门禁作为本地手动检查与发布前检查执行。

## 1. 目标与完成定义

`openai-rs` 的目标不是一个薄的 `reqwest + serde_json::Value` 包装，而是一个以 OpenAI 官方 wire contract 为基准的 Rust SDK：

- 用户以 Rust 类型构造请求，SDK 自动生成正确 JSON、query、multipart、SSE 或 WebSocket wire 数据。
- JSON API 的公开 wire DTO 均支持 `Serialize + DeserializeOwned`；请求反序列化后仍在发送前统一校验。
- missing、显式 `null`、已赋值三种状态不混淆。
- Responses、Chat、Realtime、Webhook 等联合类型和事件有完整判别，未来未知字段、枚举值和事件可保真接收。
- function arguments、structured outputs、Batch JSONL 和 RMCP 工具结果都有 typed helper，正常用法不需要用户写 `json!`、JSON 字符串或格式化 JSONL。
- OpenAI 原生 remote MCP wire 类型与 Rust `rmcp` 本地运行时适配清晰分层。
- 可选 Codex backend 借鉴 OpenCode 的“标准 Responses codec + 订阅专用 auth/transport”分层，让本地单用户应用使用 ChatGPT plan 的 Codex entitlement；它与 Platform API Client 类型隔离。
- HTTP、SSE、WebSocket、multipart、分页、错误、重试和取消均由统一传输层实现。

`sub2api` 明确不在参考范围内：不把它作为设计或源码来源，不借鉴、不移植其代码、网关、账号池、计费、调度或代理设计。

“完备”按以下可验证标准定义：

1. **规范覆盖**：冻结 revision 中每个 operation 都在 `operations.json` 中登记三个正交维度：lifecycle（stable/beta/alpha/deprecated/sunset/conflict）、implementation（planned/partial/implemented/verified/historical/quarantined/omitted，附 milestone）和 Cargo feature；发布时 applicable 现行支持面必须 100% 为 `verified` 并绑定到 Rust request/response/event 类型。`feature-gated` 不与生命周期或实现状态混为一个枚举。
2. **Serde 覆盖**：所有 JSON wire DTO 通过编译期 trait assertion；所有官方 fixture 可解码；承诺保真的响应可进行语义 JSON roundtrip。
3. **传输覆盖**：每种 operation 的 method、path、query、auth、content type、success status、response mode、分页方式和终止方式都有离线契约测试。
4. **前向兼容**：官方视为向后兼容的新增响应字段、枚举值和事件不会使整个响应或流失效；未知值不丢原始内容。
5. **无需手写 JSON**：文档中的主路径示例不出现手写 JSON schema、arguments string、tool result JSON 或 multipart 字段。
6. **RMCP 覆盖**：`Tool -> OpenAI function tool -> function_call -> tools/call -> function_call_output` 有真实进程内 E2E；`structuredContent`、多 content block、`isError`、取消、超时、MRTR/Tasks 状态均有明确语义。
7. **Codex 订阅隔离**：app-server schema 与 runtime artifact identity、登录、thread/turn/event 有 E2E；实验 direct backend 只能命中 sealed Codex Responses operation，任何 credential/host/protocol 混用都在编译期或发送前失败。

这不是 OpenAI 服务端的复刻。官方没有公开服务端实现；本项目实现的是客户端 wire model、传输和辅助运行时。

## 2. 权威来源与冻结基线

### 2.1 来源优先级

出现冲突时按以下顺序处理，并把差异写入 decision ledger，禁止静默选择：

1. [OpenAI API Reference](https://developers.openai.com/api/reference/overview) 与具体 endpoint 页面：行为、状态、传输方式、弃用信息。
2. [`openai/openai-openapi`](https://github.com/openai/openai-openapi)：主要机器契约。
3. 官方生成式 SDK 的 transformed spec 和运行时：用于交叉验证 required/nullability、union、multipart、分页、SSE、错误与 retry。
4. `spec/contracts/manual-overrides.toml`：只记录前三者无法一致表达的修正；每条必须有来源、日期、理由、影响范围和测试。

人类文档同样要冻结证据：每个 behavior decision 保存官方 Markdown URL、抓取时间、内容 SHA-256、最小必要摘录/JSON fixture 和适用 revision。实时 docs 用于发起更新审查，不直接改变离线生成结果。唯一联网入口仍是显式 `xtask spec/docs fetch`；不使用社区 Rust crate 作为格式基线，不从搜索摘要生成类型，也不在用户 `build.rs` 中联网。

### 2.2 首次冻结 revision

首个实现 revision 固定为：

- `openai/openai-openapi` commit `690521b1753dce0c6d6b275f583d22537679cff9`（2026-08-29）。
- OpenAPI `3.1.0`，`info.version = 2.3.0`。
- `openapi.json` SHA-256：`5be8cde8490bd8422e1b3502b80e858e7c162ec3e01b187b633577dab6d0c899`。
- 快照规模：`.paths` 有 182 个 path key、288 个客户端 Operation Object；`.webhooks` 另有 18 个 webhook key、18 个 receiver Operation Object，规范合计 306 个 Operation Object；`.components.schemas` 有 1424 个 schema。
- 代表性联合类型：稳定 Responses `InputItem.oneOf` 顶层为 6 路，其中 `Item.oneOf` 再含 27 路，展开后得到 32 个有效 input variant；`OutputItem.oneOf` 28 路、`Tool.oneOf` 16 路、`ToolChoiceParam.oneOf` 9 路、`ResponseStreamEvent.anyOf` 58 路、`RealtimeServerEvent.anyOf` 46 路。
- 规范复杂度还包括大量 `anyOf`、`oneOf`、`allOf`、discriminator 和遗留 `nullable`，不能直接采用通用 OpenAPI Rust generator 的默认输出。

官方源码交叉基线：

- `openai-node` commit `eea2292a4a523da9405161dde0a79ac5dc2ecb2a`。
- `openai-python` commit `b19c2161b1eac80fbf1f6f67a64a50af99c53356`。
- 两者提交的 `api_reference/openapi.transformed.yml` 在该基线具有相同 SHA-256：`1a9e90cd0c3b98cec8fec7d12b7aaeaa5e4d5110a0bd3f6456a6958a08127430`。
- `openai-go` commit `4d062949c62507e56514af8c7beb186dc09ac075`，用于交叉检查 tri-state 参数、raw JSON、额外字段和分页族。
- 上述 pinned SDK commits 的生成器名是 Castiron；源码仍含 `x-stainless-*` 扩展和 `X-Stainless-*` header，不能据此把生成器误称为 Stainless。

不要使用 `openai-openapi` 的 GitHub latest release：其 release tag 长期滞后且没有 Responses API。规范必须按审核过的 commit SHA 冻结。

### 2.3 本机参考项目

`zai-rs` 审计 revision 为 `d426957d51b1149f8271762360e4641184e62fb1`。重点借鉴：

- [规范 provenance](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/spec/upstream/SOURCES.toml#L1) 与 [operation contract](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/spec/contracts/operations.json#L1)。
- [统一 JSON 发送边界](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/client/operation.rs#L128)：`B: Serialize`、`R: DeserializeOwned`。
- [stream typestate 与 trait](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/model/traits.rs#L241)。
- [tagged message union](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/model/chat_message_types.rs#L41) 与 [tagged content union](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/model/chat_message_types.rs#L224)。
- [SSE 任意分片解析](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/model/sse_parser.rs#L15) 与 [重复 JSON key 防护](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/serde_helpers.rs#L14)。
- [可重放 multipart factory](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/client/transport/multipart.rs#L144)。
- [RMCP tool 转换](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/toolkits/rmcp_kits.rs#L57) 与 [result 转换](https://github.com/AnlangA/zai-rs/blob/d426957d51b1149f8271762360e4641184e62fb1/src/toolkits/rmcp_kits.rs#L76)。

明确不照搬：

- sealed 模型 ZST 和 `(Model, Message): Bounded` 矩阵；OpenAI alias、snapshot、fine-tuned/custom model 变化太快。
- 所有字段均为 `Option` 的成功响应。
- `#[serde(other)] Unknown` 或把未知字符串映射为 `None` 的信息丢失策略。
- discriminator 加多个可选 payload 字段来模拟 union。
- 把完整响应 pretty JSON 当成 `Debug`。
- 让用户手写 function JSON Schema、arguments string 和 tool result JSON。

### 2.4 OpenCode/Codex 订阅认证参考

本机 OpenCode 审计固定为 `anomalyco/opencode` `dev@d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83`（MIT，工作树干净）：

- [Codex auth plugin](https://github.com/anomalyco/opencode/blob/d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83/packages/opencode/src/plugin/codex.ts#L12)：browser PKCE、device flow、refresh、account id、订阅 transport。
- [通用 ProviderAuth 状态机](https://github.com/anomalyco/opencode/blob/d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83/packages/opencode/src/provider/auth.ts#L102) 与 [tagged credential store](https://github.com/anomalyco/opencode/blob/d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83/packages/opencode/src/auth/index.ts#L14)。
- [标准 Responses provider](https://github.com/anomalyco/opencode/blob/d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83/packages/opencode/src/provider/provider.ts#L175)：请求 JSON 仍由标准 SDK 生成，订阅差异留在 transport。

Rust 安全实现同时以官方 `openai/codex` `63d213884daea50e4f74efc192cdc44f549b67d5`（Apache-2.0）为优先实现 oracle：

- [官方 Rust browser login server](https://github.com/openai/codex/blob/63d213884daea50e4f74efc192cdc44f549b67d5/codex-rs/login/src/server.rs#L159)。
- [官方 Rust device-code flow](https://github.com/openai/codex/blob/63d213884daea50e4f74efc192cdc44f549b67d5/codex-rs/login/src/device_code_auth.rs#L62)。
- [refresh singleflight/guarded reload](https://github.com/openai/codex/blob/63d213884daea50e4f74efc192cdc44f549b67d5/codex-rs/login/src/auth/manager.rs#L2764) 与 [keyring/file/ephemeral storage](https://github.com/openai/codex/blob/63d213884daea50e4f74efc192cdc44f549b67d5/codex-rs/login/src/auth/storage.rs#L408)。

来源优先级是：官方 ChatGPT/Codex 文档 > 官方 Codex Rust 实现 > 本机 OpenCode。OpenCode 提供第三方客户端分层与 UX 参考；OAuth/后端常量不能因其开源就被当成稳定公共 API。

## 3. Workspace 与发布结构

采用“单一用户入口、分层 workspace”结构：

```text
openai-rs/
├── Cargo.toml                         # workspace
├── crates/
│   ├── openai-rs/                    # 门面 crate，用户只依赖它
│   ├── openai-rs-types/              # 全部 JSON wire DTO 与 serde kernel
│   ├── openai-rs-client/             # HTTP/SSE/WS/multipart/pagination
│   ├── openai-rs-codex/              # 可选 Codex app-server/订阅 backend
│   ├── openai-rs-rmcp/               # 可选 rmcp bridge
│   └── openai-rs-contract-tests/     # publish=false；跨 crate 契约测试
├── xtask/                             # 不发布；spec、codegen、drift、fixtures
├── spec/
│   ├── upstream/
│   │   ├── openapi-2026-08-29.json
│   │   ├── openapi-transformed-node-<sha>.yaml
│   │   ├── openapi-transformed-python-<sha>.yaml
│   │   ├── codex/app-server-<version>-schema.json
│   │   └── docs/<topic>-<date>.md       # 仅冻结实际采用的行为证据
│   ├── SOURCES.toml
│   └── contracts/
│       ├── operations.json
│       ├── discriminators.json
│       ├── nullability.json
│       ├── lifecycle.toml
│       ├── codex-compatibility.toml
│       ├── manual-overrides.toml
│       └── decisions.md
├── testdata/fixtures/                 # 共享只读 fixture
├── fuzz/                              # 独立 workspace，从根 workspace exclude
└── docs/
```

依赖方向必须单向：

```text
openai-rs-types <- openai-rs-client <- openai-rs-rmcp
        ^--- openai-rs-codex
openai-rs facade -> types + client + optional codex/rmcp
```

发布原则：

- 五个公开 package（facade/types/client/codex/rmcp）同版本发布，内部依赖使用精确同版本要求；contract-tests 不发布。
- `openai-rs` 只做 re-export、feature 汇合和一页式用户入口。
- `openai-rs-types` 不依赖 Tokio、reqwest 或 rmcp，可用于代理、fixture、持久化和服务端 webhook。
- `openai-rs-codex` 只暴露 Codex app-server/订阅能力，不能把订阅 credential 注入标准 Platform Client。
- `openai-rs-rmcp` 隔离 rmcp 的 MSRV、SemVer 和 transport feature。
- `openai-rs-contract-tests` 依赖全部内部 crate并承载 HTTP/Serde/UI/E2E；virtual workspace 根目录不放 Cargo 无法发现的 `tests/*.rs`。
- 若 crates.io 名称不可用，在 M0 先确定发布名；Rust import 路径仍保持 `openai_rs`。

初始 MSRV 设为 Rust 1.88、edition 2024，因为当前 `rmcp 3.1.4` 的 MSRV 是 1.88。各子 crate 若能声明更低 MSRV，可在实际 dependency audit 后单独降低，但 facade 的 all-features 门槛仍为 1.88。

初始 Facade feature graph：

| Feature | 默认 | 作用/依赖 |
|---|---:|---|
| `client` | 是 | HTTP/SSE/multipart/pagination resource facade |
| `rustls-tls` | 是 | reqwest rustls；两种 TLS 可同时编译，runtime 默认选 rustls |
| `native-tls` | 否 | reqwest native TLS；与 rustls 同时启用时由显式 `TlsBackend` 选择 |
| `structured-output` | 是 | schemars + strict-schema normalizer，保证主路径无需手写 schema |
| `realtime` | 否 | Responses/Realtime WebSocket transport |
| `webhook-verification` | 否 | webhook HMAC/signature verify 与 unwrap |
| `admin` | 否 | AdminClient 与 Administration resources |
| `workload-identity` | 否 | token exchange；`x509` 子 feature 加 mTLS |
| `codex-app-server` | 否 | 官方 JSON-RPC/stdio client；ChatGPT managed browser/device login |
| `codex-access-token` | 否 | 依赖 `codex-app-server`；可信本地自动化的 Codex workspace access token |
| `experimental-codex-direct` | 否 | 纯 Rust、OpenCode 风格的订阅 Responses transport；依赖 `rustls-tls`，非公共 `/v1` contract |
| `experimental-codex-direct-device` | 否 | 依赖 `experimental-codex-direct`；device-code flow 的独立 beta gate |
| `rmcp` | 否 | `openai-rs-rmcp/client` 基础 bridge |
| `rmcp-stdio` / `rmcp-http-rustls` / `rmcp-http-native-tls` / `rmcp-server*` / `rmcp-auth*` | 否 | 精确转发第 10.2 节 adapter features |
| `beta-chatkit` / `beta-responses-multi-agent` / `alpha-graders` | 否 | 不稳定面，独立 namespace |
| `legacy-completions` / `legacy-realtime` / `sunset-videos` | 否 | 默认文档不推荐；有明确 lifecycle warning |

`full` 只聚合现行稳定 Platform API 能力、rustls 和经审计的 RMCP rustls client，不自动启用 Codex subscription、experimental、alpha/beta/legacy/sunset。普通 REST resource 不逐个拆 feature，避免 feature powerset 爆炸。`--all-features` 可合法双开 TLS；测试分别覆盖 rustls-only、native-only、both-with-explicit-selection。

## 4. 规范同步与代码生成

### 4.1 生成策略

采用“生成 wire contract，手写 ergonomics 和 transport”的混合模式：

- 生成：请求/响应 DTO、字符串枚举、tagged union 分支、route metadata、operation inventory、字段文档、compile-time trait assertions。
- 手写：HTTP client、auth、错误、retry、SSE、WebSocket、multipart factory、分页器、structured-output schema 转换、RMCP bridge、便利方法。
- 生成源码提交到 Git；普通构建完全离线且不需要 spec 文件。
- 每个 generated file 标注 source revision、JSON Pointer 和“不可手改”。

### 4.2 OpenAPI lowering

`xtask codegen` 需要自己的中间表示，按顺序完成：

1. 载入 OpenAPI 3.1，验证 SHA、title、version、path/schema 计数。
2. 解析本地 `$ref`，检测循环并对递归边使用 `Box`/`Arc`；禁止联网解析外部 `$ref`。
3. 将 `allOf` 保留为 schema intersection 语义；只有证明属性与约束可兼容时才扁平化。对 `$ref` sibling、required 冲突、`minimum/maximum`、length、pattern、items、`propertyNames`、`not` 等约束做显式合并/冲突检测，不能把 `allOf` 简化成覆盖式 map merge。
4. 将 `const`/单值 enum 转成 literal discriminator。
5. 把 `oneOf`、`anyOf` 分类为 primitive union、shape union、tagged union、ambiguous union。
6. 对 discriminator 值不唯一的 union 建候选集合。例如 `InputMessage` 与 `OutputMessage` 都可能是 `type = "message"`，必须再按 role/required fields 验证。
7. 生成 request/response 两套类型；即使字段名相同，也不复用 requiredness 不同的 DTO。
8. 读取 path/query/header/cookie parameter 的 `style`、`explode`、`allowReserved`、array/object 规则（含 deepObject），为每个 operation 生成 `RequestParts` encoder；不能依赖通用 `Display` 或手写 query 拼接。
9. 读取 OpenAPI `encoding`，生成 multipart JSON/SDP part、重复数组字段和 filename 规则；根据 `Accept` 和实际 `Content-Type` 选择动态 response codec。
10. 提取 pagination、auth、固定 query/header（含 beta route）、success MIME/status、stream termination、retry/body replayability 到 operation contract。
11. 应用显式 override，并拒绝 orphan、失效或无测试的 override。

### 4.3 同步命令

计划提供：

```text
cargo xtask spec fetch --rev <40-char-sha>
cargo xtask spec verify
cargo xtask codegen
cargo xtask codegen --check
cargo xtask fixtures extract
cargo xtask drift --from <sha> --to <sha>
cargo xtask codex schema --codex-bin <audited-path>
cargo xtask codex schema --check
```

`.cargo/config.toml` 明确定义 `xtask = "run --package xtask --"` alias；没有该 alias 时，文档和本地门禁使用完整的 `cargo run -p xtask -- ...`。只有 `spec/docs fetch` 可联网；它必须要求显式 SHA 或显式官方 URL，记录 commit/获取日期/byte length/SHA-256/license/计数。`codegen --check` 在临时目录生成并要求零 diff。

### 4.4 漂移分类

- 红色阻断：删除 path/schema、required 增加、类型收窄、枚举删除、security/MIME/path 改变。
- 黄色审核：新增 operation、optional field、enum string、discriminator branch、stream event。
- 绿色文档：description/example 变化且无 wire 影响。

新增 enum/event 虽被 OpenAI 视为兼容变更，但对封闭 Rust enum 是破坏，因此必须有 unknown fallback 测试并更新 manifest。每周任务只生成 diff/PR，不自动覆盖审核过的快照。

## 5. Serde 核心设计

### 5.1 请求与响应分离

- 所有 JSON request 和 response 类型都实现 `Serialize + DeserializeOwned + Send + Sync`。
- 请求字段私有，必填字段进入 `new`/builder；`send` 前执行结构和跨字段校验。
- 响应按官方 required 字段建模，不用“全 Option + 事后判断空对象”。
- 响应 struct 同样保持字段私有并提供 accessor/iterator/convenience helper；公开 enum/未来可扩展 struct 使用 `#[non_exhaustive]`。下游不能靠 struct literal 或穷尽 pattern 锁死当前字段集，从而允许新增已知响应字段而不制造无谓 SemVer break。
- multipart 的文件句柄本身不是 JSON DTO；其 JSON/form 元数据仍双向 Serde，发送层实现 `ToMultipart`。
- binary、audio、zip、SDP 等 raw body 使用专用类型，不伪装为 JSON。

### 5.2 精确存在性

单层 `Option<T>` 无法区分 missing 和 `null`。基础类型定义为：

```rust
pub enum Omittable<T> {
    Omitted,
    Value(T),
}

pub enum Nullable<T> {
    Null,
    Value(T),
}
```

组合规则：

| OpenAPI 字段 | Rust wire 类型 |
|---|---|
| required, non-null | `T` |
| required, nullable | `Nullable<T>` |
| optional, non-null | `Omittable<T>` |
| optional, nullable | `Omittable<Nullable<T>>` |

`Omittable<T>: Default` 固定为 `Omitted`；生成字段使用 `#[serde(default, skip_serializing_if = "Omittable::is_omitted")]`。字段存在时由自定义 deserializer 解 `T`，因此 `Omittable<T>` 遇到 null 会失败，而 `Omittable<Nullable<T>>` 可显式接收 null。`Nullable<T>` 本身不实现 Default，required-nullable 字段不加 `default`，所以缺失必然报错。builder 默认 `Omitted`；`.field(value)` 发送值，`.field_null()` 只在 nullable 字段存在，`.clear_field()` 回到 omitted。token-level `serde_test`、fixture 和 property test 同时锁定这些规则。

### 5.3 开放字符串枚举

响应枚举使用 `Unknown(Box<str>)` 并自定义 Serde：

```rust
#[non_exhaustive]
pub enum ResponseStatus {
    Completed,
    Failed,
    InProgress,
    Cancelled,
    Queued,
    Incomplete,
    Unknown(Box<str>),
}
```

未知字符串必须按原值重新序列化。请求 builder 默认只接受已知值；需要前瞻兼容时提供显式 `from_raw`，而不是给所有枚举静默放宽。

### 5.4 Tagged union 与未知事件

正常对象 union 使用真正的 Rust enum；只在 JSON primitive kind/形状严格互斥时使用 `#[serde(untagged)]`。

```rust
#[non_exhaustive]
pub enum ResponseOutputItem {
    Message(OutputMessage),
    FunctionCall(FunctionCall),
    McpCall(McpCall),
    // generated variants ...
    Unknown(UnknownTaggedObject),
}
```

解码步骤：

1. 递归拒绝重复 JSON key，尤其是重复 discriminator。
2. 要求 discriminator 存在且为字符串。
3. 已知 tag 只尝试该 tag 的候选集合；多个候选时按 required fields/role 做严格判别。
4. 已知 tag 但 payload 畸形时返回 decode error，不能降级成 Unknown 掩盖协议错误。
5. 未来 tag 保存 discriminator 和 `Box<RawValue>`/semantic object，允许原样或语义 roundtrip。

SSE、Responses WebSocket、Realtime 与 Webhook 未知事件都向用户产出 `Unknown`，不会终止健康连接。

### 5.5 额外响应字段

官方把“响应对象新增属性”定义为向后兼容。每个 response object 生成私有、只读的 `ExtraFields`，正常 Debug 不显示内容：

- Deserialize 保存未知字段。
- Serialize 合并已知字段与 extra，实现语义 roundtrip。
- extra 不公开可变引用；序列化拒绝与已知字段同名的保留键，避免重复 key。
- 请求不默认开放 extra；提供明确命名的 `extra_body_raw` escape hatch，并在发送时拒绝覆盖已知字段。

### 5.6 ID、时间、任意 JSON 与 secret

- `ResponseId`、`FileId`、`BatchId` 等是 `#[serde(transparent)]` 字符串 newtype，但不强制 prefix；官方明确 opaque ID 长度/格式可变。
- `ModelId(Cow<'static, str>)` 是开放 newtype，提供已知常量和 `new("ft:...")`；不建立 sealed 模型能力全集。
- wire 时间戳保留规范整数；`chrono` helper 可选，不改变序列化格式。
- `serde_json::Value/Map` 只用于规范明确开放的 JSON Schema、metadata、structured content、unknown/raw 和 escape hatch。
- API key、Admin key 和 workload credential 不能实现通用 `Serialize`，只能由 auth header encoder 通过受限 secret exposure 读取。只有协议明确放在 JSON body 内的 MCP authorization/header 使用可受控序列化的 wire secret wrapper；所有 secret 的 `Debug`/`Display` 永远脱敏。

### 5.7 JSON 字符串字段

Function call arguments 在 wire 上必须仍是 JSON string，流式 delta 甚至可能是半段 JSON。使用：

```rust
pub struct JsonText(Box<str>);

impl JsonText {
    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self>;
    pub fn deserialize_as<T: DeserializeOwned>(&self) -> Result<T>;
}
```

响应反序列化只保存 string，不提前要求其内部合法；执行工具时再解析并要求 object。用户生成 output 时走 `from_serializable`，无需手动 `.to_string()`。

## 6. Trait、builder 与公开 API

### 6.1 内部 operation trait

将 `zai-rs` 的泛型 send 边界改进为关联 request/response：

```rust
pub(crate) trait ApiOperation: sealed::Sealed {
    type Request: ValidateRequest + Send + Sync;
    type Response: Send + 'static;
    type RequestCodec: EncodeRequest<Self::Request>;
    type ResponseCodec: DecodeResponse<Self::Response>;
    const META: OperationMeta;
}

pub(crate) trait JsonOperation: ApiOperation
where
    Self::Request: Serialize + DeserializeOwned,
    Self::Response: Serialize + DeserializeOwned + Send + Sync + 'static,
{}

pub(crate) trait StreamingOperation: ApiOperation {
    type Event: Serialize + DeserializeOwned + Send + Sync + 'static;
    const TERMINATION: StreamTermination;
}
```

基础 trait 通过 codec 覆盖 JSON、multipart one-shot/replayable、raw bytes/text、empty-or-JSON 和动态 MIME；`JsonOperation` 再锁定 JSON request/response 的双向 Serde 契约。生成的 `EncodeRequest` 把 path/query/header/body 写入 `RequestParts`，完整执行 style/explode/deepObject/array 规则；`DecodeResponse` 根据 status、`Content-Type` 和 operation contract 选择 JSON/SSE/raw/void decoder。

`OperationMeta` 至少记录 method、route segments、固定 query/header、auth scope、parameter encoding、request encoding、response MIME/mode、success statuses、pagination、retry compatibility、lifecycle、implementation 和 feature。

路径参数按单 segment percent-encode，不允许 `format!` 拼接用户输入；transport 不暴露为业务 API。

### 6.2 Stream typestate

只把稳定的协议模式编码进类型，不编码易变模型目录：

```rust
pub struct NonStreaming;
pub struct Streaming;
pub struct CreateResponseRequest<M = NonStreaming> { /* ... */ }
```

- `.into_streaming()` 返回 `CreateResponseRequest<Streaming>` 并设置正确 wire flag。
- 非流式类型只能调用 `create`，流式类型只能调用 `create_stream`。
- 反序列化后的 `stream` 字段与 typestate 矛盾时构造失败。
- Chat、Audio 和 Images 的“同路径、不同返回模式”采用同一方案。

### 6.3 Resource facade

公开调用形态：

```rust
client.responses().create(request).await?;
client.responses().create_stream(request.into_streaming()).await?;
client.files().upload(upload).await?;
client.vector_stores().list(params).await?.items();
```

返回 `ApiResponse<T>`，包含 typed body 和 `ResponseMeta`，实现 `Deref<Target = T>`、`body()`、`request_id()`、`into_inner()`。流对象在握手后提供相同 metadata。

### 6.4 Typed tools 与 structured outputs

```rust
pub trait ToolSpec {
    type Arguments: DeserializeOwned + schemars::JsonSchema + Send + Sync + 'static;
    type Output: Serialize + schemars::JsonSchema + Send + Sync + 'static;
    fn name() -> &'static str;
    fn description() -> &'static str;
}

pub trait ToolHandler: ToolSpec {
    fn call(
        &self,
        arguments: Self::Arguments,
        context: ToolContext,
    ) -> impl Future<Output = Result<Self::Output, ToolExecutionError>> + Send;
}
```

提供：

- `FunctionTool::for_type::<T>(name, description)`。
- `FunctionCall::arguments_as::<T>()`。
- `FunctionCallOutput::json(call_id, &value)`。
- `ResponseFormat::json_schema::<T>(name)`。
- `Response::output_parsed::<T>()` 与 refusal/incomplete 分流。
- `ToolRegistry` 通过内部 object-safe erased adapter 存放 `ToolHandler`，自动 dispatch、解析参数、传播 cancellation/deadline、限制并发、调用 handler、序列化结果。业务级 `ToolExecutionError` 按 policy 生成 in-band tool output；取消/transport/protocol 错误保持 SDK error。重复 tool name 注册直接失败。

`schemars 1.x` 产生的 JSON Schema 2020-12 必须经过 OpenAI strict subset 正规化与验证：根为 object、递归设置 `additionalProperties: false`、把 optional 转为 required+nullable、保留 `$defs`、拒绝无法等价转换的 keyword/external ref，并返回带 JSON Pointer 的错误，不能静默删语义。限制不能散落为 magic numbers；由带来源日期的 contract 表驱动。当前至少包括 5000 个 object property、10 层嵌套、schema 中 property/definition/enum/const 字符串合计 120,000 字符、全部 enum 合计 1000 个值，以及单个字符串 enum 超过 250 个值时总长度最多 15,000 字符。解析模型结果前必须先分流 refusal 和 incomplete；它们不保证符合目标 schema。

Responses 与 Chat Completions 的 function tool wire shape 不同：Responses 是扁平 `{type,name,parameters,...}`，Chat 是 `{type,function:{...}}`。二者可以共享 typed schema 输入，但不能复用同一个 wire DTO。

### 6.5 正常路径示例

```rust
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct WeatherArgs {
    city: String,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct WeatherResult {
    city: String,
    temperature_c: i32,
}

let tool = FunctionTool::for_type::<WeatherArgs>(
    "get_weather",
    "Return current weather",
)?;

let request = CreateResponseRequest::new(
    ModelId::new("gpt-5.6"),
    ResponseInput::text("深圳天气如何？"),
)
.with_tool(tool);

let response = client.responses().create(request).await?;
let call = response.function_calls().next().ok_or(Error::NoToolCall)?;
let args: WeatherArgs = call.arguments_as()?;
let output = FunctionCallOutput::json(
    call.call_id(),
    &WeatherResult {
        city: args.city,
        temperature_c: 28,
    },
)?;
```

这个主路径没有 `json!`、raw string JSON、手写 schema 或手工格式化 output。

## 7. HTTP、错误、重试与资源控制

### 7.1 Client

`Client` 是廉价 `Clone` 的 `Arc<Inner>`，持有：

- `https://api.openai.com/v1/` base URL；自定义 base 只用于显式配置和测试。
- secret credential provider、organization/project headers。
- reqwest connection pool、TLS 后端、代理策略。
- connect、attempt、overall、SSE handshake、SSE idle timeout。
- JSON/error/SSE/binary body size上限。
- concurrency semaphore、retry policy、user-agent 与安全 tracing。

默认禁止跨 origin 携带 credential 的 redirect；最安全方案是默认不跟随 redirect。非 HTTPS base 只允许显式 opt-in 的 literal loopback 测试地址。

### 7.2 Auth 类型

- 公开 `Client`：API key 或 workload identity bearer token，访问普通 API；内部可使用不公开的 generic `ClientCore<Credential>` 实现复用。
- `AdminClient<AdminCredential>`：Admin API key，仅访问 Administration operation。
- `CodexAppServerClient` 与实验性的 `DirectCodexResponsesClient` 位于独立 crate；ChatGPT OAuth/Codex access token 类型不实现 Platform `Credential`，标准 `Client` 无法接收。
- 类型系统阻止普通 client 调 admin method；workload identity token 不进入 Admin client。
- workload identity 的 token exchange 与 X.509/mTLS 作为后期独立 feature。

### 7.3 Error

```text
Error::Api
Error::Transport
Error::Timeout
Error::Encode
Error::Decode
Error::Stream
Error::Cancelled
Error::Validation
Error::Schema
```

`ApiError` 保存 HTTP status、typed `message/type/param/code`、`x-request-id`、安全的 bounded raw body 与 rate-limit metadata。状态便利分类覆盖 400、401、403、404、409、422、429、5xx；未知 error body 仍保留 HTTP 语义。

日志只输出 operation id、脱敏 route template、status、request id、attempt、时长和 byte/count 摘要；不输出 header 值、body、prompt、模型输出、tool arguments、cursor、signed URL 或 MCP content。

### 7.4 Retry

默认提供 `RetryPolicy::OpenAiCompatible`，归一化 pinned Node/Python/Go SDK 的共同语义，并把三者不同之处定义成显式 crate policy：

- 默认最多 2 次 retry。
- 严格值 `x-should-retry: true|false` 覆盖常规 HTTP status 判定，但不能覆盖用户取消、overall deadline、不可重放 body 或已经交付的响应。
- overall deadline 尚未耗尽且 body 可重放时，连接错误、受支持的 transport timeout、408、409、429、5xx 可 retry；用户取消和 deadline 到期不可 retry。
- 先读 `retry-after-ms`，再读 `Retry-After` 秒数或 HTTP date。
- 本地 backoff 为 `min(0.5s × 2^n, 8s) × [0.75, 1.0]`。本 crate 默认只接受有限且 `<= 60s` 的服务端 delay；超限则停止 retry，而不是无界等待。上限和超限行为可配置并有 paused-time tests。

重试还必须满足 body 可重放；one-shot streaming body/reader 永不重试，由已验证 replayable factory 每次重建/重开的流式 body可以重试；一旦向用户交付 response/event 就永不重试。另提供 `RetryPolicy::Conservative`，仅对安全/显式幂等 operation 重试，供资源创建场景采用。

内部 logical request id 只用于本地关联，不自动写入 wire。用户显式提供 `X-Client-Request-Id` 时验证 ASCII 且不超过 512 字符，并按一个逻辑请求透传；SDK 不自行发明 Idempotency-Key header。

## 8. SSE、WebSocket、multipart、分页与 raw body

### 8.1 SSE

通用 parser 输出：

```rust
pub struct SseFrame {
    pub event: Option<Box<str>>,
    pub data: Box<str>,
    pub id: Option<Box<str>>,
    pub retry: Option<Duration>,
}
```

必须支持任意 byte fragmentation、UTF-8 跨 chunk、BOM、LF/CRLF/lone CR、comment/keepalive、空 data、多行 data 按 `\n` 拼接、同 chunk 多事件、有界内存和提前 drop。

终止策略按 operation 记录，不能全局假设 `[DONE]`：

- Chat/legacy sentinel。
- Responses lifecycle terminal 是 `response.completed`、`response.failed`、`response.incomplete`；独立 `error` event 与 `Response.error`/`response.failed` 不混用。当前 decoder policy 由官方 SDK fixture 固化为“产出 stream error 后关闭”，而不是把所有未来 error-like event 先验视为 terminal。官方稳定 Responses SSE 不定义 `[DONE]`，兼容代理的 sentinel 只能由传输层消费。
- 某些原始音频/图片流的专用 terminal。
- 合法 EOF 或 unexpected EOF。

提供两层 API：

- `EventStream<ResponseStreamEvent>`：逐 typed event。
- `ResponseEventStream`/`ResponsesAccumulator`：snapshot、output text、tool argument 聚合、`final_response()`、取消。

`output_text` 是 SDK convenience，不是 wire 字段；实现必须遍历全部 output message/content，不能假定 `output[0]`。

### 8.2 Responses WebSocket 与 Realtime

- Responses WebSocket 和 Realtime 是两套 event family，不复用错误 enum。
- 每套都有 client event、server event、unknown raw、sequence/state 校验和 close semantics。
- 稳定 `responses.connect()` 是独立 transport operation：实现持久 WebSocket 建连/auth、可分离 sink/stream、发送 `response.create`、接收 typed stream events、背压、显式 close 和保守重连策略；不能只有 event codec。
- Realtime 首版实现 WebSocket codec、client secret、call control、SDP request/response；不把完整 WebRTC media stack 强绑进核心依赖。
- `/realtime/calls` 是 multipart（SDP + JSON session），响应是 raw SDP text。

### 8.3 Multipart

```rust
pub enum Upload {
    Path(PathBuf),
    Bytes { bytes: Bytes, filename: FileName, content_type: Mime },
    Reader(OneShotReader),
    Stream(OneShotStream),
}
```

- Path/Bytes 可重放；Reader/Stream 明确 one-shot。
- Path 首次准备时记录可移植的 size/mtime 与平台可用的 file identity；每次 retry 重新打开并复核，路径替换、symlink 目标变化或内容元数据变化时 fail closed，不把不同文件当作同一逻辑请求。需要可变来源时调用方改用显式 one-shot stream，因而不重试。
- 文件流式传输，不先整体读入内存。
- filename、field name、MIME、CRLF/header 注入需验证。
- endpoint-specific encoder 读取 OpenAPI encoding，决定 repeated field、`field[]`、nested key、JSON part、SDP part 与 filename。
- multipart 重试必须重建 boundary/form；测试解析 part 比较内容，不比较随机 raw boundary。

### 8.4 Pagination

实现多种 cursor strategy，而不是统一猜测：

- last item ID -> `after`。
- response `last_id`。
- response `next`/`next_page`。
- before/after cursor。
- offset/page。
- 只有 envelope、实际不分页。

wire page 留在 types crate；client 提供 `next_page()` 和 `TryStream` 自动翻页。翻页复制原请求和 RequestOptions，只替换 cursor；cursor 当 opaque query 值，检测重复 cursor 防无限循环，不跟随任意 server-provided URL。

### 8.5 Raw response

文件、zip、audio、image、SDP 和 text/SRT/VTT 使用 `ByteStream`/`TextBody`；支持流式保存与 size limit。不得为日志或错误探测提前把大文件全部缓冲。

## 9. Responses API 首要垂直切片

稳定 Responses 面优先完成：

- `POST /responses`：JSON 或 SSE。
- `GET /responses/{response_id}`：JSON，支持 query-driven stream 时分离 API。
- `DELETE /responses/{response_id}`。官方 raw HTTP 示例返回 deleted object，但当前 TypeScript SDK 签名为 `void`；定义 `DeletedResponse` 并兼容空成功体，把差异固化为 override + 双 fixture。
- `POST /responses/{response_id}/cancel`；只允许创建时 `background = true` 的 Response。
- `POST /responses/compact`。
- `GET /responses/{response_id}/input_items`。
- `POST /responses/input_tokens`。
- `responses.connect()` 持久 WebSocket（不是 `.paths` 内的普通 REST operation，单列 transport contract）。

核心模型至少覆盖：

- input string 与完整 32 分支 input item union；三个 `type = "message"` schema 必须二阶段判别。
- output item 28 个稳定分支。
- tool 16 个稳定分支与 9 路 tool choice union。
- text/structured output、reasoning、include、conversation、prompt、background、compaction。
- function/custom/MCP/tool-search/shell/computer/file-search/web-search/image/code-interpreter 等调用项。
- 58 个 SSE event 与 Responses WebSocket events。
- official examples、unknown fields/events、无损 `ResponseOutputItem -> ResponseInputItem` 续轮 helpers。
- background poll/cancel，以及 `starting_after = sequence_number` 的断流恢复；只有创建时启用 stream 的 background response 可恢复流。

Beta multi-agent Responses 必须单独使用 `beta-responses-multi-agent` feature、固定 `beta=true` query 和 `OpenAI-Beta: responses_multi_agent=v1`，不能混入稳定 enum 或默认代码路径；两项都进入 OperationMeta 与 HTTP contract。

## 10. RMCP 集成计划

### 10.1 两层必须分开

**OpenAI 原生 MCP wire** 属于 core types，不依赖 rmcp：

- `Tool::Mcp`，typed server URL/connector/tunnel source。
- secret authorization/headers。
- `allowed_tools`、`require_approval`、`defer_loading`。
- `mcp_list_tools`、`mcp_call`、`mcp_approval_request/response` output 与 stream events。

默认不能替用户设置 `require_approval = "never"`；敏感操作默认要求显式 policy/approval。

**Rust rmcp bridge** 位于 `openai-rs-rmcp`：连接本机 stdio 或 Streamable HTTP MCP server，把 MCP tools 转为 OpenAI function tools并在本地执行。它不是虚构的 `/mcp` REST resource。

### 10.2 rmcp version 与 features

审计基线固定为 [`rmcp 3.1.4`](https://docs.rs/rmcp/3.1.4/rmcp/)，edition 2024、MSRV 1.88；代码包含 MCP 2026-07-28 支持，但该版本的 `ProtocolVersion::LATEST` 仍是 2025-11-25。因此不能依赖默认版本常量，必须显式协商并分别测试两个协议族。为保证此计划可复现，初版精确依赖：

```toml
rmcp = { version = "=3.1.4", default-features = false, optional = true }
```

`openai-rs-rmcp` 的 feature 必须真实 forward 上游 feature；Facade 再同名转发 adapter feature：

```toml
[features]
model = ["dep:rmcp"]
client = ["model", "rmcp/client"]
client-stdio = ["client", "rmcp/transport-child-process"]
client-http-rustls = [
  "client",
  "rmcp/transport-streamable-http-client-reqwest",
  "rmcp/reqwest",
]
client-http-native-tls = [
  "client",
  "rmcp/transport-streamable-http-client-reqwest",
  "rmcp/reqwest-native-tls",
]
server = ["model", "rmcp/server"]
server-macros = ["server", "rmcp/macros"]
server-stdio = ["server", "rmcp/transport-io"]
server-http = ["server", "rmcp/transport-streamable-http-server"]
server-elicitation = ["server", "rmcp/elicitation"]
request-state-sealing = ["model", "rmcp/request-state"]
auth-core = ["client", "rmcp/auth"]
auth-jwt = ["auth-core", "rmcp/auth-client-credentials-jwt"]
```

Facade 的完整映射为：`rmcp -> adapter/client`、`rmcp-stdio -> adapter/client-stdio`、`rmcp-http-rustls -> adapter/client-http-rustls`、`rmcp-http-native-tls -> adapter/client-http-native-tls`、`rmcp-server -> adapter/server`、`rmcp-server-macros -> adapter/server-macros`、`rmcp-server-stdio -> adapter/server-stdio`、`rmcp-server-http -> adapter/server-http`、`rmcp-server-elicitation -> adapter/server-elicitation`、`rmcp-request-state -> adapter/request-state-sealing`、`rmcp-auth -> adapter/auth-core`、`rmcp-auth-jwt -> adapter/auth-jwt`。OAuth core 与 TLS transport 解耦，调用者再任选 rustls/native transport。

Facade 的 `rmcp` 至少开启 adapter `client`，否则 `RunningService`/client API 不存在。ClientHandler 的 elicitation callback 本身属于 client API且默认 decline；`rmcp/elicitation` 主要用于 server typed elicitation helper。`request-state` 是 MRTR state sealing codec，不是 MRTR wire 能力的开关。不启用 rmcp 默认 feature，首版不暴露其 `local` feature；非 local handler 保持 `Send + Sync + 'static`。

### 10.3 Catalog 与 schema 转换

`McpConnection<S>` 持有 owning `RunningService` 并负责显式 `close()/waiting()`；executor 不能只保存克隆 Peer 后让 owning service 被 drop。连接配置用本地 redacted type，在边界转换为 rmcp transport config，不能直接 Debug 可能含 `auth_header/custom_headers` 的上游配置。

`McpToolCatalog`：

- 默认手动分页 `list_tools`，设置 max-pages、max-tools、cursor-cycle detection 和 overall deadline，并保留完整 `rmcp::model::Tool`；`list_all_tools()` 只作为可信 server 的显式 convenience，不能作为无界默认。
- 由于 orphan rule，不能实现 `TryFrom<&rmcp::model::Tool> for` 另一个 crate 的 `FunctionTool`。定义本地 `McpToolExt` 或自由函数 `to_responses_function_tool` / `to_chat_function_tool`；两种 OpenAI wire shape 分别生成，不做 lossless 假承诺。
- `ToolNameMap` 校验名称/长度并维护稳定可逆 alias；冲突返回错误，绝不静默覆盖。
- `SchemaPolicy::{Preserve, OpenAiCompatible, OpenAiStrict}`。
- MCP JSON Schema 2020-12 转 OpenAI strict subset 可以失败；禁止联网解引用外部 `$ref`。
- `input_schema` 与 `output_schema` 都保留；annotations 只是提示，不能单独决定安全或审批。

### 10.4 执行循环

`McpToolExecutor`：

1. 从全部 Responses function calls 提取调用，不能只看第一个 output。
2. `JsonText` 解码为 object；malformed/scalar/null 在调用 MCP server 前失败。
3. 构造 `CallToolRequestParams`，保留 call id/name 映射。
4. 以有界并发执行；完成顺序可乱，但提交给模型的 output 保持原调用顺序。
5. 将 `CallToolResult.result_type`、全部 `content`、`structured_content`、`is_error`、`_meta` 转成 `McpToolOutputEnvelope`；不能只取首个 text 或 structuredContent 后丢弃状态/错误标志。
6. 自动生成 `function_call_output` 输入并继续 Responses，直到无 tool call、达到 max rounds、取消或失败。

协议结果使用 `BridgeOutcome::{Complete(McpToolOutputEnvelope), InputRequired(...), Task(...)}`；真正的 adapter error 只包括 `InvalidArguments`、`SchemaIncompatible`、`NameCollision`、`Transport`、`Protocol`、`Cancelled`、`Timeout`、`RoundLimit`。

MCP 工具级 `isError=true` 是 in-band tool result；JSON-RPC/transport failure 才是 SDK error。

### 10.5 取消、进度、elicitation、MRTR、Tasks

- 核心执行路径使用 `Peer::send_cancellable_request` 取得 `RequestHandle`；supervisor 持有 handle，并在 outer future/stream drop 后异步调用 `RequestHandle::cancel`。不能用关闭整个 `RunningService` 代替单调用取消。
- OpenAI future/stream drop 应取消对应 MCP request；MCP cancellation 应 drop/abort 对应 OpenAI work。
- idle timeout 可由匹配 progress token 重置，但必须同时有不可延长的 max total timeout。
- elicitation 默认 decline；只有显式用户 handler 才能 accept，URL flow 不能自动打开。
- `call_tool_once`/高层 `call_tool` 不返回 RequestHandle，只有不需要可取消/可观察 request options 的简化 API 才能使用；核心 executor 自己解 `Complete | InputRequired | Task`。
- `RunningService::call_tool` 可在配置用户 handler 后自动驱动 MRTR，但必须限制轮数并原样回传 requestState；遇到 Task 会返回 unexpected response，不能用于完整 Tasks 路径。
- Tasks 使用 `send_cancellable_request` + `tasks/get/update/cancel` 独立状态机，并按协商 capability 启用。
- Tasks 遵守 server 的 `poll_interval_ms`/`ttl_ms`，`InputRequired` 通过 `tasks/update` 回传，Completed 解码为完整 `CallToolResult`，Failed/Cancelled 单独分流；`tasks/cancel` 只视为 cooperative acknowledgement。
- 建连使用 `serve_with_lifecycle` 的 Discover/Auto 模式，preferred version 显式为 2026-07-28，legacy fallback 为 2025-11-25；两个协商结果分别 E2E，不能仅测试 `LATEST`。
- 不使用 `ClientInfo::default()` 的空 capabilities。由 builder 按真实本地能力构造 `ClientInfo`：只有安装 Tasks driver 才广告 Tasks extension；只有配置 form/URL elicitation handler 才广告对应 elicitation mode；未实现的 roots/sampling 等能力保持关闭。E2E 同时断言 advertised capability 与可执行 handler 一致。
- stdio child transport 增加本地 bounded framing/line limit，或在无法包裹上游 `AsyncRwTransport` 时把无界入站行风险写入 feature 文档并默认只允许受信任子进程。

### 10.6 可选 server adapter

`rmcp-server` 把同一个带关联类型的 `ToolSpec` 注册为 `rmcp::handler::server::router::tool::ToolRoute<S>`，输入使用 `Parameters<T>`，typed structured output 使用 `rmcp::Json<T>`。注册前显式拒绝重复 route name，因为上游 router 的覆盖行为不能作为冲突策略。默认不把付费 OpenAI operation 自动暴露成 MCP tool；用户必须显式 allowlist，以免形成无限调用、数据泄漏或意外费用。

## 11. Codex/ChatGPT 订阅认证（借鉴 OpenCode）

### 11.1 产品与协议边界

OpenAI 官方明确区分两类身份：[Authentication](https://learn.chatgpt.com/docs/auth) 将其描述为“ChatGPT subscription access”与“API key usage-based access”；[Pricing](https://learn.chatgpt.com/docs/pricing) 也把 ChatGPT plan 的 Codex 额度与 Platform API token 计费分开。因此：

- `Client`/`AdminClient` 只面向 `api.openai.com/v1`，接受 Platform API key、WIF 或 Admin key。
- `CodexAppServerClient` 面向官方 Codex app-server JSON-RPC，使用 app-server 管理的 ChatGPT browser/device login；这是默认、文档支持的订阅路径。
- `DirectCodexResponsesClient` 只在 `experimental-codex-direct` 下提供 OpenCode 风格的纯 Rust transport，且仅面向 Codex Responses backend；不声称是通用 OpenAI API 或稳定公共 contract。
- ChatGPT credential、app-server transport token 与 Platform credential 是三个不同类型，不能相互转换或发往错误 host。
- 绝不实现公网 API 网关、多用户共享、多账号池、账号轮换、额度聚合、凭证转发、转售或 OpenAI-compatible 通用代理。

[Codex App Server](https://learn.chatgpt.com/docs/app-server) 是官方给自定义产品的深度集成面，负责认证、会话、审批和流式 agent event；[Codex access tokens](https://learn.chatgpt.com/docs/enterprise/access-tokens) 只用于可信的本地 CLI/app-server 自动化，通用 OpenAI API 仍使用 Platform API key。

### 11.2 从 OpenCode 借鉴的设计

借鉴以下分层，而不是逐行翻译：

```text
标准 Responses DTO/Serde
  -> Codex 专用 request policy
  -> OAuth/TokenManager
  -> host-locked transport
```

具体优点：

- API key 与 OAuth 使用 tagged credential 类型和统一 provider-auth 方法接口。
- browser PKCE 与 headless device-code 共享同一 token/session 模型。
- 请求仍由标准 Responses SDK/DTO 自动序列化，订阅差异只在 transport/auth middleware。
- 每次发送前读取最新 credential；refresh 后保存旋转后的 token。
- 从 token/account state 派生 `ChatGPT-Account-Id`，不要求用户手写 header。
- OAuth 与 API-key 模型策略分离；session header、instructions/store/item-id 等 Codex 兼容规则集中处理。

OpenCode 的固定审计链接见第 2.4 节。其代码采用 MIT；若复制实质表达而不仅借鉴思想，NOTICE/许可中保留相应归属。实际 Rust 细节优先采用 Apache-2.0 的官方 Codex Rust 实现。

### 11.3 默认实现：Codex app-server client

新增 `openai-rs-codex`，默认公开 `CodexAppServerClient`：

```text
openai-rs-codex/src/
  app_server/
    process.rs             # owning child + stdio JSONL
    codec.rs               # bounded JSON-RPC framing
    client.rs              # request id、pending map、notifications
    generated/             # 从固定 Codex runtime schema 生成
    account.rs             # login/read/logout/rate limits/usage
    thread.rs              # thread/start/resume/fork
    turn.rs                # turn/start/steer/interrupt + streamed events
    approvals.rs
  direct/                  # 仅 experimental-codex-direct
  error.rs
```

实施要求：

1. 固定一个审核过的 Codex runtime revision，运行 `codex app-server generate-json-schema --out ...`，提交 schema 与 SHA；Rust DTO 由该 schema 生成，用户 build 不启动 Codex 也不联网。
2. runtime 只能来自调用者显式路径或已验证 PATH 项；库不自动下载、更新或执行未知二进制。仓库提交一份精确兼容清单，把每个受支持平台的“已发布 runtime 版本 + 可执行文件 SHA-256”映射到对应 schema SHA-256；启动时同时校验版本与二进制哈希，只接受清单中的精确组合。未知版本、未知哈希及源码构建常见的 `0.0.0` 一律 fail closed；`initialize` 返回的 `userAgent` 不作为协议兼容证据。
3. 默认创建 openai-rs 专用 `CODEX_HOME`/runtime profile（目录 0700）并使用环境 allowlist；清除继承的 `OPENAI_API_KEY`、`CODEX_ACCESS_TOKEN`、provider/base-url 和会改变 auth/provider 的配置。复用现有 Codex home/config 只能显式 opt-in，并清楚说明会共享/修改登录状态。
4. 默认使用 owning child process + stdio JSONL。`Drop`/显式 `close` 终止并回收 child；bounded line、pending request、event queue 与进程 stderr 均有上限。
5. 先完成 `initialize` request，再准确发送一次无 `id` 的 `initialized` notification，之后才允许其他调用；request id correlation、server request、notification、取消和进程退出有独立状态机。
6. browser login 发送 `account/login/start { type: "chatgpt" }`，返回 `loginId/authUrl`；调用者选择是否打开浏览器，app-server 自己承载 callback、持久化和 refresh。
7. device login 发送 `type: "chatgptDeviceCode"`，把 `verificationUrl/userCode` 作为 typed event 给 UI；等待 `account/login/completed`，并可用 `account/login/cancel` 取消。
8. request `account/read`、`account/logout`、`account/rateLimits/read`、`account/usage/read` 与 notifications `account/updated`、`account/rateLimits/updated` 分别类型化；plan/rate-limit 值保持开放枚举/可选字段。运行 turn 前读取 `account/read` response，验证账户属于预期的 ChatGPT provider class；精确认证来源不能从该 response 或通知中猜测，必须由本 client 已完成的 browser/device 登录流程，或由调用者对专用预认证 profile 的显式证明确定。`account/updated` 只监测后续账户变化；发现意外 API-key、provider 或账户切换时立即禁用 turn。
9. Business/Enterprise 的 Codex access token 只在显式模式下通过 `CODEX_ACCESS_TOKEN` 环境变量传给专用 child，或由用户先在该专用 `CODEX_HOME` 执行 `codex login --with-access-token`；它不经过 `account/login/start`。不得把该 token 发给 Platform API，也不得与保护 app-server WebSocket 的 transport token 复用。
10. 官方当前将 app-server command 与 WebSocket transport 标为 experimental、unsupported for production。stdio 是本 crate 默认且官方文档化的实验路径，但不得宣称 production-supported；远程 WebSocket 额外只允许 loopback/SSH 场景并默认关闭。

这个模式完整复现 OpenCode 的“ChatGPT 订阅登录体验”，但 OAuth client id、callback server、token store 与 refresh 由官方 runtime 管理。

### 11.4 实验实现：纯 Rust direct Codex Responses

为满足无外部 runtime 的本地单用户场景，可在独立 `experimental-codex-direct` feature 中实现 OpenCode 风格 transport。它不是标准 `Client` 的 credential option。

首版 sealed operation allowlist 仅包含经 fixture 验证的 `ResponsesCreate`/stream：

```text
ResponsesCreate
  -> POST https://chatgpt.com/backend-api/codex/responses
其他 Platform operation
  -> UnsupportedCodexOperation
```

安全/兼容要求：

- URL 由常量和 operation id 构造，不接收 raw URL/base URL，不用 pathname substring rewrite。
- 禁止 redirect；任何未识别 host/path 在加入 bearer 之前失败。
- 用户不能覆盖 `Authorization`、`ChatGPT-Account-Id`、`Host`、Cookie、originator 等保留 header。
- `originator` 使用真实项目身份 `openai-rs`，不冒充 `opencode` 或第一方 Codex；User-Agent 只含最小必要版本/平台信息。
- 注入随机、非敏感 session/thread id；不把 credential、account id、prompt 或 response 写入日志。
- 仅发送 Responses wire；Chat Completions body 不能改 URL 后原样发送。未来转换必须有独立 typed mapper 与双向 fixture。
- `validate_for_codex()` 对 Codex backend 尚不支持的字段显式报错，不像 OpenCode 那样静默清除 `maxOutputTokens`。
- 不把模型 cost 标成 0；使用 `BillingMode::ChatGptSubscription`，说明仍受 plan entitlement、rate limit、credits 与公平使用限制。
- `ModelId` 保持开放，但 server-discovered/compatibility catalog 只提供提示；不用 `parseFloat` 猜测未来模型版本。

Direct mode 的 browser/device OAuth 实现借鉴 OpenCode UX，但代码优先移植官方 `codex-rs/login` 的安全模式：

- Authorization Code + PKCE S256、256-bit state、offline refresh。
- callback 仅绑定 `127.0.0.1`，精确 method/path、bounded request、一次性 state、deadline/cancel/RAII cleanup。
- token exchange、token 校验和安全持久化全部成功后才显示成功；错误页不插入远端 HTML，添加 CSP/no-store/nosniff。
- device flow 有 15 分钟总 deadline、取消、server interval、429/Retry-After 和有界 backoff；403/404 pending 语义由冻结 fixture确定。
- OAuth client identity、scope、issuer 与 endpoint 都从审核过的官方 Codex revision冻结；不复制 OpenCode 的 `originator`，也不把这些常量承诺为稳定 OpenAPI。

### 11.5 TokenManager、claims 与存储

Direct mode 定义单账号 `CredentialStore`；不支持账号池：

```rust
pub trait CredentialStore: Send + Sync + 'static {
    async fn load(&self) -> Result<Option<StoredCodexSession>, StoreError>;
    async fn save(&self, session: &StoredCodexSession) -> Result<(), StoreError>;
    async fn delete(&self) -> Result<(), StoreError>;
}
```

- `KeyringStore` 为默认；`MemoryStore` 用于测试/显式无持久化。
- `FileStore` 仅在显式 feature 下：目录 0700、文件创建即 0600、拒绝 symlink、文件锁、同目录 temp、fsync、atomic rename；keyring 失败不能静默降级到文件。
- 不读取、修改、导入或接管 `~/.codex/auth.json`、OpenCode `auth.json` 或浏览器 cookie。
- secret 使用 `secrecy/zeroize`，不实现 Serialize/Display，Debug 恒脱敏；存储只保留协议所需 token、expiry、selected account id 和格式版本，不保存 email 等无关 PII。
- `logout` 原子删除本地凭据；只有真实完成 revoke contract 时才宣称远端撤销。

`TokenManager` 使用 async singleflight + guarded reload：提前 60 秒刷新，锁内 double-check；新 refresh token 缺失时保留旧 token，存在时执行 rotation；先原子保存再发布内存 snapshot。`invalid_grant` 进入 `ReauthenticationRequired` 并阻止 refresh storm；401 只允许一次 guarded refresh+replay。多进程共享 store 时使用不含 secret 的锁文件与 account-generation/CAS，account id 变化 fail closed。

JWT claim 解码与身份验证分开：

- ID token 优先，namespaced `https://api.openai.com/auth.chatgpt_account_id` 次之；多个/冲突 workspace 要求显式选择。
- ID token 尽可能通过 OIDC/JWKS 校验 signature、issuer、audience、expiry、nonce。
- 若 access-token payload 只能兼容性 decode，API 明确命名 `decode_unverified_claims`；其结果只是 routing hint，不能作为授权依据。
- `ChatGptAccountId` 是不透明、不可由普通调用者覆盖的 newtype，转换为 header 前严格校验。

### 11.6 明确不照搬的 OpenCode 缺陷

- 进程全局 `oauthServer/pendingOAuth` 单槽、固定端口且未明确 loopback。
- token exchange 完成前返回成功页；失败/拒绝/超时可能泄漏 listener。
- 未转义 `error_description` 的 HTML 插值。
- device `while(true)` 无 deadline/cancel/slow-down 状态。
- 过期后才 refresh、无 singleflight、假定每次都有新 refresh token。
- 未验证 JWT、静默取 `organizations[0]`。
- dummy key + 任意 Request URL monkey-patch；可能把 bearer 发往非 Codex origin并丢失原 Request 属性。
- `includes()` path 匹配、把 Chat body 直接改投 Responses endpoint、丢 query。
- `parseFloat` 模型放行、订阅 cost=0、auth 变化依赖外部 dispose。
- 非原子整文件 read-modify-write、写后 chmod、跟随 symlink、读错当空对象。
- Codex header/参数 hook 误作用于 API-key OpenAI 请求。

### 11.7 Codex 专项测试与验收

App-server：

- pinned schema codegen zero-diff、initialize/version mismatch、request correlation、server request、notification、malformed/oversize JSONL。
- child start/exit/crash/cancel/drop、stderr 上限、无 zombie、并发 pending 上限。
- browser/device login start/completed/cancel/logout、account updated、rate limits/usage、plan unknown value。
- thread/turn/event/approval 的最小 E2E；凭证始终只存在于 app-server，不进入 Rust log/serde/debug。

Direct experimental：

- PKCE RFC vector、authorize URL golden、state/code 缺失/重复/错误、OAuth denied、callback timeout/drop/port collision/cleanup。
- device pending/success/429/denied/expired/malformed/deadline/cancel、轮询间隔。
- OIDC/JWT malformed/oversize/signature/issuer/audience/expiry/nonce、claim 冲突、多 workspace选择。
- fake keyring、文件权限/atomic/symlink/migration、secret-marker 全日志扫描。
- 100 并发请求只 refresh 一次；rotation、missing refresh token、invalid_grant、瞬时失败、store失败、account变化。
- host/path/header/redirect fail-closed；只有 sealed Responses operation 能命中 Codex host。
- `trybuild` 证明标准 `Client` 不能接收 subscription session，Codex clients 无 Admin/Files/raw send。
- `codex-app-server`、`experimental-codex-direct`、device、TLS、RMCP组合分别编译；真实订阅 smoke 只手工 opt-in，任何常规自动化检查都不保存 credential。

### 11.8 与 RMCP 的关系

RMCP tool loop 依赖一个 typed Responses executor trait，而不依赖 credential concrete type。Platform Client 与 direct Codex Responses Client 可实现该 trait；app-server 模式则通过 thread/turn/tool event adapter 接入。任何 MCP server/handler 都拿不到 ChatGPT token。不要围绕已弃用的 `codex mcp-server` 设计新集成，使用 app-server。

## 12. API 资源范围与优先级

所有 path 默认位于 `https://api.openai.com/v1`。

| 等级 | 资源 | 说明 |
|---|---|---|
| P0 | Responses | 首要稳定 API；含 JSON、SSE、WebSocket、background、compact、input items/tokens |
| P1 | Chat Completions、Models、Embeddings、Moderations | Chat 保持兼容，但新项目默认引导 Responses |
| P1 | Files、Uploads | multipart、bytes download、分页；也是 Batch/FT/Vector Store 基础 |
| P2 | Batches、Vector Stores、Webhooks | 异步状态、JSONL helper、分页、签名验证 |
| P2 | Audio、Images generation/edit | multipart、raw media、多格式响应、SSE partial events |
| P3 | Fine-tuning Jobs、Containers、Skills、Evals、Graders | 状态机、zip/raw content、多级资源 |
| P3 | Conversations、Content Provenance | 当前规范的现行资源 |
| P3 | Realtime GA | WS events、client secrets、SDP calls、SIP control；完整 WebRTC media 不强绑核心 |
| P4 | Administration、Usage/Costs、Workload Identity | 独立 auth/client/feature；含 RBAC、projects、certificates、spend/data retention |
| P4 | custom voice、checkpoint permissions | 受限权限，独立 feature |
| Optional backend | Codex app-server / ChatGPT subscription | 独立 client；不是 Platform `/v1` resource；direct transport 为 experimental |
| Optional | Beta ChatKit、Responses multi-agent、FT alpha graders | 独立 feature、独立 namespace、明确稳定性 |
| Legacy optional | `POST /completions` | `legacy-completions`，默认关闭 |
| Sunset / historical | Assistants/Threads/Runs | 官方公告 2026-08-26 sunset；旧 reference 仍可见，作为历史类型与迁移资料，不生成默认可调用 client method |
| Deprecated / sunset | Videos/Sora | 公告 2026-09-24 shutdown；进入 quarantine。若实现工作发生在 shutdown 前，只能放 `sunset-videos` 临时 feature；之后降为 historical types |
| Contract conflict | `/images/variations` | DALL·E 2 页面称已移除，但 API reference 仍列 endpoint；进入 quarantine/decision ledger，不能把任一来源静默当真 |
| Legacy optional | Realtime Beta sessions | 只在 `legacy-realtime`，GA flow 优先 |

`operations.json` 对每项分别记录 lifecycle、implementation、feature/release visibility 与官方证据，保证“看见并决定过”，而非扫描漏掉。

## 13. 测试资产与质量门禁

### 13.1 目录

```text
testdata/fixtures/<resource>/<operation>/
  request-min.json
  request-full.json
  response.json
  error.json
  stream.sse
  fixture.toml             # official/synthetic/curated、URL/Pointer/SHA、方向、预期

crates/openai-rs-contract-tests/tests/
  wire_fixtures.rs
  wire_properties.rs
  operation_bindings.rs
  http_contract.rs
  sse.rs
  multipart.rs
  pagination.rs
  codex_contract.rs
  rmcp_contract.rs
  feature_isolation.rs

fuzz/fuzz_targets/
  sse.rs
  tagged_union.rs
  error_envelope.rs
  multipart_names.rs
  codex_auth.rs
  rmcp_bridge.rs
```

普通 PR 测试全部离线；真实 API smoke 只能 opt-in 使用 secret，并自动清理创建的资源。

### 13.2 Serde

区分三类保证：

1. typed roundtrip：`T -> Value -> T` 后相等。
2. semantic JSON roundtrip：`Value -> T -> Value` 忽略 object key 顺序后相等。
3. decode-only compatibility：只用于会规范化的历史 wire，并在 manifest 记录原因。

每个 applicable operation 至少覆盖：最小请求、全字段请求、成功、API error、缺 required、错误类型、跨字段冲突。没有官方 example 时生成有 JSON Pointer 和规则版本的 synthetic fixture，不能虚构为“官方样例”。Fixture manifest 冻结期望数量和 `official | synthetic | curated` 来源。单独覆盖 missing/null/empty/false/0、unknown string、unknown tag、known-tag malformed、重复 key、额外字段和 ambiguous union。

使用 `serde_path_to_error` 验证错误路径；用冻结 JSON Schema 反向验证 Rust 请求。为核心 DTO 写 `proptest`，官方 fixture 同时进入 fuzz corpus。

### 13.3 SSE/WS

- 短 fixture 穷举每个 byte split；长 fixture随机 chunk partition、byte-by-byte、all-in-one。
- 覆盖 UTF-8、CR/LF/CRLF、BOM、comment、event/id/retry、多 data、空 data、多个事件、malformed JSON/UTF-8、oversize、断连。
- endpoint-specific terminal、terminal 后额外数据、unexpected EOF、error event。
- transport/decode error 只 yield 一次后终止。
- 提前 break/drop abort 请求并释放连接 permit；gate 测试证明背压与不预读全 body。
- fuzz invariant：无 panic/死循环、内存有界、事件不重复、错误后终止、Debug 不泄密。

### 13.4 HTTP/multipart/pagination

使用 `hyper + hyper-util + http-body-util` 的 scripted loopback server 捕获 method/path/query/headers/body，并支持延迟、任意 chunk、半途断连、counter 和 semaphore gate。

- 每个 operation 自动生成 HTTP contract test。
- 错误 body 覆盖 JSON、HTML/text、malformed、truncated、empty、错误 MIME。
- retry 用 paused Tokio time，覆盖所有状态、header、backoff、deadline 与不可重放 body。
- multipart 解析 parts，覆盖 path/bytes/reader、空文件、Unicode filename、CRLF 注入、boundary-like bytes、JSON/SDP parts、失败前 preflight、大文件不双缓冲。
- pagination 覆盖所有 cursor family、空页、重复 cursor、drop 后不再请求。

### 13.5 Compile-time

用 `trybuild` + `static_assertions`：

- Streaming/NonStreaming 不能调用错误 terminal。
- endpoint request/response/event 关联类型准确。
- 下游不能实现 sealed operation/type-state trait。
- custom transport 必须 `Send + Sync`。
- 所有 JSON request/response/event 必须 `Serialize + DeserializeOwned + Send + Sync + 'static`；multipart handle/raw stream 只验证其专用 codec trait。
- typed tool 缺 `Serialize`、`DeserializeOwned`、`JsonSchema`、`Send/Sync` 时失败。
- Platform `Client` 不能接收 ChatGPT/Codex credential；Codex clients 不暴露 Admin/Files/raw URL，app-server transport token 不能作为模型 credential。
- 未启用 feature 时 API 不可见；单 feature 正向示例可编译。

### 13.6 RMCP

使用 `tokio::io::duplex` 或 Worker 启动真实 rmcp client/server：

- initialize/lifecycle、list pagination、tools/call、close。
- schema `$defs`、nullable、additionalProperties、external ref、name collision。
- malformed/scalar/null arguments 时 server 调用次数为 0。
- 全部 ContentBlock（text/image/audio/resource/resource_link）、多块顺序、structured content 与 isError。
- transport/protocol/tool error 分流、timeout、cancel、progress、elicitation、MRTR、Tasks。
- stdio child 退出/僵尸清理、Streamable HTTP rustls/native-tls feature。
- 固定 `@modelcontextprotocol/conformance@0.2.0-alpha.10`（rmcp-v3.1.4 官方 workflow 使用的版本），对 2025-11-25 与 2026-07-28 client/server dated suites 运行；Tasks extension 单独运行并维护严格、可审查的 expected-failure baseline。

### 13.7 本地质量门禁与 release

提交或发布前在本机必须通过：

```text
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
cargo test --workspace --no-default-features
cargo test --workspace --all-features
cargo +1.88 check --workspace --all-features --locked
cargo hack check --feature-powerset --depth 2
cargo doc --workspace --all-features --no-deps
cargo xtask spec verify
cargo xtask codegen --check
```

Linux 跑完整矩阵；macOS/Windows 跑 no-default、default、rmcp、all-features。Nightly/scheduled 跑 minimal versions、Miri（纯 parser/serde）、fuzz、mutants、upstream drift 和可选真实 smoke。

Release gate：`cargo-semver-checks`、public API diff、`cargo-deny`/audit、package contents、docs.rs all-features、所有 examples 编译、无未审核 drift。operation、discriminator、RMCP content variant 覆盖必须 100%；wire/parser/union 分支覆盖目标不低于 95%。

## 14. 分阶段实施与验收

### M0：仓库与规范基线

交付：

- 初始化 Git、workspace、toolchain/MSRV、README、rustfmt/clippy/deny 与本地门禁命令；不创建 GitHub CI/CD。默认采用 Rust 生态常见的 `MIT OR Apache-2.0` 双许可证并在发布前确认。
- 固化第 3 节完整 feature graph、默认集合、TLS 选择/双开规则、Facade->adapter forwarding 和 `full` 语义。
- vendor OpenAPI/SDK transformed spec 与实际采用的官方 docs evidence，建立 `SOURCES.toml`。
- 记录 OpenCode/official Codex commits；为 `openai-rs-codex` 冻结 app-server generated schema、登录文档与 direct-backend compatibility ledger。
- 生成 operation/discriminator/nullability inventory：288 个 client operation 与 18 个 webhook receiver operation 分开登记 lifecycle、implementation、feature；Responses WebSocket connect 等非 REST transport contract 另列。
- 建立 codegen IR、override schema 和零 diff check。

验收：来源 hash 可离线验证；inventory 无重复/遗漏；普通构建不联网。

### M1：Serde kernel 与 codegen proof

交付：

- `Omittable`、`Nullable`、open enum、unknown tagged object、ExtraFields、ID/ModelId、JsonText、secret wrapper。
- 选择 10 个最复杂 schema 验证 allOf、recursive ref、ambiguous discriminator、nullable 和 extra fields。
- generated request builder、accessor、trait assertion。

验收：missing/null/value、known/unknown、duplicate key、semantic roundtrip 的 unit/property/compile tests 全绿。

### M2：Responses 非流式垂直切片

交付：

- Client/auth/error/meta/route/JSON transport。
- `POST/GET/DELETE/cancel/compact/input_items/input_tokens`。
- input/output/tool/tool-choice 全部稳定 union。
- `output_text()`、`function_calls()`、`to_input_items()` helper。

验收：官方所有 Responses 非流式 examples 解码；mock server 验证 request wire；unknown field/item roundtrip。

### M3：Responses streaming、structured output 与 typed tools

交付：

- SSE parser、58 个 event、termination、backpressure/cancel。
- `responses.connect()` 持久 WebSocket：建连/auth、sink/stream、close、背压、重连 policy 与 E2E。
- `retrieve_stream`、`resume_stream(starting_after)` 与 background `poll_until_terminal`。
- accumulator/final response。
- schemars strict schema normalizer、ToolSpec/ToolHandler/ToolRegistry、typed output parse 与 registry E2E。

验收：所有 event fixture、任意 byte split、tool delta 聚合、refusal/incomplete、schema failure path、fuzz 门禁。

### M4a：Codex/ChatGPT 订阅认证

交付：

- `openai-rs-codex` 与标准 Platform Client 的类型/host/protocol 隔离。
- pinned Codex app-server schema、stdio JSON-RPC client、browser/device managed login、account/rate-limit/usage、thread/turn/event/approval。
- 可选 Codex access-token 路径。
- `experimental-codex-direct`：hardened PKCE/device、Keyring/TokenManager singleflight、host-locked Responses create/stream transport与兼容性 ledger。

验收：app-server 本地 E2E；credential 永不离开正确 host/process；direct mode 的并发 refresh、callback/device、安全存储、redirect/header/operation allowlist 全门禁；默认/full build 不启用订阅功能。

### M4b：RMCP

交付：

- OpenAI native MCP wire 已在 core 完整可用。
- `openai-rs-rmcp` catalog、schema policy、name map、executor、round loop。
- stdio/Streamable HTTP feature、取消/进度/elicitation/MRTR/Tasks。
- 可选 typed-tool -> rmcp server adapter。

验收：真实 in-process E2E、全部 content/result/error 形态、feature isolation、MSRV、conformance。

### M5：兼容性核心 REST

交付：Chat Completions（JSON/SSE/stored CRUD）、Models、Embeddings、Moderations、Files、Uploads。

验收：各 operation fixture+HTTP contract 100%；multipart/raw bytes/pagination 资源门禁通过。

### M6：Batch、Vector Store、media 与 webhook

交付：

- Batches + typed `BatchLine<O>`/JSONL writer。
- Vector Store/store file/file batch/search。
- Audio、Images generation/edit 的 multipart/raw/SSE 模式。
- Webhook signature verify + event unwrap。

验收：用户无需手写 JSONL；大文件/媒体流不双缓冲；webhook 先验签后反序列化。

### M7：高级资源

交付：Fine-tuning Jobs、Containers、Skills、Evals、Graders、Conversations、Content Provenance。

验收：所有状态机、zip/raw content、嵌套 pagination 和 polling helper 有 deadline/cancel/terminal tests。

### M8：Realtime GA

交付：Realtime client/server events、WS session、client secrets、translation、SDP call create、SIP/WebRTC call control。

验收：事件全分支、unknown event、base64 audio、WS backpressure/close/reconnect policy、SDP multipart/text 契约。

### M9：Administration 与身份

交付：Admin client、organization/project/RBAC/keys/audit/certificates/usage/cost/spend/data retention、workload identity token exchange 和可选 X.509。

验收：普通 credential 编译期不能调用 admin；所有 secret/log/mTLS/URL 安全测试通过。

### M10：Beta/legacy 与发布硬化

交付：可选 ChatKit、Responses multi-agent、FT alpha graders、legacy Completions/Realtime types；明确不实现的 sunset surface 文档；全套 drift/fuzz/semver/docs/examples。

验收：feature stability 标签准确；默认 API 不暴露 dead endpoints；全部 release gates 通过。

## 15. 主要风险与应对

| 风险 | 应对 |
|---|---|
| 官方规范、docs、SDK 不一致 | 来源优先级 + decision ledger + override 必须有 fixture |
| 1424 schema 导致 codegen/编译膨胀 | types/client/rmcp 分 crate；按模块生成；测量编译时间后再做 Box/feature 优化 |
| discriminator 不唯一或 union 重叠 | 二阶段候选判别；禁止泛化 `untagged`；known malformed 不降级 Unknown |
| API 把新增 event 当兼容，Rust enum 易破坏 | `Unknown(raw)` + 定期 drift + discriminator manifest |
| nullable/omitted 丢失 | `Omittable<Nullable<T>>` 组合，不依赖单层 Option |
| tool schema 不符合 OpenAI strict subset | 显式 schema policy、可失败转换、JSON Pointer 错误，不静默删 keyword |
| POST retry 可能重复副作用 | 官方兼容/保守两种 policy；body replayability；stream/one-shot 禁止重试 |
| multipart 大文件内存与 retry | 流式 body、replayable factory、每次重开、RSS/分配测试 |
| RMCP side effect/elicitation 风险 | 默认审批、默认 decline、显式命令与 allowlist、bounded rounds/timeouts |
| ChatGPT credential 被误发到 Platform/第三方 host | 独立 credential/client 类型、sealed operation/host allowlist、禁 redirect、compile/runtime contract tests |
| Codex app-server/direct backend 漂移 | app-server 使用“发布版本 + 平台二进制 SHA-256 -> schema SHA-256”精确清单并拒绝未知/`0.0.0` runtime；direct feature 独立 snapshot/drift、默认/full 不启用 |
| sunset API 污染默认面 | operation disposition 记录，但不生成活跃 client method |
| raw/extra 泄漏用户数据 | Debug 摘要与 secret redaction；raw 只显式 accessor；日志绝不输出 body |

## 16. 最终架构取舍摘要

- 主机器契约是固定 SHA 的官方 OpenAPI；docs 定义行为，官方 SDK 是执行层 oracle。
- wire types/codegen 与 transport/ergonomics 分开；生成结果提交，用户构建不联网。
- 所有 JSON DTO 双向 Serde；请求严格、响应开放；required/null/omitted 精确。
- Rust enum 表达 tagged union，未知字符串和事件保留原值/raw payload。
- 延续 `zai-rs` 的 operation registry、typestate、SSE hardening、replayable multipart；不延续 sealed model matrix、全 Option response 和 Value-in/Value-out 工具接口。
- 模型使用开放 `ModelId`，trait 只约束稳定协议能力和传输模式。
- 正常 function/structured-output/RMCP 路径从 Rust 类型自动生成 schema、解析 arguments、序列化 output。
- OpenAI 原生 remote MCP 与本地 rmcp runtime bridge 是两个模块；核心 DTO 不依赖 rmcp。
- OpenCode 的“标准 Responses codec + 订阅 transport”分层被吸收进独立 Codex crate；官方 app-server 为默认，纯 Rust direct backend 是 host-locked experimental，不把订阅变成通用 API。
- 支持面完整度由 manifest、fixture、compile contract 和 transport contract 量化；已下线接口明确归档。

## 17. 关键参考

- [OpenAI API reference index](https://developers.openai.com/api/reference/llms.txt)
- [API overview 与 backwards compatibility](https://developers.openai.com/api/reference/overview)
- [Responses create](https://developers.openai.com/api/reference/resources/responses/methods/create)
- [Responses streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events)
- [Responses persistent WebSocket connect](https://developers.openai.com/api/reference/typescript/resources/responses/methods/connect)
- [Background mode](https://developers.openai.com/api/docs/guides/background)
- [Function calling](https://developers.openai.com/api/docs/guides/function-calling)
- [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs)
- [MCP and Connectors](https://developers.openai.com/api/docs/guides/tools-connectors-mcp)
- [Realtime guide](https://developers.openai.com/api/docs/guides/realtime)
- [Administration overview](https://developers.openai.com/api/reference/administration/overview)
- [Codex/ChatGPT authentication](https://learn.chatgpt.com/docs/auth)
- [Codex App Server](https://learn.chatgpt.com/docs/app-server)
- [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk)
- [ChatGPT/Codex pricing](https://learn.chatgpt.com/docs/pricing)
- [Codex access tokens](https://learn.chatgpt.com/docs/enterprise/access-tokens)
- [Assistants deprecated reference](https://developers.openai.com/api/reference/typescript/resources/beta/subresources/assistants)
- [Videos deprecated reference](https://developers.openai.com/api/reference/typescript/resources/videos)
- [DALL·E 2 model status](https://developers.openai.com/api/docs/models/dall-e-2)
- [`openai/openai-openapi` pinned commit](https://github.com/openai/openai-openapi/tree/690521b1753dce0c6d6b275f583d22537679cff9)
- [`openai-node` pinned commit](https://github.com/openai/openai-node/tree/eea2292a4a523da9405161dde0a79ac5dc2ecb2a)
- [`openai-python` pinned commit](https://github.com/openai/openai-python/tree/b19c2161b1eac80fbf1f6f67a64a50af99c53356)
- [`openai-go` Responses implementation](https://github.com/openai/openai-go/blob/4d062949c62507e56514af8c7beb186dc09ac075/responses/response.go)
- [`openai/codex` pinned auth implementation](https://github.com/openai/codex/tree/63d213884daea50e4f74efc192cdc44f549b67d5/codex-rs/login)
- [`anomalyco/opencode` pinned Codex plugin](https://github.com/anomalyco/opencode/blob/d1f597b5b5abfe330aa30ca3c33ca043bf9b9a83/packages/opencode/src/plugin/codex.ts)
- [`rmcp 3.1.4`](https://docs.rs/rmcp/3.1.4/rmcp/)
- [`rmcp-v3.1.4` source](https://github.com/modelcontextprotocol/rust-sdk/tree/rmcp-v3.1.4)
