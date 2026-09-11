# Changelog

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
