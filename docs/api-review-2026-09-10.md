# Review against the latest OpenAI API documentation

> Follow-up: all four findings were fixed in code. See the [fix record](api-fixes-2026-09-10.md). This report retains the evidence and reproduction results from before those fixes.

Review date: 2026-09-10. Subject: the current workspace, including the preceding review's fixes.

This round found **three reproducible behavior-compatibility issues and one gap in coverage of a published endpoint**. Several newer features lacked dedicated types but could already pass through extension fields or open enums; they were not classified as unusable.

## Evidence and scope

The repository pins `openai/openai-openapi@690521b1753dce0c6d6b275f583d22537679cff9` as `openapi-2026-08-29.json`. Later entries in the official [changelog](https://developers.openai.com/api/docs/changelog) described new error guidance, GPT-6 Astra, asynchronous tool calls, instructions injected during execution, reasoning configuration updates within a conversation, image quality options, and cache diagnostics.

The review fetched 25 official documentation pages or indexes, compared them with the source and pinned specification, and ran a separate Rust reproduction program. Source URLs, content hashes, inspected areas, and actual output appear in the [evidence manifest](api-review-2026-09-10-evidence.json). Online documentation was not substituted for the project's build inputs.

P1 denotes a functional behavior error to prioritize; P2 denotes a compatibility issue or missing endpoint to address.

## 1. [P1] FunctionTool discarded `async: true`

**Location:** `crates/openai-rs-types/src/responses.rs:2020`, `FunctionTool`.

The [official asynchronous tool-calling guide](https://developers.openai.com/api/docs/guides/async-tool-calling) requires `async: true` on function/custom tool definitions to let the model continue working after issuing a tool call. Returned call items also carry that flag.

At review time, `FunctionTool` had neither an `async` field nor a flattened `ExtraFields` container. Deserializing official tool JSON into `FunctionTool` and using it in `CreateResponseRequest` therefore silently removed the flag.

Minimal reproduction:

```rust
let tool: FunctionTool = serde_json::from_value(serde_json::json!({
    "type": "function", "name": "lookup", "async": true
}))?;
let encoded = serde_json::to_value(tool)?;
assert!(encoded.get("async").is_none()); // Observed behavior before the fix.
```

**Impact:** requests built through the normal typed path lost the caller's asynchronous tool semantics. Namespaced function tools reused the same type and were also affected. Constructing an Unknown variant manually was a workaround, but did not fix silent data loss in the typed path.

**Comparison:** `CustomTool` and `FunctionCall` had extension containers. The reproduction preserved the same flag on custom tools, so it would have been incorrect to classify all tool types as lacking asynchronous support.

**Recommended fix:** add an optional boolean and builder methods, inspect other function-tool DTO conversions, and test both serialization round trips and actual request bodies. Preserve unknown fields consistently with the project's lossless-data contract.

## 2. [P2] Structured Outputs rejected valid recursive schemas

**Locations:** `crates/openai-rs-types/src/structured.rs:633` and `:656`.

The [official Structured Outputs documentation](https://developers.openai.com/api/docs/guides/structured-outputs#recursive-schemas-are-supported) explicitly supports recursion and gives examples using root `$ref: "#"` and explicit references into `$defs`.

At review time, the normalizer returned `RecursiveReference` immediately for `#`. Recursive references with siblings such as descriptions were also rejected when inlining revisited the same target. This affected shared helpers including `StructuredOutput::new`, `FunctionTool::for_type`, and `TypedFunction::new`.

Observed local output:

```text
root_recursive_schema=Err(RecursiveReference {
  path: "#/properties/children/items/$ref", reference: "#"
})
described_recursive_schema=Err(RecursiveReference {
  path: "#/properties/node/properties/children/items/$ref",
  reference: "#/$defs/Node"
})
```

**Impact:** the SDK could reject officially supported tree structures, recursive UIs, and linked lists before sending a request. Not every recursive form failed: some `$defs` references without siblings were retained.

**Recommended fix:** distinguish valid schema references from unbounded inline expansion. Preserve valid recursive references, normalize their definitions, and bound actual expansion work. Report genuine alias cycles and dangling references separately. Update tests and decisions that pinned root-recursion rejection.

## 3. [P2] Long Retry-After delays were replaced with short local backoffs

**Locations:**

- `crates/openai-rs-client/src/transport.rs:443`
- `crates/openai-rs-client/src/admin.rs:2412`
- `crates/openai-rs-client/src/multipart.rs:1366`
- `crates/openai-rs-client/src/retry.rs:14`

The [error-code guide](https://developers.openai.com/api/docs/guides/error-codes) requires honoring `Retry-After` for `429 slow_down` and `503 server_is_overloaded`; the [September 2 changelog entry](https://developers.openai.com/api/docs/changelog) explicitly requires waiting at least the specified duration.

The default `max_server_delay` was 120 seconds. A valid server delay above that limit was classified as `TooLong`, then followed the same local exponential-backoff path as an absent hint. The first retry usually occurred after only a few hundred milliseconds. Although previously recorded and tested as intentional behavior, this conflicted with the updated API requirements.

A local mock server returned `Retry-After: 121`. Requests through the public `Client` produced:

| Mock response | Required minimum wait | Measured interval between requests |
|---|---:|---:|
| HTTP 429 / `slow_down` | 121,000 ms | 379 ms |
| HTTP 503 / `server_is_overloaded` | 121,000 ms | 430 ms |

**Impact:** premature retries during rate limiting or overload could trigger further rejection and add server load. Both HTTP cases were reproduced locally; the same Administration and multipart policy was confirmed in source.

**Recommended fix:** never shorten a valid server minimum. If it exceeds the caller's wait limit or remaining request budget, stop automatic retries and return an error retaining retry metadata. Otherwise, wait the full duration. Use local backoff only when hints are absent or unparseable.

## 4. [P2, missing endpoint] Safety webhooks were supported but alert retrieval was missing

**Locations:** resource entry points in `crates/openai-rs-client/src/client.rs` and `crates/openai-rs-types/src/webhooks.rs:545`.

The published [Retrieve a safety alert endpoint](https://developers.openai.com/api/reference/typescript/resources/safety/subresources/alerts/methods/retrieve) is `GET /safety/alerts/{id}`. The [misalignment-monitoring guide](https://developers.openai.com/api/docs/guides/safety-checks/misalignment-monitoring) requires an authorized API key from the same project and the webhook's `data.id` for retrieval.

The repository could already decode and verify `safety.alert.created`, but lacked a `Client` resource, retrieval method, and SafetyAlert DTO. The pinned REST operation inventory also lacked this endpoint.

**Impact:** applications receiving an alert ID through the SDK had to construct another HTTP request themselves to retrieve its details. This was an endpoint-coverage gap, not a webhook-signature failure.

**Recommended addition:** provide retrieval with ordinary Platform project credentials and handle nullable `reason`, `request_paused`, `request_id`, `response_id`, and alert error types correctly. Do not place this operation on `AdminClient`, which accepts organization-administration credentials.

## New features already supported through extension fields

| Official feature | Observed behavior | Assessment |
|---|---|---|
| Prompt Cache Diagnostics | `comparison_response_id` survives through `PromptCacheOptionsParam` extensions; `Response` extensions retain `prompt_cache_diagnostics` | Dedicated builders and result types were missing, but passthrough worked |
| `response.steer` and accepted/pending/failed events | Client events encode losslessly as Unknown variants; receive paths retain unknown events | Named event types, field validation, and a `send_steer` helper were missing |
| `configuration_update` input items | Decode as `ResponseInputItem::Unknown` and pass request validation | Passthrough worked; a named type was missing |
| Image 2.5 `xhigh` and `max` | `from_raw` preserves values; Images and Responses image-tool validation accept them | Named enum members were missing; unknown values were not rejected |
| New model names such as `gpt-6-astra` | Model IDs are transmitted as strings without a hard-coded allowlist | The SDK did not block the new model names |

These findings were checked against [cache diagnostics](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics), the [WebSocket event reference](https://developers.openai.com/api/reference/resources/responses/websocket-events), [reasoning updates within conversations](https://developers.openai.com/api/docs/guides/reasoning#change-reasoning-mid-conversation), and [image generation](https://developers.openai.com/api/docs/guides/image-generation). Passthrough tests exercised local types and serialization; they were not live service feature tests.

## Inventory comparisons without detected drift

These comparisons cover field-name or event-discriminator sets, not equality of every nested schema constraint:

| Inventory | Current documentation | Repository pin | Set difference |
|---|---:|---:|---|
| Responses create top-level request fields | 31 | 31 | None |
| Chat Completions create top-level request fields | 37 | 37 | None |
| Images generate top-level request fields | 14 | 14 | None |
| Files create top-level request fields | 3 | 3 | None |
| Responses SSE event discriminators | 58 | 58 | None |
| Realtime client events | 11 | 11 | None |
| Realtime server events | 46 | 46 | None |

Top-level counts alone were insufficient: changes such as function-tool `async` and cache diagnostics occurred within nested objects or existing response structures.

For lifecycle handling, the project places Evals behind explicit compatibility features and records a read-only date of 2026-10-31 and shutdown on 2026-11-30, matching the [deprecation documentation](https://developers.openai.com/api/docs/deprecations). Assistants had shut down, consistent with the project's intentional omission. Videos/Sora was explicitly out of scope and was not counted as a new defect.

## Verification and limits

- A separate Rust program referenced the workspace's types/client crates, reproduced the three behavior issues, and verified several passthrough cases. It completed successfully; output is retained in the evidence manifest.
- HTTP reproductions used only `127.0.0.1` and placeholder credentials, with no live model requests.
- The review produced this report and its evidence manifest. Subsequent fixes and validation for the four findings are recorded separately.
- The full test suite was not rerun during the read-only review. The preceding round's 1,238 passing tests did not prove compatibility with newly fetched documentation.
- No new complete immutable OpenAPI snapshot was obtained to replace or validate every node of the 1,424 older schemas. Conclusions are limited to the documented pages, inventories, and implementation areas listed here.
- Codex app-server and the direct subscription backend use contracts separate from the Platform API. Platform documentation was not used to infer their compatibility with newer versions.
