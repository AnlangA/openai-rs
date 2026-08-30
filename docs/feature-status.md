# Feature status

This document distinguishes Cargo feature selection from implemented API
coverage. A feature name in a manifest is not a stability or completeness
guarantee.

Status terms:

- **MVP**: part of the first tested Responses vertical slice.
- **Scaffold**: crate or feature boundary exists, but the public behavior is not
  complete.
- **Planned**: reserved for a later implementation milestone.
- **Experimental**: no compatibility or production-support promise.

## Facade features

| Feature | Default | Status | Notes |
|---|---:|---|---|
| `client` | Yes | MVP | Enables the Platform API client facade. Coverage includes Responses, Chat Completions, Files/Uploads, Batches, Vector Stores, Models, Embeddings, and Moderations. |
| `rustls-tls` | Yes | MVP | Rustls-backed Platform transport. Implies `client`. |
| `native-tls` | No | Scaffold | Native TLS transport selection. Implies `client`. |
| `structured-output` | Yes | MVP | Typed schema generation and strict-subset normalization are implemented; keyword/limit coverage remains pre-release. |
| `realtime` | No | MVP types | Complete pinned GA client/server event unions plus session, audio, transcription, client-secret, SDP/call, and SIP DTOs. The dedicated Realtime connection client remains in progress. |
| `webhook-verification` | No | MVP | Typed webhook verification with explicit secret handling. |
| `admin` | No | Planned | Administration operations require a separate credential and client boundary. |
| `workload-identity` | No | Planned | Platform workload identity only; not a Codex subscription credential. |
| `x509` | No | Planned | Extends workload identity with X.509/mTLS support. |
| `full` | No | Scaffold | Convenience bundle for the non-Codex client path. It intentionally excludes experimental Codex, admin, workload identity, X.509, and RMCP server features. |

## RMCP features

| Feature | Status | Notes |
|---|---|---|
| `rmcp` | MVP | Typed catalog, arguments, execution control, local dispatch, and lossless result encoding; no automatic exposure of paid OpenAI operations. |
| `rmcp-stdio` | Scaffold | Child-process stdio transport. |
| `rmcp-http-rustls` | Scaffold | Streamable HTTP client with rustls. |
| `rmcp-http-native-tls` | Scaffold | Streamable HTTP client with native TLS. |
| `rmcp-server` | Scaffold | Optional server adapter. |
| `rmcp-server-stdio` | Scaffold | Server stdio transport. |
| `rmcp-auth` | Scaffold | RMCP authentication support. |

OpenAI's remote MCP tool wire format and the local `rmcp` adapter are separate
contracts. Enabling the adapter does not turn arbitrary API resources into MCP
tools; applications must explicitly register or allowlist tools.

## Codex features

| Feature | Status | Notes |
|---|---|---|
| `codex-app-server` | Experimental | JSON-RPC client plus one audited mapping for Codex 0.144.5 on `aarch64-apple-darwin`. Only the exact executable and vendored schema hashes in the compatibility manifest are accepted; every other runtime fails closed. |
| `codex-access-token` | Experimental | Trusted local Business/Enterprise automation only. The runtime receives `CODEX_ACCESS_TOKEN` or a prior `codex login --with-access-token` login; this is not an `account/login/start` variant. |
| `experimental-codex-direct` | Experimental | Host-locked Responses create/SSE, browser PKCE, OIDC verification, singleflight refresh, and ephemeral credential storage for the private Codex backend. Not a public OpenAI API contract. |
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

- The REST MVP covers Responses create, retrieve, delete, cancel, compact,
  input-item listing, and input-token counting.
- Responses SSE create-stream, bounded decoding, and all 58 events in the
  pinned stable union are typed.
- Models list/retrieve/delete, Embeddings create, and Moderations create have
  typed client resource methods.
- Chat Completions includes non-streaming/SSE creation, stored completion
  resources, messages, and pagination.
- Files/Uploads includes replayable and one-shot multipart requests, streaming
  downloads, and upload-part/complete/cancel lifecycle methods.
- Batches includes JSONL workflow helpers; Vector Stores includes store, file,
  file-batch, pagination, search, and polling methods.
- Realtime has complete pinned 11-branch client and 46-branch server event
  unions and the related GA DTOs; its dedicated connection client is not yet
  complete.
- Additional operations are added only with typed request/response and error
  fixtures.
- Other resource families remain incomplete even if their feature boundary is
  already named.
- Unknown response fields and events are retained only where the wire contract
  is intentionally forward-compatible.
