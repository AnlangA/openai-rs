# openai-rs

A typed Rust SDK for the OpenAI API, built around lossless wire types and the
Responses API.

> [!IMPORTANT]
> This repository is pre-release and under active staged development. It is not
> a complete OpenAI API implementation, has not made a stable API promise, and
> should not yet be used as a drop-in replacement for an official SDK. No
> crates.io release is documented or supported yet.

## Current status

The workspace, crate boundaries, feature flags, MSRV policy, and contract-test
infrastructure are present. The pre-release implementation includes typed
Responses REST, SSE, and persistent WebSocket paths; Chat Completions; Files,
Uploads, Batches, Vector Stores, Models, Embeddings, Moderations, media,
Fine-tuning, Evals, Conversations, Containers, Skills, and Content Provenance
resources; and the pinned GA Realtime transport, including its 11-client/
46-server event unions, WebSocket connection, WebRTC signaling, client-secret
operations, and SIP call control. Administration, workload identity, and X.509
are implemented behind separate default-off trust boundaries. The repository
still does not claim complete OpenAPI coverage or a stable public API.

| Area | Status |
|---|---|
| Lossless Serde primitives | Implemented for the MVP; still pre-release |
| Typed Responses REST slice | Implemented for the MVP; contract coverage is still growing |
| Responses SSE streaming | Public MVP path and 58-event stable union implemented |
| Models, Embeddings, Moderations | Typed MVP resource methods implemented |
| Chat Completions | Typed create/SSE, stored resources, messages, and pagination implemented |
| Files and Uploads | Typed replayable/one-shot multipart, download, and upload lifecycle implemented |
| Batches and Vector Stores | Typed resource methods, pagination/polling, and workflow helpers implemented |
| Media, Fine-tuning, and Evals | Typed audio/image, fine-tuning job, and eval/run methods implemented |
| Conversations, Containers, Skills, Content Provenance | Typed resource, pagination, multipart/content-stream, and provenance-check methods implemented |
| Realtime GA | 11 client events, 46 server events, WebSocket transport, client secrets, WebRTC SDP, and SIP call control implemented |
| Administration | Separate `AdminClient`/`AdminApiKey`; 119 sealed operations plus 3 fine-tuning checkpoint-permission operations implemented |
| Workload identity and X.509 | RFC 8693-backed `Client` auth and an isolated mTLS `X509Client` implemented |
| Full OpenAPI resource coverage | Not implemented |
| Default-off gated/compatibility APIs | Custom Voice, alpha Graders, beta ChatKit, beta multi-agent Responses, legacy Completions, and legacy Realtime are implemented only behind explicit features |
| RMCP bridge | Typed local Responses-function bridge implemented; transport, server, and auth feature flags pass through to the pinned `rmcp` dependency |
| Codex app-server integration | Experimental JSONL client implemented for one exact audited Codex 0.144.5 macOS arm64 artifact |
| Direct Codex Responses transport | Private experimental host-locked create/SSE and hardened browser/device auth implemented; off by default |

See [feature status](docs/feature-status.md) for the exact Cargo feature matrix
and [architecture boundaries](docs/architecture.md) for credential and protocol
separation.

## Design goals

- Preserve required, nullable, optional, and unknown wire values instead of
  collapsing them into a single Rust representation.
- Keep request and response models separate where their contracts differ.
- Make Responses non-streaming and streaming modes distinct at the type level.
- Retain forward-compatible unknown string values, response fields, and stream
  events where the protocol permits them.
- Keep Platform API, Codex subscription, and app-server transport credentials in
  separate types and trust boundaries.
- Generate and audit API bindings from pinned upstream artifacts rather than
  treating a moving network specification as a build input.

## Workspace

| Crate | Responsibility |
|---|---|
| `openai-rs-sdk` (`openai_rs`) | Public facade and feature selection |
| `openai-rs-types` | Wire types, IDs, presence semantics, and schema helpers |
| `openai-rs-client` | Platform API HTTP, SSE, retry, and resource facades |
| `openai-rs-rmcp` | Optional typed RMCP adapters |
| `openai-rs-codex` | Isolated experimental Codex integration |
| `openai-rs-contract-tests` | Cross-crate contract and compile-time tests |
| `xtask` | Pinned-artifact and repository consistency checks |

## Cargo features

Default features are `client`, `rustls-tls`, and `structured-output`.

