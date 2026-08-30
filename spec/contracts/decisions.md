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

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `downloadFile`, override `OVR-0001`
- Sources: raw OpenAPI is captured at the pinned SHA but declares an incompatible
  JSON string. A synthetic binary fixture is checked in at
  `testdata/fixtures/files/downloadFile/response.bin`.
- Decision: decode the successful body as raw bytes with
  `application/octet-stream`, not JSON.
- Impact: response codec and generated operation contract.
- Tests: generated contract asserts `response.mode = raw`;
  `openai-rs-client::multipart::tests::raw_download_stream_is_not_json_decoded`.

## D0003 — CreateFile expiration uses bracketed multipart keys

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `createFile`, override `OVR-0002`
- Sources: official SDK runtime serialization category; loopback capture in
  `openai-rs-client` multipart tests.
- Decision: encode `expires_after[anchor]` and `expires_after[seconds]` as
  multipart field names.
- Impact: multipart request encoder, not the JSON schema shape.
- Tests: `openai-rs-client::multipart::tests::create_file_sends_bracket_fields_and_raw_bytes`.

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

- Status: accepted (synthetic fixture accepted for local verification; does not block release)
- Reviewed: 2026-08-30
- Scope: `OpenAIFile.required`, override `OVR-0005`
- Sources: pinned raw OpenAPI and official SDK transformed contract require the
  field; the official example omits it.
- Decision: keep `status` required for output decoding while recording the
  example discrepancy.
- Impact: File resource requiredness and fixture quarantine.
- Tests: capture or synthetic File response containing `status`.

## D0007 — Empty File lists provisionally require cursor ids

- Status: accepted (synthetic fixture accepted for local verification; does not block release)
- Reviewed: 2026-08-30
- Scope: `ListFilesResponse.required`, override `OVR-0006`
- Sources: pinned raw OpenAPI requires `first_id` and `last_id`; no audited empty
  response fixture is frozen yet.
- Decision: retain raw requiredness until an empty-list fixture proves absent or
  nullable behavior.
- Impact: File pagination response DTO.
- Tests: frozen empty `listFiles` response fixture.

## D0008 — MCPApprovalResponse ghost request_id is directional

- Status: accepted (synthetic fixture accepted for local verification; does not block release)
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

## D0009 — Batch lifecycle fields accept explicit null

- Status: accepted; live response fixture still requested
- Reviewed: 2026-08-30
- Scope: `Batch.errors`, `output_file_id`, `error_file_id`, and optional
  lifecycle timestamp fields
- Sources: the pinned raw OpenAPI and pinned official SDK types describe these
  fields as optional but non-null; an official Batch example renders some of
  them as explicit `null`.
- Decision: model `errors`, output/error file ids, and optional lifecycle
  timestamps as `Omittable<Nullable<T>>`. This preserves all three states
  instead of using `Option<T>` and matches the official example's explicit-null
  behavior while retaining missing-field compatibility.
- Impact: Batch response decoding and fixture quarantine.
- Overrides: directional handwritten DTO override pending a generated override id.
- Tests: missing/null/value lifecycle tests; still add a captured Batch response
  to confirm every affected field.

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

## D0011 — Vector Store search query forms are directional unions

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `VectorStoreSearchRequest.query` and
  `VectorStoreSearchResultsPage.search_query`
- Sources: the pinned component/official SDK schemas permit scalar-or-array
  request input and type the returned `search_query` as an array, while the
  endpoint example returns a scalar string. Older Vector Store endpoint examples
  are therefore treated as stale evidence rather than silently overriding the
  current component contract.
- Decision: expose lossless directional unions for scalar and array forms in
  both positions. Do not normalize a one-element array into a string, or a
  scalar response into an array, during roundtrip.
- Impact: Vector Store search request and response DTOs/builders; legacy example
  fixtures remain quarantined.
- Overrides: none.
- Tests: request and response scalar/array typed and semantic JSON roundtrips,
  including a known-array malformed-payload rejection.

## D0012 — Realtime session events follow the pinned OpenAPI response shape

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Realtime session-created/session-updated server event payloads
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9`;
  pinned Node/Python SDK aliases expose a request-shaped session type in some
  generated event declarations.
- Decision: decode the server event with the OpenAPI response-shaped session,
  including its required id/object fields. Do not accept the SDK alias as proof
  that a request-shaped object is valid server output.
- Impact: Realtime server-event DTO requiredness.
- Overrides: none.
- Tests: 46-branch server-event manifest test plus known-malformed session-event
  fixture.

## D0013 — Sunset, deprecated, and conflicting operations are not callable

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Assistants/Threads/Runs, Videos, and `createImageVariation`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Assistants migration
  guidance and deprecated Assistants/Videos reference pages reviewed on the
  stated date; the conflicting DALL-E 2 removal guidance and image-variation
  reference page.
- Decision: record all 33 Assistants/Threads/Runs and Videos operations as
  `omitted`, and record `createImageVariation` as `quarantined`. Preserve them
  in the generated inventory, but do not generate public request types, client
  methods, or a Cargo feature that makes them callable.
- Reason: the Assistants family has reached its announced sunset, Videos has a
  dated shutdown path, and the image-variation sources conflict. A complete
  inventory must not be confused with an unsafe promise that every historical
  path remains callable.
- Impact: operation disposition and coverage accounting only; the supported
  typed client surface remains unchanged.
- Overrides: none.
- Tests: lifecycle/feature classification unit test, implementation-status
  parser unit test, generated operation inventory zero-diff check, and absence
  from all facade feature bundles.

## D0014 — GA Responses shares Chat/Beta prompt-cache and reasoning wire types

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `PromptCacheBreakpoint`, `PromptCacheOptions`, `PromptCacheMode`,
  `PromptCacheTtl`, `Reasoning`, `ContextManagementParam`, `ModerationParam`,
  and `Annotation`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Responses create,
  prompt-caching, and reasoning guides reviewed on the stated date.
- Decision: GA Responses handwritten DTOs are the single source for these
  shared schemas. Chat and Beta re-export the same types. Schema-IR now
  projects the named schemas and asserts IR enum values ⊆ handwritten
  `open_string_enum` known variants. `prompt_cache_retention` remains as a
  deprecated sibling of `prompt_cache_options.ttl`.
- Reason: the previous GA copies serialized `{ "type": "explicit" }` and
  invented `auto`/`disabled` cache modes that are not in the pin.
- Impact: request/response JSON for Responses, Chat, and Beta; schema-IR
  selection; drift no longer treats `/evals` or `/fine_tuning` as omitted
  families.
- Overrides: none.
- Tests: Responses/Chat wire fixtures, xtask schema-IR enum-subset test, and
  `cargo run --locked -p xtask -- check`.
