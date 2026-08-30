# Feature status

This document distinguishes Cargo feature selection from implemented API
coverage. A feature name in a manifest is not a stability or completeness
guarantee.

Enabling a feature compiles client-side code and dependencies only. It does not
provision credentials or grant an account, organization, project, service
account, model, or checkpoint any server-side permission, entitlement, role,
or preview access. An implemented access-controlled operation can therefore
return an authorization or eligibility error from the service.

Status terms:

- **Implemented**: a public, typed implementation and contract tests exist;
  this is not a stable-API or complete-family promise.
- **Alpha**, **Beta**, and **Legacy**: implemented but deliberately kept behind
  a maturity-specific, default-off boundary.
- **Experimental**: implemented without a compatibility or production-support
  promise.
- **Pass-through**: selects a capability of the pinned upstream dependency; the
  SDK does not add its own lifecycle wrapper for it.

## Facade features

| Feature | Default | Status | Notes |
|---|---:|---|---|
| `client` | Yes | Implemented | Enables the Platform `Client`. Coverage includes Responses, Chat Completions, Files/Uploads, Batches, Vector Stores, Models, Embeddings, Moderations, media, Fine-tuning, Evals, Conversations, Containers, Skills, and Content Provenance. |
| `rustls-tls` | Yes | Implemented | Rustls-backed Platform transport. Implies `client`. |
| `native-tls` | No | Implemented | Native TLS transport selection. Implies `client`. |
| `structured-output` | Yes | Implemented | Typed schema generation and strict-subset normalization; keyword/limit coverage remains pre-release. |
| `realtime` | No | Implemented | Pinned GA 11-client/46-server event unions, Realtime WebSocket connection, client-secret and translation-secret REST methods, WebRTC SDP signaling, SIP call control, and the persistent Responses WebSocket client. |
| `webhook-verification` | No | Implemented | Typed verification for the pinned 18-event webhook union with explicit secret handling. |
| `admin` | No | Implemented | Dedicated `AdminApiKey`/`AdminClient`, sealed typed requests for the 119-operation Administration manifest, convenience resource facades, and three fine-tuning checkpoint-permission methods. Never added to the ordinary `Client`. |
| `workload-identity` | No | Implemented | RFC 8693 subject-token exchange for the ordinary Platform `Client`, with token caching, singleflight/proactive refresh, and bounded one-time 401 replay where the request is replayable. Not a Codex subscription credential. |
| `x509` | No | Implemented | Isolated rustls mTLS `X509Client` with pinned regional origins, X.509 token exchange, non-streaming Responses create/retrieve/cancel/compact/count, and Models list/retrieve. It exposes neither Realtime nor arbitrary URLs. |
| `custom-voice` | No | Implemented | Six typed Custom Voice/consent operations. Access remains controlled by the service. |
| `alpha-graders` | No | Alpha | Two typed alpha Graders operations under the explicit unstable facade. |
| `beta-chatkit` | No | Beta | Six typed ChatKit session/thread operations and pagination. |
| `beta-responses-multi-agent` | No | Beta | Seven typed beta Responses operations, SSE, and persistent WebSocket create/inject support for the multi-agent contract. |
| `legacy-completions` | No | Legacy | Default-off typed JSON/SSE support for only `POST /completions`; Responses is preferred. |
| `legacy-realtime` | No | Legacy | Two deprecated pre-GA Realtime session-token operations. Use the GA `realtime` feature for new integrations. |
| `full` | No | Bundle | Exactly `client`, `rustls-tls`, `structured-output`, `realtime`, `webhook-verification`, `rmcp`, and `rmcp-http-rustls`. It excludes Custom Voice, all experimental/alpha/beta/legacy features, Administration, workload identity, X.509, and the remaining RMCP transport/server/auth features. |

The Administration inventory consists of 119 sealed manifest operations plus a
separate three-operation checkpoint-permission manifest (list, create, and
delete). Both execute only through `AdminClient` with `AdminApiKey`; neither is
reachable through the ordinary Platform `Client`.

## RMCP features

