# Changelog

## 0.2.1 - 2026-09-20

All five published crates use version 0.2.1: `openai-rs-sdk`, `openai-rs-types`,
`openai-rs-client`, `openai-rs-codex`, and `openai-rs-rmcp`. The minimum supported
Rust version remains 1.88.0.

### Migration notes

- `ResponseUsage::input_tokens_details()` and `output_tokens_details()` now
  return `Option<&InputTokensDetails>` and `Option<&OutputTokensDetails>`.
  Handle `None` as an unreported breakdown rather than an explicit zero.
- `CreateProjectServiceAccountApiKeyBody` adds the public `expires_in_seconds`
  field. Callers constructing the full struct directly must set that field or
  use `..Default::default()`.

```toml
[dependencies]
openai-rs-sdk = "0.2.1"
```

### Responses provider compatibility

- Accept GLM Responses that omit token breakdowns, error/incomplete details,
  and echoed request settings. Preserve omissions during serialization while
  retaining validation of required core fields and malformed reported values.
- Preserve StepFun reasoning `status: null` and output-text `logprobs: null`
  through decoding and conversation-history replay. Existing constructor and
  accessor signatures and missing-logprobs defaults are unchanged.
- Add full-response and two-turn HTTP regression coverage alongside the
  existing DeepSeek compatibility tests.

### API additions

- Add ten Videos operations behind the opt-in `legacy-videos` feature,
  including multipart uploads, JSON asset references, and binary downloads.
- Support service-account key expiry parameters and typed expiration timestamps.
- Expose Responses prompt-cache comparison ids, cache diagnostics, safety
  details, and async helpers for function/custom tools and calls, including
  applicable beta Responses and Conversation fields.
- Add the Safety Alerts convenience accessor and expose the alert object tag.

### Reliability and contracts

- Deliver complete SSE events before reporting malformed data later in the
  same HTTP chunk, and stop direct Codex SSE parsing immediately after `[DONE]`.
- Keep workload-identity credential acquisition within request deadlines and
  release refresh slots after provider or cleanup panics.
- Keep frozen operation inventories separate from documented additions,
  reject duplicate routes, and fingerprint the actual contract input bytes.

## 0.2.0 - 2026-09-11

All five published workspace crates use version 0.2.0:
`openai-rs-sdk`, `openai-rs-types`, `openai-rs-client`, `openai-rs-codex`, and
`openai-rs-rmcp`. The minimum supported Rust version remains 1.88.0.

### Breaking change

`InputTokensDetails::cache_write_tokens()` now returns `Option<u64>` instead
of `u64`. Compatible Responses providers such as DeepSeek may omit this field;
omission is retained as `None`, while an explicit zero remains `Some(0)`.
The original response fields survive serialization round trips.

Update the dependency version explicitly when migrating from 0.1.x:

```toml
[dependencies]
openai-rs-sdk = "0.2.0"
```

Applications reading the cache-write counter must handle `None` as an
unreported count rather than assuming that the provider reported zero.

### Diagnostics and examples

- Add graded HTTP diagnostics for request outcomes, retries, timing, transport
  errors, and JSON decoding positions.
- Add raw request/response JSON events at the `openai_rs_client::http_body`
  TRACE target, including responses that fail typed decoding. Raw bodies may
  contain conversation data; authentication headers are excluded.
- Initialize standard `RUST_LOG` subscribers in every example and use English
  terminal prompts and diagnostics.

### Documentation

- Translate project text, fixtures, and review reports to English while
  preserving UTF-8 test coverage.
- Document public APIs, field semantics, error and panic conditions, and crate
  examples using Rustdoc conventions.
- Enforce documentation lints and repair links for builds without default
  features.

See the [logging guide](docs/logging.md) for configuration and the
[development guide](docs/development.md) for validation commands.
