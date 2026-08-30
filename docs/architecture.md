# Architecture and trust boundaries

`openai-rs` separates wire types, Platform transport, optional protocol bridges,
and Codex integration so that enabling one capability does not broaden another
credential's authority.

```text
openai-rs-sdk
  |-- openai-rs-types
  |-- openai-rs-client       (optional Platform API transport)
  |-- openai-rs-rmcp         (optional MCP adapter)
  `-- openai-rs-codex        (optional experimental Codex integration)
```

## Platform API

The standard client is for documented OpenAI Platform operations. Its default
origin is `https://api.openai.com/v1`. API keys, workload-identity tokens, and
administration credentials are distinct capabilities; an ordinary client must
not gain administration methods merely because it can hold a bearer token.

Request serialization is operation-specific. User-controlled path parameters
are encoded as path segments, and redirect policy must not carry credentials to
another origin.

## Codex app-server

The app-server client lives in a separate crate and feature boundary. It speaks
the pinned app-server JSON-RPC protocol to an explicitly selected Codex runtime.
The runtime owns managed ChatGPT browser/device authentication. The integration
tracks the [official Codex app-server
documentation](https://learn.chatgpt.com/docs/app-server) and an exact audited
runtime artifact; a moving documentation page alone is not a schema pin.

The M0 repository does not yet ship an audited runtime-to-schema mapping.
Consequently, the app-server feature is a protocol/client scaffold rather than
an out-of-the-box supported runtime integration; unknown runtimes fail closed.

The client must complete the `initialize` request and then send exactly one
`initialized` notification before other methods. Runtime artifacts must map to
the checked-in schema by exact audited identity; a loose version range is not a
sufficient schema guarantee.

Codex access tokens are loaded by the runtime through `CODEX_ACCESS_TOKEN` or a
login previously created with `codex login --with-access-token`. They are not a
`personalAccessToken` branch of `account/login/start` and are not reusable as
app-server WebSocket transport tokens. See the [official access-token
guidance](https://learn.chatgpt.com/docs/enterprise/access-tokens).

## Direct Codex transport

The direct transport is explicitly experimental. It targets a private Codex
backend contract, not the public Platform OpenAPI contract. It must:

- expose only a sealed, fixture-backed operation allowlist;
- construct the exact host and path internally;
- reject redirects and unknown destinations before adding credentials;
- prevent callers from overriding credential, account, host, cookie, or
  originator headers;
- keep its credential types incompatible with the Platform client; and
- remain disabled by default and outside the `full` bundle.

It must never become a generic OpenAI-compatible proxy, credential forwarder,
account pool, or resale gateway.

## RMCP

The RMCP crate adapts explicitly registered typed tools. MCP server/handler code
does not receive ChatGPT or Codex credentials. OpenAI's remote MCP tool wire
types and a local RMCP client/server adapter remain separate concerns.

## Provenance and generation

The M0 OpenAPI input is vendored and hash-verified, while code generation has no
registered generators yet. Future generated API artifacts must be derived from
pinned, hashed upstream inputs. Normal builds and tests consume checked-in
artifacts; they do not download a moving schema or execute an unknown generator.
A refresh is an explicit maintainer operation whose source revision, hash, and
generated diff are reviewed.

## Excluded dependency and design source

The workspace has no `sub2api` dependency and does not invoke it at runtime.
`sub2api` is not a source for code, protocol constants, gateway behavior,
account management, billing, scheduling, or proxy design in this project.
