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

## D0002 — File download is raw binary

- Status: accepted; wire fixture pending
- Reviewed: 2026-08-30
- Scope: `downloadFile`, override `OVR-0001`
- Sources: raw OpenAPI is captured at the pinned SHA but declares an incompatible
  JSON string; exact official-reference Markdown and binary wire fixture remain
  pending.
- Decision: decode the successful body as raw bytes with
  `application/octet-stream`, not JSON.
- Impact: response codec and generated operation contract.
- Tests: generated contract asserts `response.mode = raw`; add a binary fixture.

## D0003 — CreateFile expiration uses bracketed multipart keys

- Status: accepted; SDK capture pending
- Reviewed: 2026-08-30
- Scope: `createFile`, override `OVR-0002`
- Sources: official SDK runtime serialization category; exact request capture is
  pending.
- Decision: encode `expires_after[anchor]` and `expires_after[seconds]` as
  multipart field names. The client multipart encoder is deferred to its own
  implementation milestone.
- Impact: multipart request encoder, not the JSON schema shape.
- Tests: capture both bracketed parts before marking the override complete.

## D0004 — Upload.object is required

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `Upload.required`, override `OVR-0003`
- Sources: official SDK transformed contract at Node commit
  `eea2292a4a523da9405161dde0a79ac5dc2ecb2a`, artifact SHA-256
  `1a9e90cd0c3b98cec8fec7d12b7aaeaa5e4d5110a0bd3f6456a6958a08127430`.
- Decision: require the constant discriminator `object = "upload"`.
- Impact: directional Upload DTO requiredness.
- Tests: output fixture must contain `object`.

## D0005 — CreateUpload uses the six-value File purpose domain

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateUploadRequest.purpose`, override `OVR-0004`
- Sources: same pinned official SDK transformed contract as D0004.
- Decision: accept `assistants`, `batch`, `fine-tune`, `vision`, `user_data`, and
  `evals`.
- Impact: request enum and validation.
- Tests: cover `user_data` and `evals` in addition to the four raw-spec values.

## D0006 — File.status remains required

- Status: provisional; official example fixture pending
- Reviewed: 2026-08-30
- Scope: `OpenAIFile.required`, override `OVR-0005`
- Sources: pinned raw OpenAPI and official SDK transformed contract require the
  field; the official example omits it.
- Decision: keep `status` required for output decoding while recording the
  example discrepancy.
- Impact: File resource requiredness and fixture quarantine.
- Tests: capture a current File response containing `status`.

## D0007 — Empty File lists provisionally require cursor ids

- Status: provisional; empty-list fixture pending
- Reviewed: 2026-08-30
- Scope: `ListFilesResponse.required`, override `OVR-0006`
- Sources: pinned raw OpenAPI requires `first_id` and `last_id`; no audited empty
  response fixture is frozen yet.
- Decision: retain raw requiredness until an empty-list fixture proves absent or
  nullable behavior.
- Impact: File pagination response DTO.
- Tests: freeze an empty `listFiles` response.

## D0008 — MCPApprovalResponse ghost request_id is directional

- Status: quarantined; bidirectional fixtures pending
- Reviewed: 2026-08-30
- Scope: `MCPApprovalResponse.required`, override `OVR-0007`
- Sources: pinned raw OpenAPI requires `request_id` although that name is absent
  from `properties`; current wire behavior still needs fixtures.
- Decision: input DTOs do not fabricate or require the undefined field. Output
  resources keep `request_id` required and preserve it in the anomaly manifest
  until fixtures resolve the conflict.
- Impact: request/output DTO split and schema anomaly tracking.
- Tests: input compile/serde test, output requiredness test, then input/output wire
  fixtures.

## D0009 — Batch nullable examples remain quarantined

- Status: provisional; live response fixture pending
- Reviewed: 2026-08-30
- Scope: `Batch.errors` and optional lifecycle timestamp fields
- Sources: the pinned raw OpenAPI and pinned official SDK types describe these
  fields as optional but non-null; an official Batch example renders some of
  them as explicit `null`.
- Decision: retain the stricter machine-contract interpretation for now. Do not
  silently widen every optional timestamp/error field to nullable from an
  example alone.
- Impact: Batch response decoding and fixture quarantine.
- Overrides: none until a current wire fixture establishes service behavior.
- Tests: current strict missing/non-null tests; add a captured Batch response
  before accepting or rejecting a nullability override.

## D0010 — Vector-store metadata example omission remains quarantined

- Status: provisional; live response fixture pending
- Reviewed: 2026-08-30
- Scope: `VectorStoreObject.metadata`
- Sources: the pinned raw OpenAPI and official SDK types require the nullable
  field, while an official example omits it entirely.
- Decision: keep `metadata` required-nullable so missing and explicit null are
  not collapsed. Do not relax requiredness without a current service fixture.
- Impact: Vector Store response decoding and fixture quarantine.
- Overrides: none.
- Tests: required-nullable unit test; add a captured empty-metadata Vector Store
  response before changing the contract.
