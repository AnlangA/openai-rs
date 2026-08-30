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
Fine-tuning, Conversations, Containers, Skills, and Content Provenance
resources; Evals via default-off `legacy-evals`; and the pinned GA Realtime transport, including its 11 client-event
and 46 server-event unions, WebSocket connection, WebRTC signaling, client-secret
operations, and SIP call control. Administration, workload identity, and X.509
are implemented behind separate default-off trust boundaries. The repository
still does not claim a stable public API. Against the pinned OpenAPI inventory,
all 254 applicable client operations are verified; 33 sunset/deprecated
operations are explicitly omitted and one conflicting operation is
quarantined. The 18 webhook receiver operations are verified independently
of that client disposition.

| Area | Status |
|---|---|
| Lossless Serde primitives | Implemented; still pre-release |
| Typed Responses REST slice | Implemented; contract coverage is still growing |
| Responses SSE streaming | Public path and 58-event stable union implemented |
| Models, Embeddings, Moderations | Typed resource methods implemented |
| Chat Completions | Typed create/SSE, stored resources, messages, and pagination implemented |
| Files and Uploads | Typed replayable/one-shot multipart, download, and upload lifecycle implemented |
| Batches and Vector Stores | Typed resource methods, pagination/polling, and workflow helpers implemented |
| Media and Fine-tuning | Typed audio/image and fine-tuning job methods implemented |
| Conversations, Containers, Skills, Content Provenance | Typed resource, pagination, multipart/content-stream, and provenance-check methods implemented |
| Realtime GA | 11 client events, 46 server events, WebSocket transport, client secrets, WebRTC SDP, and SIP call control implemented |
| Administration | Separate `AdminClient`/`AdminApiKey`; 119 sealed operations plus 3 fine-tuning checkpoint-permission operations implemented |
| Workload identity and X.509 | RFC 8693-backed `Client` auth and an isolated mTLS `X509Client` implemented |
| Pinned operation disposition | 254 applicable client operations verified; 33 sunset/deprecated operations omitted; 1 conflicting operation quarantined; 18 webhook receivers verified |
| Default-off gated/compatibility APIs | Custom Voice, alpha Graders, beta ChatKit, beta multi-agent Responses, legacy Completions, legacy Evals (shutdown 2026-11-30), and legacy Realtime are implemented only behind explicit features |
| RMCP bridge | Typed local Responses-function bridge implemented; transport, server, and auth feature flags pass through to the pinned `rmcp` dependency |
| Codex app-server integration | Experimental JSONL client implemented for one exact audited Codex 0.144.5 macOS arm64 artifact |
| Direct Codex Responses transport | Private experimental host-locked create/SSE and browser auth implemented, with separately gated device auth; off by default |

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
| Gated and compatibility APIs | `custom-voice`, `alpha-graders`, `beta-chatkit`, `beta-responses-multi-agent`, `legacy-completions`, `legacy-evals`, `legacy-realtime` | Implemented, default-off, and explicitly access-controlled, alpha, beta, or legacy |
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

## Responses main path example

The typed tool-call and follow-up inference loop requires no raw JSON or manual schema construction:

```rust
use openai_rs::{
    ApiKey, Client,
    responses::{CreateResponseRequest, FunctionCallOutput, FunctionTool, ResponseInput},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
struct WeatherArgs {
    city: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct WeatherResult {
    city: String,
    temperature_c: i32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = ApiKey::new(std::env::var("OPENAI_API_KEY")?)?;
    let client = Client::new(api_key)?;

    let tool = FunctionTool::for_type::<WeatherArgs>("get_weather", "Return current weather")?;

    let request = CreateResponseRequest::new("gpt-5.4", "What is the weather in Shenzhen?")
        .with_tool(tool);

    let response = client.responses().create(request).await?;

    // Typed function call dispatch without raw JSON
    if let Some(call) = response.function_calls().next() {
        let args: WeatherArgs = call.arguments_as()?;
        let output = FunctionCallOutput::json(
            call.call_id(),
            &WeatherResult {
                city: args.city,
                temperature_c: 28,
            },
        )?;

        let mut follow_up_items = response.to_input_items();
        follow_up_items.push(output.into());

        let follow_up = client
            .responses()
            .create(CreateResponseRequest::new(
                "gpt-5.4",
                ResponseInput::items(follow_up_items),
            ))
            .await?;

        println!("{}", follow_up.output_text());
    } else {
        println!("{}", response.output_text());
    }

    Ok(())
}
```

The shape follows the [official OpenAI Responses
contract](https://developers.openai.com/api/reference/resources/responses/methods/create):
`POST /responses` accepts typed input and returns an ordered array of output
items. Do not assume that the first output item is always an assistant text
message. The same code is checked as the executable
[`responses` example](crates/openai-rs/examples/responses.rs).

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
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo check -p openai-rs-sdk --all-targets --no-default-features --locked
cargo check --workspace --all-targets --all-features --locked
cargo +1.88.0 check --workspace --all-targets --all-features --locked
cargo run --locked -p xtask -- check
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
