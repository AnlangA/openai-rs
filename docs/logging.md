# Logging

The SDK emits local `tracing` spans and events. Applications choose filtering,
formatting, and destinations; the library never installs a global subscriber.
All repository examples initialize a subscriber and read `RUST_LOG`, defaulting
to `warn` when unset. No separate SDK logging variable or builder flag is needed.

## Terminal examples

```powershell
# HTTP results and warnings/errors
$env:RUST_LOG = "openai_rs_client=info"
cargo run -p openai-rs-sdk --example chat_loop

# Requests, timeout budgets and response sizes
$env:RUST_LOG = "openai_rs_client=debug"

# Include raw JSON bodies, even if typed response decoding fails
$env:RUST_LOG = "openai_rs_client=trace"
cargo run -p openai-rs-sdk --example chat_loop 2> chat-json.log

# Disable all tracing events
$env:RUST_LOG = "off"
```

Restart the process after changing `RUST_LOG`. The example's prompts and replies
use stdout; tracing diagnostics use stderr. User-facing input/request error
messages still appear when tracing is off.

| Level | What it records |
|---|---|
| `error` | Terminal HTTP rejection with operation/status/request ID/retries/elapsed time; transport and body read failure flags; JSON error category, field path and line/column |
| `warn` | Retry reason, count and delay; status/request ID for HTTP retries; request deadline exhaustion |
| `info` | Accepted HTTP response headers with operation/status/request ID/retries/elapsed time |
| `debug` | Request method/route template in spans, sending attempts with remaining timeout, buffered body byte count, authentication invalidation |
| `trace` | Serialized JSON requests and raw JSON responses, including errors before decoding and intermediate retry bodies in the Platform JSON transport |

`info` measures time until response headers. It does not measure full body
download, model stream completion, or successful typed JSON decoding. A 200
response can therefore produce an INFO header event followed by an ERROR decode
event. JSON errors include the request ID if the server provides one.

`error` through `debug` exclude JSON bodies. Raw body events use the separate
`openai_rs_client::http_body` target, so filters can be combined:

```powershell
# SDK metadata plus only its raw body events
$env:RUST_LOG = "openai_rs_client=debug,openai_rs_client::http_body=trace"
# Keep body diagnostics off while tracing other client internals
$env:RUST_LOG = "openai_rs_client=trace,openai_rs_client::http_body=off"
```

HTTP result metadata covers the normal JSON, multipart/media/download,
Administration, Realtime call signaling, workload identity and X.509 paths.
Raw body tracing is limited to the Platform JSON transport, including the JSON
request that starts an SSE stream. It excludes SSE/WebSocket frames, multipart
uploads, binary downloads, Administration bodies and authentication exchanges.
JSON size limits remain in force; truncated HTTP error bodies have
`body.truncated=true`. Bodies rejected by the success-size limit are not dumped.

Authentication headers are never logged. Raw JSON is not redacted: it can
contain conversations and any application secrets explicitly placed in a body.
Inspect logs before sharing them. No credential is needed to enable logging.

## Your own application

Add a subscriber alongside the SDK (or configure the subscriber your application
already uses):

```toml
[dependencies]
openai-rs-sdk = "0.1.2"
tracing-subscriber = { version = "0.3", default-features = false, features = ["env-filter", "fmt"] }
```

Initialize once at application startup:

```rust
fn init_logging() -> std::io::Result<()> {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::WARN.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(std::io::Error::other)
}
```

`RUST_LOG` uses the Rust module name `openai_rs_client` with underscores, not
the Cargo package name `openai-rs-client`. `RUST_LOG=trace` also works but enables
other dependencies' trace events. See the upstream
[EnvFilter documentation](https://docs.rs/tracing-subscriber/0.3.23/tracing_subscriber/filter/struct.EnvFilter.html)
for directive syntax. The expanded events described here require the current
source checkout; they are not included in the previously published 0.1.2 crates.
