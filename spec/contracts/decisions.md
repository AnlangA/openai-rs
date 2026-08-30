# Contract Decision Ledger

This ledger records reviewed differences between the OpenAI API reference,
the pinned `openai/openai-openapi` snapshot, official SDK evidence, and the
manual override layer. A live upstream change does not alter generated code
until a decision is recorded here and its fixtures pass.

## D0001 — Initial OpenAPI baseline

- Status: accepted
- Reviewed: 2026-08-30
- Scope: complete machine-readable API contract
- Decision: pin `openai/openai-openapi` commit
  `690521b1753dce0c6d6b275f583d22537679cff9` and require the byte hash and
  inventory recorded in `spec/SOURCES.toml`.
- Reason: generation and ordinary builds must remain offline and reproducible.
- Tests: `cargo run -p xtask -- spec verify` and
  `cargo run -p xtask -- codegen --check`.

## Decision template

- Status: proposed | accepted | superseded | rejected
- Reviewed: YYYY-MM-DD
- Scope: JSON Pointer, operation id, schema, or runtime behavior
- Sources: immutable official URLs and captured evidence hashes
- Decision: exact behavior selected
- Reason: why higher-priority sources do not resolve the conflict
- Impact: generated/runtime surface affected
- Overrides: override ids, or `none`
- Tests: fixtures and contract tests that enforce the decision
