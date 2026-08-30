# openai-rs

A typed Rust SDK for the OpenAI API, built around lossless wire types and the
Responses API.

> [!IMPORTANT]
> This repository is pre-release and under active M0/MVP development. It is not
> a complete OpenAI API implementation, has not made a stable API promise, and
> should not yet be used as a drop-in replacement for an official SDK. No
> crates.io release is documented or supported yet.

## Current status

The workspace, crate boundaries, feature flags, MSRV policy, and initial
contract-test infrastructure are present. The pre-release MVP includes typed
Responses create, retrieve, delete, cancel, compact, input-item listing, and
input-token counting operations and a public SSE create-stream path. Complete
stable event coverage, broader resource families, and most optional features
remain in progress, scaffolded, or planned.

| Area | Status |
|---|---|
| Lossless Serde primitives | Implemented for the MVP; still pre-release |
| Typed Responses REST slice | Implemented for the MVP; contract coverage is still growing |
| Responses SSE streaming | Public MVP path implemented; full event coverage is incomplete |
| Full OpenAPI resource coverage | Not implemented |
| Realtime, administration, and webhook helpers | Feature boundaries reserved; not complete |
| RMCP bridge | Optional scaffold; not production-ready |
| Codex app-server integration | Experimental; only one exact Codex 0.144.5 macOS arm64 artifact is audited |
| Direct Codex Responses transport | Experimental, private-backend compatibility work only |

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
| Typed helpers | `structured-output`, `realtime`, `webhook-verification`, `admin`, `workload-identity`, `x509` | Several are reserved for later milestones |
| RMCP | `rmcp`, `rmcp-stdio`, `rmcp-http-rustls`, `rmcp-http-native-tls`, `rmcp-server`, `rmcp-server-stdio`, `rmcp-auth` | Optional; no implicit tool exposure |
| Codex app-server | `codex-app-server`, `codex-access-token` | Experimental and isolated from Platform credentials |
| Direct Codex | `experimental-codex-direct`, `experimental-codex-direct-device` | Unstable private-backend compatibility; off by default |
| Convenience bundle | `full` | Non-Codex client bundle; it is not every feature and does not imply complete endpoint coverage |

Enabling a feature only selects code and dependencies. It does not by itself
mean that every API operation in that area is implemented. The authoritative
status is maintained in [docs/feature-status.md](docs/feature-status.md).

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

## Authentication boundaries

- Platform API operations use Platform credentials and target
  `https://api.openai.com/v1` by default.
- Codex app-server authentication is separate, optional, and experimental.
- A Codex access token is not a Platform API key and is not an app-server
  WebSocket transport token.
- The direct Codex feature targets a private, non-OpenAPI backend contract and
  must remain host- and operation-locked.

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