| Feature | Status | Notes |
|---|---|---|
| `rmcp` | Implemented | Typed tool catalog/name/schema adaptation, argument validation, execution control, cancellable `RmcpExecutor` dispatch through a caller-supplied initialized `ServerSink`, and lossless result encoding. |
| `rmcp-stdio` | Pass-through | Enables the pinned `rmcp` crate's child-process stdio client transport. |
| `rmcp-http-rustls` | Pass-through | Enables the pinned `rmcp` crate's streamable HTTP client with rustls. |
| `rmcp-http-native-tls` | Pass-through | Enables the pinned `rmcp` crate's streamable HTTP client with native TLS. |
| `rmcp-server` | Pass-through | Enables the pinned `rmcp` crate's server APIs; `openai-rs-rmcp` does not define an SDK-specific server adapter. |
| `rmcp-server-stdio` | Pass-through | Enables the pinned `rmcp` crate's server stdio transport. |
| `rmcp-auth` | Pass-through | Enables the pinned `rmcp` crate's authentication support. |

OpenAI's remote MCP tool wire format and the local `rmcp` adapter are separate
contracts. The SDK-authored transport integration starts at an already
initialized RMCP peer; applications set up and authenticate stdio/HTTP/server
lifecycles with the pinned upstream crate. Enabling the adapter does not turn
arbitrary API resources into MCP tools; applications must explicitly register
or allowlist tools.

## Codex features

| Feature | Status | Notes |
|---|---|---|
| `codex-app-server` | Experimental | Implemented bounded JSONL/JSON-RPC client with initialize/initialized handshake, typed account/login/rate-limit/usage, thread-start, turn-start/interrupt methods, inbound request replies, and known/unknown notification handling. Only the exact audited Codex 0.144.5 `aarch64-apple-darwin` executable/schema mapping is accepted; every other runtime fails closed. |
| `codex-access-token` | Experimental | Trusted local Business/Enterprise automation only. The runtime receives `CODEX_ACCESS_TOKEN` or a prior `codex login --with-access-token` login; this is not an `account/login/start` variant. |
| `experimental-codex-direct` | Experimental | Implemented host-locked Responses create/SSE, browser PKCE, OIDC verification, singleflight refresh, and ephemeral credential storage for the private Codex backend. It is not a public OpenAI API contract or a general proxy. |
| `experimental-codex-direct-device` | Experimental | Adds bounded device-code authentication to the direct backend. |
| `experimental-codex-direct-keyring` | Experimental | Adds explicit OS keyring persistence; the default direct store remains ephemeral. |

The default and `full` feature sets do not enable Codex support. Codex
credentials must never be accepted by the standard Platform `Client`, and
app-server transport authentication must remain separate from model
credentials. Direct mode is off by default; prefer app-server whenever its
runtime boundary fits the application.

## API coverage policy

The repository does not claim full OpenAPI coverage until generated operation
inventory, public bindings, fixtures, and contract tests agree with a pinned
specification. Until then:

- Responses REST covers create, retrieve, delete, cancel, compact, input-item
  listing, and input-token counting.
- Responses SSE create-stream, bounded decoding, and all 58 events in the
  pinned stable union are typed.
- Enabling `realtime` also provides the persistent typed Responses WebSocket
  transport.
- Models list/retrieve/delete, Embeddings create, and Moderations create have
  typed client resource methods.
- Chat Completions includes non-streaming/SSE creation, stored completion
  resources, messages, and pagination.
- Files/Uploads includes replayable and one-shot multipart requests, streaming
  downloads, and upload-part/complete/cancel lifecycle methods.
- Batches includes JSONL workflow helpers; Vector Stores includes store, file,
  file-batch, pagination, search, and polling methods.
- Media includes typed speech, transcription, translation, image generation,
  image editing, raw-byte/text/SSE modes, and bounded streams.
- Fine-tuning includes job create/retrieve/list/cancel/pause/resume, event and
  checkpoint listing, pagination, and bounded polling. Checkpoint access
  permissions remain Administration-only through `AdminClient`.
- Evals includes typed eval/run/output-item operations, pagination, and bounded
  run polling.
- Conversations includes conversation and item CRUD/list operations;
  Containers includes container/file CRUD, attach/upload, pagination, and
  streamed content.
- Skills includes skill/version lifecycle, directory or zip upload, pagination,
  and streamed content; Content Provenance includes the typed multipart check.
- Realtime includes the pinned 11-branch client and 46-branch server event
  unions, WebSocket transport, client secrets, WebRTC SDP signaling, and SIP
  accept/reject/hangup/refer call control.
- Administration has its own 119-operation sealed manifest plus the three
  checkpoint-permission operations, all behind a distinct client and
  credential type.
- Workload identity and X.509 authentication paths are implemented with the
  narrower boundaries described above.
- Additional operations are added only with typed request/response and error
  fixtures.
- Other resource families or operations remain incomplete even if related
  types or a feature boundary already exist.
- Unknown response fields and events are retained only where the wire contract
  is intentionally forward-compatible.