| Feature group | Features | Boundary |
|---|---|---|
| Platform transport | `client`, `rustls-tls`, `native-tls` | Standard OpenAI Platform API client |
| Typed helpers | `structured-output`, `realtime`, `webhook-verification` | Structured output, GA Realtime transports, Responses WebSocket, and webhook verification |
| Privileged identity boundaries | `admin`, `workload-identity`, `x509` | Implemented and default-off; Administration and X.509 use dedicated client/credential boundaries |
| Gated and compatibility APIs | `custom-voice`, `alpha-graders`, `beta-chatkit`, `beta-responses-multi-agent`, `legacy-completions`, `legacy-realtime` | Implemented, default-off, and explicitly access-controlled, alpha, beta, or legacy |
| RMCP | `rmcp`, `rmcp-stdio`, `rmcp-http-rustls`, `rmcp-http-native-tls`, `rmcp-server`, `rmcp-server-stdio`, `rmcp-auth` | Local bridge implemented; transport/server/auth selections are upstream `rmcp` feature pass-throughs; no implicit tool exposure |
| Codex app-server | `codex-app-server`, `codex-access-token` | Experimental and isolated from Platform credentials |
| Direct Codex | `experimental-codex-direct`, `experimental-codex-direct-device`, `experimental-codex-direct-keyring` | Unstable private-backend compatibility; off by default; app-server is preferred |
| Convenience bundle | `full` | Exactly `client`, rustls TLS, structured output, Realtime, webhook verification, the RMCP client bridge, and RMCP HTTP/rustls support |

`full` intentionally excludes Custom Voice, every experimental, alpha, beta,
and legacy surface, Administration, workload identity, X.509, and the other
RMCP transport/server/auth selections. It is not "all features" and does not
imply complete endpoint coverage.

Enabling any feature only selects client-side code and dependencies. It never
grants an account, organization, project, service account, or model any
server-side permission, entitlement, or preview access. The authoritative
implementation status is maintained in
[docs/feature-status.md](docs/feature-status.md).

## Responses MVP example

The current minimal typed flow is:

```rust
use openai_rs::{responses::CreateResponseRequest, ApiKey, Client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;
    let request = CreateResponseRequest::new(
        "gpt-5.4",
        "Explain why typed API clients are useful in one sentence.",
    );

    let response = client.responses().create(request).await?;
    println!("{}", response.output_text());
    Ok(())
}
```

The shape follows the [official OpenAI Responses
contract](https://developers.openai.com/api/reference/resources/responses/methods/create):
`POST /responses` accepts typed input and returns an ordered array of output
items. Do not assume that the first output item is always an assistant text
message.

## Legacy Completions (opt-in)

The legacy text Completions endpoint is excluded by default. Enable it only for
an existing integration:

```toml
[dependencies]
openai-rs-sdk = { version = "0.1.0", features = ["legacy-completions"] }
```

```rust
use openai_rs::{legacy::CreateCompletionRequest, ApiKey, Client};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = Client::new(ApiKey::new("test-placeholder")?)?;
let request = CreateCompletionRequest::new("legacy-model", "Complete this sentence:");
let completion = client.completions().create(request).await?;
# let _ = completion;
# Ok(())
# }
```

This feature implements only fixed-route `POST /completions` JSON/SSE. It does
not revive Assistants, Threads, or Runs, and Responses remains the recommended
API for new code.

## Authentication boundaries

- Platform API operations use Platform credentials and target
  `https://api.openai.com/v1` by default.
- Codex app-server authentication is separate, optional, and experimental.
- A Codex access token is not a Platform API key and is not an app-server
  WebSocket transport token.
- The direct Codex feature targets a private, non-OpenAPI backend contract and
  remains host- and operation-locked, experimental, and off by default. Prefer
  the official app-server path when it meets the integration requirements.

This project has no `sub2api` dependency. It does not embed, invoke, proxy
through, or derive implementation code from `sub2api`; gateway, account-pool,
resale, and credential-forwarding designs are outside this repository's scope.

## Development

The workspace MSRV is Rust 1.88.0. Start with:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check -p openai-rs-sdk --all-targets --no-default-features
cargo check --workspace --all-targets --all-features
cargo +1.88.0 check --workspace --all-targets
cargo run -p xtask -- check
cargo deny --all-features check
```

See [development.md](docs/development.md) for the full local quality contract.

## Security

Do not report credential leaks or exploitable auth/transport issues in a public
issue. Follow [SECURITY.md](SECURITY.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
