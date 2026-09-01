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
- Tests: `openai-rs-types::files::tests::retrieve_file_fixture_round_trips_the_pin_required_status`
  (include_str wiring of the frozen synthetic fixture, round-8 item 8-01) and
  `openai-rs-types::files::tests::file_status_is_required_even_though_deprecated`.

## D0007 — Empty File lists provisionally require cursor ids

- Status: accepted (synthetic fixture accepted for local verification; does not block release)
- Reviewed: 2026-08-30
- Scope: `ListFilesResponse.required`, override `OVR-0006`
- Sources: pinned raw OpenAPI requires `first_id` and `last_id`; no audited empty
  response fixture is frozen yet.
- Decision: retain raw requiredness until an empty-list fixture proves absent or
  nullable behavior.
- Impact: File pagination response DTO.
- Tests: `openai-rs-types::files::tests::empty_list_files_fixture_pins_required_cursor_ids`
  (include_str wiring of the frozen empty `listFiles` fixture — decodes the
  `"file_none"` sentinels and fails on a dropped id — round-8 item 8-01).

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
- Tests: input/output wire fixtures wired by round-8 item 8-01:
  `openai-rs-types::responses::tests::mcp_approval_output_fixture_preserves_the_ghost_request_id`,
  `openai-rs-types::responses::tests::mcp_approval_output_fixture_without_request_id_round_trips_both_shapes`,
  plus the existing `remaining_item_fields_match_python_and_openapi_inventory`
  and `local_shell_output_and_mcp_approval_decode_without_ghost_fields`.

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

## D0015 — Responses fields and limits match pinned OpenAPI and Python SDK

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateResponse` / `Response` request-response field inventory, numeric
  and metadata limits, Structured Outputs strict-subset keywords
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`CreateResponse`,
  `CreateModelResponseProperties`, `ModelResponseProperties`,
  `ResponseProperties`, `Response`, `ServiceTierResponses`, `Metadata`);
  official Python SDK `response_create_params.py` and `response.py` on
  `openai/openai-python` main as reviewed on the stated date; official
  Structured Outputs supported-schema guidance (strict subset accepts `anyOf`,
  not `allOf`/`oneOf`/`not`/`if`/`then`/`else`).
- Decision:
  1. Keep every Python `ResponseCreateParamsBase` field on the handwritten
     create body. `stream` remains typestate-split; `stream_options` remains
     streaming-only.
  2. Type the stored `Response` echo fields that the Python SDK already
     exposes: `moderation`, `prompt_cache_key`, `prompt_cache_options`,
     `prompt_cache_retention`, and `top_logprobs`. Unknown future fields stay
     in `ExtraFields`.
  3. Model `service_tier` as `Omittable<Nullable<ServiceTier>>` on both the
     create body and `Response`, matching `ServiceTierResponses` (`null` plus
     `auto|default|flex|scale|priority|fast|ultrafast`). Chat keeps the
     narrower pinned `ServiceTier` enum (`ultrafast` is Responses-only).
  4. Enforce pinned limits through explicit `CreateResponseRequest::validate`
     rather than rejecting them during Serde decode: `temperature` 0..=2,
     `top_p` 0..=1, `top_logprobs` 0..=20, `max_output_tokens` >= 16,
     metadata 16 pairs / 64-char keys / 512-char values, `safety_identifier`
     64 characters, non-empty `context_management` when present.
  5. Reject the official unsupported Structured Outputs composition and
     advanced object/array keywords (`allOf`, `oneOf`, `not`, `if`/`then`/`else`,
     `prefixItems`, `contains`, `uniqueItems`, and the previously rejected
     `patternProperties` family). Keep `anyOf`, `items`, `minItems`/`maxItems`,
     and numeric/string constraint keywords.
- Reason: the previous GA `Response` DTO dropped several echoed fields into
  `ExtraFields`, could not send `service_tier: null`, and the structured-output
  normalizer advertised a stricter subset than it enforced. Python TypedDicts
  collapse omit/null; this crate keeps the three-state wire model and adds an
  opt-in validator for the documented limits.
- Impact: Responses request/response JSON; Structured Outputs schema
  normalization; public accessors and `CreateResponseConstraintError`.
- Overrides: none
- Tests: `response_decodes_python_sdk_echo_fields`,
  `create_request_service_tier_null_matches_openapi`,
  `create_request_validate_enforces_pinned_limits`,
  `create_response_fields_match_python_and_openapi_inventory`,
  `rejects_unsupported_keywords`.

## D0016 — Chat Completions moderation echo and request limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateChatCompletionRequest` / `CreateChatCompletionResponse.moderation`
- Sources: pinned OpenAPI `ChatCompletionModeration` (list-shaped
  `moderation_results` + `error`); official Python
  `src/openai/types/chat/chat_completion.py` and
  `completion_create_params.py` as reviewed on the stated date.
- Decision:
  1. Decode Chat `moderation` as the pinned list-shaped union, not as
     unstructured `Value` and not as the Responses single-result shape.
  2. Enforce Chat create limits through explicit
     `CreateChatCompletionRequest::validate`: `messages` minItems 1,
     `temperature` 0..=2, `top_p` 0..=1, `frequency_penalty`/`presence_penalty`
     -2..=2, `top_logprobs` 0..=20, `n` 1..=128, metadata 16×64/512, and
     `safety_identifier` 64 characters.
- Reason: Python types Chat moderation as `moderation_results` with a
  `results` array; collapsing that to `Value` hid a directional contract that
  differs from GA Responses.
- Impact: Chat completion/chunk decode; public `ChatCompletionModeration`
  accessors; `CreateChatCompletionConstraintError`.
- Overrides: none
- Tests: `chat_completion_decodes_python_moderation_results_list`,
  `chat_create_validate_enforces_pinned_limits` (partial, noted by round-8
  item 8-26: only the frequency-penalty and `n` limit error paths are
  asserted there — the temperature/top_p/presence_penalty/top_logprobs/
  metadata/safety_identifier rejections and the empty-messages guard still
  lack negative assertions; to be completed by the 8-14 fix, see the planned
  D0245 addendum).

## D0017 — Embeddings, Speech, Images, Transcription, and Fine-tuning limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateEmbeddingRequest`, `CreateSpeechRequest`,
  `CreateTranscriptionRequest`, `CreateTranslationRequest`,
  `CreateImageRequest`, `CreateImageEditJsonRequest`,
  `CreateImageEditMultipartRequest`, `CreateFineTuningJobRequest`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`CreateEmbeddingRequest`,
  `CreateSpeechRequest`, `CreateTranscriptionRequest`,
  `CreateTranslationRequest`, `CreateImageRequest`,
  `CreateImageEditRequest`, `CreateFineTuningJobRequest`, `Metadata`);
  official Python SDK `embedding_create_params.py`,
  `audio/speech_create_params.py`, `image_generate_params.py`,
  `image_edit_params.py`, and `fine_tuning/job_create_params.py` as
  reviewed on the stated date.
- Decision:
  1. Request field inventories already match the Python TypedDicts and the
     pin. Keep lossless Serde decode for out-of-range values.
  2. Enforce Embeddings limits through explicit
     `CreateEmbeddingRequest::validate`: reject empty scalar/string items,
     empty token batches, array lengths outside `1..=2048`, and
     `dimensions < 1`.
  3. Enforce Speech `input`/`instructions` `maxLength: 4096` through
     `CreateSpeechRequest::validate`. `speed` remains the existing
     `SpeechSpeed` constructor/Serde bound (`0.25..=4.0`).
  4. Enforce transcription `languages` minItems 1, `known_speaker_*`
     maxItems 4 with matching lengths, and `temperature` `0..=1`.
     Translation reuses the temperature check.
  5. Enforce image prompt ceilings by model: `dall-e-2` 1000,
     `dall-e-3` generation 4000, GPT image models 32000. Image `n`,
     `output_compression`, and `partial_images` remain constructor-bounded
     types. Image-edit `moderation` stays generate-only on the official
     pin and Python `ImageEditParams`; the JSON edit DTO may still carry
     it as an extra optional.
  6. Enforce Fine-tuning `suffix` `1..=64`, `seed` `0..=2147483647`, and
     shared metadata 16×64/512 through `CreateFineTuningJobRequest::validate`.
- Reason: these families already exposed every official request field, but
  callers could construct values the pin documents as illegal. Opt-in
  validators match D0015/D0016 and keep three-state wire decode intact.
- Impact: public constraint-error types and `validate` methods on the
  listed create requests.
- Overrides: none
- Tests: `embedding_create_validate_enforces_pinned_limits`,
  `embedding_create_fields_match_python_and_openapi_inventory`,
  `speech_create_validate_enforces_pinned_limits`,
  `speech_create_fields_match_python_and_openapi_inventory`,
  `transcription_create_validate_enforces_pinned_limits`,
  `image_create_validate_enforces_model_prompt_limits`,
  `fine_tuning_create_validate_enforces_pinned_limits`,
  `fine_tuning_create_fields_match_python_and_openapi_inventory`.

## D0018 — Remaining-family field inventory and Completions/Chat stop limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Batches, Files/Uploads, Vector Stores, Containers, Skills, Content
  Provenance, Conversations, Moderations, Voices, legacy Completions, Chat
  `stop`/`logit_bias`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python SDK create
  TypedDicts reviewed on the stated date.
- Decision:
  1. Remaining supported create-request inventories already match the pin:
     Batch (`input_file_id`, `endpoint` including `/v1/videos`,
     `completion_window`, `metadata`, `output_expires_after`), Upload
     (filename/purpose/bytes/mime_type/expires_after; six-value purpose
     remains D0005), Vector Store create/file/file-batch, Container body
     (name/file_ids/expires_after/skills/memory_limit/network_policy),
     Conversation (`items`, `metadata`), Moderation (`input`, `model`),
     Voice/consent, Skills and Content Provenance multipart-only bodies.
     Assistants/Videos stay omitted (D0013). Realtime GA request DTOs
     follow the response-shaped session (`audio`, `output_modalities`)
     rather than the stale flat `RealtimeSessionCreateRequest` component
     (D0012).
  2. Chat and Completions `stop` arrays are `1..=4`. `logit_bias` values
     are `-100..=100`. Completions additionally validate `best_of` `0..=20`,
     `logprobs` `0..=5`, `n` `1..=128`, `temperature` `0..=2`, `top_p`
     `0..=1`, and penalties `-2..=2` through opt-in `validate()`.
- Reason: the remaining families had no missing official request fields;
  the leftover defect was undocumented stop/bias/completions numeric
  limits that callers could construct.
- Impact: `CreateChatCompletionConstraintError` stop/bias variants;
  `CreateCompletionConstraintError` and `validate` on legacy Completions.
- Overrides: none
- Tests: `chat_create_validate_enforces_pinned_limits`,
  `completion_create_validate_enforces_pinned_limits`,
  `completion_create_fields_match_python_and_openapi_inventory`.

## D0019 — Compact request prompt-cache and service_tier fields

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `POST /responses/compact` body
  (`CompactResponseMethodPublicBody` / `CompactResponseRequest`)
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python SDK
  `src/openai/types/responses/response_compact_params.py` as reviewed on
  the stated date.
- Decision:
  1. Type every Python `ResponseCompactParams` / pin field on the GA
     compact body: `model`, `input` (nullable), `instructions`,
     `previous_response_id`, `prompt_cache_key`, `prompt_cache_options`,
     `prompt_cache_retention`, and `service_tier`.
  2. Reuse the GA Responses prompt-cache and `ServiceTier` types. Compact
     `ServiceTierEnum` omits `scale`/`ultrafast`; those remain sendable
     through the open enum.
  3. Enforce compact `prompt_cache_key` `maxLength: 64` through opt-in
     `validate()`. Serde decode stays lossless.
  4. Beta compact already exposed this inventory; do not change it.
- Reason: the previous GA compact DTO dropped the cache and tier fields
  that both the pin and the Python SDK already accept.
- Impact: `CompactResponseRequest` JSON; `CompactResponseConstraintError`.
- Overrides: none
- Tests: `compact_request_fields_match_python_and_openapi_inventory`,
  `compact_request_validate_enforces_prompt_cache_key_limit`.

## D0020 — Remaining response-field inventory and Models/File Search gaps

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Models, Responses usage, file-search/code-interpreter tools,
  EasyInputMessage/OutputMessage phase, ChatKit session, Images response,
  stored Chat update/list, Conversation create/update/resource
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`Model`, `ResponseUsage`,
  `FileSearchTool`, `RankingOptions`, `CodeInterpreterTool`,
  `EasyInputMessage`, `OutputMessage`, `ChatSessionResource`,
  `CreateChatSessionBody`, `ImagesResponse`, `Image`, `ChatCompletionList`,
  `ConversationResource`, `CreateConversationBody`,
  `UpdateConversationBody`); official Python `src/openai/types/model.py`
  as reviewed on the stated date.
- Decision:
  1. Type `Model.shutdown_date` as `Omittable<Nullable<String>>` so official
     list examples can send explicit `null` and retrieve examples can send a
     `YYYY-MM-DD` date. Do not leave the field in `ExtraFields`.
  2. Type Responses `usage.compute_units` as `Omittable<Nullable<u64>>`,
     matching Chat Completions and the pin (`null` when the service reports
     it).
  3. Type every official `FileSearchTool` option on the handwritten tool:
     `max_num_results` (`1..=50` via opt-in `validate()`), `ranking_options`
     (`ranker`, `score_threshold` `0..=1`, `hybrid_search`), and nullable
     `filters` reused from the shared Vector Store `Filters` union. Serde
     decode stays lossless.
  4. Type Code Interpreter `allowed_callers` the same way as Function tools.
  5. Type Easy Input / Output message `phase` as
     `Omittable<Nullable<MessagePhase>>` (`commentary` | `final_answer`) so
     Codex-class follow-ups can resend the official field.
  6. ChatKit session create/response, Images response/image, stored Chat
     update/list, and Conversation create/update/resource inventories already
     match the pin; lock them with field-inventory tests. Assistants/Videos
     remain omitted (D0013).
- Reason: a full OpenAPI-to-Rust property diff showed these response and
  tool fields still dropped into `ExtraFields`, so callers could not send
  file-search options or read announced model shutdown dates. ChatKit,
  Images, stored Chat, and Conversation request/response keys were already
  aligned.
- Impact: Models decode; Responses usage/tool/message JSON; public
  accessors and `CreateResponseConstraintError` file-search variants.
- Overrides: none
- Tests: `model_decodes_python_and_openapi_shutdown_date`,
  `model_list_preserves_mixed_shutdown_dates`,
  `response_usage_decodes_compute_units`,
  `message_phase_roundtrips_on_input_and_output`,
  `file_search_tool_fields_match_python_and_openapi_inventory`,
  `file_search_tool_validate_enforces_pinned_limits`,
  `code_interpreter_tool_sends_allowed_callers`,
  `create_session_fields_match_python_and_openapi_inventory`,
  `images_response_fields_match_python_and_openapi_inventory`,
  `stored_chat_update_and_list_match_openapi_inventory`,
  `conversation_request_and_resource_match_openapi_inventory`.

## D0021 — Responses hosted-tool request fields are sendable

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `WebSearchTool`, `WebSearchPreviewTool`, `ImageGenTool`,
  `CustomToolParam`, `FunctionShellToolParam`, `ToolSearchToolParam`,
  `ApplyPatchToolParam`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  tool TypedDicts reviewed on the stated date.
- Decision:
  1. Type official Web Search options: `external_web_access`, nullable
     `filters.allowed_domains`, nullable `user_location`, and
     `search_context_size`. Preview also types `search_content_types`.
  2. Type every `ImageGenTool` option (`model`, `quality`, `size`,
     `output_format`, `output_compression` `0..=100`, `moderation`,
     `background`, nullable `input_fidelity`, `input_image_mask`,
     `partial_images` `0..=3`, `action`). Limits are opt-in `validate()`.
  3. Type Custom tool `description`, flat Responses `format`
     (`text` | `{type,syntax,definition}`), `defer_loading`, and
     `allowed_callers`. This format is not the nested Chat Completions
     `{type:"grammar",grammar:{...}}` shape.
  4. Type Shell `environment` as the pinned
     `container_auto` / `local` / `container_reference` union plus
     `allowed_callers`. Type Tool Search `execution` / `description` /
     `parameters`. Type Apply Patch `allowed_callers`.
  5. Computer, programmatic, and local-shell tools remain tag-only
     because the pin exposes only `type`.
- Reason: those tools were implemented as tag-only DTOs, so official
  options could only land in private `ExtraFields` and could not be sent.
- Impact: Responses create-tool JSON; `CreateResponseConstraintError`
  image-generation variants.
- Overrides: none
- Tests: `web_search_and_image_tools_match_python_and_openapi_inventory`,
  `image_generation_tool_validate_enforces_pinned_limits`,
  `remaining_response_tools_send_official_fields`.

## D0022 — Computer-use and apply_patch item fields are sendable

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ComputerToolCall`, `ComputerToolCallOutput`,
  `ComputerCallOutputItemParam`, `ComputerAction`, `ComputerScreenshotImage`,
  `ComputerCallSafetyCheckParam`, `ApplyPatchToolCall`,
  `ApplyPatchToolCallOutput`, `ApplyPatchCreateFileOperation`,
  `ApplyPatchUpdateFileOperation`, `ApplyPatchDeleteFileOperation`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  computer-use and apply_patch item TypedDicts reviewed on the stated date.
- Decision:
  1. Type computer-use `action` as the pinned 9-variant `ComputerAction`
     union (`click` / `double_click` / `drag` / `keypress` / `move` /
     `screenshot` / `scroll` / `type` / `wait`) plus `actions` for batched
     `computer_use`. Type `pending_safety_checks` as
     `ComputerSafetyCheck` (`id`, nullable `code` / `message`).
  2. Type computer-use output `output` as `ComputerScreenshot`
     (`type:computer_screenshot`, `image_url`, `file_id`) and type
     nullable `acknowledged_safety_checks`. Follow-up conversion copies
     resource `id` / `status` / acknowledgements onto the input item.
  3. Type apply_patch `operation` as the pinned
     `create_file` / `delete_file` / `update_file` union, including the
     required `diff` on create/update. Type apply_patch output `output`
     as nullable log text so callers can send patch results.
  4. Future action/operation tags stay in `UnknownTaggedObject`. Serde
     decode stays lossless for known tags; malformed known tags are not
     downgraded.
- Reason: those official item fields lived only in `ExtraFields` or an
  untyped `Value`, so callers could not construct computer-use
  acknowledgements, batched `actions`, or apply_patch diffs.
- Impact: Responses input/output item JSON for computer-use and
  apply_patch.
- Overrides: none
- Tests: `computer_call_fields_match_python_and_openapi_inventory`,
  `apply_patch_operation_fields_match_python_and_openapi_inventory`.

## D0023 — Realtime translation WebSocket events are typed

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `RealtimeTranslationClientEvent`, `RealtimeTranslationServerEvent`,
  `RealtimeTranslationSessionUpdateRequest`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Realtime
  translation event TypedDicts reviewed on the stated date.
- Decision:
  1. Type the pinned 3-branch translation client union:
     `session.update`, `session.input_audio_buffer.append`, and
     `session.close`. Session update reuses translation audio input/output
     configuration; append uses typed `RealtimeAudio`.
  2. Type the pinned 7-branch translation server union, including reused
     `RealtimeServerEventError` plus `session.created` / `updated` /
     `closed` and the transcript/audio deltas.
  3. Type official delta metadata: nullable `elapsed_ms` on transcript
     and audio deltas, plus `sample_rate`, `channels`, and `format`
     (`pcm16`) on `session.output_audio.delta`. These must not remain in
     `ExtraFields`.
  4. Future translation event tags stay in `UnknownRealtimeObject`.
     Malformed known tags are not downgraded.
- Reason: translation REST secrets were typed, but the WebSocket event
  family was missing, so official alignment metadata could not be read
  or sent.
- Impact: `realtime` feature event unions; no change to the GA 11/46
  conversation Realtime unions.
- Overrides: none
- Tests: `event_unions_match_the_pinned_discriminator_manifest`,
  `translation_events_match_python_and_openapi_inventory`,
  `translation_event_unions_decode_every_pinned_tag`.

## D0024 — Web-search and shell call actions are typed

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `WebSearchToolCall.action`, `LocalShellExecAction`,
  `FunctionShellAction`, `FunctionShellCall.environment`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  web-search and shell item TypedDicts reviewed on the stated date.
- Decision:
  1. Type web-search `action` as the pinned
     `search` / `open_page` / `find_in_page` union, including deprecated
     `query`, `queries`, and URL `sources`.
  2. Type local-shell `action` as `exec` with required `command` / `env`
     and optional nullable `timeout_ms` / `working_directory` / `user`.
  3. Type function-shell `action` as `{commands, timeout_ms,
     max_output_length}` (no `type` discriminator in the pin). Type
     call `environment` as the existing
     `container_auto` / `local` / `container_reference` union.
  4. Future action tags stay in `UnknownTaggedObject`. Serde decode
     stays lossless for known tags.
- Reason: those official item actions were untyped `Value`, so callers
  could not read search queries or execute shell argv without
  hand-indexing JSON.
- Impact: Responses web-search, local-shell, and function-shell item
  JSON.
- Overrides: none
- Tests: `web_search_and_shell_actions_match_python_and_openapi_inventory`.

## D0025 — Reasoning, Code Interpreter, and tool-call payload fields are typed

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ReasoningItem`, `SummaryTextContent`, `ReasoningTextContent`,
  `CodeInterpreterTool.container`, `CodeInterpreterCall.outputs`,
  `FunctionShellCallOutput.output`, `ToolSearchOutput.tools`,
  `ToolCallCaller`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  reasoning, code-interpreter, shell-output, and tool-search TypedDicts
  reviewed on the stated date.
- Decision:
  1. Type reasoning `summary` as `SummaryTextContent[]` and expose official
     optional `encrypted_content`, `content` (`ReasoningTextContent[]`),
     and `status`. `encrypted_content` is constructible so ZDR /
     `store:false` follow-ups do not depend on private `ExtraFields`.
  2. Type code-interpreter `container` as `string | {type:auto,...}`
     including `file_ids` (`<=50`, opt-in `validate()`), nullable
     `memory_limit`, and `network_policy` (`disabled` | `allowlist`).
     Type call `outputs` as `logs` | `image`. Add `interpreting` to
     `ResponseItemStatus`.
  3. Type function-shell output chunks as `{stdout, stderr, outcome}`
     with `timeout` / `exit` outcomes, plus sendable `caller` and
     `max_output_length`. Type tool-search `tools` as `ResponseTool[]`.
     Type function/custom tool-call `output` as the pinned
     `string | InputContent[]` union (`FunctionCallOutputValue`).
  4. Type `ToolCallCaller` as `direct` | `program`+`caller_id` and wire
     it onto function, custom, shell, and apply_patch call/output items.
     Future tags stay in `UnknownTaggedObject`. Serde decode stays
     lossless; `validate()` is not invoked by the HTTP client.
     JSON Schema `parameters` / `arguments` and free-form allowed-tool
     selectors remain `Value`.
- Reason: those official structured payloads were `Value` or ExtraFields
  only, so callers could not send encrypted reasoning, automatic CI
  containers, shell outcomes, or programmatic callers.
- Impact: Responses input/output item JSON; `CreateResponseConstraintError`
  code-interpreter `file_ids` variant.
- Overrides: none
- Tests: `reasoning_and_code_interpreter_fields_match_python_and_openapi_inventory`,
  `shell_output_and_tool_search_fields_match_python_and_openapi_inventory`.

## D0026 — File-search results and typed stream payload parts

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `FileSearchCall.results`, `FileSearchResult`,
  `ResponseItemStatus` (`searching` / `generating`),
  `ReasoningSummaryPartAddedEvent.part`,
  `ReasoningSummaryPartDoneEvent.part`,
  `OutputTextAnnotationAddedEvent.annotation`,
  `LocalShellCallOutput.status`, `McpListedTool.annotations`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  file-search and stream-event TypedDicts reviewed on the stated date.
- Decision:
  1. Type file-search `results` as nullable
     `{file_id, text, filename, attributes, score}[]`. Attributes are the
     pinned string/number/boolean map (Responses-local; `vector_stores`
     cannot be imported from `responses`).
  2. Add official item statuses `searching` (file search) and
     `generating` (image generation) to `ResponseItemStatus`.
  3. Type reasoning-summary stream `part` as `SummaryTextContent` and
     output-text `annotation` as the existing `Annotation` union.
  4. Type local-shell output `status` and MCP listed-tool `annotations`.
     Serde decode stays lossless; `validate()` is not invoked by the HTTP
     client.
- Reason: file-search hits and those stream payloads were ExtraFields or
  `Value`, so callers could not read retrieved filenames/scores or typed
  summary parts.
- Impact: Responses file-search items and stream events.
- Overrides: none
- Tests: `file_search_results_and_typed_stream_parts_match_openapi_inventory`.

## D0027 — Shell environment memory, network, and skills are sendable

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `FunctionShellContainerAuto`, `FunctionShellLocalEnvironment`,
  `ContainerSkill`, `LocalSkill`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  shell environment TypedDicts reviewed on the stated date.
- Decision:
  1. Type automatic-container official options: `file_ids` (`<=50`),
     nullable `memory_limit`, `network_policy` (`disabled` | `allowlist`),
     and `skills` (`skill_reference` | `inline`, `<=200`). Reuse the
     Responses-local code-interpreter memory/network types.
  2. Type local-environment `skills` as `{name, description, path}[]`
     (`<=200`). Inline zip `data` stays redacted in Debug.
  3. Skill id length `1..=64` and the count/file-id limits are opt-in
     `validate()`. Serde decode stays lossless; `validate()` is not
     invoked by the HTTP client.
- Reason: those official environment fields lived only in private
  `ExtraFields`, so callers could not send container memory, network
  policy, or skills.
- Impact: Responses shell-tool create JSON;
  `CreateResponseConstraintError` shell-environment variants.
- Overrides: none
- Tests: `shell_environment_fields_match_python_and_openapi_inventory`.

## D0028 — MCP tool-call errors are the pinned tagged union

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `McpCall.error`, `McpCallError`, `ResponseItemStatus::Calling`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  MCP tool-call TypedDicts reviewed on the stated date.
- Decision:
  1. Type `mcp_call.error` as nullable
     `mcp_protocol_error` | `mcp_tool_execution_error` | `http_error`.
     Protocol/HTTP carry `code` + `message`; execution `content` stays
     `Value` because the pin leaves that schema empty.
  2. Add official MCP status `calling` to `ResponseItemStatus`.
  3. Future error tags stay in `UnknownTaggedObject`. Serde decode of
     known tags is strict; a string `error` is not invented by the pin.
- Reason: the field was typed as `String`, so official structured MCP
  errors could not decode on the known `mcp_call` branch.
- Impact: Responses MCP call item JSON.
- Overrides: none
- Tests: `mcp_call_error_union_matches_python_and_openapi_inventory`.

## D0029 — Remaining official item fields leave ExtraFields

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `McpApprovalResponse`, `McpApprovalResponseResource`,
  `ApplyPatchToolCallOutput` / `ApplyPatchToolCallOutputItemParam`,
  `ToolSearchCallItemParam`, `AdditionalToolsItemParam`,
  `CompactionSummaryItemParam`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  item TypedDicts reviewed on the stated date.
- Decision:
  1. Type MCP approval-response input `id` and `reason` as
     `Omittable<Nullable<String>>`. Official `reason: null` must decode;
     callers can send a stored item `id`. Resource `reason` is the same
     nullable string. Ghost `request_id` stays required on the resource
     only (D0008).
  2. Type apply-patch output `caller` on both the sendable input item and
     the API resource. Resource also types official `created_by`.
  3. Type tool-search call input `id`, `call_id`, `execution`, and
     `status`. Type additional-tools and compaction input `id`. Those
     were ExtraFields-only, so callers could not send them.
  4. Serde decode stays lossless; `validate()` is not invoked by the HTTP
     client. Output-to-input conversion copies the newly typed fields.
- Reason: a follow-up OpenAPI-to-Rust property diff found official
  sendable item fields still trapped in private ExtraFields, plus one
  nullability offset that rejected official MCP approval JSON.
- Impact: Responses input/output item JSON.
- Overrides: none
- Tests: `remaining_item_fields_match_python_and_openapi_inventory`.

## D0030 — Resource item fields are readable and round-trip

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `FunctionToolCallOutputResource`, `FunctionToolCallResource`,
  `CustomToolCall` / `CustomToolCallResource`,
  `CustomToolCallOutputResource`, `ComputerToolCallOutputResource`,
  `ToolSearchCall`, `ToolSearchOutput`, `FunctionShellCall`,
  `FunctionShellCallOutput`, `ApplyPatchToolCall`, `CompactionItem`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  resource item TypedDicts reviewed on the stated date.
- Decision:
  1. Type official `FunctionToolCallOutputResource` fields that lived only
     in ExtraFields: `call_id`, `name`, `namespace`, `caller`, and
     `created_by`. `Response::to_input_items` copies the overlapping
     input fields instead of dropping them to ExtraFields.
  2. Type resource `created_by` on function/custom/computer/tool-search/
     shell/apply-patch/compaction items. Custom-tool calls also type
     resource `status`.
  3. `created_by` stays resource-side; input params that omit it are not
     given a sendable copy. Serde decode stays lossless.
- Reason: official output resources could not expose `call_id` or
  `created_by` as named fields, and converting a function-call output
  back to input dropped those properties from the typed DTO.
- Impact: Responses output-item JSON and `to_input_items`.
- Overrides: none
- Tests: `resource_item_fields_match_python_and_openapi_inventory`.

## D0031 — Function-tool parameters and pinned tool-name limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `FunctionTool` / `FunctionToolParam`, `CustomToolParam`,
  `NamespaceToolParam`, `allowed_callers` on hosted tools
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; official Python Responses
  function-tool TypedDicts reviewed on the stated date.
- Decision:
  1. Type function-tool `parameters` as `Omittable<Nullable<Value>>`. The
     sendable `FunctionToolParam` requires only `type`+`name`; official
     omit/`null` must decode. `FunctionTool::new` still sends an empty
     object schema.
  2. Enforce FunctionToolParam `name` `1..=128` and `[A-Za-z0-9_-]` through
     opt-in `validate()`. Namespace `name` must be non-empty and `tools`
     `minItems: 1`. Present non-null `allowed_callers` must be non-empty
     (`minItems: 1`) on function, custom, MCP, code-interpreter, shell, and
     apply-patch tools.
  3. Serde decode stays lossless; `validate()` is not invoked by the HTTP
     client.
- Reason: official function tools without `parameters` could not decode on
  the known `function` branch, and callers could construct names/empty
  caller lists that the pin rejects.
- Impact: Responses tool JSON; `CreateResponseConstraintError` function/
  namespace/`allowed_callers` variants.
- Overrides: none
- Tests: `function_tool_parameters_and_name_match_python_and_openapi_inventory`.

## D0032 — Remaining official nullability and opt-in create limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Realtime `Prompt`, Realtime output `speed`, Fine-tuning
  hyperparameters, Chat deprecated `functions`, MCP `tunnel_id`, Container
  `skill_id`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`Prompt`,
  `RealtimeSessionCreateRequestGA.audio.output.speed`,
  `FineTuneSupervisedHyperparameters`, `FineTuneDPOHyperparameters`,
  `FineTuneReinforcementHyperparameters`, `CreateChatCompletionRequest.functions`,
  `MCPTool.tunnel_id`, `SkillReferenceParam.skill_id`); official Python SDK
  types reviewed on the stated date.
- Decision:
  1. Type Realtime session/response `prompt` as
     `Omittable<Nullable<PromptReference>>` so official `prompt: null` decodes
     and can be sent. GA `audio.output.speed` is checked by opt-in
     `RealtimeSessionCreateRequest::validate()` as `0.25..=1.5`.
  2. Walk Fine-tuning legacy and method hyperparameters in
     `CreateFineTuningJobRequest::validate()`: `batch_size` `1..=256`,
     `n_epochs` `1..=50`, `learning_rate_multiplier` `> 0`, DPO `beta` `(0, 2]`,
     `compute_multiplier` `(0.00001, 10]`, `eval_interval`/`eval_samples` `>= 1`.
  3. Enforce deprecated Chat `functions` `minItems: 1` / `maxItems: 128`, MCP
     `tunnel_id` `^tunnel_[a-z0-9]{32}$`, and Container referenced `skill_id`
     `1..=64` through opt-in `validate()`.
  4. Serde decode stays lossless; `validate()` is not invoked by the HTTP
     client.
- Reason: official Realtime session fixtures send `prompt: null`, and callers
  could construct hyperparameter, function-list, tunnel, and skill-id values
  the pin rejects.
- Impact: Realtime session/response JSON; Fine-tuning/Chat/Responses/Container
  constraint errors.
- Overrides: none
- Tests: `realtime_prompt_null_and_output_speed_match_python_and_openapi_inventory`,
  `fine_tuning_create_validate_enforces_hyperparameter_limits`,
  `chat_create_validate_enforces_pinned_limits`,
  `function_tool_parameters_and_name_match_python_and_openapi_inventory`,
  `create_container_validate_enforces_skill_id_length`.

## D0033 — Compact/count-tokens sendability and Admin official nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CompactResponseRequest` / `BetaCompactResponseRequest` `model`,
  `CountInputTokensRequest` public setters, Admin update/create nullability,
  `EvalSamplingParams.response_format`/`text`, Admin opt-in create limits
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`ModelIdsCompaction`,
  `TokenCountsBody`, `ProjectUpdateRequest`, `UserRoleUpdateRequest`,
  `ProjectUserUpdateRequest`, `ProjectServiceAccountCreateRequest`,
  `PublicCreateOrganizationRoleBody`, `PublicUpdateOrganizationRoleBody`,
  `AdminApiKeyCreateRequest.expires_in_seconds`, `CreateGroupBody.name`,
  `ToggleCertificatesRequest.certificate_ids`,
  `UpdateOrganizationSpendLimitBody.threshold_amount`); official Python SDK
  types reviewed on the stated date.
- Decision:
  1. Type compact `model` as `Omittable<Nullable<String>>` on GA and beta so
     official `model: null` decodes and can be sent. Compact `validate()` also
     checks string `input` `maxLength` 10485760.
  2. Expose public CountTokens setters for official `previous_response_id`,
     `parallel_tool_calls`, `reasoning`, `text`, `truncation`, and matching
     `*_null` helpers already typed on the DTO.
  3. Wrap remaining official Admin anyOf-null request fields in
     `Omittable<Nullable<_>>`. Add `response_format`/`text` setters on
     `EvalSamplingParams`.
  4. Enforce Admin create/update pin bounds through opt-in `validate()`:
     API-key expiry `1..=31536000`, group name `1..=255`, certificate ids
     `1..=10`, spend-limit threshold `>= 1`. Serde stays lossless.
- Reason: official compact/session fixtures send `model: null`; callers could
  not construct official count-token or eval sampling fields; Admin update
  bodies rejected official nulls.
- Impact: Compact/count-token/Admin/Eval request JSON and constraint errors.
- Overrides: none
- Tests: `compact_request_fields_match_python_and_openapi_inventory`,
  `count_input_tokens_request_sends_official_fields`,
  `admin_request_nulls_and_limits_match_python_and_openapi_inventory`,
  `run_data_sources_enforce_nested_tag_sets_and_build_typed_requests`.

## D0034 — Input content official nulls and Eval run/source setters

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `InputText` / `InputImage` / `InputFile` optional fields,
  `PromptReference.version`, `CreateEvalRunRequest.metadata`,
  Eval stored-completions / responses source filters
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`InputTextContentParam`,
  `InputImageContentParamAutoParam`, `InputFileContentParam`, `Prompt.version`,
  `CreateEvalRunRequest`, `CreateEvalResponsesRunDataSource.source`,
  `CreateEvalJSONLRunDataSource.stored_completions`); official Python SDK
  types reviewed on the stated date.
- Decision:
  1. Type official anyOf-null input-part fields as `Omittable<Nullable<_>>`
     (`file_id`/`filename`/`file_data`/`file_url`/`image_url`/`detail` on
     image, `prompt_cache_breakpoint` on text/image/file). File `detail`
     stays `Omittable<ImageDetail>` because the pin is a non-null `$ref`.
  2. Type `PromptReference.version` as `Omittable<Nullable<String>>` and add
     `version_null()`.
  3. Expose public setters for `CreateEvalRunRequest.metadata` and the
     official stored-completions / responses source filters already typed on
     the DTOs. Serde decode stays lossless.
- Reason: official input fixtures send explicit `null` on file/image/text
  optionals and prompt version; Eval run/source metadata and filters could
  not be constructed through the public builder.
- Impact: Responses input-content JSON; Eval run/source request JSON.
- Overrides: none
- Tests: `input_content_and_prompt_version_accept_official_nulls`,
  `run_data_sources_enforce_nested_tag_sets_and_build_typed_requests`.

## D0035 — Realtime Metadata nulls and compact send/validate parity

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `RealtimeResponse` / `RealtimeResponseCreateParams` `metadata`,
  GA and beta compact request `*_null` setters, beta compact `validate()`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`Metadata`,
  `RealtimeResponse.metadata`, `RealtimeResponseCreateParams.metadata`,
  `CompactResponseMethodPublicBody`, `BetaCompactResponseMethodPublicBody`);
  official Python SDK types reviewed on the stated date.
- Decision:
  1. Type Realtime response and `response.create` `metadata` as
     `Omittable<Nullable<BTreeMap<String, String>>>` so official
     `metadata: null` decodes and can be sent.
  2. Expose the remaining official compact anyOf-null setters on GA
     (`instructions` / `previous_response_id` / `prompt_cache_options` /
     `prompt_cache_retention`) and the matching beta compact setters
     (`input` / `instructions` / `previous_response_id` / `prompt_cache_key`
     / `prompt_cache_options` / `prompt_cache_retention` / `service_tier`).
  3. Mirror GA compact opt-in `validate()` on `BetaCompactResponseRequest`
     for `prompt_cache_key` `maxLength` 64 and string `input`
     `maxLength` 10485760. Serde decode stays lossless.
- Reason: official Realtime response fixtures send `metadata: null`;
  compact request fields are private, so missing `*_null` helpers blocked
  sending official nulls; beta compact lacked the D0033 pin checks.
- Impact: Realtime response JSON; compact request JSON and constraint
  errors.
- Overrides: none
- Tests: `realtime_prompt_null_and_output_speed_match_python_and_openapi_inventory`,
  `compact_request_fields_match_python_and_openapi_inventory`,
  `compact_request_sends_official_nulls_and_enforces_pin_limits`.

## D0036 — Count-tokens official nulls and remaining Realtime create limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CountInputTokensRequest` / `BetaCountInputTokensRequest`
  conversation and remaining `*_null` setters, count-tokens string
  `input` `validate()`, Realtime session/client-secret pin bounds
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`TokenCountsBody`,
  `BetaTokenCountsBody`, `RealtimeCreateClientSecretRequest.expires_after`,
  `RealtimeTruncation.retention_ratio`, `RealtimeTurnDetection` Server VAD
  `idle_timeout_ms`, `AudioTranscription.languages`); official Python SDK
  types reviewed on the stated date.
- Decision:
  1. Expose `conversation_null()` on GA count-tokens and the remaining
     official anyOf-null setters on beta count-tokens (`model` / `input` /
     `instructions` / `conversation` / `parallel_tool_calls` /
     `previous_response_id` / `reasoning` / `text` / `tool_choice` /
     `tools`).
  2. Add opt-in `validate()` on GA and beta count-tokens for string `input`
     `maxLength` 10485760. Serde decode stays lossless; tests do not
     allocate a 10 MiB fixture.
  3. Extend Realtime opt-in `validate()` for client-secret
     `expires_after.seconds` `10..=7200`, `retention_ratio` `0..=1`,
     Server VAD `idle_timeout_ms` `5000..=30000`, and present
     `transcription.languages` `minItems: 1`. Nested session configs on
     client-secret create are walked.
- Reason: count-tokens fields are private, so missing `*_null` helpers
  blocked official nulls; Realtime create already validated output speed
  but left the other pin keywords unchecked.
- Impact: Count-tokens request JSON; Realtime session/client-secret
  constraint errors.
- Overrides: none
- Tests: `count_input_tokens_request_sends_official_fields`,
  `create_and_count_requests_keep_beta_only_fields_typed`,
  `realtime_prompt_null_and_output_speed_match_python_and_openapi_inventory`.

## D0037 — CreateResponse remaining official null setters

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateResponseRequest` / `CreateStreamingResponseRequest`
  remaining anyOf-null setters; matching beta create wrappers
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`CreateResponse`); official
  Python SDK types reviewed on the stated date.
- Decision:
  1. Expose public `*_null` setters for every remaining official
     anyOf-null create field already typed as `Omittable<Nullable<_>>`
     (`background`, `conversation`, `context_management`, `include`,
     `max_output_tokens`, `max_tool_calls`, `metadata`, `moderation`,
     `parallel_tool_calls`, `previous_response_id`, `prompt`,
     `prompt_cache_key`, `prompt_cache_options`,
     `prompt_cache_retention`, `reasoning`, `store`, `temperature`,
     `top_p`, `truncation`).
  2. Forward the same official nulls on `BetaCreateResponseRequest`,
     including beta-local `context_management` / `moderation` /
     `multi_agent` / `reasoning`. Serde decode stays lossless.
- Reason: create-body fields are private; missing `*_null` helpers
  blocked sending official nulls that the pin already types.
- Impact: Responses create request JSON (GA and beta).
- Overrides: none
- Tests: `create_request_sends_typed_context_moderation_and_explicit_nulls`.

## D0038 — Beta create forwards, stream_options null, image partial_images null

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `BetaCreateResponseRequest` remaining official forwards,
  GA/beta streaming `stream_options`, streaming image `partial_images`,
  `BetaReasoningConfig` official nulls
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`BetaCreateResponse`,
  `ResponseStreamOptions`, `PartialImages`, `BetaReasoning`); official
  Python SDK types reviewed on the stated date.
- Decision:
  1. Forward remaining official create fields through the private beta
     `base` (`conversation`, `instructions_null`, `prompt`,
     `prompt_cache_retention`, `safety_identifier`, `service_tier`,
     `top_logprobs`, `user`) and expose opt-in `validate()`.
  2. Type streaming `stream_options` as
     `Omittable<Nullable<ResponseStreamOptions>>` so official
     `stream_options: null` decodes and can be sent.
  3. Expose `with_partial_images_null()` on streaming image create/edit
     builders, and `generate_summary` / `*_null` on
     `BetaReasoningConfig`. Serde decode stays lossless.
- Reason: beta create hid official fields behind a private wrapper;
  `Omittable<ResponseStreamOptions>` rejected official null; streaming
  image `partial_images` could not send null.
- Impact: Beta/GA streaming create JSON; image streaming JSON; beta
  reasoning JSON.
- Overrides: none
- Tests: `create_and_count_requests_keep_beta_only_fields_typed`,
  `request_builders_emit_typed_multimodal_and_tool_json`,
  `image_generation_typestate_preserves_null_and_open_values`.

## D0039 — Nested official nulls, compact_threshold, and Eval sampling validate

- Status: accepted
- Reviewed: 2026-08-30
- Scope: nested Responses/Chat/Completions/Evals official `anyOf [T, null]`
  sendability, context-management `compact_threshold` `minimum: 1000`,
  Eval sampling `max_completions_tokens` `minimum: 1`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`Reasoning`, `ModerationPolicy`,
  `ContextManagement`, `MCPTool`, `WebSearchFilters`, `WebSearchTool`,
  `ImageGenTool`, `EvalSamplingParams`, Chat/Completions `stream_options`);
  official Python SDK types reviewed on the stated date.
- Decision:
  1. Expose `*_null` setters on `ReasoningConfig`, `ModerationPolicy`,
     `ModerationConfig`, `ContextManagement`, `McpTool`, `WebSearchFilters`,
     `WebSearchTool`, `WebSearchPreviewTool`, and `ImageGenerationTool`
     for official anyOf-null fields already typed as
     `Omittable<Nullable<_>>`.
  2. Create-body `validate()` enforces pin `compact_threshold` `minimum: 1000`
     (`MIN_COMPACT_THRESHOLD`) when a numeric value is present. Official
     `null` and omitted values skip the bound.
  3. Chat streaming `with_stream_options_null` and legacy Completions
     `stream_options_null` send official `stream_options: null`.
  4. `EvalSamplingParams` gains `*_null` for `seed` / `top_p` /
     `temperature` / `max_completion_tokens` / `max_completions_tokens`
     and opt-in `validate()` for pin `max_completions_tokens` `minimum: 1`.
     Serde decode stays lossless.
- Reason: nested official nulls were decodable but not sendable; two
  remaining schema-keyword minima were not opt-in validated.
- Impact: Responses create/tool JSON; Chat and Completions streaming JSON;
  Eval sampling JSON. Decode remains lossless.
- Overrides: none
- Tests: `create_request_sends_typed_context_moderation_and_explicit_nulls`,
  `create_request_validate_enforces_pinned_limits`,
  `eval_sampling_params_sends_official_nulls_and_enforces_pin_limits`,
  `request_typestate_controls_stream_wire_fields`,
  `request_builders_cover_echo_best_of_suffix_and_stream_typestate`.

## D0040 — Remaining sendable official nulls on tools, input parts, evals, completions

- Status: accepted
- Reviewed: 2026-08-30
- Scope: remaining private-field sendable `anyOf [T, null]` setters on
  `FunctionTool`, `InputImage` / `InputFile`, Eval run sources, and
  legacy Completions create
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`FunctionTool`,
  `ResponseInputImage` / `ResponseInputFile`, `EvalStoredCompletionsSource`,
  `EvalResponsesSource`, `CreateCompletionRequest`); official Python SDK
  types reviewed on the stated date.
- Decision:
  1. `FunctionTool` sends official `description` / `output_schema` /
     `strict` / `allowed_callers` nulls in addition to `parameters`.
  2. `InputImage` sends `image_url` / `file_id` nulls; `InputFile` sends
     `file_id` / `file_url` / `file_data` / `filename` nulls. Decode of
     official nulls was already lossless.
  3. `EvalStoredCompletionsSource` and `EvalResponsesSource` expose
     `*_null` for every official nullable filter.
  4. Legacy Completions builders send official nulls for `echo` /
     `suffix` / `max_tokens` / `n` / `logprobs` / `logit_bias` / `stop` /
     `temperature` / `top_p` / `frequency_penalty` / `presence_penalty` /
     `seed` / `best_of`. Receive-side ExtraFields and tag-only tools stay
     without setters.
- Reason: those official nulls decoded but could not be constructed on
  private-field request types.
- Impact: Responses tool/input JSON; Eval run-source JSON; Completions
  create JSON. Decode remains lossless.
- Overrides: none
- Tests: `function_tool_parameters_and_name_match_python_and_openapi_inventory`,
  `input_content_and_prompt_version_accept_official_nulls`,
  `eval_run_sources_send_official_filter_nulls`,
  `request_builders_cover_echo_best_of_suffix_and_stream_typestate`.

## D0041 — Remaining hosted-tool official nulls and MCP tool-choice name

- Status: accepted
- Reviewed: 2026-08-30
- Scope: remaining sendable official `anyOf [T, null]` on hosted tools and
  MCP tool choice
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`CustomToolParam`,
  `ApplyPatchToolParam`, shell tool, `ToolSearchToolParam`,
  `ToolChoiceMCP`); official Python SDK types reviewed on the stated date.
- Decision:
  1. Expose `allowed_callers_null` on `CustomTool`, `FunctionShellTool`,
     and `ApplyPatchTool`.
  2. Expose `description_null` / `parameters_null` on `ToolSearchTool`.
  3. Type `McpToolChoice.name` as `Omittable<Nullable<String>>` so official
     `name: null` decodes and can be sent. Serde stays lossless.
- Reason: hosted-tool official nulls were not sendable; MCP tool-choice
  `name` rejected official null.
- Impact: Responses tool and tool_choice JSON. Decode remains lossless.
- Overrides: none
- Tests: `remaining_response_tools_send_official_fields`.

## D0042 — Response resource official background and truncation nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: GA `Response` resource `background` and `truncation`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`ResponseProperties.background`,
  `Response.truncation`); official Python SDK response types reviewed on
  the stated date.
- Decision: type `Response.background` as
  `Omittable<Nullable<bool>>` and `Response.truncation` as
  `Omittable<Nullable<TruncationStrategy>>` so official resource JSON
  with `background: null` / `truncation: null` decodes and round-trips.
  Beta `Response` already used these three-state types. Count-tokens
  `truncation` stays non-null (`TokenCountsBody.truncation` is a
  non-null `$ref`). `output_text` remains an SDK-only convenience
  getter. Serde decode stays lossless.
- Reason: GA response decode rejected official nulls that the pin and
  Python SDK emit.
- Impact: Responses GET/create resource JSON. Create-request builders
  were already three-state.
- Overrides: none
- Tests: `response_decodes_python_sdk_echo_fields`.

## D0043 — ComputerScreenshot and ResponseText verbosity official nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ComputerScreenshotContent.image_url` / `file_id`;
  `ResponseTextParam.verbosity` (`Verbosity`)
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ComputerScreenshotContent.image_url` / `file_id` are
  `anyOf [string, null]`; `Verbosity` is `anyOf [enum, null]`);
  official Python SDK types reviewed on the stated date.
- Decision:
  1. Type `ComputerScreenshot.image_url` and `file_id` as
     `Omittable<Nullable<String>>` with `image_url_null` /
     `file_id_null` so official screenshot JSON nulls decode and
     can be sent.
  2. Type `ResponseTextConfig.verbosity` as
     `Omittable<Nullable<ResponseTextVerbosity>>` with
     `verbosity_null` so official `verbosity: null` decodes and
     can be sent. Serde stays lossless.
- Reason: official resource/request JSON `"image_url": null`,
  `"file_id": null`, and `"verbosity": null` failed on
  `Omittable<T>` without `Nullable`.
- Impact: Responses computer-use output and text config JSON.
- Overrides: none
- Tests: `computer_call_fields_match_python_and_openapi_inventory`,
  `create_request_sends_typed_context_moderation_and_explicit_nulls`.

## D0044 — Remaining computer, location, and container official nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: sendable official `anyOf [T, null]` on computer-use actions,
  `ApproximateLocation`, container `memory_limit`, and function-shell
  timeouts/output caps
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`ClickParam.keys`,
  `DoubleClickAction.keys`, `DragParam.keys`, `MoveParam.keys`,
  scroll `keys`, `ComputerCallSafetyCheckParam.code` / `message`,
  `ApproximateLocation` city/country/region/timezone,
  `AutoCodeInterpreterToolParam.memory_limit`,
  function-shell `timeout_ms` / `max_output_length`); official
  Python SDK types reviewed on the stated date.
- Decision:
  1. Expose `keys` / `keys_null` on computer click, double-click,
     drag, move, and scroll actions so official modifier-key nulls
     can be sent.
  2. Expose `code_null` / `message_null` on `ComputerSafetyCheck`.
  3. Expose `country_null` / `region_null` / `city_null` /
     `timezone_null` on `WebSearchUserLocation`.
  4. Expose `memory_limit_null` on automatic code-interpreter and
     function-shell containers.
  5. Expose `timeout_ms_null` / `max_output_length_null` on
     `FunctionShellAction` and `max_output_length_null` on
     `FunctionShellCallOutputInput`. Serde stays lossless.
- Reason: these request/param fields already decoded official nulls
  but could not send them through the public builder API.
- Impact: Responses computer-use, web-search location, container,
  and shell JSON. Decode remains lossless.
- Overrides: none
- Tests: `computer_call_fields_match_python_and_openapi_inventory`,
  `web_search_and_image_tools_match_python_and_openapi_inventory`,
  `web_search_and_shell_actions_match_python_and_openapi_inventory`,
  `reasoning_and_code_interpreter_fields_match_python_and_openapi_inventory`,
  `shell_output_and_tool_search_fields_match_python_and_openapi_inventory`.

## D0045 — ComputerScreenshotContent fields and input-content branch

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ComputerScreenshotContent` / `ComputerScreenshotImage` and
  Responses `InputContent`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`;
  `ComputerScreenshotContent` requires nullable locators plus `detail`
  and optional `prompt_cache_breakpoint`;
  `ComputerScreenshotImage` (computer-call output) has optional
  non-null locators and no detail; official input-content `oneOf`
  includes `ComputerScreenshotContent`. Official Python SDK types
  reviewed on the stated date.
- Decision:
  1. Keep screenshot locators as `Omittable<Nullable<String>>` so both
     official shapes decode.
  2. Add omittable `detail` and `prompt_cache_breakpoint` so content
     payloads can send and decode those official fields.
  3. Route `computer_screenshot` through typed
     `InputContent::ComputerScreenshot` instead of `Unknown`.
     Serde stays lossless.
- Reason: official screenshot content fields were dropped and the known
  tag fell through to the future-variant branch.
- Impact: Responses input content and computer-call output JSON.
- Overrides: none
- Tests: `input_content_and_prompt_version_accept_official_nulls`,
  `computer_call_fields_match_python_and_openapi_inventory`.

## D0046 — Conversation content official prompt-cache breakpoints

- Status: accepted
- Reviewed: 2026-08-30
- Scope: persisted conversation `input_image`, `input_file`, and
  `computer_screenshot` content
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`InputImageContent`, `InputFileContent`,
  `ComputerScreenshotContent` each expose optional
  `prompt_cache_breakpoint`); official Python SDK
  `conversations.InputImageContent` /
  `InputFileContent` /
  `ComputerScreenshotContent` reviewed on the stated date.
- Decision:
  1. Type `prompt_cache_breakpoint` as
     `Omittable<PromptCacheBreakpoint>` on
     `ConversationInputImage`, `ConversationInputFile`, and
     `ConversationComputerScreenshot` so official content JSON
     decodes and can be sent.
  2. Convert persisted screenshots to typed
     `InputContent::ComputerScreenshot` instead of a role-mismatch
     error. Resource locators stay required `Nullable` because the
     pin requires `image_url` / `file_id` on screenshot content.
     Serde stays lossless.
- Reason: conversation content dropped the official breakpoint
  field that both the pin and Python SDK persist.
- Impact: Conversations message content JSON and
  conversation-to-Responses conversion.
- Overrides: none
- Tests: `conversation_request_and_resource_match_openapi_inventory`.

## D0047 — Message phase nulls and function-shell send/validate

- Status: accepted
- Reviewed: 2026-08-30
- Scope: official `Message.phase` nullability; function-shell call
  and output item request fields
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`Message.phase` / `MessagePhase-2` are `anyOf` null;
  `FunctionShellCallItemParam` exposes nullable `id` /
  `caller` / `status` / `environment` and `call_id`
  `minLength` 1 / `maxLength` 64;
  `FunctionShellCallOutputItemParam` exposes nullable `id` /
  `caller` / `status` and the same `call_id` bounds;
  `FunctionShellCallOutputContentParam.stdout` /
  `stderr` have `maxLength` 10485760)
- Decision:
  1. Keep `OutputMessage.phase` as
     `Omittable<Nullable<MessagePhase>>` and add `phase_null`.
  2. Type persisted `ConversationMessage.phase` as
     `Omittable<Nullable<MessagePhase>>` (not a bare string)
     and add `phase_null` so official conversation JSON
     decodes and can be resent.
  3. Add sendable official-null setters on
     `FunctionShellCallInput` (`id` / `caller` / `status` /
     `environment`) and `FunctionShellCallOutputInput`
     (`id` / `caller` / `status`).
  4. Add opt-in `validate()` for the pinned `call_id` and
     stdout/stderr limits. Serde stays lossless; `validate()`
     is not invoked by the HTTP client. Tests do not allocate
     a 10,485,760-character string.
- Reason: follow-up assistant messages must be able to send
  official `phase: null`, and function-shell input items
  dropped sendable official nulls plus the pinned call/output
  limits.
- Impact: Responses and Conversations message JSON; function-shell
  input-item builders and opt-in validation.
- Overrides: none
- Tests: `message_phase_roundtrips_on_input_and_output`,
  `conversation_request_and_resource_match_openapi_inventory`,
  `shell_output_and_tool_search_fields_match_python_and_openapi_inventory`.

## D0048 — Remaining item-param official nulls and call-id limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: remaining Responses input-item request params with official
  `anyOf` nulls and pinned `call_id` / name / namespace / output
  limits
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`FunctionCallOutputItemParam`, `ToolSearchCallItemParam`,
  `ToolSearchOutputItemParam`, `ComputerCallOutputItemParam`,
  `ApplyPatchToolCallItemParam`, `ApplyPatchToolCallOutputItemParam`,
  `AdditionalToolsItemParam`, `CompactionSummaryItemParam`,
  `CustomToolCallOutput`)
- Decision:
  1. Add sendable official-null setters on function-call output
     (`id` / `call_id` / `name` / `namespace` / `caller` /
     `status`), tool-search call/output (`id` / `call_id` /
     `status`), computer-call output (`id` / `status`),
     apply-patch call/output (`id` / `caller`), additional
     tools (`id`), compaction (`id`), and custom-tool output
     (`caller`).
  2. Add opt-in `validate()` for pinned `call_id` `1..=64`,
     function-call output `name` `1..=128`, namespace
     `1..=64` plus `[A-Za-z0-9_-]`, and the documented
     10,485,760 / 20,971,520 character caps. Serde stays
     lossless; tests do not allocate those large strings.
- Reason: these item params still dropped official nulls and
  the same call-id limits already applied to function-shell
  items.
- Impact: Responses input-item builders and opt-in validation.
- Overrides: none
- Tests: `item_param_official_nulls_and_call_id_limits_match_openapi`.

## D0049 — Function/custom caller nulls, program limits, remaining item nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: remaining Responses/Conversations item constructors whose official
  pin still allowed JSON null or documented `call_id` / program text
  limits after D0048
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`FunctionToolCall`, `CustomToolCall`, `CompactionTriggerItemParam`,
  `ProgramItemParam`, `ProgramOutputItemParam`, `MCPApprovalResponse`,
  `LocalShellToolCallOutput`, `LocalShellExecAction`,
  `WebSearchActionOpenPage`, `InputFileContent`)
- Decision:
  1. Add sendable official-null setters for function/custom-tool
     `caller`, compaction-trigger `id`, MCP approval `id`, local-shell
     output `status`, open-page `url`, local-shell exec
     `timeout_ms` / `working_directory` / `user`, and conversation
     input-file `file_id`.
  2. Type compaction-trigger `id` as `Omittable<Nullable<String>>`
     instead of dropping it into `ExtraFields`.
  3. Add opt-in `validate()` for program `call_id` `1..=64` and
     `code` / `fingerprint` / `result` maxLength 10,485,760. Serde
     stays lossless; tests do not allocate those large strings.
- Reason: these remaining item constructors still dropped official
  nulls or skipped the same call-id / output-length limits already
  applied to other tool items.
- Impact: Responses/Conversations item builders and opt-in validation.
- Overrides: none
- Tests: `function_call_program_and_remaining_official_nulls_match_openapi`.

## D0050 — Beta item official-null setters

- Status: accepted
- Reviewed: 2026-08-30
- Scope: preview Responses multi-agent item constructors whose official
  `Beta*ItemParam` / content schemas still allowed JSON null after D0049
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`Beta*ItemParam.agent` / `id` / `caller` / `phase` / `status`,
  `BetaInputTextContentParam`, `BetaInputImageContentParamAutoParam`,
  `BetaAgentMessageItemParam`, `BetaMultiAgentCallItemParam`,
  `BetaMultiAgentCallOutputItemParam`)
- Decision:
  1. Add sendable official-null setters on stable beta item metadata
     (`agent` / `caller` / `phase`), prompt-cached messages
     (`id` / `agent` / `phase` / `status` / breakpoint), inter-agent
     text/image locators and breakpoints, agent messages, and
     multi-agent call/output (`id` / `agent`).
  2. Decode of official nulls was already lossless; this only exposes
     constructors that can send the same three-state values.
- Reason: beta wrappers reused GA codecs but still dropped official
  preview-only nulls on the flattened metadata and beta-only items.
- Impact: beta Responses item builders. Serde remains lossless.
- Overrides: none
- Tests: `beta_item_official_nulls_match_openapi`.

## D0051 — Conversation input-image official locator nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: persisted conversation `InputImageContent` locators whose official
  pin allows JSON null
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`InputImageContent.image_url`, `InputImageContent.file_id`)
- Decision: add sendable official-null setters for conversation input-image
  `image_url` and `file_id`. Resource `detail` and
  `prompt_cache_breakpoint` stay non-null, matching the pin.
- Reason: Responses input-image already sent these official nulls; the
  conversation resource constructor did not.
- Impact: Conversations input-image builders.
- Overrides: none
- Tests: `conversation_request_and_resource_match_openapi_inventory`.

## D0052 — Program-caller `caller_id` opt-in limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ProgramToolCallCallerParam.caller_id` `1..=64` when a program
  caller is sent on Responses tool items
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ProgramToolCallCallerParam`)
- Decision: add opt-in `validate()` for program `caller_id` and walk it
  from function/custom/shell/apply-patch input items. Serde stays
  lossless.
- Reason: program items already enforced `call_id` `1..=64`; the nested
  caller that references those programs still dropped the same pin.
- Impact: Responses item opt-in validation.
- Overrides: none
- Tests: `function_call_program_and_remaining_official_nulls_match_openapi`.

## D0053 — Function/custom/MCP call constructors match official requiredness

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Responses function, custom-tool, and MCP call input
  constructors versus pinned required/optional properties and the
  Python SDK Stainless types
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`FunctionToolCall`, `CustomToolCall`, `CustomToolCallOutput`,
  `MCPToolCall`); Python
  `openai.types.responses.response_function_tool_call` /
  `response_input_item`
- Decision:
  1. Add `FunctionCall::call` for the official required
     `call_id` / `name` / `arguments` shape, plus `with_id` /
     `with_status` for stored echoes.
  2. Add custom-tool `id` / `namespace` and custom-tool-output `id`
     setters for official optional non-null fields.
  3. Add `McpCall::new` and official-null setters for
     `approval_request_id` / `output` / `error`.
- Reason: constructors still required stored-only fields or dropped
  sendable official optionals that the pin and Python SDK expose.
- Impact: Responses item builders. Serde remains lossless.
- Overrides: none
- Tests: `function_custom_and_mcp_call_constructors_match_openapi`.

## D0054 — MCP list-tools constructors and official error nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `MCPListTools` / `MCPListToolsTool` request constructors
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`; Python
  `response_input_item.McpListTools`
- Decision: add `McpListTools::new` / `McpListedTool::new` and official-null
  setters for list-tools `error` and listed-tool `description` /
  `annotations`.
- Reason: these input items still had no send path for official nulls
  after D0053 added MCP call constructors.
- Impact: Responses MCP item builders.
- Overrides: none
- Tests: `function_custom_and_mcp_call_constructors_match_openapi`.

## D0055 — Hosted-call constructors match official required fields

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Responses hosted-call input constructors versus pinned
  required properties and official required-nulls
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`MCPApprovalRequest`, `ComputerToolCall`, `WebSearchToolCall`,
  `ImageGenToolCall`, `CodeInterpreterToolCall`, `LocalShellToolCall`)
- Decision:
  1. Add `McpApprovalRequest::new` for required
     `id` / `server_label` / `name` / `arguments`.
  2. Add `ComputerCall::new` plus `with_action` / `with_actions` /
     `with_pending_safety_checks` for official optional action fields.
  3. Add `WebSearchCall::new` and `LocalShellCall::new` from required
     action unions.
  4. Add `ImageGenerationCall::new` with official required
     `result: null`, and `CodeInterpreterCall::new` with official
     required `code` / `outputs` nulls.
- Reason: these input-union hosted calls still only had getters after
  D0054, so callers could not construct the official required send
  shape including required-null result/code/outputs.
- Impact: Responses hosted-call builders. Serde remains lossless.
- Overrides: none
- Tests: `hosted_call_constructors_match_openapi_required_fields`.

## D0056 — Function-output constructor and official non-null optionals

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `FunctionCallOutputItemParam` requiredness and official
  non-null `namespace` / custom-tool `id` fields
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`FunctionCallOutputItemParam`, `FunctionToolCall.namespace`,
  `CustomToolCall.namespace`, `CustomToolCallOutput.id`)
- Decision:
  1. Add `FunctionCallOutput::from_output` for the official required
     `output` shape, plus `with_call_id` for the official optional
     nullable `call_id`.
  2. Type function/custom-tool `namespace` and custom-tool-output `id`
     as `Omittable<String>` so unofficial `"field": null` fails decode.
- Reason: `FunctionCallOutput::new` still required `call_id` after
  D0053 even though the pin only requires `type` / `output`. Function
  and custom-tool namespace, and custom-tool-output `id`, are official
  optional non-null strings, not anyOf-null.
- Impact: Responses item builders and decode of unofficial nulls.
  Constraint validation remains opt-in.
- Overrides: none
- Tests: `function_custom_and_mcp_call_constructors_match_openapi`.

## D0057 — CreateResponse `prompt_cache_options` is official non-null

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateResponse.prompt_cache_options` versus
  `CompactResponseMethodPublicBody.prompt_cache_options`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`CreateResponse` / `ResponseProperties` `$ref`
  `PromptCacheOptionsParam`; compact body `anyOf` `[ref, null]`)
- Decision: type create-response `prompt_cache_options` as
  `Omittable<PromptCacheOptions>` and remove the unofficial
  `prompt_cache_options_null` send path. Compact keeps official-null
  setters because the compact body is anyOf-null.
- Reason: create-response only `$ref`s `PromptCacheOptionsParam`;
  unofficial `"prompt_cache_options": null` is not in the pin.
- Impact: Responses create builders and decode of unofficial nulls.
  Compact and stored Response resources are unchanged.
- Overrides: none
- Tests: `create_request_sends_typed_context_moderation_and_explicit_nulls`.

## D0058 — Responses stream events expose official optional fields

- Status: accepted
- Reviewed: 2026-08-30
- Scope: remaining official optional properties on
  `ResponseStreamEvent` members
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ResponseShellCallCommandDeltaStreamingEvent.obfuscation`,
  `ResponseReasoningSummaryPartDoneEvent.status`,
  `ResponseImageGenCallPartialImageEvent` `size` / `quality` /
  `background` / `output_format`)
- Decision:
  1. Type shell-command delta `obfuscation` as `Omittable<String>`.
  2. Type reasoning summary-part done `status` as
     `Omittable<ReasoningSummaryPartStatus>` (`incomplete`).
  3. Type image-generation partial-image `size` as `Omittable<String>`
     and `quality` / `background` / `output_format` as the existing
     image-generation open enums.
  4. These optionals are official non-null; unofficial `"field": null`
     fails decode.
- Reason: a remaining-family pin hunt found these official optional
  properties falling through ExtraFields after D0057.
- Impact: Responses SSE event structs. Serde remains lossless.
- Overrides: none
- Tests: `stream_event_optional_fields_match_openapi`.

## D0059 — Official Administration and Usage query filters

- Status: accepted
- Reviewed: 2026-08-30
- Scope: remaining official GET query parameters on Administration
  and Usage operations
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`list-audit-logs` `actor_emails[]` / `resource_ids[]` /
  `tenant_only` / `before`; `list-users` `emails`; `list-projects`
  `include_archived`; `list-project-api-keys` `owner_project_access`;
  `list-project-rate-limits` / spend-alert lists `before`;
  `usage-images` `sources` / `sizes`; `usage-file-search-calls`
  `vector_store_ids`; `usage-web-search-calls` `context_levels`;
  `getCertificate` `include`; `retrieve-project-group` `group_type`)
- Decision:
  1. Type audit-log `actor_emails`, `resource_ids`, and `tenant_only`
     on `AuditLogListParams` as official non-null optionals.
  2. Extend the shared `AdminListParams` bag with official non-null
     `before`, `emails`, `include_archived`, and
     `owner_project_access` (`ProjectAccessFilter`: `active` /
     `inactive` / `any`). Resource `ProjectAccessState` stays
     `active` / `inactive` only.
  3. Extend `UsageQueryParams` with official non-null `sources`,
     `sizes`, `vector_store_ids`, and `context_levels`.
  4. Add `CertificateGetParams.include` and
     `ProjectGroupGetParams.group_type`.
  5. Unofficial `"field": null` fails decode. Array query names keep
     the existing encoder style (`project_ids`, not `project_ids[]`).
- Reason: a remaining-family pin hunt of GET query parameters found
  these official filters missing after D0058.
- Impact: Administration/Usage query DTOs and Admin client helpers.
  Serde remains lossless. `validate()` is not called on decode.
- Overrides: none
- Tests: `admin_query_filters_match_openapi`.

## D0060 — Type official Administration objects previously stored as JSON bags

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Administration response/request objects that the pin types
  structurally but Rust stored as `AdminJsonObject` / non-null scalars
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ProjectApiKey.owner` / `ProjectApiKeyOwnerUser` /
  `ProjectApiKeyOwnerServiceAccount`; `AuditLogActorServiceAccount`;
  `Invite.projects` / `InviteRequest.projects`; `Group` embedded in
  `GroupRoleAssignment`; `UserRoleAssignment.user`; `User.user`;
  `User.projects`; `AssignedRoleDetails` required anyOf-nulls;
  official assignment `object` enums `group.role` / `user.role` /
  `group.user` / `*.deleted`)
- Decision:
  1. Type `ProjectApiKey.owner` as `ProjectApiKeyOwner` with official
     `type` / `user` / `service_account` members.
  2. Type audit-actor `service_account` as
     `AuditActorServiceAccount`.
  3. Type invite project memberships as
     `InviteProjectMembership` (`id` + `member`/`owner`).
  4. Type `GroupRoleAssignment.group` as official `Group`
     (`GroupSummary` with `scim_managed`) and
     `UserRoleAssignment.user` as `User`.
  5. Type `User.user` as `NestedUserDetails` and `User.projects` as
     `UserProjectList` (official anyOf-null envelope).
  6. Type `AssignedRoleDetails.created_at` / `updated_at` /
     `created_by` / `created_by_user_obj` / `metadata` /
     `assignment_sources` as official required `Nullable` values.
  7. Recognize official assignment discriminators on
     `AssignmentObject`. Unofficial historical strings remain known
     variants so decode stays lossless.
- Reason: a remaining-family pin hunt found these official structured
  objects compressed into `AdminJsonObject` or rejected official
  nulls after D0059 closed query-parameter gaps.
- Impact: Administration DTOs. Serde remains lossless.
  `validate()` is not called on decode. `AdminJsonObject` remains
  for official `additionalProperties` bags
  (`created_by_user_obj`, `metadata`, error envelopes).
- Overrides: none
- Tests: `admin_typed_objects_match_openapi`.

## D0061 — Official Usage result discriminators and required-null cursors

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Usage result `object` constants and Administration required
  cursor pages
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`UsageFileSearchCallsResult.object` =
  `organization.usage.file_searches.result`;
  `UsageWebSearchCallsResult.object` =
  `organization.usage.web_searches.result`;
  `ListCertificatesResponse` /
  `ListProjectCertificatesResponse` /
  `OrganizationSpendAlertListResource` /
  `ProjectSpendAlertListResource` required `first_id`/`last_id`
  anyOf string|null;
  `GroupUserDeletedResource.object` = `group.user.deleted`)
- Decision:
  1. Route file-search and web-search usage results with the official
     `file_searches` / `web_searches` discriminators, not the
     endpoint-path `*_calls` strings.
  2. Type `AdminRequiredCursorPage.first_id` / `last_id` as required
     `Nullable<String>` so official `"first_id": null` decodes.
  3. Recognize official `group.user.deleted` on `AssignmentObject`.
- Reason: a remaining-family pin hunt found official payloads decoding
  as `UsageResult::Unknown` and official empty certificate/spend-alert
  pages failing decode after D0060.
- Impact: Administration Usage union routing and certificate/spend-alert
  list pages. Serde remains lossless.
- Overrides: none
- Tests: `usage_bucket_routes_known_results_strictly_and_future_results_losslessly`,
  `admin_required_cursor_page_accepts_official_null_ids`.

## D0062 — Official AuditLog event types and event-specific payloads

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `AuditLog`, `AuditLogEventType`, and official event-specific
  payload objects
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`#/components/schemas/AuditLog` names 55 event-specific properties;
  `#/components/schemas/AuditLogEventType` enumerates 147 strings;
  payload properties have no `required` and no official nulls;
  `certificate.deleted.certificate` is PEM text;
  `AuditLogActorApiKey.type` is `user` | `service_account`;
  `role.bound_to_resource.source` /
  `role.unbound_from_resource.source` are the five official
  connector-mutation paths)
- Decision:
  1. Recognize every official `AuditLogEventType` string as a known
     `AuditEventType` variant. Future unofficial values remain
     `Unknown`.
  2. Type the 55 official dotted event keys on `AuditLog` as
     `Omittable` payload structs (empty official objects stay
     `AdminJsonObject`). Tenant events that exist only on the enum
     keep their payloads in `extra`.
  3. Type `AuditActorApiKey.kind` as the official user/service-account
     enum and `certificate.deleted.certificate` as `WireSecret`.
- Reason: official `api_key.created` / `project.archived` objects were
  stored only in `ExtraFields`, and 123 official event-type strings
  decoded as `Unknown`.
- Impact: Administration audit-log responses. Serde remains lossless
  for future keys and unofficial event types. `validate()` remains
  opt-in.
- Overrides: none
- Tests: `admin_audit_event_payloads_match_openapi`,
  `audit_common_envelope_and_event_specific_payload_are_lossless`.

## D0063 — Official Responses stream/WebSocket error envelopes

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ResponseErrorEvent.code` and `ResponseWsError`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ResponseErrorEvent.code` / `param` are required anyOf string|null;
  `ResponseWsError.error` is `#/components/schemas/ErrorPayload` with
  required `code`/`param` anyOf string|null and optional `headers`)
- Decision:
  1. Type SSE `StreamErrorEvent.code` as required `Nullable<String>`
     so official `"code": null` decodes.
  2. Route GA WebSocket `type=error` objects that nest `error` to
     `ResponsesServerEvent::WebSocketError`, matching the official
     `ResponseWsError` / `ErrorPayload` shape already used on beta.
  3. Name official `ErrorPayload.headers` on Realtime error details.
- Reason: official SSE `"code": null` failed decode, and official
  `ResponseWsError` could not be parsed as the flat SSE error event.
- Impact: Responses SSE/WebSocket error events. Serde remains lossless.
- Overrides: none
- Tests: `stream_events_distinguish_terminal_and_unknown_events`,
  `websocket_events_reuse_create_and_stream_codecs_losslessly`.

## D0064 — Official Realtime documented nulls on transcription and response envelopes

- Status: accepted
- Reviewed: 2026-08-30
- Scope: Realtime transcription-failed error, response lifecycle
  fields, and returned session `include`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`RealtimeServerEventConversationItemInputAudioTranscriptionFailed`
  / `RealtimeBetaServerEventConversationItemInputAudioTranscriptionFailed`
  x-oaiMeta examples send `error.param: null`;
  `RealtimeServerEventResponseCreated` /
  `RealtimeServerEventResponseDone` examples send
  `status_details: null` and `usage: null`;
  `RealtimeCreateClientSecretResponse` example sends
  `session.include: null`;
  official Python `ConversationItemInputAudioTranscriptionFailedEvent.Error`
  types `code`/`param` as `Optional[str]`;
  live `conversation.item.input_audio_transcription.failed` and
  `response.done` payloads send `code: null`)
- Decision:
  1. Type `RealtimeTranscriptionError.code` / `param` as
     `Omittable<Nullable<String>>` so official `"param": null` and
     live `"code": null` decode. `type` / `message` stay non-null
     strings; unofficial `"message": null` still fails.
  2. Type `RealtimeResponse.status_details` / `usage` as
     `Omittable<Nullable<_>>` so official lifecycle examples decode.
  3. Type `RealtimeResponseFailure.code` as
     `Omittable<Nullable<String>>`.
  4. Type returned `RealtimeSession.include` /
     `RealtimeTranscriptionSession.include` as
     `Omittable<Nullable<Vec<String>>>`. Create-request `include`
     stays a non-null array.
- Reason: the pin's documented examples and live Realtime envelopes
  send these nulls, but the property schemas omit `anyOf`/`nullable`.
  `Omittable<T>` rejected the official wire.
- Impact: Realtime receive DTOs. Serde remains lossless.
  `validate()` is not called on decode. Request-side include stays
  omitted-or-array.
- Overrides: none
- Tests: `official_realtime_documented_nulls_decode`.

## D0065 — Official Realtime transcription `language` null

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `AudioTranscriptionResponse.language` /
  `RealtimeAudioTranscription.language`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`RealtimeTranscriptionSessionCreateResponse` and
  `RealtimeTranscriptionSessionCreateResponseGA` x-oaiMeta examples
  send `language: null` inside the transcription object;
  `POST /realtime/transcription_sessions` response example does the
  same; property schema is `type: string` without `anyOf`/`nullable`)
- Decision: type `RealtimeAudioTranscription.language` as
  `Omittable<Nullable<String>>` so official `"language": null`
  decodes on both the shared transcription object and nested GA
  `audio.input.transcription`. `prompt` stays a non-null string;
  unofficial `"prompt": null` still fails. The GA example's
  `"format": "pcm16"` string is a stale shape against official
  `RealtimeAudioFormats` objects and is not accepted.
- Reason: the pin's documented transcription-session examples send
  `language: null`, but `Omittable<String>` rejected the official
  wire.
- Impact: Realtime transcription receive/request config. Serde
  remains lossless. `validate()` is not called on decode.
- Overrides: none
- Tests: `official_realtime_transcription_language_null_decodes`,
  `official_legacy_transcription_session_language_null_decodes`.

## D0066 — Official Chat completion `tool_calls` / `function_call` nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ChatCompletionResponseMessage.tool_calls` /
  `function_call` on stored and create-list responses
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ChatCompletionList` x-oaiMeta example sends
  `choices[].message.tool_calls: null` and
  `function_call: null`; property schemas are a non-null array /
  object without `anyOf`/`nullable`; official Python
  `ChatCompletionMessage` types both as `Optional`)
- Decision: type `ChatResponseMessage` and
  `ChatCompletionStoreMessage` `tool_calls` /
  `function_call` as `Omittable<Nullable<_>>` so the official
  stored-completion list example decodes. Request
  `ChatAssistantMessage.tool_calls` stays a non-null array.
  Stream deltas are unchanged. Unofficial `"annotations": null`
  still fails. Echo-only list fields (`tools`, `tool_choice`,
  `input_user`, `response_format`) remain in `ExtraFields`.
- Reason: the pin's documented Chat completion list example
  sends these nulls, but `Omittable<T>` rejected the official
  wire.
- Impact: Chat completion receive DTOs. Serde remains lossless.
  `validate()` is not called on decode.
- Overrides: none
- Tests: `official_chat_completion_list_message_nulls_decode`.

## D0067 — Official Eval run `sampling_params` and Fine-tuning event `data` nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateEvalCompletionsRunDataSource.sampling_params` /
  `CreateEvalResponsesRunDataSource.sampling_params` and
  `FineTuningJobEvent.data`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`GET /evals/{eval_id}/runs` x-oaiMeta list example sends
  `data_source.sampling_params: null` on a `completions` source;
  `GET /fine_tuning/jobs/{fine_tuning_job_id}/events` x-oaiMeta
  example sends `data: null` on message events; property schemas
  are non-null objects without `anyOf`/`nullable`)
- Decision:
  1. Type model-backed Eval run `sampling_params` as
     `Omittable<Nullable<EvalSamplingParams>>` so the official
     list-run example decodes. Create-request objects still
     serialize as objects when set. Unofficial `"model": null`
     still fails.
  2. Type `FineTuningJobEvent.data` as
     `Omittable<Nullable<Map<String, Value>>>` so official
     `"data": null` decodes. Unofficial `"message": null` still
     fails.
  3. Label-model `sampling_params` remains ExtraFields: the
     official grader schema does not name that property.
- Reason: the pin's documented list examples send these nulls,
  but `Omittable<T>` rejected the official wire.
- Impact: Eval run data-source and Fine-tuning event receive
  DTOs. Serde remains lossless. `validate()` is not called on
  decode.
- Overrides: none
- Tests: `official_eval_run_list_sampling_params_null_decodes`,
  `events_checkpoints_and_pages_preserve_fields_and_cursors`.

## D0068 — Official beta Response `usage` and `user` nulls

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `BetaResponse.usage` / `BetaResponse.user`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`BetaResponseCreatedEvent` / `BetaResponseInProgressEvent`
  x-oaiMeta examples send `response.usage: null` and
  `response.user: null`; `BetaResponseCompletedEvent` also sends
  `user: null`; property schemas are a non-null
  `BetaResponseUsage` ref and a deprecated string without
  `anyOf`/`nullable`. GA `Response` already uses
  `Omittable<Nullable<_>>` for both.)
- Decision: type beta receive `usage` and `user` as
  `Omittable<Nullable<_>>` so official lifecycle envelopes
  decode. Create-request `user` stays a non-null string.
  Unofficial `"status": null` still fails. Example-only
  `reasoning_effort` remains ExtraFields.
- Reason: the pin's documented beta stream examples send these
  nulls, but `Omittable<T>` rejected the official wire.
- Impact: beta Responses receive DTOs. Serde remains lossless.
  `validate()` is not called on decode.
- Overrides: none
- Tests: `official_beta_response_usage_and_user_nulls_decode`.

## D0069 — Official Responses input-item list user message resources

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ResponseInputItem` message routing used by
  `ResponseInputItemList` and `BetaResponseItemList`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ResponseItemList` / `BetaResponseItemList` x-oaiMeta examples and
  `GET /responses/{response_id}/input_items` documented envelopes send
  `{type: message, id, role: user, content: [input_text]}`; official
  `ItemResource` / `BetaItemResource` include
  `InputMessageResource` / `BetaInputMessageResource`, which are
  input messages plus a required `id`. `OutputMessage` role is the
  const `assistant`.)
- Decision: route `type=message` to `OutputMessage` only when the
  object has `id` **and** `role=assistant`. User / system / developer
  resources with an `id` take the existing stored-input branch.
  Unofficial `"status": null` on a stored input message still fails.
- Reason: presence of `id` is not an assistant discriminator;
  official list-input-item envelopes are input-message resources and
  rejected `unknown variant user, expected assistant`.
- Impact: Responses and beta Responses input-item list decode.
  Serde remains lossless. `validate()` is not called on decode.
- Overrides: none
- Tests: `official_response_item_list_user_message_resource_decodes`,
  `official_beta_response_item_list_user_message_resource_decodes`.

## D0070 — Official send-side `file_data` / inline skill / inject limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `InputFileContentParam.file_data`, `InlineSkillSourceParam.data`,
  `BetaResponseInjectEvent.input`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`InputFileContentParam.file_data` string `maxLength` 73400320;
  `InlineSkillSourceParam.data` / `BetaInlineSkillSourceParam.data`
  `minLength` 1 and `maxLength` 70254592; `BetaResponseInjectEvent.input`
  `maxItems` 16384)
- Decision: expose the three official send-side limits as named constants
  and enforce them from opt-in `validate()`:
  1. `InputFile.file_data` has no official `minLength`; present
     non-null strings must be at most 73,400,320 characters.
     Create-response message and function/custom-tool output content
     walks the same helper. Official `"file_data": null` still decodes
     and skips the bound.
  2. Inline skill zip `data` must be `1..=70,254,592` characters on
     Responses shell-container skills and Container create skills.
  3. `BetaResponseInjectEvent.input` must contain at most 16,384 items.
  Serde decode stays lossless. `validate()` is not called on decode.
- Reason: these request `maxLength` / `minLength` / `maxItems` values
  were already in the pin but not hooked into the existing opt-in
  constraint walk used by other official send-side limits.
- Impact: create-response, container create, and beta inject
  `validate()` only.
- Overrides: none
- Tests: `create_request_validate_enforces_official_file_and_skill_payload_limits`,
  `create_container_validate_enforces_inline_skill_source_length`,
  `inject_event_validate_enforces_official_max_items`.

## D0071 — Official Responses WebSocket `stream_id` limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ResponsesClientEventResponseCreate.stream_id` /
  `BetaResponsesClientEventResponseCreate.stream_id`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (both create-event envelopes declare `stream_id` as a string with
  `minLength` 1, `maxLength` 256, and
  `pattern` `^[A-Za-z0-9_.-]+$`)
- Decision: expose `MIN_STREAM_ID_CHARS` / `MAX_STREAM_ID_CHARS` and
  enforce the official length plus `[A-Za-z0-9_.-]` charset from
  opt-in `ResponsesCreateEvent::validate()` and
  `BetaResponsesCreateEvent::validate()`. Omitted `stream_id` skips
  the bound. Unofficial values such as `"agent 1"` still decode.
- Reason: the pin already documented the WebSocket lane id contract,
  but create-event `validate()` did not check it.
- Impact: Responses and beta Responses WebSocket create events.
  `validate()` is not called on decode.
- Overrides: none
- Tests: `websocket_create_validate_enforces_official_stream_id`,
  `create_event_validate_enforces_official_stream_id`.

## D0072 — Official web search `web_search_2025_08_26` type

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `WebSearchTool.type` / `BetaWebSearchTool.type` /
  hosted `ToolChoice`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`WebSearchTool` / `BetaWebSearchTool` `type` enum is
  `web_search` | `web_search_2025_08_26`; default remains
  `web_search`)
- Decision: accept both official discriminators as
  `ResponseTool::WebSearch`. `WebSearchTool::new()` still emits
  `web_search`. `WebSearchTool::web_search_2025_08_26()` emits the
  dated tag. Hosted `tool_choice` `{type: web_search_2025_08_26}`
  decodes as `ToolChoice::Hosted`. Unofficial future tags still
  take the Unknown branch.
- Reason: the dated official tool type was routed to
  `UnknownTaggedObject` because the union only matched
  `web_search`.
- Impact: Responses and beta Responses tool and tool-choice
  decode/send. Serde remains lossless.
- Overrides: none
- Tests: `web_search_and_image_tools_match_python_and_openapi_inventory`,
  `frozen_union_manifests_route_every_known_discriminator_strictly`.

## D0073 — Official web search preview `web_search_preview_2025_03_11` type

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `WebSearchPreviewTool.type` / `BetaWebSearchPreviewTool.type` /
  hosted `ToolChoice`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`WebSearchPreviewTool` / `BetaWebSearchPreviewTool` `type` enum is
  `web_search_preview` | `web_search_preview_2025_03_11`; default remains
  `web_search_preview`)
- Decision: accept both official discriminators as
  `ResponseTool::WebSearchPreview`. `WebSearchPreviewTool::new()` still
  emits `web_search_preview`.
  `WebSearchPreviewTool::web_search_preview_2025_03_11()` emits the dated
  tag. Hosted `tool_choice` `{type: web_search_preview_2025_03_11}`
  already decoded as `ToolChoice::Hosted`. Unofficial future tags still
  take the Unknown branch.
- Reason: the dated official preview tool type was routed to
  `UnknownTaggedObject` because the union only matched
  `web_search_preview`.
- Impact: Responses and beta Responses tool decode/send. Serde remains
  lossless.
- Overrides: none
- Tests: `web_search_and_image_tools_match_python_and_openapi_inventory`,
  `frozen_union_manifests_route_every_known_discriminator_strictly`.

## D0074 — Official input-image and image-reference `image_url` limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `InputImageContentParamAutoParam.image_url` /
  `BetaInputImageContentParamAutoParam.image_url` /
  `ImageRefParam.image_url` / conversation input-image `image_url`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (param `image_url` `maxLength` 20,971,520; official `"image_url": null`
  skips the bound)
- Decision: hook the official send-side `maxLength` into opt-in
  `validate()`. `InputImage::validate()`, create-response input walking,
  `ConversationInputImage::validate()`, and Images JSON
  `ImageReference` / edit-body `validate()` enforce the bound.
  Serde decode remains lossless. Resource `InputImageContent.image_url`
  has no official `maxLength` and is unchanged.
- Reason: this official request `maxLength` was unenforced because the
  same number already existed as compaction `encrypted_content`.
- Impact: Responses, Conversations, and Images JSON edit validate paths.
  Overlong unofficial URLs still decode.
- Overrides: none
- Tests: `create_request_validate_enforces_official_file_and_skill_payload_limits`,
  conversation input-image validate, JSON image-edit `image_url` validate.

## D0075 — Official `input_text` / apply_patch `diff` 10,485,760 limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `InputTextContentParam.text` / `BetaInputTextContentParam.text` /
  `ApplyPatchCreateFileOperationParam.diff` /
  `ApplyPatchUpdateFileOperationParam.diff` /
  `BetaEncryptedContentParam.encrypted_content` / beta create input walking
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (those request strings have `maxLength` 10,485,760)
- Decision: hook the official send-side `maxLength` into opt-in
  `validate()`. `InputText::validate()` and create-response input walking
  enforce input-text. `ApplyPatchCallInput::validate()` enforces create/update
  diffs. Beta create now walks its own `input` items (stable, prompt-cached,
  and inter-agent text/image/encrypted). Serde decode remains lossless.
- Reason: the same 10,485,760 literal already existed for compact input,
  function-call output, and shell stdout/stderr, so a number-only hunt
  treated input-text and apply_patch diffs as covered.
- Impact: Responses and beta Responses validate paths. Overlong unofficial
  payloads still decode.
- Overrides: none
- Tests: `create_request_validate_enforces_official_file_and_skill_payload_limits`,
  `create_request_validate_walks_official_input_text_and_agent_payloads`.

## D0076 — Official domain-secret and apply_patch `path` limits

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ContainerNetworkPolicyDomainSecretParam` /
  `BetaContainerNetworkPolicyDomainSecretParam` /
  apply_patch operation `path`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`domain`/`name` `minLength` 1; `value` `1..=10485760`; apply_patch
  `path` `minLength` 1)
- Decision: hook the official send-side lengths into opt-in `validate()`.
  Container create walks allowlist `domain_secrets`. Apply-patch create,
  update, and delete operations enforce non-empty `path`. ChatKit session
  `user` `minLength` 1 remains constructor/decode-enforced
  (`ChatKitUserId`). Serde decode of unofficial empty/overlong container
  secrets and apply_patch paths remains lossless.
- Reason: a post-D0075 request-string inventory found these remaining
  official length keywords; ChatKit `user` was already enforced.
- Impact: Containers create and Responses apply_patch validate paths.
- Overrides: none
- Tests: `create_container_validate_enforces_official_domain_secret_limits`,
  `create_request_validate_enforces_official_file_and_skill_payload_limits`.

## D0077 — Official Chat content-array `minItems: 1`

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ChatCompletionRequestDeveloperMessage.content` /
  `ChatCompletionRequestSystemMessage.content` /
  `ChatCompletionRequestUserMessage.content` /
  `ChatCompletionRequestAssistantMessage.content` /
  `ChatCompletionRequestToolMessage.content` /
  `PredictionContent.content`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (those request oneOf array branches have `minItems: 1`)
- Decision: hook the official send-side array floor into opt-in `validate()`.
  Chat create walks developer/system/user/tool content parts and present
  assistant content parts. Predicted-output `content` parts use a distinct
  error. Empty string content remains official. Serde decode of unofficial
  empty arrays remains lossless.
- Reason: Chat `validate()` already rejected empty `messages` and empty
  `functions`, but did not walk nested content-array branches.
- Impact: Chat create validate path.
- Overrides: none
- Tests: `chat_create_validate_enforces_pinned_limits`.

## D0078 — Official Completions token-prompt and Eval timestamp floors

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CreateCompletionRequest.prompt` token / token-batch arrays /
  `EvalResponsesSource.created_after` / `created_before`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (token and token-batch prompt arrays `minItems: 1`, including nested
  batches; Eval Responses timestamps `minimum: 0`)
- Decision: hook those official send-side floors into opt-in `validate()`.
  Completions string and string-array prompts have no official `minItems`
  and stay unchecked. `prompt: null` skips the token walk.
  `EvalStoredCompletionsSource` timestamps have no official `minimum` and
  stay unchecked. Serde decode remains lossless.
- Reason: a post-D0076 request-constraint inventory found these remaining
  official `minItems` / `minimum` keywords that `validate()` did not walk.
- Impact: Completions and Evals validate paths.
- Overrides: none
- Tests: `completion_create_validate_enforces_pinned_limits`,
  `eval_responses_source_validate_enforces_official_created_timestamp_minimum`.

## D0079 — Official output-text annotation-added `annotation` null

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `ResponseOutputTextAnnotationAddedEvent.annotation` /
  `Beta` twin (`BetaAnnotation` anyOf-null)
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`annotation` is required `anyOf` `[Annotation, null]` /
  `[BetaAnnotation, null]`)
- Decision: type the required field as `Nullable<Annotation>` so official
  `"annotation": null` decodes on the stable event. Beta stream events
  reuse the stable codec, so the same envelope decodes there. Serde
  remains lossless.
- Reason: a post-D0078 official-null response inventory found this
  remaining required anyOf-null that Rust stored as a non-null
  `Annotation`.
- Impact: Responses / beta Responses stream event decode.
- Overrides: none
- Tests: `file_search_results_and_typed_stream_parts_match_openapi_inventory`,
  `official_beta_output_text_annotation_null_decodes`.

## D0080 — Official JSON image-edit `prompt` `minLength: 1`

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `EditImageBodyJsonParam.prompt`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`EditImageBodyJsonParam.prompt` is a string with `minLength` 1 and
  `maxLength` 32000)
- Decision: hook the official send-side floor into opt-in `validate()`.
  JSON image-edit bodies reject an empty `prompt`. Image generation and
  multipart image-edit `prompt` have no official `minLength` and stay
  unchecked. Serde decode of unofficial empty JSON-edit prompts remains
  lossless.
- Reason: a post-D0079 request-constraint inventory found this remaining
  official `minLength` that `validate()` did not walk. Generation prompt
  limits stay model-specific maxima only.
- Impact: JSON image-edit validate path.
- Overrides: none
- Tests: `image_json_edit_references_are_exact_and_stream_typed`.

## D0081 — Official `max_concurrent_subagents` minimum

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `BetaMultiAgentParam.max_concurrent_subagents`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`BetaMultiAgentParam.max_concurrent_subagents` is an integer with
  `minimum: 1` and no official upper bound)
- Decision: expose `MIN_CONCURRENT_SUBAGENTS` and enforce the official
  floor from opt-in beta create `validate()`. Omitted
  `max_concurrent_subagents` and `multi_agent: null` skip the bound.
  Serde decode of unofficial `0` remains lossless.
- Reason: beta create `validate()` already walked input payloads but not
  this official multi-agent floor.
- Impact: beta Responses create validate path.
- Overrides: none
- Tests: `create_and_count_requests_keep_beta_only_fields_typed`.

## D0082 — Official Realtime client `event_id` `maxLength` 512

- Status: accepted
- Reviewed: 2026-08-30
- Scope: GA `RealtimeClientEvent*` / translation client events `event_id`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (those client-event schemas declare `event_id` as a string with
  `maxLength` 512; `RealtimeClientEventOutputAudioBufferClear.event_id`
  has no official `maxLength`)
- Decision: expose `MAX_REALTIME_EVENT_ID_CHARS` and enforce the official
  ceiling from opt-in `RealtimeClientEvent::validate()` and
  `RealtimeTranslationClientEvent::validate()`. Omitted `event_id` skips
  the bound. `output_audio_buffer.clear` is not checked. Nested
  `session.update` bodies reuse session `validate()`. Serde decode of
  unofficial overlong ids remains lossless.
- Reason: session create already enforced speed, retention, idle-timeout,
  languages, and client-secret lifetime, but client-event `event_id`
  `maxLength` was unhooked.
- Impact: Realtime and translation client-event validate paths.
- Overrides: none
- Tests: `realtime_client_event_validate_enforces_official_event_id`.

## D0083 — Official `UserListResource` items are `GroupUser`

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `UserListResource`, `GroupUser`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`UserListResource.data.items` `$ref` `GroupUser`; `GroupUser` requires
  `id`/`name`/`email` with `email` anyOf-null; `list-group-users` returns
  `UserListResource`; `retrieve-group-user` returns `GroupMemberUser`)
- Decision: decode list-group-users pages as `AdminNextPage<GroupUser>`.
  Keep `GroupMemberUser` for retrieve-group-user. Extra retrieve-only
  fields on a list item stay in `ExtraFields`.
- Reason: official `UserListResource` examples send `{id, name, email}`
  and failed decode when the list alias pointed at `GroupMemberUser`
  (`picture` / `is_service_account` / `user_type` are retrieve-required).
- Impact: Admin group-user list JSON.
- Overrides: none
- Tests: `admin_typed_objects_match_openapi`.

## D0084 — Official `RoleListResource` items are `AssignedRoleDetails`

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `RoleListResource`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`RoleListResource.data.items` `$ref` `AssignedRoleDetails`;
  `PublicRoleListResource.data.items` `$ref` `Role`; list-*-role-assignment
  operations return `RoleListResource`; list-roles / list-project-roles
  return `PublicRoleListResource`)
- Decision: decode role-assignment lists as
  `AdminNextPage<AssignedRoleDetails>`. Keep
  `PublicRoleListResource = AdminNextPage<Role>`.
- Reason: official assignment-list examples omit `Role.object` and include
  AssignedRoleDetails timestamps / metadata / assignment_sources. Decoding
  those pages as `Role` rejected official wire.
- Impact: Admin role-assignment list JSON.
- Overrides: none
- Tests: `admin_typed_objects_match_openapi`.

## D0085 — Compact/count and nested tool `validate()` walk official Tool/InputItem bounds

- Status: accepted
- Reviewed: 2026-08-30
- Scope: `CompactResponseRequest`, `BetaCompactResponseRequest`,
  `CountInputTokensRequest`, `BetaCountInputTokensRequest`,
  `AdditionalToolsInput`, `ToolSearchOutputInput`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`CompactResponseMethodPublicBody.input` / `TokenCountsBody.input` are
  string-or-`InputItem` array; `TokenCountsBody.tools` items `$ref` `Tool`;
  `AdditionalToolsItemParam.tools` and `ToolSearchOutputItemParam.tools`
  items `$ref` `Tool`)
- Decision: opt-in `validate()` on compact and count-tokens bodies walks
  item-array `input` with the same helpers as create. Count-tokens also
  walks top-level `tools`. `additional_tools` and `tool_search_output`
  input items walk nested `tools`. Serde decode of unofficial values
  remains lossless. Compact official `instructions` stays string-or-null
  and is not treated as an item array.
- Reason: create `validate()` already enforced official Tool/InputItem
  bounds, but compact/count and nested tool-bearing input items skipped
  those walks.
- Impact: Responses compact/count and create input-item validate paths.
- Overrides: none
- Tests: `compact_and_count_validate_walk_official_input_items_and_tools`,
  `create_and_count_requests_keep_beta_only_fields_typed`.

## D0086 — Fine-tune grader, container allowlist, and image-edit count floors

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CreateFineTuningJobRequest` reinforcement grader
  `sampling_params.max_completions_tokens`, `CreateContainerBody`
  allowlist `allowed_domains` / `domain_secrets`,
  `ImageEditJsonRequestBody.images`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`GraderScoreModel.sampling_params.max_completions_tokens` `minimum: 1`;
  `ContainerNetworkPolicyAllowlistParam.allowed_domains` /
  `domain_secrets` `minItems: 1`; JSON image-edit `images`
  `minItems: 1` / `maxItems: 16`)
- Decision: opt-in parent `validate()` walks those official bounds.
  Nested `multi` graders are walked. Serde decode of unofficial `0`
  tokens remains lossless. Empty allowlist arrays remain decode-rejected
  and are now also validate-rejected for programmatic construction.
- Reason: create `validate()` already walked fine-tune hyperparameters,
  container skill/secret contents, and JSON edit prompts, but skipped
  these official collection/token floors.
- Impact: Fine-tuning create, Container create, JSON image-edit validate
  paths.
- Overrides: none
- Tests: `fine_tuning_create_validate_enforces_score_model_token_floor`,
  `create_container_validate_enforces_official_domain_secret_limits`,
  `image_json_edit_references_are_exact_and_stream_typed`.

## D0087 — Vector Store operations send official `OpenAI-Beta: assistants=v2`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: all Vector Store client operations
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`x-oaiMeta.curl` on
  list/create/get/modify/delete/file/file-batch operations) and the
  current Python SDK (`src/openai/resources/vector_stores/vector_stores.py`
  always injects `OpenAI-Beta: assistants=v2`, including search / file
  attribute / content)
- Decision: every Vector Store JSON call sends
  `OpenAI-Beta: assistants=v2` via the existing static-header transport.
  The header is not an OpenAPI `parameters` entry; official curl and the
  Python SDK are the send-side authority.
- Reason: ChatKit and legacy Realtime already sent their documented
  `OpenAI-Beta` values; Vector Stores omitted the official Assistants
  beta header, so live calls diverged from the pin examples and Python
  SDK.
- Impact: `openai-rs-client` Vector Store transport.
- Overrides: none
- Tests: `store_crud_list_and_search_match_pinned_routes`,
  `attached_file_routes_preserve_ids_query_and_bodies`,
  `file_batch_routes_match_pinned_contract`.

## D0088 — Official Realtime `post_instructions` `minimum: 0`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimeTruncationTokenLimits.post_instructions`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`RealtimeTruncation` retention-ratio `token_limits.post_instructions`
  is an integer with `minimum: 0`)
- Decision: expose `MIN_REALTIME_POST_INSTRUCTIONS` and enforce the
  official floor from opt-in session `validate()` (including nested
  `session.update` / client-secret session bodies). Omitted
  `post_instructions` skips the bound. Serde decode of unofficial
  negatives remains lossless.
- Reason: session create already enforced `retention_ratio` but not this
  nested official floor. The field is `i64`, so `minimum: 0` is not
  vacuous.
- Impact: Realtime session create / update validate paths.
- Overrides: none
- Tests: `realtime_prompt_null_and_output_speed_match_python_and_openapi_inventory`.

## D0089 — Official moderation result `category_applied_input_types` is required

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CreateModerationResponse.results[].category_applied_input_types`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`CreateModerationResponse` result items `required` includes
  `category_applied_input_types`; Responses/Chat moderation outcome
  objects already require the same field)
- Decision: store the field as a required
  `BTreeMap<String, Vec<ModerationAppliedInputType>>`. Category names
  stay an open map for forward-compatible extra keys. Do not weaken
  requiredness because the legacy `text-moderation-007` path example
  omits the property.
- Reason: `POST /moderations` results treated the official required
  property as `Omittable`, so re-encoded official omni envelopes could
  drop a schema-required key and incomplete mocks could decode.
- Impact: `openai-rs-types` moderation response DTO and client mock.
- Overrides: none
- Tests: `moderation_response_preserves_unknown_categories_and_fields`,
  `moderations_create_sends_typed_body`.

## D0090 — Official beta Responses `include` names `web_search_call.results`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaIncludeEnum` / `BetaResponseIncludable`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`BetaIncludeEnum` and
  GA `IncludeEnum` both enumerate `web_search_call.results`; GA
  `ResponseIncludable` and `ConversationItemInclude` already name it)
- Decision: add the official `web_search_call.results` member as
  `BetaResponseIncludable::WebSearchResults`. Unknown future include
  strings remain lossless via `Unknown`.
- Reason: beta retrieve / list / create `include` helpers only accepted
  the official value through `Unknown`, so the pinned beta include
  inventory was incomplete relative to the official enum and the GA
  sibling.
- Impact: `openai-rs-types` beta Responses include enum.
- Overrides: none
- Tests: `official_beta_include_enum_names_web_search_results`.

## D0091 — Official Responses `MessageRole` names critic, tool, and unknown

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `MessageRole` / `BetaMessageRole`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`MessageRole` and
  `BetaMessageRole` both enumerate `unknown`, `user`, `assistant`,
  `system`, `critic`, `discriminator`, `developer`, and `tool`;
  conversation sibling `ConversationMessageRole` already names them)
- Decision: add the official members as `UnknownRole`, `Critic`,
  `Discriminator`, and `Tool`. Unknown future role strings remain
  lossless via the catch-all `Unknown`. Official item-form
  `InputMessage` stays `user` / `system` / `developer`
  (`StoredInputMessageRole`); official `OutputMessage` stays
  assistant-only.
- Reason: Responses `InputMessage` / `AdditionalTools` only accepted
  the extra official roles through `Unknown`, so the pinned
  `MessageRole` inventory was incomplete relative to the official
  enum and the conversation sibling.
- Impact: `openai-rs-types` Responses message role enum (shared with
  beta Responses).
- Overrides: none
- Tests: `official_message_role_names_all_pin_members`.

## D0092 — Official Response usage requires `cache_write_tokens`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ResponseUsage.input_tokens_details.cache_write_tokens`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`ResponseUsage`
  `input_tokens_details.required` is `cached_tokens` and
  `cache_write_tokens`); official Python SDK
  `InputTokensDetails.cache_write_tokens: int` (not `Optional`);
  official compact `x-oaiMeta.example` sends `"cache_write_tokens": 0`
- Decision: model `cache_write_tokens` as a required `u64` on
  `InputTokensDetails`. Chat/Completions `prompt_tokens_details` stays
  `Omittable` because the official `CompletionUsage` nested object does
  not list that property as required.
- Reason: callers could not rely on the official required cache-write
  count; omitting it was a field-requiredness offset of the same class
  as D0089. Incomplete fixtures that drop the property are not
  authority to weaken the pin.
- Impact: `openai-rs-types` Responses usage DTO (shared with beta
  Responses and compact).
- Overrides: none
- Tests: `official_response_usage_requires_cache_write_tokens`.

## D0093 — Official prompt-cache request and response schemas are distinct

- Status: accepted
- Reviewed: 2026-08-31
- Scope: Responses / beta Responses `prompt_cache_options`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`CreateResponse` /
  compact bodies `$ref` `PromptCacheOptionsParam` with no required
  properties; `Response` / `BetaResponse` `$ref` `PromptCacheOptions` /
  `BetaPromptCacheOptions` whose `required` is `ttl` and `mode`);
  official Python SDK `Response.PromptCacheOptions.mode` and `ttl` are
  non-`Optional` literals
- Decision: keep request builders on `PromptCacheOptionsParam` /
  `BetaPromptCacheOptionsParam` with independently omittable `ttl` and
  `mode`. Model the response echo as `PromptCacheOptions` /
  `BetaPromptCacheOptions` with both fields required. Chat create stays
  on the request-param shape because `CreateChatCompletionRequest`
  `$ref`s `PromptCacheOptionsParam` and the Chat completion resource
  does not echo the official options object.
- Reason: a single `Omittable` type for both schemas hid the official
  requiredness of the response echo, the same class of field offset as
  D0092. Incomplete request objects are not authority to weaken the
  resource schema.
- Impact: `openai-rs-types` Responses and beta Responses prompt-cache
  DTOs. Create/compact request constructors take the Param type.
- Overrides: none
- Tests: `official_response_prompt_cache_options_requires_ttl_and_mode`.

## D0094 — Official compact resource requires `usage`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CompactResource.usage` / Rust `CompactedResponse.usage`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9` (`CompactResource.required`
  is `id`, `object`, `output`, `created_at`, `usage`; `usage` `$ref`s
  `ResponseUsage` without anyOf-null); official schema example and
  compact `x-oaiMeta` response example send `usage` including
  `cache_write_tokens`; official Python SDK
  `CompactedResponse.usage: ResponseUsage` (not `Optional`)
- Decision: model compact `usage` as required `ResponseUsage`. Unofficial
  omit and `"usage": null` fail decode. Beta compact already required
  `ResponseUsage`. Stored `Response.usage` stays optional because the
  official `Response` schema does not list `usage` in `required`.
- Reason: callers could not rely on official compaction token
  accounting; treating it as omittable/nullable was a field-requiredness
  offset of the same class as D0092. Incomplete fixtures that send
  `"usage": null` are not authority to weaken the pin.
- Impact: `openai-rs-types` compact response DTO and compact client
  loopback fixtures.
- Overrides: none
- Tests: `official_compact_resource_requires_usage`.

## D0095 — Official certificate activate/deactivate `object` names

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CertificateScopeResponse.object`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`OrganizationCertificateActivationResponse`,
  `OrganizationCertificateDeactivationResponse`,
  `OrganizationProjectCertificateActivationResponse`, and
  `OrganizationProjectCertificateDeactivationResponse` each require
  `object` with a closed enum:
  `organization.certificate.activation`,
  `organization.certificate.deactivation`,
  `organization.project.certificate.activation`,
  `organization.project.certificate.deactivation`)
- Decision: type the shared activate/deactivate envelope on
  `CertificateScopeObject` and name all four official members. List-page
  `AdminListObject` stays `list` / `page` only.
- Reason: the shared envelope reused `AdminListObject`, so official
  activate/deactivate discriminators decoded only as `Unknown`. That is
  the same named-member gap as D0090 / D0091.
- Impact: `openai-rs-types` admin certificate activate/deactivate
  response DTO.
- Overrides: none
- Tests: `official_certificate_scope_object_names_all_pin_members`.

## D0096 — Official compact request requires `model`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CompactResponseRequest.model` / `BetaCompactResponseRequest.model`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`CompactResponseMethodPublicBody` and
  `BetaCompactResponseMethodPublicBody` list `model` in `required`;
  `ModelIdsCompaction` / `BetaModelIdsCompaction` are `anyOf` including
  `null`); official Python SDK `ResponseCompactParams.model` is
  `Required[Union[Literal[...], str, None]]`
- Decision: model compact request `model` as required `Nullable<String>`.
  Unofficial omit fails decode. Official `"model": null` still decodes
  and is always serialized, preserving D0033's official-null send path.
  Token-count request `model` stays `Omittable` because official
  `TokenCountsBody` does not list it in `required`.
- Reason: callers could omit the official compaction model, the same
  class of requiredness offset as D0092 / D0094. Official `required`
  plus the Python `Required[...]` annotation supersede D0033's
  `Omittable` wrapping; incomplete fixtures that omit `model` are not
  authority to weaken the pin.
- Impact: `openai-rs-types` GA and beta compact request DTOs.
- Overrides: none
- Tests: `official_compact_request_requires_model`.

## D0097 — Official list pages require non-null cursor ids

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ResponseInputItemList`, `ConversationItemList`, `EvalList`,
  `EvalRunList`, `EvalRunOutputItemList` `first_id` / `last_id`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ResponseItemList`, `BetaResponseItemList`, `ConversationItemList`,
  `EvalList`, `EvalRunList`, and `EvalRunOutputItemList` each require
  `first_id` and `last_id` as `string` with no `anyOf`/`nullable`);
  official `ResponseItemList` / `EvalRunList` examples send string ids;
  official Python SDK `ResponseItemList.first_id` / `last_id` and
  `ConversationItemList.first_id` / `last_id` are `str` (not
  `Optional`)
- Decision: type those cursor ids as required non-null strings / opaque
  ids. Unofficial `"first_id": null` fails decode. Empty pages may send
  `""`, matching D0007 File lists. ChatKit / Skills / Admin required
  cursor pages stay `Nullable` because those official schemas include
  `anyOf` null. Fine-tuning and voice consent lists stay omittable
  because official schemas do not require the fields.
- Reason: GA `ResponseInputItemList` reused required-null list cursors
  while the official pin, official examples, Python SDK, and the beta
  sibling already require strings. Incomplete fixtures that send null
  are not authority to weaken the pin, the same class as D0007 / D0094.
- Impact: `openai-rs-types` list-page DTOs and client loopback fixtures
  / pagination getters.
- Overrides: none
- Tests: `official_response_item_list_requires_cursor_ids`.

## D0098 — Official function-shell action request and resource schemas are distinct

- Status: accepted
- Reviewed: 2026-08-31
- Scope: Responses `FunctionShellCallInput.action` /
  `FunctionShellCall.action`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`FunctionShellActionParam` / `BetaFunctionShellActionParam`
  `required` is `["commands"]`; `FunctionShellAction` /
  `BetaFunctionShellAction` `required` is `commands` +
  `timeout_ms` + `max_output_length`, both `anyOf` integer|null);
  official Python SDK `ShellCallAction` is `total=False` with
  `commands: Required[...]` and `timeout_ms` /
  `max_output_length: Optional[int]`
- Decision: keep input-item builders on `FunctionShellActionParam`
  with independently omittable nullable limits. Model the resource
  echo as `FunctionShellAction` with both limits required-null.
  Beta reuses the GA types. Official `"timeout_ms": null` still
  decodes on both shapes. Unofficial resource objects that omit
  the limits fail decode.
- Reason: a single required-null type rejected official Param
  payloads that omit the limits, the same class of request/resource
  split as D0093. Incomplete resource fixtures that omit the
  limits are not authority to weaken the resource schema.
- Impact: `openai-rs-types` Responses function-shell action DTOs.
  `FunctionShellCallInput::new` accepts the Param type (and
  `From<FunctionShellAction>`).
- Overrides: none
- Tests: `official_function_shell_action_param_omits_limits`.

## D0099 — Official output content names `reasoning_text`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: Responses `OutputContent` / stream `response.content_part.*`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`OutputContent` / `BetaOutputContent` `oneOf` is
  `OutputTextContent` + `RefusalContent` + `ReasoningTextContent`;
  `ResponseContentPartAddedEvent.part` and
  `ResponseContentPartDoneEvent.part` `$ref` `OutputContent`;
  `ReasoningTextContent.required` is `type` + `text`); official
  Python SDK `ResponseContentPartAddedEvent.part` is
  `Union[ResponseOutputText, ResponseOutputRefusal, PartReasoningText]`
  with `PartReasoningText.type: Literal["reasoning_text"]` and
  `text: str`
- Decision: name `reasoning_text` as `OutputContent::ReasoningText`
  using the existing `ReasoningTextContent` record. Official
  `OutputMessageContent` stays `output_text` + `refusal` only; the
  shared union is the wider stream `OutputContent` schema. Future
  tags still decode as `Unknown`. Conversation message content
  already named this member.
- Reason: official stream `content_part` payloads decoded only as
  `Unknown`, the same named-member gap as D0090 / D0091 / D0095.
  Incomplete message-content fixtures that omit `reasoning_text`
  are not authority to drop the official `OutputContent` member.
- Impact: `openai-rs-types` Responses output-content union and
  content-part stream events. Beta reuses the GA union.
- Overrides: none
- Tests: `official_output_content_names_reasoning_text`.

## D0100 — Official input-image content requires `detail`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: Responses `InputImage.detail` / `BetaAgentInputImage.detail`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`InputImageContent` / `BetaInputImageContent` `required` is
  `type` + `detail`; `detail` `$ref`s `ImageDetail` /
  `BetaImageDetail` with no `anyOf` null;
  `InputContent` / `InputMessageContentList` / `EasyInputMessage`
  and `BetaAgentMessage.content` `$ref` those resource schemas;
  official Python SDK `ResponseInputImage.detail: ImageDetail`
  is not `Optional`); conversation sibling
  `ConversationInputImage.detail` is already required `ImageDetail`
- Decision: model message/input-content `detail` as required
  `ImageDetail`. Constructors send the documented default `auto`.
  Unofficial omit and `"detail": null` fail decode. Official
  `InputImageContentParamAutoParam` (function-call output request
  arrays only) still allows omit/null; that Param schema is not
  the `InputContent` resource. Locator `image_url` / `file_id`
  stay required-null. File `detail` stays omittable because
  official `InputFileContent` does not require it.
- Reason: a single Param-shaped `Omittable<Nullable<ImageDetail>>`
  hid the official resource requiredness of `InputImageContent`,
  the same class of request/resource offset as D0093 / D0098.
  Incomplete create-response path examples that omit `detail`
  are not authority to weaken the pin (D0010 / D0096). D0034's
  Param `anyOf` null applies to `InputImageContentParamAutoParam`,
  not to `InputContent`.
- Impact: `openai-rs-types` Responses and beta agent input-image
  DTOs. `detail_null()` is removed from those resource types.
- Overrides: none
- Tests: `official_input_image_content_requires_detail`.

## D0101 — Official function-call output request image is Param-shaped

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `FunctionCallOutput.output` image parts
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`FunctionCallOutputItemParam.output` array items `$ref`
  `InputImageContentParamAutoParam`, `required` is only `type`,
  `detail` is `anyOf` including null;
  `FunctionAndCustomToolCallOutput` / `FunctionToolCallOutput`
  `$ref` `InputImageContent` whose `required` includes
  `detail`); official Python SDK
  `ResponseInputImageContentParam.detail` is `Optional`
- Decision: keep message/input-content images on `InputImage`
  (D0100). Model function-call output *request* images as
  `InputImageParam` inside `FunctionCallOutputParamValue`.
  Official Param omit and `"detail": null` decode. Resource
  `FunctionCallOutputResource` / `CustomToolCallOutput` stay on
  `FunctionCallOutputValue` with `InputContent` / required
  `detail`. `to_input_items` converts resource images into the
  Param type.
- Reason: D0100's resource requiredness rejected official
  `FunctionCallOutputItemParam` payloads that omit `detail`,
  the same request/resource split as D0098. Incomplete message
  fixtures are not authority to weaken `InputImageContent`.
- Impact: `openai-rs-types` function-call output input-item DTO.
- Overrides: none
- Tests: `official_function_call_output_param_omits_image_detail`.

## D0102 — Official beta agent-message request image is Param-shaped

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaAgentMessage.content` image parts
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`BetaAgentMessageItemParam.content` array items `$ref`
  `BetaInputImageContentParamAutoParam`, `required` is only `type`,
  `detail` is `anyOf` including null;
  `BetaAgentMessage` resource content `$ref` `BetaInputImageContent`
  whose `required` includes `detail`)
- Decision: keep resource `BetaAgentInputImage.detail` required
  (D0100). Model inter-agent *request* images as
  `BetaAgentInputImageParam` inside `BetaAgentMessageContent`.
  Official Param omit and `"detail": null` decode. Resource
  `BetaInputImageContent` still rejects omit/null. Converting a
  resource image into a message part preserves required `detail`.
- Reason: D0100's resource requiredness rejected official
  `BetaAgentMessageItemParam` payloads that omit `detail`,
  the same request/resource split as D0101. Incomplete message
  fixtures are not authority to weaken `BetaInputImageContent`.
- Impact: `openai-rs-types` beta agent-message input-item DTO.
- Overrides: none
- Tests: `official_beta_agent_message_param_omits_image_detail`,
  `official_beta_input_image_content_requires_detail`.

## D0103 — Official beta agent-message resource content is fully named

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaAgentMessage.content` resource union
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`BetaAgentMessage.content` items `$ref` `BetaInputTextContent`,
  `BetaOutputTextContent`, `BetaTextContent`,
  `BetaSummaryTextContent`, `BetaReasoningTextContent`,
  `BetaRefusalContent`, `BetaInputImageContent`,
  `BetaComputerScreenshotContent`, `BetaInputFileContent`,
  `BetaEncryptedContent`)
- Decision: name every official resource content tag on
  `BetaAgentMessageContent`. Request Param images stay
  `BetaAgentInputImageParam` (D0102). Reuse GA `OutputText`,
  `Refusal`, `SummaryTextContent`, `ReasoningTextContent`,
  `ComputerScreenshot`, and `InputFile` for identical wire
  shapes; add `BetaAgentText` for official `type: "text"`.
- Reason: resource `output_text` / `reasoning_text` / `refusal` /
  `summary_text` / `text` / `computer_screenshot` / `input_file`
  decoded only as `Unknown`, the same named-member gap as D0099.
  Incomplete request Param fixtures that omit those members are
  not authority to drop the official resource union.
- Impact: `openai-rs-types` beta agent-message content union.
- Overrides: none
- Tests: `official_beta_agent_message_names_resource_content`.

## D0104 — Official file-input detail is not image `original`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `InputFile.detail` / `ConversationInputFile.detail`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`InputFileContent.detail` `$ref` `FileInputDetail`;
  `InputFileContentParam.detail` `$ref` `FileDetailEnum`;
  `BetaInputFileContent.detail` `$ref` `BetaFileInputDetail`;
  all three enums are `auto` / `low` / `high`.
  `InputImageContent.detail` `$ref` `ImageDetail` which also
  names `original`)
- Decision: model file `detail` as `FileDetail` (`auto` / `low` /
  `high`). Image and screenshot `detail` stay `ImageDetail`
  (includes `original`). Unofficial `"original"` on a file still
  decodes losslessly as `FileDetail::Unknown`.
- Reason: sharing `ImageDetail` made official-only image
  `original` a named file-detail member, the same class of
  official domain offset as D0091 / D0095. Incomplete fixtures
  that omit file `detail` are not authority to keep the wider
  image enum.
- Impact: `openai-rs-types` Responses and Conversations file
  content DTOs.
- Overrides: none
- Tests: `official_input_file_content_uses_file_detail_domain`.

## D0105 — Official Responses file-search ranker is `RankerVersionType`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `FileSearchRanker` / `FileSearchRankingOptions.ranker`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`RankingOptions.ranker` / `BetaRankerVersionType` are
  `auto` / `default-2024-11-15`. Official schema named
  `FileSearchRanker` is Assistants-only
  `auto` / `default_2024_08_21` and is not the Responses
  ranking enum. Vector Store search already uses a separate
  `none` / `auto` / `default-2024-11-15` domain)
- Decision: name only official `RankerVersionType` members on
  Responses `FileSearchRanker`. Assistants
  `default_2024_08_21` decodes losslessly as `Unknown`.
- Reason: sharing the Assistants ranker name hid the official
  Responses domain, the same class of official enum-domain
  offset as D0104. Assistants remain omitted (D0013).
- Impact: `openai-rs-types` Responses file-search ranking DTO.
- Overrides: none
- Tests: `official_file_search_ranker_matches_ranker_version_type`.

## D0106 — Official Response error code is `ResponseErrorCode`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ResponseError.code`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`ResponseError.code` `$ref`s `ResponseErrorCode` with
  twenty named values including `data_residency_mismatch`.
  Official `ResponseErrorEvent.code` / stream `ErrorPayload`
  remain `anyOf [string, null]` and are not this enum.
  Python SDK `ResponseError.code` is the same
  `Literal[...]` set)
- Decision: model `ResponseError.code` as
  `ResponseErrorCode`. Unofficial codes decode losslessly as
  `Unknown`. Stream error codes stay open strings.
- Reason: storing the official named domain as `String` hid
  the pin, the same class of official enum-domain offset as
  D0104 / D0105.
- Impact: `openai-rs-types` Responses `ResponseError` DTO.
- Overrides: none
- Tests: `official_response_error_code_matches_response_error_code`.

## D0107 — Official tool `allowed_callers` is `CallableToolAllowedCaller`

- Status: accepted
- Reviewed: 2026-08-31
- Scope: hosted-tool `allowed_callers`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`CallableToolAllowedCaller` / `BetaCallableToolAllowedCaller`
  are `direct` / `programmatic`. Python SDK types the same
  field as `List[Literal["direct", "programmatic"]]`)
- Decision: model `allowed_callers` as
  `Vec<AllowedCaller>`. Unofficial callers decode losslessly
  as `Unknown`. Empty present arrays remain unofficial
  (`validate()` opt-in).
- Reason: storing the official two-value domain as
  `Vec<String>` hid the pin, the same class as D0106.
- Impact: `openai-rs-types` Responses tool DTOs that carry
  `allowed_callers`.
- Overrides: none
- Tests: `official_allowed_caller_matches_callable_tool_allowed_caller`.

## D0108 — Official MCP `connector_id` is the connector enum

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `McpTool.connector_id`
- Sources: pinned OpenAPI commit
  `690521b1753dce0c6d6b275f583d22537679cff9`
  (`MCPTool.connector_id` / `BetaMCPTool.connector_id` enumerate
  the eight `connector_*` service ids. Python SDK types the
  same field as `Literal["connector_dropbox", ...]`)
- Decision: model MCP `connector_id` as `McpConnectorId`.
  Unofficial connector ids decode losslessly as `Unknown`.
  Admin audit-log `connector_id` stays an opaque string.
- Reason: storing the official connector domain as `String`
  hid the pin, the same class as D0106 / D0107.
- Impact: `openai-rs-types` Responses MCP tool DTO.
- Overrides: none
- Tests: `official_mcp_connector_id_matches_connector_enum`.

## D0109 — Responses audio SSE ghost response_id is not a typed field

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `response.audio.done`, `response.audio.transcript.delta`, `response.audio.transcript.done`
- Sources: pinned schemas list `response_id` in `required` but omit it from `properties`; official Python/Node event types expose `sequence_number`/`type`/`delta` only.
- Decision: do not require typed `response_id`. If the key appears on the wire it is retained in `ExtraFields`.
- Impact: override `OVR-0008`.
- Tests: audio.done without `response_id`; extra-field preservation when present.

## D0110 — LocalShellCallOutput ghost call_id is not required

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `LocalShellCallOutput`
- Sources: pinned `LocalShellToolCallOutput.required` includes `call_id` but `properties` omit it; official output shape is `{id, output, status?}`.
- Decision: do not require typed `call_id`. If present it is retained as `Omittable` or `ExtraFields`.
- Impact: override `OVR-0009`.
- Tests: output fixture without `call_id`.

## D0111 — MCPApprovalResponseResource ghost request_id is not a typed field

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `McpApprovalResponseResource`
- Sources: pinned OpenAPI lists `request_id` in `required` but omits it from `properties`; official Python `McpApprovalResponse` exposes only `approval_request_id`.
- Decision: output resources do not require `request_id`. If present it is retained in `ExtraFields`. Input DTOs continue to send only `approval_request_id`.
- Impact: override `OVR-0007`.
- Tests: output fixture without `request_id`; extra-field preservation when present.

## D0112 — Create, compact, and token-count instructions are string or null

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CreateResponseBody.instructions`, `CompactResponseRequest.instructions`, `CountInputTokensRequest.instructions`
- Sources: pinned `CreateResponse` / `CompactResponseMethodPublicBody` / `TokenCountsBody` type `instructions` as `string | null`. `Response.instructions` remains `string | InputItem[] | null`.
- Decision: request DTOs send/accept only `Omittable<Nullable<String>>`. Resource `Response.instructions` keeps `ResponseInstructions`.
- Tests: create/compact reject instruction item arrays; string/null round-trip.

## D0113 — Compact resource output is the ItemField input union

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CompactedResponse.output` / `BetaCompactedResponse.output`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`CompactResource.output.items` / `BetaCompactResource.output.items` `$ref` `ItemField` / `BetaItemField`, whose Message branch accepts any `MessageRole`; the spec's own CompactResource example returns user-role messages plus a compaction item; Python `compacted_response.py` documents "a list of all user messages, followed by a single compaction item").
- Decision: decode compact output with the input-side unions (`Vec<ResponseInputItem>` / `Vec<BetaResponseInputItem>`), matching the pinned ItemField branches. The assistant-only `ResponseOutputItem::Message` previously rejected `role: "user"`, failing the whole 200 body.
- Reason: the compact endpoint is documented to return user messages; the output-side codec cannot represent them.
- Impact: `openai-rs-types` Responses + beta Responses compact resources; `output()` getter return type changes accordingly.
- Overrides: none
- Tests: `official_compact_resource_output_decodes_user_messages_and_compaction_item`, `beta_compact_resource_output_decodes_user_messages_and_compaction_item`.

## D0114 — File-search result attributes and Response store are three-state

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `FileSearchResult.attributes` / `Response.store`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`FileSearchToolCall.results[].attributes` → `VectorStoreFileAttributes` = `anyOf [{16-key map}, null]`; Python models it `Optional[Dict] = None`. The `Response` resource property lists have no `store`; the request-side `store` is `anyOf [bool, null]`).
- Decision: model both as `Omittable<Nullable<..>>`. `attributes` gains `attributes_null()` and `attributes_ref()`; `store` stays a convenience field so an unofficial `"store": null` echo no longer fails the decode.
- Reason: a null attributes echo under `include=file_search_call.results` previously failed the whole `Response` decode; `store` null echo likewise.
- Impact: `openai-rs-types` Responses DTOs.
- Overrides: none
- Tests: `file_search_result_attributes_support_omitted_null_and_present`, `response_store_null_echo_decodes_and_round_trips`.

## D0115 — Official program/apply-patch/tool-search status enums

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ProgramOutputItem.status`, `ApplyPatchCall(Input).status`, `ApplyPatchCallOutput(Input).status`, `ToolSearchCall/Output/Input.execution`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`ProgramOutputStatus` completed|incomplete; `ApplyPatchCallStatus(Param)` in_progress|completed; `ApplyPatchCallOutputStatus(Param)` completed|failed; `ToolSearchExecutionType` server|client. Python models each as `Literal`).
- Decision: model as `ProgramOutputStatus` / `ApplyPatchCallStatus` / `ApplyPatchCallOutputStatus` open string enums, and reuse the existing `ToolSearchExecution` for all four item `execution` fields. Unofficial values decode losslessly as `Unknown`.
- Reason: storing official enum domains as bare `String` hid the pin, the same class as D0106 / D0107.
- Impact: `openai-rs-types` Responses item DTOs; builders now take `impl Into<Enum>`.
- Overrides: none
- Tests: `official_status_and_execution_enums_retain_unknown_values`.

## D0116 — Namespace tools are function/custom only

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `NamespaceTool.tools`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`NamespaceToolParam.tools.items` = `oneOf [FunctionToolParam, CustomToolParam]`; Python `Tool = Union[ToolFunction, CustomToolParam]`).
- Decision: introduce `NamespaceToolEntry` (tagged union of `FunctionTool` / `CustomTool`, wire-identical to the matching `ResponseTool` branches) so hosted tools such as nested `web_search` are unconstructible at compile time; genuinely future nested tags decode losslessly as `Unknown`. Chose the typed union over a runtime `validate()` check.
- Reason: the element domain was wider than the pin and a nested hosted tool would produce a request the pin rejects.
- Impact: `openai-rs-types` Responses `NamespaceTool` constructor/getter.
- Overrides: none
- Tests: `namespace_tool_entries_are_function_or_custom_only`.

## D0117 — Code-interpreter allowlist domain secrets

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CodeInterpreterNetworkAllowlist.domain_secrets`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`AutoCodeInterpreterToolParam.network_policy` → `ContainerNetworkPolicyAllowlistParam.domain_secrets`, minItems 1, elements of `ContainerNetworkPolicyDomainSecretParam` with `domain`/`name` minLength 1 and `value` 1..=10485760; Python types it).
- Decision: add optional `domain_secrets: Omittable<Vec<CodeInterpreterDomainSecret>>` with `with_secret()` builder and `validate()` hooks reusing the D0076 containers-side limits. `CodeInterpreterDomainSecret` is a responses-side mirror of the containers wire shape (`WireSecret` value, redacted Debug, exposed-only PartialEq) because `ContainerDomainSecret` lacks `PartialEq` and the whole code-interpreter DTO chain derives it.
- Reason: the official optional field was missing from the typed surface.
- Impact: `openai-rs-types` Responses code-interpreter network policy.
- Overrides: none
- Tests: `code_interpreter_allowlist_domain_secrets_serialize_and_validate`.

## D0118 — SSE terminal table matches the pinned stream events

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `SseEndpointPolicy::responses()` terminal events
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`ResponseStreamEvent` has exactly 58 discriminators; `response.cancelled` exists only as a webhook event; Python/Node have no such stream event); types-side `ResponseStreamEvent::is_terminal()` recognizes only completed/failed/incomplete/error.
- Decision: drop `response.cancelled` from the Responses SSE terminal table so the transport and typed codecs agree; the tag, if ever observed, is delivered as an ordinary event.
- Reason: the extra terminal marker contradicted the pinned event set and the typed terminal classification.
- Impact: `openai-rs-client` SSE transport only; no DTO change.
- Overrides: none
- Tests: `responses_terminal_table_matches_pinned_stream_events`, updated `responses_terminal_event_is_emitted_before_completion`.

## D0119 — Retrieve-side include/order query parameters are typed

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RetrieveResponseParams.include`, `RetrieveResponseStreamParams.include`, `ListResponseInputItemsParams.order`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (both GET `include` are arrays of `IncludeEnum` — the eight values already modeled as `ResponseIncludable`; `order` is `enum [asc, desc]`; Python `response_retrieve_params.py` uses `List[ResponseIncludable]`).
- Decision: reuse `ResponseIncludable` for both client-side include parameters, and narrow `order` to a new `ResponseItemOrder` open string enum (no crate-wide sort-order type exists; containers/beta each keep their own). Unknown values still serialize verbatim via `from_raw`.
- Reason: `Vec<String>` / `Omittable<String>` hid the pinned domains, the same class as D0106.
- Impact: `openai-rs-client` Responses query params and the types-side input-item list params; builders now reject arbitrary strings.
- Overrides: none
- Tests: `retrieve_with_encodes_typed_include_query`, `list_input_items_order_serializes_pinned_asc_desc_domain`.

## D0120 — Legacy chat functions entries and Conversation function-call caller fields

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ChatCompletionFunction`, `ChatCompletionRequestBody.functions`, `ConversationFunctionCall`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`ChatCompletionFunctions` has no `strict`; `FunctionToolCall` carries `caller` anyOf ToolCallCaller|null and `namespace`, `FunctionToolCallResource` adds `created_by`); openai-python `completion_create_params.py` `Function` TypedDict (no strict) and Responses resource TypedDicts.
- Decision: split the deprecated `functions[]` element into its own `ChatCompletionFunction` DTO (required `name`; optional `description`/`parameters`); it never emits `strict`, while `tools[].function` keeps `ChatFunctionDefinition` with `strict`. Type `ConversationFunctionCall` official optional fields `namespace`/`created_by` (`Omittable<String>`) and `caller` (`Omittable<Nullable<ToolCallCaller>>`), mirroring the D0030 shape of `responses::FunctionCall` and reusing the same `ToolCallCaller` union.
- Reason: the shared definition let deprecated requests emit the pin-illegal `strict` key on `functions[]`, and conversation function-call resources trapped `caller`/`namespace`/`created_by` in ExtraFields while the adjacent function-call-output branch already typed them.
- Impact: Chat legacy `functions` request JSON (breaking: field element type changed); Conversations function-call item JSON (additive).
- Overrides: none
- Tests: `legacy_functions_entries_omit_strict_while_tools_function_keeps_it`, `function_call_resource_caller_namespace_and_created_by_match_responses_shape`.

## D0121 — Realtime connect target is a model / transcription-intent / call-id enum

- Status: accepted
- Reviewed: 2026-08-31
- Scope: GA Realtime WebSocket connection entry (`Realtime::connect*`, `realtime_websocket_url`)
- Sources: openai-node `src/realtime/internal-base.ts` pinned `eea2292a4a523da9405161dde0a79ac5dc2ecb2a` (`buildRealtimeURL` accepts exactly one of model/callID/intent; the transcription branch sets `intent=transcription` and forbids a simultaneous model); openai-python `realtime.py` `connect(*, call_id, model, ...)`; pinned OpenAPI carries no `/realtime` WS path to arbitrate.
- Decision: model the connection target as `RealtimeConnectTarget` (`Model(ModelId)` / `TranscriptionIntent` / `CallId(String)`); mutual exclusion is structural. `?model=` / `?intent=transcription` / `?call_id=` are derived from the single target, and a base URL already carrying any target key is rejected as `InvalidConfiguration` rather than merged. The prior `connect(model)` entry points remain as conveniences mapping to the Model branch.
- Reason: the transcription session was unreachable (`connect` could only produce `?model=`, which the transcription endpoint rejects) and existing calls could not be attached.
- Impact: `openai-rs-client` Realtime connect surface (additive).
- Overrides: none
- Tests: `websocket_url_derives_model_intent_and_call_id_targets`, `websocket_url_rejects_conflicting_target_query_keys`, `transcription_intent_connection_uses_intent_query_without_model`.

## D0122 — Idempotent Realtime token issuance is replayable; call control stays Never

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `POST /realtime/client_secrets`, `POST /realtime/translations/client_secrets`, legacy `/realtime/(transcription_)sessions`, `/realtime/calls/{call_id}/{accept,reject,hangup,refer}`
- Sources: openai-python `_base_client.py` and openai-node `client.ts` retry loops apply to every request (default two retries each); token issuance is idempotent.
- Decision: mark the token-issuance operations `RetryClass::Replayable` (still gated by the `retry_replayable_mutations` policy switch); the observable-side-effect call control actions accept/reject/hangup/refer keep `RetryClass::Never`.
- Reason: blanket Never made transient 429/5xx outages observable for an idempotent credential mint, below official-SDK availability.
- Impact: `openai-rs-client` Realtime + legacy Realtime operation tables.
- Overrides: none
- Tests: `client_secret_route_is_typed_and_secret_debug_is_redacted`, `translation_client_secret_uses_fixed_typed_route_and_redacts_secret`, `sip_actions_use_typed_routes_and_never_need_response_json`, `session_creation_uses_pinned_route_without_beta_header`, `transcription_creation_uses_pinned_route_and_nullable_secret`.

## D0123 — Legacy Realtime sends no OpenAI-Beta header

- Status: accepted
- Reviewed: 2026-08-31
- Scope: legacy `/v1/realtime/sessions`, `/v1/realtime/transcription_sessions`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (both operations declare empty parameter lists); `assistants=v2` belongs to the Assistants family and D0087 covers Vector Stores only.
- Decision: remove the `OpenAI-Beta: assistants=v2` header from both legacy operations; they now use the standard JSON transport.
- Reason: the header had no basis in the pin or in official SDK behavior for these routes.
- Impact: `openai-rs-client` legacy Realtime transport only.
- Overrides: none
- Tests: `session_creation_uses_pinned_route_without_beta_header`, `transcription_creation_uses_pinned_route_and_nullable_secret` (both assert the header is absent).

## D0124 — Realtime transcription delay uses a dedicated enum

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimeAudioTranscription.delay`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`AudioTranscription.delay` enum minimal|low|medium|high|xhigh).
- Decision: introduce `RealtimeTranscriptionDelay` with exactly that domain (unknown values decode losslessly as `Unknown`) instead of reusing `RealtimeReasoningEffort`.
- Reason: the value sets coincide but the semantics are unrelated; the shared name misled the public API surface.
- Impact: `openai-rs-types` Realtime transcription config (wire-identical; breaking: public field type changed from `RealtimeReasoningEffort` to `RealtimeTranscriptionDelay`).
- Overrides: none
- Tests: `transcription_delay_uses_dedicated_enum_domain`.

## D0125 — realtime.call.incoming object is optional and SIP headers are redacted

- Status: accepted
- Reviewed: 2026-08-31
- Scope: types-side duplicate `WebhookRealtimeCallIncoming` / `RealtimeCallIncomingData` / `RealtimeSipHeader`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`required` excludes `object`; the spec's own example omits it); openai-python `realtime_call_incoming_webhook_event.py` `Optional[Literal["event"]]`; openai-node `object?: 'event'`; the webhooks.rs codec for the same event already treats it as optional.
- Decision: `object` becomes `Omittable<RealtimeIncomingWebhookObjectTag>` with an `object_marker_present()` accessor; the three types get hand-written redacted Debug (name/value and payloads never printed, mirroring the webhooks.rs redline and its leak test).
- Reason: the duplicate model rejected the pinned example shape, and SIP INVITE headers can carry credentials.
- Impact: `openai-rs-types` Realtime webhook DTOs.
- Overrides: none
- Tests: `realtime_incoming_webhook_decodes_without_optional_object_marker`, `realtime_incoming_webhook_debug_does_not_leak_sip_credentials`.

## D0126 — Multipart explicit null metadata fields are dropped

- Status: accepted
- Reviewed: 2026-08-31
- Scope: runtime multipart encoder (`append_multipart_value`), reachable via `TranscriptionRequestMetadata.chunking_strategy` and all `ImageEditMultipartMetadata` Nullable fields
- Sources: openai-python `src/openai/_base_client.py:622-647` + `src/openai/_qs.py:115-129` (pinned `b19c2161`): `_stringify_item` maps None to an empty primitive string and then yields no item, so the key is dropped; openai-node `uploads.ts:458-463` (pinned `eea2292a`) rejects explicit null; the pinned OpenAPI does not define null carriage inside multipart bodies.
- Decision: `Value::Null` metadata values produce no multipart part at all (explicit null is wire-equivalent to omission); no other encoding changes. Empty-string drop semantics remain out of scope (round-1 deferred item 1-D2).
- Reason: the pin is silent, so the automated Python behavior is adopted as the least surprising, non-rejecting option; encoding a literal "null" text part matched neither official SDK.
- Impact: multipart request encoder only; no DTO shape or JSON surface change.
- Overrides: none
- Tests: `transcription_multipart_drops_explicit_null_metadata_fields`, `image_edit_multipart_drops_explicit_null_metadata_fields`.

## D0127 — Official invite role is owner/reader only

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `InviteRole` (`Invite.role` / `InviteRequest.role`)
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (both `role` enums are owner/reader); openai-python `invite_create_params.py` and openai-node `invites.ts` agree; `member` appears only on the nested `projects[].role`.
- Decision: remove the `Member` variant from `InviteRole` (owner/reader plus open `Unknown` remain). Project membership roles stay on `InviteProjectRole`. Non-official role echoes decode losslessly as `Unknown`.
- Reason: the extra variant let requests carry a top-level invite role the pin rejects.
- Impact: `openai-rs-types` Admin invite DTOs (breaking: variant removed).
- Overrides: none
- Tests: `official_invite_role_pins_owner_and_reader_only`.

## D0128 — Organization data-retention body pins the four-value domain

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `UpdateOrganizationDataRetentionBody` / `UpdateProjectDataRetentionBody` / `OrganizationDataRetentionType` / `DataRetentionType`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`UpdateOrganizationDataRetentionBody.retention_type` and `OrganizationDataRetention.type` enumerate four values; the project body and `ProjectDataRetention.type` enumerate six, adding `organization_default` and `none`); openai-node keeps the org/project split across `data-retention.ts` and `projects/data-retention.ts`.
- Decision: add a four-value open enum `OrganizationDataRetentionType` for the organization request body; split `UpdateProjectDataRetentionBody` out of the type alias so it keeps the six-value `DataRetentionType`; the shared `DataRetentionResource` intentionally stays a six-value open superset (lossless for org, project, and future values). Org-side rejection is structural: `organization_default`/`none` have no named variant and fall to `Unknown`.
- Reason: sharing the six-value enum let the organization endpoint send values the pin does not define for it.
- Impact: `openai-rs-types` Admin data-retention DTOs (breaking: `UpdateProjectDataRetentionBody` split out of the `UpdateOrganizationDataRetentionBody` alias; org body field type narrowed to the four-value enum).
- Overrides: none
- Tests: `official_org_data_retention_pins_four_value_domain`.

## D0129 — Strict schema normalization unravels sibling-key refs and rejects non-false additionalProperties

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `normalize_strict_schema` / `normalize_object` (`openai-rs-types` structured output)
- Sources: openai-python `src/openai/lib/_pydantic.py` (`resolve_ref`, `has_more_than_n_keys`; `additionalProperties: false` is only defaulted when missing, per `_ensure_strict_json_schema`); openai-node `zod-v3-strict-schema.ts:67-69` rejects open-ended records outright; the official Structured Outputs examples always use `$ref` as the sole key. schemars 1.2.2 (Cargo.lock) emits `{"description":"...","$ref":"#/$defs/X"}` for nested custom-typed fields carrying doc comments.
- Decision: a local `$ref` accompanied by sibling keys is resolved by JSON Pointer (RFC 6901 escapes) against a snapshot of the input document, inlined with sibling keys taking precedence, and the result re-enters normalization; unresolvable pointers return the new `StructuredError::UnresolvableRef { path, reference }` instead of silently passing through. Bare sole-key `$ref`s and external refs behave unchanged. `normalize_object` defaults `additionalProperties` to `false` only when the key is missing; an existing non-false value (map fields) returns `UnsupportedKeyword` carrying the field path, honoring the module's "never silently drops a schema keyword" contract.
- Reason: sibling-key refs produced strict-rejected schemas from common schemars output, and overwriting existing `additionalProperties` silently rewrote dictionary fields into "always empty".
- Impact: `openai-rs-types` structured output; new error variant `UnresolvableRef` (enum is `#[non_exhaustive]`).
- Overrides: none
- Tests: `nested_ref_fields_with_doc_comments_are_inlined`, `unresolvable_sibling_refs_are_rejected_with_path`, `sibling_keys_win_over_the_inlined_reference`, `bare_refs_without_siblings_pass_through`, `map_fields_report_additional_properties_with_path`, `additional_properties_is_defaulted_only_when_missing`.

## D0130 — ModelId constants align with the pinned model enum

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ModelId` constants (`scalar.rs`), `examples/structured_output.rs`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`ModelIdsShared` contains gpt-5.6-sol/terra/luna with no bare `gpt-5.6` member; `ResponsesOnlyModel` adds `gpt-5.6-cyber`; the bare name appears only in prose); openai-node `shared.ts` model unions agree.
- Decision: drop the `GPT_5_6 = "gpt-5.6"` constant and its "official alias" comments, add `GPT_5_6_CYBER`, and switch the structured-output example to `ModelId::GPT_5_6_SOL`. `ModelId` remains an open newtype, so raw string construction is still possible.
- Reason: the constant asserted an alias no baseline supports and the family's fourth member was missing.
- Impact: `openai-rs-types` constants and one example; no wire behavior change (breaking: public constant `ModelId::GPT_5_6` removed).
- Overrides: none
- Tests: `scalar::tests::model_id_is_open_and_round_trips` (updated constant assertions); example covered by `cargo check -p openai-rs-sdk --examples`.

## D0131 — Retry backoff honors a 120s server cap; over-cap and non-positive values fall back locally

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RetryPolicy::openai_compatible`, transport retry loop (`transport.rs`), and the multipart transport's local retry copy (`multipart.rs`)
- Sources: openai-python `_constants.py:13` (`MAX_RETRY_AFTER_DELAY = 2*60`) and `_base_client.py:791-813` (server value used only when `0 < retry_after <= MAX`, otherwise local backoff); openai-node `client.ts:1642-1650` (over-cap falls back to default backoff and still retries). Note: python's `_should_retry` (815-823) declines to retry an over-cap Retry-After; this implementation follows the node/python delay-calculation fallback semantics and records the divergence here.
- Decision: `openai_compatible()` defaults `max_server_delay` to 120s (`conservative()` keeps 60s); `ServerDelay::TooLong` now behaves like `Absent` — local exponential backoff with the same attempt-count ceiling — instead of aborting; `bounded_delay` maps `seconds <= 0.0` and past HTTP-dates to `Absent`, so `Retry-After: 0` no longer retries immediately. The multipart transport's duplicate `retry_delay` was aligned to the same semantics.
- Reason: the previous 60s cap plus abort-on-overflow made official-SDK-retried transients (e.g. `Retry-After: 90`) surface as errors, and zero-valued headers caused hot-loop retries.
- Impact: `openai-rs-client` transport defaults and both retry paths; no wire format change.
- Overrides: none
- Tests: `non_positive_retry_after_values_fall_back_to_local_backoff`, `server_retry_delays_within_the_default_bound_are_honored`, `retry_after_zero_uses_local_backoff_instead_of_retrying_immediately`, `bounded_server_retry_delays_are_obeyed_end_to_end`, `retry_after_above_the_bound_falls_back_to_local_backoff_and_keeps_retrying`.

## D0132 — Codex UserInput models exactly the five pinned variants

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-codex` `UserInput`
- Sources: pinned Codex 0.144.5 schema `#/definitions/v2/UserInput/oneOf` (TextUserInput, ImageUserInput, LocalImageUserInput, SkillUserInput, MentionUserInput only).
- Decision: delete the fabricated `audio`/`localAudio` variants; tags outside the pin are rejected rather than retained, since the app-server protocol is closed and versioned by the pinned runtime.
- Reason: sending either fabricated variant produced params the pinned 0.144.5 server cannot deserialize.
- Impact: `openai-rs-codex` protocol (breaking: two variants removed).
- Overrides: none
- Tests: `protocol::tests::user_input_accepts_exactly_the_five_pinned_variants`.

## D0133 — Codex account/usage/read takes null params and no threadUsage

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-codex` `account_usage` / `AccountUsageParams` / `AccountUsageResponse`
- Sources: pinned Codex 0.144.5 schema `#/definitions/ClientRequest` `Account/usage/readRequest` variant (`params` is `{"type":"null"}`) and `#/definitions/v2/GetAccountTokenUsageResponse` (only `summary` and `dailyUsageBuckets`).
- Decision: `account/usage/read` is always issued without a `params` key; `AccountUsageParams` and the fabricated `thread_usage` response field are removed, leaving `summary`, `dailyUsageBuckets`, and flatten extra.
- Reason: a `threadId` parameter and a `threadUsage` field existed nowhere in the pinned schema; sending the former would fail server-side validation.
- Impact: `openai-rs-codex` client API (breaking: method now parameterless).
- Overrides: none
- Tests: `app_server::client::tests::fake_child_typed_account_thread_and_turn_contracts` (wire assertions: no `threadId`, no `params` key).

## D0134 — Codex InitializeCapabilities matches the pinned schema exactly

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-codex` `InitializeCapabilities` / `InitializeParams`
- Sources: pinned Codex 0.144.5 schema `#/definitions/InitializeCapabilities` (exactly `experimentalApi`, `mcpServerOpenaiFormElicitation`, `optOutNotificationMethods`, `requestAttestation`) and `#/definitions/InitializeParams`.
- Decision: model the four optional properties (`Omittable<bool>` for the three booleans, `Omittable<Nullable<Vec<String>>>` for the array/null union); remove the invented nested `extensions` field; a new flatten `extra` map retains future capability properties losslessly.
- Reason: the client could not declare `mcpServerOpenaiFormElicitation`, and the `extensions` escape hatch nested one level too deep for the server to read.
- Impact: `openai-rs-codex` handshake protocol.
- Overrides: none
- Tests: `initialize_capabilities_serialize_exactly_the_four_pinned_properties`, `initialize_params_serialize_the_pinned_handshake_shape`, `initialize_capabilities_keep_future_properties_and_null_losslessly`, `fake_child_handshake_correlation_and_unknown_notification`.

## D0135 — Facade re-exports and example continuation (no wire change)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-sdk` facade (`lib.rs`), `examples/responses.rs`
- Sources: `RetrieveResponseParams`/`RetrieveResponseStreamParams`/`BodyPreview`/`RateLimitMetadata` and the `sse` module are public in `openai-rs-client` but were unreachable through the facade; the official function-calling guide resends `tools` on every `previous_response_id` continuation, and `CreateResponseRequest::follow_up` documents that the prefix does not carry tools.
- Decision: re-export the four client types and the `sse` module under the facade's client gate; switch the example's continuation to `follow_up_from`, which copies the stable prefix including tools, and correct its comment.
- Reason: facade-only users could not call `retrieve_with`/`retrieve_stream` or match error details without taking a direct dependency on the client crate, and the example demonstrated a continuation the model could not tool-call against.
- Impact: facade crate only; no wire behavior change.
- Overrides: none
- Tests: `crates/openai-rs/tests/facade_reexports.rs` (8 compile-level checks
  as recounted by round-8 item 8-26: the four original re-export checks plus
  four later feature-gated additions — realtime connect target, admin
  checkpoint permissions, admin operation machinery, content-provenance
  discriminators).

## D0136 — Required empty arrays survive the decode→encode round trip

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `OutputText.annotations/logprobs`, `OutputTextDeltaEvent.logprobs`, `OutputTextDoneEvent.logprobs`, `BetaMultiAgentOutputText.annotations/logprobs`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`OutputTextContent` and `ResponseTextDeltaEvent`/`ResponseTextDoneEvent` (GA+Beta) list `annotations`/`logprobs` in `required`; the service always emits `[]`).
- Decision: drop `skip_serializing_if="Vec::is_empty"` on those fields while keeping `#[serde(default)]`, so a missing key still decodes but an empty array re-encodes as `[]`.
- Reason: `to_input_items()` replay and event re-encoding previously dropped pin-required keys.
- Impact: `openai-rs-types` Responses/beta Responses DTOs; encoded JSON gains the empty-array keys.
- Overrides: none
- Tests: `output_text_required_empty_arrays_survive_decode_encode_round_trip`, `multi_agent_output_text_empty_required_arrays_round_trip`, updated `to_input_items_converts_all_output_items`.

## D0137 — Stored input-message role decodes through the open MessageRole

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `StoredInputMessage.role`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`ItemField`/`BetaItemField` Message branch accepts any `MessageRole`; compaction targets are multi-agent conversations whose stored items echo `critic`/`tool`/`discriminator` roles).
- Decision: widen the decode-side field to the open `MessageRole` (unknown values verbatim); keep `StoredInputMessageRole` as the request-construction domain via `From`; add a `role()` getter.
- Reason: the closed three-value decode failed whole compact 200 bodies carrying multi-agent roles.
- Impact: `openai-rs-types` Responses DTO (additive; constructor unchanged).
- Overrides: none
- Tests: `compact_output_stored_messages_decode_multi_agent_roles_losslessly`, `beta_compact_output_stored_messages_decode_multi_agent_roles_losslessly`.

## D0138 — HostedToolType pins the eight ToolChoiceTypes values

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `HostedToolType`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`ToolChoiceTypes.type`/`BetaToolChoiceTypes.type` enumerate eight values; `web_search` and `web_search_2025_08_26` exist only on `WebSearchTool.type`); openai-python `tool_choice_types_param.py` and openai-node agree.
- Decision: remove the two tool-only variants; the open-enum `Unknown` still decodes any string with unchanged Hosted routing.
- Reason: the send domain was wider than the pin and could emit tool_choice values the pin rejects.
- Impact: `openai-rs-types` Responses tool-choice DTO (breaking: two variants removed).
- Overrides: none
- Tests: `hosted_tool_choice_type_pins_the_eight_official_values`.

## D0139 — Code-interpreter allowlist domains enforce minItems 1 (extends D0117)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CodeInterpreterNetworkAllowlist.allowed_domains`
- Sources: pinned `ContainerNetworkPolicyAllowlistParam.allowed_domains` (required, minItems 1); containers-side `CreateContainerConstraintError::EmptyAllowedDomains`.
- Decision: `validate()` rejects an empty allowlist via new `CreateResponseConstraintError::EmptyAllowedDomains`; serde stays lossless.
- Reason: D0117 wired only `domain_secrets`; the sibling minItems was missing.
- Impact: `openai-rs-types` Responses code-interpreter validation (opt-in).
- Overrides: none
- Tests: updated `code_interpreter_allowlist_domain_secrets_serialize_and_validate`.

## D0140 — Beta compact service_tier uses the pinned five-value BetaServiceTierEnum

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaCompactResponseRequest.service_tier`
- Sources: pinned `BetaCompactResponseMethodPublicBody.service_tier` → `BetaServiceTierEnum` (auto/default/fast/flex/priority); create/echo sides stay `BetaServiceTierResponses`.
- Decision: new open enum `BetaCompactServiceTier`; create side unchanged.
- Reason: reusing the seven-value `BetaServiceTier` let the compact request send pin-external `scale`/`ultrafast`.
- Impact: `openai-rs-types` beta Responses compact builder (breaking: setter type changed).
- Overrides: none
- Tests: `compact_service_tier_pins_the_five_official_values`.

## D0141 — Multi-agent call_id is validated as 1..=64 characters

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaMultiAgentCallParam.call_id`, `BetaMultiAgentCallOutputParam.call_id`
- Sources: pinned `BetaMultiAgentCallItemParam.call_id` / `BetaMultiAgentCallOutputItemParam.call_id` (minLength 1, maxLength 64); D0049 opt-in pattern.
- Decision: `validate()` on both Param types plus `MIN/MAX_MULTI_AGENT_CALL_ID_CHARS`, reusing `CreateResponseConstraintError::CallId`, wired through `validate_beta_response_input_item`.
- Reason: the pinned bound was unchecked before sending.
- Impact: `openai-rs-types` beta Responses validation (opt-in only).
- Overrides: none
- Tests: `multi_agent_call_id_validate_enforces_pinned_bounds`.

## D0142 — Beta agent items split into Param and Resource shapes (closes round-1 item 1-D1)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaAgentMessage*`, `BetaMultiAgentCall*`, `BetaMultiAgentCallOutput*`, `BetaResponseInputItem`/`BetaResponseOutputItem` agent branches
- Sources: pinned `BetaAgentMessageItemParam`/`BetaMultiAgentCallItemParam`/`BetaMultiAgentCallOutputItemParam` (id/agent optional-nullable) vs `BetaAgentMessage`/`BetaMultiAgentCall`/`BetaMultiAgentCallOutput` (id required, agent non-nullable); openai-python and openai-node both generate separate Param/resource classes.
- Decision: three request-side Param types and three resource types; resources take a required `id` in `new()`, keep `agent: Omittable<BetaAgent>` with `id()`/`agent_ref()` getters and lose the null setters; `From<Resource> for Param` ×3 preserves the wire shape; the input union carries Params and the output union carries resources. Compact output and input-item listings deliberately keep the input union as a loose bridge over the resource-shaped `BetaItemField` (a decoded resource satisfies the Param shape losslessly).
- Reason: one dual-use type could not represent "id required on echo, optional on request" and silently accepted id-less resources.
- Impact: `openai-rs-types` beta Responses (breaking: `new()` signatures and removed resource-side null setters); `openai-rs-client` tests adapted.
- Overrides: none
- Tests: `beta_agent_items_split_param_and_resource_shapes`, `multi_agent_call_id_validate_enforces_pinned_bounds`, updated `multi_agent_items_encode_arguments_without_manual_json_formatting`, `create_and_count_requests_keep_beta_only_fields_typed`, `websocket_inject_events_are_structurally_routed`, `beta_item_official_nulls_match_openapi`, `official_beta_agent_message_param_omits_image_detail`.

## D0143 — Strict schema inlining detects reference cycles and rejects root/self and non-string `$ref`s (extends D0129)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `normalize` / `normalize_object` / `StructuredError`
- Sources: schemars 1.2.2 emits `{"$ref":"#/$defs/X","description":"…"}` for doc-commented nested recursive types; D0129's inlining recursed without bound on them (reproduced `fatal runtime error: stack overflow, aborting`); RFC 8259 requires `$ref` values to be strings; `"#"` is the document-root self reference.
- Decision: inlining propagates an active-reference chain (root→node) and returns the new catchable `StructuredError::RecursiveReference { path, reference }` when the chain repeats — chain length is bounded by the document's distinct references, guaranteeing termination. `$ref:"#"` (bare or with siblings) reports `RecursiveReference` instead of `ExternalReference`; non-string `$ref` values return `UnresolvableRef` with a path instead of passing through. D0129's behavior (bare sole-key `$ref` passthrough, sibling-key-precedence inlining, `~0`/`~1` escapes, dangling pointers) is unchanged.
- Reason: an abort cannot be caught and violates the module's error-not-silence contract; the root self reference is recursion, not an external reference; a non-string `$ref` is invalid input that must not reach the API.
- Impact: `openai-rs-types` structured output; new error variant `RecursiveReference` (enum already `#[non_exhaustive]`).
- Overrides: none
- Tests: `recursive_sibling_refs_error_instead_of_overflowing`, `transitive_ref_cycles_are_detected_through_the_chain`, `root_self_reference_reports_recursion_not_external`, `non_string_refs_are_rejected_with_path`; D0129's six tests still pass.

## D0144 — SSE default line limit matches the event limit (32 MiB)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `DEFAULT_MAX_SSE_LINE_BYTES`, `SseLimits`
- Sources: openai-python `_streaming.py` and openai-node `core/streaming.ts` both document decoding "without imposing a line or event size limit"; Responses SSE payloads are single physical `data:` lines and `response.image_generation_call.partial_image` base64 can reach several MiB.
- Decision: raise the default single-line cap from 1 MiB to 32 MiB, equal to `DEFAULT_MAX_SSE_EVENT_BYTES`; the event-level bound remains the DoS guard.
- Reason: the 1 MiB line cap could fail an entire official stream on one large partial-image or snapshot event.
- Impact: `openai-rs-client` SSE defaults; configurable via `SseLimits` as before.
- Overrides: none
- Tests: `default_line_limit_matches_the_event_limit`, `decodes_a_single_data_line_above_one_mebibyte`.

## D0145 — Query parameters drop explicit null and empty-string values

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `append_query`/`query_scalar` (`transport.rs`), stored Chat completions metadata query, fine-tuning jobs metadata query
- Sources: openai-python `_qs.py::_stringify_item` yields no item when the serialized primitive is empty, so both `None` and `""` produce no query parameter (`_base_client.py` request building); `0`/`false` still serialize.
- Decision: `Value::Null` and `Value::String("")` skip the query key entirely; other falsy scalars encode as before; a request whose keys are all skipped produces no trailing `?`. The stored-Chat null-metadata error branch and the fine-tuning `metadata=` empty-string encoding were removed in favor of omission (both now equivalent to Python's `metadata=None`).
- Reason: the previous behavior was inconsistent across call sites (one errored, one emitted `metadata=`, Python dropped the key).
- Impact: `openai-rs-client` query encoding; no DTO change.
- Overrides: none
- Tests: `null_and_empty_string_query_values_are_omitted`, `stored_list_omits_explicit_null_metadata_and_empty_after`, `jobs_list_omits_null_metadata_and_empty_after_query_keys`.

## D0146 — Multipart retry-after-ms short-circuits like the JSON transport

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `multipart.rs::retry_delay`
- Sources: openai-python `_parse_retry_after_header` returns the `retry-after-ms` value as soon as it parses; openai-node `retryRequest` skips the `retry-after` branch whenever a millisecond timeout exists; `transport.rs::server_retry_delay` (D0131) already short-circuits.
- Decision: when `retry-after-ms` parses (including over-cap, non-finite, or non-positive values), the multipart copy resolves the delay from that branch alone (local backoff fallback) and never falls back to reading `Retry-After`; unparseable values still fall back, mirroring the transport copy.
- Reason: the divergent fallback could sleep a different duration than the JSON transport for the same headers.
- Impact: `openai-rs-client` multipart retry path only.
- Overrides: none
- Tests: `retry_after_ms_short_circuits_and_never_falls_back_to_retry_after`.

## D0147 — Auto-pagination cursor falls back to the last item id

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `pagination::next_cursor` and its callers
- Sources: openai-python `pagination.py::SyncCursorPage.next_page_info` uses `data[-1].id` as the `after` cursor whenever data is non-empty, independent of the envelope `last_id`; several official list envelopes (ChatKit threads, skills, admin cursor pages) type `last_id` as nullable.
- Decision: cursor resolution order is envelope `last_id` (non-empty) → caller-supplied last item id (non-empty) → fail-closed; duplicate/missing-with-empty-data behavior unchanged; the getter and stream paths share the rule. Tagged-union item pages (ChatKit items, conversation items, Responses/Beta input items) keep fail-closed until a uniform id accessor exists in the types crate.
- Reason: a schema-legal `{"has_more": true, "last_id": null}` page previously aborted `list_pages` streams that official SDKs page through.
- Impact: `openai-rs-client` pagination and per-resource callers.
- Overrides: none
- Tests: `next_cursor_falls_back_to_the_last_item_id`, `checkpoint_pages_fall_back_to_the_last_item_id_when_last_id_is_null`, `checkpoint_pages_fail_closed_when_last_id_is_null_and_data_is_empty`.

## D0148 — Realtime WebSockets rely on tungstenite's automatic Pong reply

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimeWebSocket::recv` (`realtime.rs`), Responses WebSocket `recv` (`responses_websocket.rs`)
- Sources: tungstenite 0.29 queues the RFC 6455 Pong while reading a Ping and flushes it on the next poll; `set_additional` replaces a pending Pong, so the explicit reply coalesced rather than duplicated on the wire.
- Decision: neither recv loop writes an explicit Pong; inbound-frame policy for Realtime is centralized in a unit-locked classification helper. (The audit's "double Pong" was not wire-reproducible on 0.29; the explicit branch was still a redundant write path that surfaced write failures as spurious recv errors.)
- Reason: removes a redundant write path and documents the automatic-reply dependency.
- Impact: no wire behavior change (single Pong per Ping before and after).
- Overrides: none
- Tests: `websocket_answers_one_ping_with_exactly_one_pong`, `realtime_recv_does_not_explicitly_pong_inbound_pings`.

## D0149 — Realtime PCM rate is a typed single-value enum plus opt-in validate

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimePcmAudioFormat.rate`
- Sources: pinned `RealtimeAudioFormats` PCM branch pins `rate` to `enum: [24000]` ("Only a 24kHz sample rate is supported").
- Decision: `rate: Omittable<RealtimePcmRate>` with known `Rate24000` and lossless `Unknown(i64)`; decode stays lossless, while session/transcription/client-secret `validate()` rejects present non-24000 PCM rates with `CreateRealtimeSessionConstraintError::PcmRate`.
- Reason: the bare `Omittable<i64>` field could send `rate: 16000`, which the pinned endpoint rejects.
- Impact: `openai-rs-types` Realtime (breaking: field type changed; no in-repo callers).
- Overrides: none
- Tests: `realtime_pcm_rate_pins_24000_and_keeps_unknown_rates_lossless`, `realtime_pcm_rate_validate_rejects_non_pinned_rates`.

## D0150 — Costs query pins the one-day bucket and three-value group_by

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `UsageCostsQueryParams` / `UsageCostsBucketWidth` / `UsageCostsGroupBy` / `AdminUsage::costs`
- Sources: pinned `GET /organization/costs` (`bucket_width enum ["1d"]`, `group_by` items enum of three values, exactly eight query parameters); openai-python `usage_costs_params.py` and openai-node `UsageCostsParams` agree.
- Decision: costs uses its own `UsageCostsQueryParams` with the pinned eight parameters and typed single-value/three-value open enums; the other ten usage endpoints keep the shared superset `UsageQueryParams`; the frozen operation manifest label tracks the new parameter type with an unchanged DTO projection.
- Reason: the shared superset could send `1m`/`1h` buckets and usage-side group_by dimensions the costs endpoint does not define.
- Impact: `openai-rs-types`/`openai-rs-client` (breaking: `AdminUsage::costs` parameter type changed).
- Overrides: none
- Tests: `usage_costs_query_pins_one_day_bucket_and_three_value_group_by`.

## D0151 — Project data-retention update is wired through the facade

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AdminDataRetention::update_project`
- Sources: pinned `POST /organization/projects/{project_id}/data_retention` (body `$ref UpdateProjectDataRetentionBody`); the body type split out by D0128 previously had no caller.
- Decision: add `update_project(project_id, UpdateProjectDataRetentionBody) -> ApiResponse<ProjectDataRetention>`, wiring the already-frozen `OpUpdateProjectDataRetention` after the adjacent `update_organization` pattern.
- Reason: the operation contract and request body existed but the endpoint was unreachable.
- Impact: `openai-rs-client` admin facade (pure addition).
- Overrides: none
- Tests: `data_retention_update_project_posts_pinned_route_and_six_value_domain`.

## D0152 — Project service-account update role is member/owner only

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `UpdateProjectServiceAccountBody.role` / `ProjectServiceAccountUpdateRole`
- Sources: pinned `UpdateProjectServiceAccountBody.role enum ["member","owner"]`; openai-python and openai-node agree; `none` exists only on the resource enums.
- Decision: the update body uses a two-value open enum; resource and create-response enums keep `ProjectServiceAccountRole`.
- Reason: the update body could send the resource-only `none` (D0127 pattern).
- Impact: `openai-rs-types` Admin (breaking: field type changed).
- Overrides: none
- Tests: `project_service_account_update_role_pins_member_and_owner`.

## D0153 — Vector-store and Batch metadata limits are opt-in validate; decode stays lossless

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `VectorStoreMetadata`, `BatchMetadata`, `BatchCustomId`, `StaticChunkingStrategy`, `CreateVectorStoreRequest::validate`, `UpdateVectorStoreRequest::validate`, `CreateBatchRequest::validate`
- Sources: pinned `Metadata` carries no `maxProperties`/`maxLength` (the 16/64/512 limits live in descriptions); `StaticChunkingStrategy.chunk_overlap_tokens` has no schema constraint (only `max_chunk_size_tokens` 100..=4096 does); openai-python decodes `Dict[str, str]` without validation; D0015/D0016/D0017 established the opt-in pattern; batch output files echo the caller's `custom_id`, and only input lines carry the non-empty rule.
- Decision: metadata maps decode as lossless pass-throughs; 16/64/512 stay in `insert()` while the overlap rule moves to `validate()` only (constructors keep just the schema-backed `100..=4096` range); new request-level `validate()` hooks cover metadata and chunking; `max_chunk_size_tokens` remains decode-enforced (schema-backed); `BatchCustomId` accepts empty echoes while `new()` keeps rejecting empty input lines.
- Reason: a server-echoed oversized (but schema-legal) metadata map previously failed the whole Batch/VectorStore decode, and the chunking prose rule fired during decode.
- Impact: `openai-rs-types` Vector Stores/Batches DTOs and validation surface.
- Overrides: none
- Tests: `oversized_metadata_decodes_and_request_validate_rejects`, `oversized_batch_metadata_decodes_and_request_validate_rejects`, `chunking_schema_range_is_decode_enforced_and_overlap_is_opt_in`, `batch_custom_id_decode_is_lossless_while_input_construction_rejects_empty`.

## D0154 — File/Batch list limits enforce only the schema-backed lower bound

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `FileListLimit`, `BatchListLimit`
- Sources: pinned `/files` and `/batches` `limit` parameters carry only `default` (no `maximum`; the ceilings exist in descriptive prose); openai-python passes unbounded integers.
- Decision: drop the invented 10,000/100 ceilings and keep `>= 1`; the default constants (10000/20) stay.
- Reason: the client previously rejected values the pinned endpoints accept.
- Impact: `openai-rs-types` Files/Batches query types (breaking: public constants `MAX_FILE_LIST_LIMIT`/`MAX_BATCH_LIST_LIMIT` removed, as are the identical no-caller predicates `is_create_file_purpose`/`is_create_upload_purpose`).
- Overrides: none
- Tests: `file_list_limit_requires_at_least_one`, `batch_list_limit_requires_at_least_one`, `list_files_accepts_limits_above_documented_prose_ceiling`, `list_batches_accepts_limits_above_documented_prose_ceiling`.

## D0155 — Batch input-line envelope and VS file-batch per-file form follow the pinned schema

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BatchLine`, `CreateVectorStoreFileBatchRequest`
- Sources: pinned `CreateVectorStoreFileBatchRequest` anyOf requires exactly one of `file_ids`/`files`; `files` plus global `attributes`/`chunking_strategy` satisfies the schema (the description only notes the global fields are ignored); batch output lines already retain unknown envelope fields via ExtraFields.
- Decision: `BatchLine` gains a flatten `ExtraFields` (matching output lines); decoding accepts `files` + global fields while builders reject that combination with the new `VectorStoreValidationError::GlobalFieldsWithPerFileBatch` (clear copy instead of the misleading "exactly one of" message); the `file_ids` XOR `files` decode rejection stays.
- Reason: the decoder rejected schema-legal input and the error message misattributed the cause; the input envelope was stricter than the output envelope without cause.
- Impact: `openai-rs-types` Batches/Vector Stores DTOs.
- Overrides: none
- Tests: `batch_line_retains_unknown_envelope_fields`, `per_file_batch_with_global_fields_decodes_but_builders_reject`.

## D0156 — Legacy best_of must exceed an explicit n

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CreateCompletionRequest::validate()`, `CreateCompletionConstraintError`
- Sources: pinned `CreateCompletionRequest.best_of` description ("`best_of` must be greater than `n`"); openai-python performs no client-side check.
- Decision: opt-in `validate()` rejects `best_of <= n` only when both fields carry explicit non-null values (new `BestOfNotGreaterThanN`); omitted or official-null values skip the relation; serde decode stays lossless.
- Reason: the pinned constraint was expressible but unvalidated.
- Impact: `openai-rs-types` legacy Completions validate surface (additive variant).
- Overrides: none
- Tests: `completion_validate_enforces_best_of_greater_than_n`.

## D0157 — Eval sampling params split per host

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `EvalCompletionsSamplingParams`, `EvalResponsesSamplingParams`, `EvalGraderSamplingParams`, run data sources, `ScoreModelGrader.sampling_params`
- Sources: pinned `CreateEvalCompletionsRunDataSource.sampling_params` / `CreateEvalResponsesRunDataSource.sampling_params` (all properties non-null; completions carries `response_format`, responses carries `text`) and `GraderScoreModel.sampling_params` (four nullable fields plus `max_completions_tokens`); openai-python models three independent SamplingParams TypedDicts.
- Decision: replace the merged union with three host-specific types; run hosts are non-null (no null setters, decode rejects inner nulls), the grader host keeps official null setters and the minimum-1 `validate()`; `From` conversions carry shared fields and rename the token-cap spelling.
- Reason: the merged union let run hosts send grader-only fields, both token spellings, `text` + `response_format` together, and explicit nulls the pinned run schemas do not allow.
- Impact: `openai-rs-types` Evals API (breaking: type split replaces `EvalSamplingParams`).
- Overrides: none
- Tests: `run_sampling_params_hosts_are_non_null_and_convert_to_grader_params`, `grader_sampling_params_sends_official_nulls_and_enforces_pin_limits`, `run_data_sources_enforce_nested_tag_sets_and_build_typed_requests`.

## D0158 — Alpha grader union uses threshold-free Param forms

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `Grader`, `MultiGraderMember`, `PythonGraderParam`, `ScoreModelGraderParam`, `TextSimilarityGrader`
- Sources: pinned `GraderPython`/`GraderScoreModel`/`GraderTextSimilarity` carry no `pass_threshold` (it exists only on the `EvalGrader*` allOf extensions); openai-python uses `*GraderParam` unions for alpha run/validate and adds the threshold only on `TestingCriterion`.
- Decision: the alpha union and multi members use grader-side forms — `TextSimilarityGrader` drops its optional threshold, new `PythonGraderParam`/`ScoreModelGraderParam` mirror the base schemas; `TestingCriterion` keeps the threshold-bearing types; bidirectional `From` conversions drop/omit the threshold.
- Reason: shared DTOs let alpha-only requests emit the Eval-resource-only `pass_threshold` key.
- Impact: `openai-rs-types` Evals alpha API (breaking: variant types changed, one field removed).
- Overrides: none
- Tests: `alpha_grader_unions_carry_no_pass_threshold`, `grader_param_forms_convert_with_and_without_pass_threshold`.

## D0159 — Codex login URL fields keep the pinned string on the wire and parse at the consumer

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `LoginAccountResponse.auth_url/verification_url`, `BrowserLogin`/`DeviceCodeLogin`
- Sources: pinned Codex 0.144.5 `v2/LoginAccountResponse` types both fields as plain strings (no `uri` format).
- Decision: the DTO keeps `Option<String>` so a non-absolute value cannot fail the whole login response; the public types keep `url::Url` and parse at the consumption site, returning an `UnexpectedResponse` that names the wire key on failure.
- Reason: the `Url` decode was stricter than the pinned schema.
- Impact: `openai-rs-codex` protocol/client (public API unchanged).
- Overrides: none
- Tests: `login_account_response_accepts_non_absolute_urls`, `login_start_rejects_non_absolute_urls_with_explicit_errors`.

## D0160 — RMCP compact policy flattens only text-only content

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ResultEncoding::CompactWhenPossible` (`openai-rs-rmcp` `result.rs`)
- Sources: MCP 2025-06-18 Tools defines the text block beside `structuredContent` as a redundant serialization (dropping it is permitted); image/audio/resource/resource_link blocks carry information the spec does not define as redundant.
- Decision: `structuredContent` replaces the envelope only when `content` is empty or entirely text blocks; rich-block combinations keep the lossless envelope; the policy docs state the rule.
- Reason: the previous unconditional flatten silently dropped rich content blocks, contradicting the module's own documentation.
- Impact: `openai-rs-rmcp` result encoding.
- Overrides: none
- Tests: `compact_policy_keeps_envelope_when_structured_content_pairs_with_rich_blocks`, `compact_policy_flattens_structured_content_only_when_content_is_text_only`.

## D0161 — Webhook tolerance compares whole seconds; minimum accepted window is one second

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `WebhookVerifier::with_tolerance`
- Sources: the window comparison uses `Duration::as_secs()`; official verifiers express tolerance in whole seconds (python int, node timestamp parsed as integer).
- Decision: `with_tolerance` rejects Durations below one second via the existing `InvalidTolerance` error instead of truncating sub-second values to a zero window.
- Reason: `from_millis(500)` previously passed validation but behaved identically to a rejected zero tolerance.
- Impact: `openai-rs-client` webhook verification configuration.
- Overrides: none
- Tests: `sub_second_tolerances_are_rejected_instead_of_truncating_to_zero`.

## D0162 — Blank function arguments decode to an empty object

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `parse_function_arguments` (`openai-rs-rmcp` `arguments.rs`)
- Sources: MCP treats omitted arguments as "no arguments"; tools exposed through the catalog are deliberately non-strict, and non-strict tools may return an empty string for zero-argument calls.
- Decision: empty or whitespace-only argument strings map to an empty `JsonObject`; other invalid inputs keep their error path.
- Reason: zero-argument dispatch previously failed instead of calling the tool.
- Impact: `openai-rs-rmcp` argument decoding.
- Overrides: none
- Tests: `blank_arguments_decode_to_an_empty_object`.

## D0163 — Round-2 recorded positions (no code change)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: multipart empty strings; connect timeouts; `Response`/`BetaResponse.created_at`; redirect policy; transport-error retry scope; `TypedFunction` output schema normalization
- Sources: openai-python `_qs.py` drops empty serialized primitives while openai-node `addFormValue` sends empty strings verbatim (the two official SDKs disagree; the pinned OpenAPI is silent — only the JSON image-edit `prompt` carries `minLength: 1`); python `DEFAULT_TIMEOUT` is 600s overall with a 5s connect budget while node's SDK-level timeout is 10 minutes with no SDK connect timeout (undici's default 10s applies); the pinned spec types 120 of 122 unixtime fields as `integer` and only `Response`/`BetaResponse.created_at` as `number`; official SDKs follow redirects by default; openai-python retries every transport `Exception` while this crate retries connect/timeout classes; the pinned `FunctionToolParam.output_schema` describes any JSON value.
- Decision: multipart empty-string metadata fields keep being sent verbatim (null fields are dropped per D0126 — closes round-1 item 1-D2); connect timeouts stay 10s (HTTP) / 30s (WebSocket) and are documented as a deliberate middle ground (closes 1-D3); `created_at` stays `i64` under the integral-seconds assumption the service observes (decimal-second echoes would fail the decode); redirects stay disabled so credentials never cross origins; the transport-error retry scope stays connect/timeout; `TypedFunction` keeps normalizing `output_schema` with strict rules and an object root, matching the strict tool validator's expectations.
- Reason: each position either splits the official baselines (choosing the least destructive side), matches the only realistic wire behavior, or follows an explicit repository security posture already stated in `docs/architecture.md`.
- Impact: none (documentation of current behavior).
- Overrides: none
- Tests: D0126's multipart null tests; existing timeout/redirect/retry tests.

## D0164 — Facade re-exports keep pace with client-root additions (round 2)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-sdk` facade (`RealtimeConnectTarget`, `AdminCheckpointPermissions`)
- Sources: both types are public at the client root (`connect_target`/`connect_target_with` are the only `call_id`-attach entry points; `AdminClient::checkpoint_permissions()` returns the resource client), continuing the D0135 pattern.
- Decision: re-export `RealtimeConnectTarget` under the realtime gate and `AdminCheckpointPermissions` under the admin gate; compile-level nameability tests lock both paths.
- Reason: facade-only users could not name the connect-target enum or the checkpoint-permissions client type.
- Impact: facade crate only; no wire behavior change.
- Overrides: none
- Tests: `realtime_connect_target_is_nameable_through_the_facade`, `admin_checkpoint_permissions_is_nameable_through_the_facade`.

## D0165 — GA compact service_tier uses the pinned five-value ServiceTierEnum

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CompactResponseRequest.service_tier`
- Sources: pinned OpenAPI commit `690521b1753dce0c6d6b275f583d22537679cff9` (`CompactResponseMethodPublicBody.service_tier` → `ServiceTierEnum` = auto/default/fast/flex/priority); openai-node agrees at the same position. Extends D0140, which fixed only the beta side.
- Decision: new open enum `CompactServiceTier` (wire-identical to `BetaCompactServiceTier`); create-side and `Response` echo keep the seven-value tiers.
- Reason: reusing the seven-value `ServiceTier` let the GA compact request send pin-external `scale`/`ultrafast`.
- Impact: `openai-rs-types` Responses compact builder (breaking: setter type narrowed).
- Overrides: none
- Tests: `compact_service_tier_pins_the_five_official_values`.

## D0166 — Easy-form input message pins the four-role construction domain

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `InputMessage::new`
- Sources: pinned `EasyInputMessage.role` enum (user/assistant/system/developer); openai-python `EasyInputMessageParam.role` Literal.
- Decision: new `EasyInputMessageRole` open enum (plus `InputMessage::assistant()` and `From<StoredInputMessageRole>`); the decode-side field stays the open `MessageRole` per D0137.
- Reason: the eight-value resource domain let easy-form requests carry roles the pinned request schema rejects.
- Impact: `openai-rs-types` Responses input construction (breaking: constructor parameter narrowed).
- Overrides: none
- Tests: `easy_input_message_role_pins_the_four_official_values`.

## D0167 — computer_screenshot is item-form-only message content

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `EasyInputContent`, `MessageContent`, `FunctionCallOutputContent`, `FunctionCallOutputValue`
- Sources: pinned `InputMessageContentList`→`InputContent`, `FunctionCallOutputItemParam.output`, and `FunctionAndCustomToolCallOutput` each enumerate exactly three branches (text/image/file); only the item-form `Message.content` includes `ComputerScreenshotContent`. D0045's citation covered the item-form union only.
- Decision: new three-branch `EasyInputContent` union for easy-form and function-output param paths; item-form keeps the four-branch `InputContent`; the cross-domain `From<Vec<InputContent>>` conversions were removed so the screenshot variant cannot be smuggled onto param paths; out-of-domain tags still decode as `Unknown` losslessly.
- Reason: the shared param union let requests carry a content branch the pinned request schemas do not define.
- Impact: `openai-rs-types` Responses param content unions (breaking: variant and conversion removals).
- Overrides: partially supersedes D0045's scope (its item-form routing stands).
- Tests: `param_content_unions_exclude_computer_screenshot_outside_item_form`.

## D0168 — Per-host item-status enums narrow the construction domain

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `MessageStatus`, `FunctionCallItemStatus`, `McpToolCallStatus`, `WebSearchToolCallStatus`, `FileSearchToolCallStatus`, `ImageGenToolCallStatus`, `CodeInterpreterToolCallStatus`, and the narrowed constructors (`StoredInputMessage::status`, `OutputMessage::new`, `FunctionCall::new`, item `new`/`with_status` setters)
- Sources: pinned per-item status enums (`MessageStatus`, `FunctionCallItemStatus`, `MCPToolCallStatus` incl. calling/failed, `WebSearchToolCall.status` incl. searching, `FileSearchToolCall.status`, `ImageGenToolCall.status` incl. generating, `CodeInterpreterToolCall.status` incl. interpreting); openai-python models each item as its own `Literal`.
- Decision: seven per-host small open enums with `From` into the shared `ResponseItemStatus`; every request-side constructor and status setter takes the host enum (decoded statuses replay through `from_raw`, e.g. the conversation adapter). Response decoding keeps the shared eight-value superset (D0025/D0026/D0028).
- Reason: the shared union as the construction domain let any item carry statuses its pinned schema rejects, inconsistent with D0115's per-host treatment.
- Impact: `openai-rs-types` Responses item constructors (breaking: parameter types narrowed); conversation/structured/rmcp call sites adapted.
- Overrides: none
- Tests: `per_host_item_status_enums_pin_official_domains`.

## D0169 — Legacy Realtime numeric ranges are opt-in validate; decode stays lossless

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `LegacyRealtimeSpeed`, `LegacyRealtimeTemperature`, `LegacyRealtimeMaxResponseOutputTokens`, `LegacyRealtimeSecretExpiration`, and the two legacy request `validate()` hooks
- Sources: D0015/D0017/D0153 precedent; only `speed` carries schema-backed min/max (0.25–1.5), the rest are description prose; openai-python/node decode without validation; the GA client-secret request already used the opt-in pattern.
- Decision: the four leaf types deserialize transparently (the `Limited` arm widens `u16`→`i64` so any JSON integer round-trips); constructors keep rejecting out-of-range values; the new request-level `validate()` hooks re-check values that arrived via Serde.
- Reason: decode-time rejection of prose-only ranges failed whole official payloads that the schema permits
- Impact: `openai-rs-types` legacy Realtime (breaking: `Limited(i64)`, `finite() -> Option<i64>`).
- Overrides: none
- Tests: `out_of_range_numeric_values_decode_losslessly_and_round_trip`, `request_validate_rejects_decoded_out_of_range_values`.

## D0170 — Legacy session response follows the official flat shape; client_secret is defensive Omittable

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `LegacyRealtimeSessionCreateResponse.client_secret`
- Sources: the pinned `RealtimeSessionCreateResponse` is a new nested shape without `client_secret` while the pinned *request* requires one — a request/response swap artifact; openai-python (beta) and openai-node both type the response with the required flat `client_secret {value, expires_at}`.
- Decision: follow the two official SDKs' flat shape, and relax `client_secret` to `Omittable` (missing-key tolerant) so the pinned nested shape would also decode (its undeclared fields land in `ExtraFields`); the accessor returns the presence state.
- Reason: the required field was the only hard failure point against the artifact shape; defensive widening costs nothing on the baseline shape.
- Impact: `openai-rs-types` legacy Realtime (breaking: accessor returns `&Omittable<...>`).
- Overrides: none
- Tests: `session_response_without_client_secret_decodes_new_nested_shape`, updated `response_secret_debug_is_redacted_and_json_round_trips`.

## D0171 — Realtime keepalive is opt-in; create_call is explicitly single-shot

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimeKeepalive`, `RealtimeWebSocketConfig.keepalive`, `Realtime::create_call`
- Sources: openai-python rides `websockets` defaults (keepalive ping 20s, pong timeout 20s) with no typed opt-out; openai-node ships no WebSocket transport (browsers cannot client-ping); this crate documents no automatic reconnect (D0122/D0148). Closes round-2 item 2-D1.
- Decision: `RealtimeKeepalive { ping_interval, pong_timeout }` (non-zero validated) is available via `RealtimeWebSocketConfig::keepalive` (default `None` = off, matching the no-behavior-change side of the split baselines) plus `with_keepalive_intervals` for callers that cannot name the type. `recv` drives it with a biased `select!`: a ping is sent every interval and the connection fails with `WebSocketProtocol` when no inbound frame of any kind arrives within `ping_interval + pong_timeout` (any-frame liveness, not per-Pong matching, so middleboxes swallowing control frames cannot cause false timeouts); no automatic reconnect. `create_call` is documented and annotated as retry-equivalent `RetryClass::Never` (a replay could double-answer the call), matching accept/reject/hangup/refer; python's retry-everything default is the recorded divergent baseline. `RealtimeKeepalive` is re-exported at the client root and the facade.
- Reason: idle sessions previously hung forever with no liveness detection; the call-create classification was implicit.
- Impact: `openai-rs-client` Realtime config and transport; no default behavior change.
- Overrides: none
- Tests: `keepalive_policy_is_opt_in_and_validates_its_fields`, `keepalive_times_out_idle_connection_without_inbound_frames`, `keepalive_disabled_by_default_leaves_idle_connection_unchanged`, `keepalive_silence_window_refreshes_on_every_inbound_frame`, `create_call_is_single_shot_even_after_a_retryable_looking_error`.

## D0172 — Image edit quality and size split per host

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ImageEditMultipartMetadata.quality`, `ImageEditJsonRequestBody.quality`/`.size`
- Sources: pinned `CreateImageEditRequest.quality` (standard/low/medium/high/auto — no `hd`); pinned `EditImageBodyJsonParam.quality` (low/medium/high/auto) and `.size` (closed auto/1024x1024/1536x1024/1024x1536, no bare-string branch); multipart edit and generation `size` keep `anyOf [string, enum]`.
- Decision: new open enums `ImageEditMultipartQuality` (5), `ImageEditJsonQuality` (4), `ImageEditJsonSize` (4, `Unknown` retained); generation-side `ImageQuality`/`ImageSize` unchanged; lossless `From<narrow>` conversions; builder setters narrowed.
- Reason: reusing the generation enums let both edit hosts send pin-external values.
- Impact: `openai-rs-types` image edit builders (breaking: setter types).
- Overrides: none
- Tests: `image_edit_multipart_quality_pins_the_five_official_values`, `image_edit_json_quality_pins_the_four_official_values`, `image_edit_json_size_pins_the_four_closed_values`.

## D0173 — FT reinforcement grader union splits top-level and nested member domains

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `experimental_graders::Grader`, `ReinforcementGraderMember`, `GraderCollection`, `MultiGrader`, `validate_reinforcement_grader`
- Sources: pinned `FineTuneReinforcementMethod.grader`/`ValidateGraderRequest`/`RunGraderRequest` oneOf (exactly five branches, no `GraderLabelModel`); `GraderMulti.graders` oneOf includes `GraderLabelModel` and excludes `GraderMulti` (non-recursive); the evals-side D0158 split is the in-repo precedent.
- Decision: the top-level union drops `label_model`; a new member union (label_model yes, multi no) backs the nested collection; the One|Many array-compat shape stays and gains a `one()` constructor; out-of-domain tags decode as `Unknown` losslessly.
- Reason: the merged union let the top level carry label_model and nested multi recurse without bound, both outside the pinned schemas
- Impact: `openai-rs-types` Fine-tuning (breaking: `Grader::LabelModel` removed; collection member type narrowed).
- Overrides: none
- Tests: `top_level_label_model_grader_is_not_constructible_but_stays_lossless`, `nested_multi_member_is_not_recursively_constructible_but_stays_lossless`.

## D0174 — Vector-store list limits enforce only the schema-backed lower bound

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `VectorStoreListLimit`, `VectorStoreValidationError::InvalidListLimit`
- Sources: the three pinned VS list `limit` parameters carry only `default: 20` (no `maximum`; the 1..=100 ceiling is prose); openai-python passes unbounded integers; D0154 established the same treatment for Files/Batches; search `max_num_results` keeps its schema-backed 1..=50.
- Decision: drop `MAX_VECTOR_STORE_LIST_LIMIT`, keep `>= 1`, keep the default constant.
- Reason: the invented ceiling rejected values the pinned endpoints and official SDKs accept
- Impact: `openai-rs-types` Vector Stores query types (breaking: public constant removed).
- Overrides: none
- Tests: `vector_store_list_limit_requires_at_least_one`.

## D0175 — Skills object discriminators split per host

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `SkillObjectType`, `SkillListObjectType`
- Sources: the pinned `SkillResource.object`/`SkillListResource.object`/`SkillVersionListResource.object` are single-value constants (`skill`/`list`/`list`); openai-python types them per host; the files crate's `FileObjectType`/`FileListObjectType` split is the in-repo precedent.
- Decision: split the shared two-value enum into `SkillObjectType{Skill}` and `SkillListObjectType{List}` (both list pages share the latter); the version/deleted single-value enums stay.
- Reason: a shared two-value enum blurred the per-host constants the pin and official SDKs define
- Impact: `openai-rs-types` Skills (response-side only; no wire change).
- Overrides: none
- Tests: `object_discriminators_are_split_per_host`.

## D0176 — Administration transport gains the platform retry semantics

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AdminClient`/`AdminClientBuilder`/`AdminInner::send`
- Sources: openai-python `_base_client.py` retries every request by default (including mutations); transport.rs already implements the D0131 classification and delay semantics; the admin channel previously had no retry surface at all.
- Decision: `AdminInner` carries a `RetryPolicy` (builder `with_retry_policy`, default `openai_compatible`); GET/DELETE classify as Safe, POST as Replayable (gated by `retry_replayable_mutations`); connect/timeout errors retry before a status is known; `request_timeout` doubles as the per-operation budget; the transport's private delay helpers are minimally duplicated with a same-source note. The sealed trust boundary is unchanged.
- Reason: 119 operations — including read-only GETs — failed on transient 429/5xx while the platform channel retried.
- Impact: `openai-rs-client` admin channel (new builder method).
- Overrides: none
- Tests: `admin_get_retries_429_to_success_with_local_backoff`, `admin_post_retry_is_gated_by_replayable_mutations`, `admin_get_does_not_retry_when_the_policy_is_disabled`.

## D0177 — AssignmentObject pins the six evidenced discriminators

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AssignmentObject`
- Sources: the four pinned assignment resources use single-value `object` constants (group.user / group.role / user.role / group.user.deleted); `DeletedRoleAssignmentResource.object` is a free string whose description exemplifies `group.role.deleted`/`user.role.deleted`; the four `organization.*.assignment` values appear nowhere in any baseline.
- Decision: remove the four phantom variants; unknown values still decode as `Unknown` losslessly.
- Reason: phantom variants mislead pattern matching and documentation without any baseline evidence
- Impact: `openai-rs-types` Admin (response-side; misleading variants removed).
- Overrides: none
- Tests: updated `admin_typed_objects_match_openapi`.

## D0178 — Usage group_by is typed as the ten-value endpoint union

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `UsageGroupBy`, `UsageQueryParams.group_by`
- Sources: the union of the eleven pinned usage endpoints' `group_by` item enums equals ten values, matching the `UsageDimensions` field set; openai-python types each endpoint's list as `Literal` unions; costs' `line_item` stays on `UsageCostsGroupBy` (D0150).
- Decision: new open enum `UsageGroupBy`; the shared usage parameter narrows to `Omittable<Vec<UsageGroupBy>>`.
- Reason: an untyped vector accepted arbitrary group_by strings the pinned endpoint enums reject
- Impact: `openai-rs-types` Admin usage query (breaking: field type).
- Overrides: none
- Tests: `usage_group_by_pins_the_ten_value_union_of_the_usage_endpoints`.

## D0179 — AdminListObject is the single `list` constant

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AdminListObject`
- Sources: every pinned admin list envelope types `object` as the single `list` constant; the only `page` envelope is `UsageResponse`, already carried by the dedicated `UsagePageTag`.
- Decision: remove the `Page` variant; decoding stays lossless via `Unknown`.
- Reason: the Page variant had no pinned list envelope behind it
- Impact: `openai-rs-types` Admin (response-side narrowing).
- Overrides: none
- Tests: `admin_list_object_pins_the_single_list_constant`.

## D0180 — Administration query encoder drops explicit null and empty-string values

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `append_query_value` (`openai-rs-client` admin channel). Closes round-2 item 2-D3.
- Sources: D0145 established the drop rule for the JSON transport after openai-python `_qs.py::_stringify_item`; the admin encoder was the last path emitting `key=`.
- Decision: `Value::Null` and empty strings skip the key; array/nested recursion inherits the leaf rule; a fully-dropped query yields no `?`. The admin channel's only Nullable query field (`after`) treats explicit null as omission, matching Python's `after=None`.
- Reason: the admin channel was the last path emitting `key=` for values both official SDKs drop or reject
- Impact: `openai-rs-client` admin query encoding only.
- Overrides: none
- Tests: `query_encoder_drops_null_and_empty_string_leaf_values`, updated `query_encoder_supports_arrays_null_and_deep_objects`.

## D0181 — open_string_enum exposes AsRef only, not Borrow

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `open_string_enum!` (`scalar.rs`)
- Sources: the std `Borrow` contract requires borrowed and owned values to agree under Eq/Ord/Hash; the derived enum implementations hash the discriminant (unit variants exclude the wire string entirely) and order by discriminant, so both necessarily violate the contract (`map.get("str")` would compile and probe the wrong bucket).
- Decision: the macro no longer generates `impl Borrow<str>`; `AsRef<str>`/`as_str`/`Display` remain; `opaque_string_id!` keeps its (consistent) `Borrow`.
- Reason: no in-repo call sites depend on it (workspace-wide check green); removing it eliminates a silent-miss collection-key trap.
- Impact: `openai-rs-types` macro surface (breaking only for `&str`-probing keyed collections of open enums, which do not exist in-tree).
- Overrides: none
- Tests: `open_enum_borrow_is_as_ref_only_and_keys_stay_whole_values`.

## D0182 — Sibling-key ref inlining is bounded by a node budget

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `MAX_REF_INLINE_NODES`, `StructuredError::ExpansionBudgetExceeded`, `normalize`/`normalize_object`
- Sources: D0143's chain detection bounds single-path depth only; an acyclic DAG whose levels each carry two sibling-key references expands geometrically (40 levels ≈ 2^40 nodes).
- Decision: every inline charges the merged subtree's node count against `MAX_REF_INLINE_NODES` (100,000, saturating); exhaustion returns `ExpansionBudgetExceeded { path, budget }` while the in-flight schema stays bounded. The module contract documents the chain (depth) / budget (total) split.
- Reason: chain detection bounds depth, not total size, so a wide acyclic DAG could exhaust memory
- Impact: `openai-rs-types` structured output; new public constant and error variant (enum already `#[non_exhaustive]`).
- Overrides: none
- Tests: `wide_ref_dag_trips_the_expansion_budget_instead_of_exploding`, `small_ref_dag_stays_within_the_expansion_budget`.

## D0183 — Codex app-server enums are pinned to the schema domains

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-codex` protocol (ThreadStartParams/TurnStartParams/UserInput/Turn/CancelLoginResponse/Account/RateLimitSnapshot/AccountUpdatedNotification)
- Sources: pinned Codex 0.144.5 `v2/AskForApproval` (two-branch oneOf), `v2/SandboxMode` (3), `v2/Personality` (3), `v2/ReasoningSummary` (4), `v2/ImageDetail` (4), `v2/TurnStatus` (4), `v2/CancelLoginAccountStatus` (2), `v2/PlanType` (12), `v2/RateLimitReachedType` (5), `v2/AuthMode` (7); `v2/ReasoningEffort` has no enum (stays `String`).
- Decision: send- and receive-side closed domains become D0115-style open enums (known + `Unknown`); `AskForApproval` is a two-branch union (mode enum + `GranularAskForApproval` object whose wire keys stay snake_case per the pin: `mcp_elicitations`/`request_permissions`/`rules`/`sandbox_approval`/`skill_approval`); the pinned `unknown` plan literal is named `UnknownPlan` (existing convention); `Account.kind`'s doc now lists `amazonBedrock`.
- Reason: bare `String` hid pinned domains; the granular approval object was previously inexpressible.
- Impact: `openai-rs-codex` protocol (breaking: field types).
- Overrides: none
- Tests: `thread_start_params_serialize_typed_enum_domains`, `approval_policy_granular_branch_matches_the_pinned_wire_shape`, `turn_start_params_serialize_typed_enum_domains`, `image_detail_covers_the_pinned_domain_and_keeps_unknowns`, `receive_side_enums_decode_known_and_unknown_values`.

## D0184 — RMCP catalog name limit is single-sourced from the types-side function-tool constant

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `MAX_FUNCTION_NAME_BYTES` (`openai-rs-rmcp` catalog.rs)
- Sources: pinned `FunctionToolParam.name` maxLength 128; `openai_rs_types::responses::MAX_FUNCTION_TOOL_NAME_CHARS` is the same bound used by the request-side validator.
- Decision: the catalog's valid-name budget aliases the types-side public constant (64 → 128); the mapped-name prefix budget stays derived (limit − 2 separator − 16 hex), so mapped names land exactly at the 128-byte bound.
- Reason: the previous 64-byte threshold unnecessarily mapped 65–128-byte valid MCP names and could drift from the request-side validator.
- Impact: `openai-rs-rmcp` name policy; 65–128-byte names are now exposed verbatim.
- Overrides: none
- Tests: `name_length_boundaries_follow_the_pinned_function_tool_limit`.

## D0185 — RMCP lossless envelope retains resultType (SEP-2322) and _meta

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ToolResultEnvelope` (`openai-rs-rmcp` result.rs)
- Sources: rmcp 3.1.4 `CallToolResult` carries `result_type: Option<ResultType>` (wire `resultType`; absent means complete) and `meta` (wire `_meta`); SEP-2322 adds the partial/complete discriminator.
- Decision: the envelope adds `result_type: Option<String>` and `meta: Option<Map<String, Value>>`, both skipped when absent; `from_rmcp` copies both verbatim (preserving non-`complete` discriminators such as `input_required`); the type docs now describe the full mirror.
- Reason: dropping the two fields contradicted the envelope's lossless contract once peers adopt SEP-2322
- Impact: `openai-rs-rmcp` result encoding; `CompactWhenPossible` flattening unchanged.
- Overrides: none
- Tests: `lossless_envelope_round_trips_result_type_and_meta`, `lossless_envelope_omits_result_type_and_meta_keys_when_absent`, plus new assertions in `lossless_envelope_covers_every_rmcp_content_kind`.

## D0186 — Facade re-exports keep pace with client-root additions (round 3)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `openai-rs-sdk` facade (`AdminOperation`, `AdminQuery`, `AdminAuthScope`, `AdminRequestEncoding`, `AdminResponseMode`, `AdminClientOperationContract`, both operation manifests, `C2paValidationState`, `ContentProvenanceObjectType`, `ProvenanceDetectionOutcome`, `RealtimeKeepalive`)
- Sources: all names are public at the client root (the traits bound public generic methods; the provenance discriminants are returned by already-re-exported getters; `RealtimeKeepalive` configures the re-exported realtime client). Continues D0135/D0164.
- Decision: mirror the client-root export list under the corresponding facade gates, plus compile-level nameability tests; the client crate's `tagged_union_reject_known!` macro is now gated behind `legacy-evals` so feature-unified builds of dependent crates stay warning-free.
- Reason: facade-only users could not name public bounds, manifests, or the new keepalive configuration
- Impact: facade crate only; no wire behavior change.
- Overrides: none
- Tests: `admin_operation_machinery_is_nameable_through_the_facade`, `content_provenance_discriminators_are_nameable_through_the_facade`.

## D0187 — Round-3 recorded positions (docs/tests only)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: SSE strict handshakes; MultiGrader array form; Accept-header refinement; Uploads bytes pre-check; README example parity; webhook signature-cap test
- Sources: openai-python `_streaming.py` consumes only `data.type` and openai-node tolerates a missing `event:` line, yet every official fixture and spec example emits `event:` matching `data.type`; the pinned `GraderMulti.graders` schema is a single object while the spec's own example uses an array; openai-python sends the default `Accept: application/json` everywhere and the server does not branch on Accept; the pinned upload `bytes` has no minimum; the README claims parity with the executable example.
- Decision: keep the three SSE strict checks (content-type gate, `event:` presence, `event` == `data.type`) as intentional anti-misrouting hardening with no observed official counterexample; keep the `MultiGraderMembers` One|Many decode with the explicit `many()` constructor as documented example compatibility; keep the per-mode Accept refinement; keep the defensive `bytes < 0` upload pre-check (alongside the path-segment guards, it rejects only semantically impossible values while 0 stays legal); the README Responses sample is byte-identical to `examples/responses.rs` (verified by diff) and must move with it; the webhook signature-cap test now exercises the real 32-valid-candidate branch (with an invalid-candidate counter-assertion).
- Reason: each position either hardens beyond lenient baselines without rejecting any evidenced wire form, or documents parity the repo already promises.
- Impact: documentation and test coverage only (plus the README sample).
- Overrides: none
- Tests: `rejects_replay_future_tamper_and_unusable_signature_lists` (name as
  renamed by D0225), `multi_grader_accepts_pinned_single_and_official_example_array`,
  existing SSE handshake tests.

## D0188 — Prompt-cached message drops the unsupported id/status null setters (corrects D0050)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaPromptCachedInputMessage` null setters. Closes round-3 item 3-29 (round-2 item 2-D2).
- Sources: within the pinned beta input union, the message tag resolves to `BetaEasyInputMessage` (no id/status; `phase` anyOf-null) or `BetaInputMessage` (`agent` anyOf-null; `status` a non-null enum; no id/phase); `id` exists only on the resource shape `BetaInputMessageResource`, which the input union never references; openai-python and openai-node param types carry neither field. D0050's Sources cited the `Beta*ItemParam` family, which does not cover this message-tag branch.
- Decision: remove `id_null()` and `status_null()` (no pinned branch permits either null); keep the non-null `id()`/`status()` setters as documented replay-compat supersets (decoded stored items carry them, and the field serialization stays lossless); `agent`/`agent_null`/`phase`/`phase_null` keep their branch-backed evidence. D0050's evidence attribution is hereby corrected rather than superseded.
- Reason: the two null setters could emit keys no pinned request schema defines, and the ledger cited the wrong family as cover.
- Impact: `openai-rs-types` beta Responses construction surface (breaking: two setters removed).
- Overrides: corrects D0050
- Tests: updated `beta_item_official_nulls_match_openapi` (id/status keys now asserted absent, agent/phase nulls retained).


## D0189 — Item-status construction narrowed on the remaining five hosts (extends D0168)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ComputerCall::new`, `LocalShellCall::new`, `ToolSearchCallInput::status`, `ReasoningItem::status`, `LocalShellCallOutput::status`, `BetaPromptCachedInputMessage::status`
- Sources: the pinned item request schemas type each `status` as the three-value trio (ToolSearch reuses `FunctionCallItemStatus`); round-3's D0168 missed these five construction sites plus the beta prompt-cached message.
- Decision: all five take `FunctionCallItemStatus` (or `MessageStatus` on the beta message); decode-side fields stay the shared open superset; the per-host test table grew to twelve hosts.
- Reason: the leftover sites could still construct statuses their pinned schemas reject.
- Impact: `openai-rs-types` Responses construction surface (breaking: parameter types narrowed).
- Overrides: none
- Tests: extended `per_host_item_status_enums_pin_official_domains`, `beta_prompt_cached_message_status_pins_message_trio`.

## D0190 — Tool-call output items expose read accessors

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `WebSearchCall`, `FileSearchCall`, `ComputerCall`, `LocalShellCall`, `ImageGenerationCall`, `CodeInterpreterCall`, `McpListTools`, `McpCall` impl blocks
- Sources: the pinned output items require `id`/`status` (and carry results, code, container ids, MCP errors) and both official SDKs expose them as public attributes; the crate already provided `id()`/`status()` on `OutputMessage`/`FunctionCall`, making the omission self-inconsistent.
- Decision: hand-written read getters (`status()`, `id()`, plus `call_id()`, `result()`, `code()`, `container_id()`, `error()` where applicable). Rust's name collision forced three builders to the `with_*` convention (`ImageGenerationCall::result`→`with_result`, `CodeInterpreterCall::code`→`with_code`, `McpListTools::error`→`with_error`). The `required_tagged_record!` macro deliberately generates nothing — macro-level getters would return `&String`/`&Nullable<String>` shapes and collide with builder names.
- Reason: failure states (`status: failed`, MCP `error`) were unobservable on the typed surface.
- Impact: `openai-rs-types` Responses output items (breaking: three builder renames, pre-1.0).
- Overrides: none
- Tests: `tool_call_output_items_expose_read_accessors`.

## D0191 — output_parsed routes Failed; accumulator stream errors carry the full payload

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `OutputParseError`, `Response::output_parsed`, `ResponseAccumulatorError::Stream`
- Sources: the pinned `Response.error` object (required, nullable) was dropped when routing a failed response through `output_parsed` (only Incomplete was routed); the pinned stream error event requires `param` and `sequence_number` and codes can be null.
- Decision: `OutputParseError::Failed(ResponseError)` joins Incomplete (a failed response with `error: null` synthesizes an `Unknown("failed_without_error")` payload to keep the variant typed); `ResponseAccumulatorError::Stream` now carries `code: Option<Box<str>>`, `param: Option<Box<str>>`, and `sequence_number`, with Display omitting absent segments; the Incomplete Display no longer debug-prints the reason.
- Reason: failed-response error details were silently replaced by `NoTextOutput`, and WebSocket-lane consumers lost param/sequence context.
- Impact: `openai-rs-types` Responses parse and accumulator error surfaces (breaking: `ResponseAccumulatorError::Stream.code` changed from `String` to `Option<Box<str>>` and gained fields).
- Overrides: none
- Tests: `response_output_parsed_branches`, `accumulator_stream_error_keeps_code_param_and_sequence_number`.

## D0192 — Beta response and WebSocket error surfaces mirror GA

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaResponse`, `BetaWebSocketErrorDetails`, `BetaWebSocketErrorEvent`, `StreamErrorEvent::param`, `BetaResponsesServerEvent::is_error`
- Sources: the pinned beta schemas reuse the GA error/incomplete shapes and the GA `BetaErrorPayload` fields (`code`/`param` required-nullable, optional `headers`); the GA twins already exposed full accessors and `ExtraFields`.
- Decision: `BetaResponse` gains `error()`/`incomplete_details()` (reusing GA types); the beta WS error details gain `code()`/`param()`/`headers()`/`extra_fields()` and the event gains `status()`/`extra_fields()` (both via flatten `ExtraFields`); `StreamErrorEvent::param()` joins its siblings; `is_error()` helpers document the channel split — SSE-shaped standalone errors are fatal (Err), the WS `type:"error"` envelope is lane-level and stays an Ok delivery (round-4 rejection 4-R1).
- Reason: mirrored wire shapes had asymmetric readability and lost unknown fields on re-encode.
- Impact: `openai-rs-types` beta Responses surface (additive).
- Overrides: none
- Tests: `beta_websocket_error_envelope_round_trips_every_field`, `server_event_is_error_covers_sse_and_websocket_shapes`, `beta_response_exposes_error_and_incomplete_details_accessors`, extended `stream_events_distinguish_terminal_and_unknown_events`.

## D0193 — In-band stream errors: legacy completions checks data errors; media flat error frames; truthiness predicate

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `completions.rs::decode_chunk`, `media.rs::decode_media_event`, the shared `error`-key predicate (chat/completions/media)
- Sources: openai-python `_streaming.py` raises on `data.get("error")` truthiness for every stream and openai-node throws on `sse.event === 'error'` or `data.error`; a legacy-completions error frame previously degraded to `Error::Decode` ("missing field id"); a media frame with `{"type":"error",...}` and no `event:` line decoded as an Unknown event and could be swallowed entirely by a following `[DONE]`.
- Decision: legacy completions decodes through `serde_json::Value` first and routes error keys to `StreamError::from_body`; media routes `type == "error"` frames the same way; the error-key predicate mirrors Python truthiness (`null`, `false`, `""`, `{}`, `0` pass; non-empty objects/strings/true fail).
- Reason: typed error semantics (code/param/request-id classification) were lost or delayed to an unrelated `UnexpectedEof`.
- Impact: `openai-rs-client` stream decoders only.
- Overrides: none
- Tests: legacy-completions mirror of `create_stream_surfaces_in_band_data_error`, media flat-error and truthiness cases.

## D0194 — All SSE streams honor sse_limits; finish loops are fail-stop

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `MediaEventStream::from_response` and its five call sites, the finish() loops in chat/completions/media, `response_stream.rs` finish loop, `ClientBuilder::sse_limits` docs, media SSE error-channel tests
- Sources: D0144 raised the default line cap for the very media payloads (multi-MiB `partial_image`) that the media streams then ignored by hardcoding defaults; `response_stream.rs` already failed-stop after an error while the other three finish loops kept yielding items from the same EOF flush.
- Decision: media streams read `transport().sse_limits()` like every other stream and the builder doc now says "all SSE streams"; all finish loops return after yielding an error; the response_stream finish loop's defensive Error handling is aligned with the push loop; media gains the missing error-path tests (remote error event, data error key, missing terminal, wrong content type, mid-body read failure).
- Reason: user-configured limits were silently ignored on five endpoints and error-then-item sequences violated the crate-wide fail-stop posture.
- Impact: `openai-rs-client` media transport and stream loops.
- Overrides: none
- Tests: `sse_limits_configuration_rejects_long_lines`, EOF-flush error-stop cases, media error-channel suite.

## D0195 — StreamError is channel-neutral and exposes the official type

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `StreamError` (`error.rs`)
- Sources: the pinned `Error`/`ErrorPayload` require `type`; both official SDKs expose it on stream-raised errors; the Display string said "OpenAI Responses stream error" while the type is shared by chat, legacy completions, media, and Responses streams.
- Decision: Display/doc become endpoint-neutral ("OpenAI stream error"); `kind()` exposes `type` from the flat body first and the nested `error` envelope second, alongside `code`/`param`.
- Impact: `openai-rs-client` error display (message text change) and accessor (additive).
- Overrides: none
- Tests: `stream_error_display_is_channel_neutral`, `stream_error_exposes_type_from_flat_and_nested_bodies`.

## D0196 — The API error envelope is per-field lenient

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ApiErrorBody`, `StreamErrorBody` (`error.rs`)
- Sources: openai-python reads each field independently from the body; a single wrong-typed field previously failed the whole envelope deserialization and dropped `code`/`param`/`type` for every malformed-but-partial body.
- Decision: all four envelope fields become `Option<Value>` with per-field lenient extraction (non-string scalars stringify, message included); stringification passes through the inline redactor; the leniency chain, fallback copy, malformed bodies, and flat→nested precedence are now test-locked, along with `Send + Sync` assertions for `Error`/`ApiError`/`StreamError`/`BodyPreview`.
- Impact: `openai-rs-client` error parsing (more fields survive malformed bodies).
- Overrides: none
- Tests: `api_error_malformed_field_does_not_discard_sibling_fields`, `api_error_non_string_message_is_stringified`, `api_error_string_error_key_falls_back_without_panicking`, `api_error_malformed_body_falls_back_without_secondary_failure`, plus the Send/Sync assertions.

## D0197 — ResponseMeta retains retry hints

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ResponseMeta`, `ApiError::retry_after`
- Sources: openai-python `APIStatusError.response.headers` and openai-node `APIError.headers` keep the full header set, so users can read `Retry-After` after retries are exhausted; only the six `x-ratelimit-*` headers were retained here.
- Decision: `ResponseMeta` records `retry_after` (the raw `retry-after-ms` or `Retry-After` text, ms preferred) and `x_should_retry` (literal true/false only) with public accessors; numeric interpretation and the zero/negative gating stay in the transport retry domain (D0131/D0201). `Location` remains unretained per the minimal-exposure stance.
- Impact: `openai-rs-client` operation metadata (additive).
- Overrides: none
- Tests: `retry_after_prefers_retry_after_ms_over_retry_after`, `retry_after_falls_back_to_retry_after_header`, `retry_after_absent_when_neither_header_is_present`, `retry_after_preserves_non_numeric_values_verbatim`, `should_retry_keeps_only_literal_booleans`, `api_error_exposes_retry_hints_from_response_headers`.

## D0198 — WebSocket handshake bodies, close frames, and the realtime failure posture

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `Error::WebSocketHandshake` (+`handshake_body()`), `map_websocket_error`, `RealtimeInbound::Closed`, `close_code()`/`close_reason()` on both WS clients, realtime recv failure branches, `RealtimeWebSocketConfig::write_buffer_bytes` (probe-write-failure test seam)
- Sources: tungstenite 0.29 buffers the non-101 response body in `Error::Http` (it was discarded, losing the API's auth/rate-limit detail — python's `InvalidStatus` carries the full response); `Message::Close` frames carry code/reason that both official stacks expose (`ConnectionClosedError.rcvd` / ws `close(code, reason)`); the realtime recv comment promised probe deactivation on error paths that did not set `closed`.
- Decision: handshake failures carry a `BodyPreview` (sanitized, truncated flag per the buffered tail; the envelope message stays out of Display per the D0208 preview stance); `Closed` carries the optional close frame and both clients record the last code/reason behind accessors (an unframed EOF stays `None` so coded closes remain distinguishable); every realtime transport/protocol failure (read error, Reject, keepalive timeout, probe write failure, oversized frame) retires the socket while event decode failures keep it open (node parity).
- Impact: `openai-rs-client` WS error surfaces (additive fields; realtime failure semantics tightened).
- Overrides: none
- Tests: `handshake_errors_carry_a_sanitized_body_preview`, `handshake_rejection_preserves_the_json_error_body` (×2), `peer_close_code_and_reason_stay_readable`, `websocket_close_code_and_reason_survive_the_close_handshake`, `rejected_frame_retires_the_realtime_socket`, `event_decode_failure_keeps_the_realtime_socket_open`, `keepalive_probe_write_failure_retires_the_realtime_socket`.

## D0199 — Recorded positions: realtime handshake single-shot; admin mint-retry interaction; chat/completions clean-EOF fail-stop (partial D0163 correction)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimeWebSocket::connect`, `AdminApiKeys::create`/`with_retry_policy`, chat/completions EOF handling; D0163 timeout wording
- Sources: neither official SDK retries WebSocket connections (Responses WS exposes an opt-in initial-connect policy; the realtime socket binds session state — model/intent/call_id — that is not replayable, so the knob is deliberately absent); the pinned admin key creation responses return `value` exactly once while the default policy replays POSTs like openai-python does (an orphaned credential is possible if the first attempt lands); D0163 cited python's `DEFAULT_TIMEOUT` as a 600s overall budget when httpx applies it per I/O operation — this crate's `request_timeout` is a total budget (node parity), which truncates long streams at 600s.
- Decision: document all three positions in rustdoc (realtime handshake single-shot with the "loop yourself" note; the mint-retry interaction with `conservative()`/`disabled()` escape hatches covering all three once-only endpoints); the chat/completions clean-EOF-without-`[DONE]` error joins the D0187 hardening family; D0163's timeout attribution is corrected by this entry; `Error::Timeout`/`Error::ResponseBody` docs state the invariant (Timeout = no response received; ResponseBody = response received, source distinguishes).
- Impact: documentation only.
- Overrides: corrects D0163's python-timeout attribution
- Tests: existing EOF/handshake tests.

## D0200 — Administration error-body reading matches the platform

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `admin.rs::error_from_response`/`read_limited`
- Sources: D0176 declared the admin channel behaviorally identical to the transport, but oversized error bodies produced `BodyTooLarge` (dropping the typed envelope) where the platform truncates into an `ApiError`, and interrupted reads produced `Transport` (dropping status/request-id) where the platform uses `ResponseBody`.
- Decision: admin reads use the transport's truncate-into-`ApiError` semantics for error bodies and map read interruptions to `ResponseBody { status, request_id }`; the helper is a line-for-line private copy annotated as same-source with `transport.rs`.
- Impact: `openai-rs-client` admin channel error surfaces.
- Overrides: none
- Tests: `oversized_error_body_is_truncated_into_a_typed_api_error`, `interrupted_error_body_read_surfaces_response_body_with_status`.

## D0201 — A parseable retry-after-ms decides alone

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `server_retry_delay` in `transport.rs` and the admin same-source copy
- Sources: openai-python returns the parsed millisecond value immediately (never consulting `Retry-After`); round-2 D0146 fixed the multipart copy but negative/non-finite parseable values still fell through to `Retry-After` in the transport and admin copies.
- Decision: any successfully parsed `retry-after-ms` value short-circuits the decision — positive and bounded becomes `Valid`, everything else (zero, negative, NaN, Inf, over-cap) becomes the local-backoff path; the guards moved into `bounded_delay`.
- Impact: `openai-rs-client` retry delay selection on mixed headers.
- Overrides: none
- Tests: `parseable_retry_after_ms_decides_alone_and_never_falls_back` (transport and admin copies).

## D0202 — Batch polling gains a completion-window preset

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `PollOptions::for_batches`, poll rustdocs on batches/fine-tuning/evals, poll.rs module docs
- Sources: the pinned `completion_window` enum's only value is `24h`; the generic `PollOptions::new()` (1s/10min) expires 144× before any realistic batch finishes; the module doc omitted Batches and background-mode Responses.
- Decision: `for_batches()` (5s interval, 24h deadline) joins the fine-tuning/evals presets; the three poll entry points point at their presets (the API stays explicit-supply); module docs list the real consumers.
- Impact: `openai-rs-client` polling surface (additive).
- Overrides: none
- Tests: `for_batches_preset_covers_the_only_batch_completion_window`, `batch_poll_accepts_for_batches_preset_options`.

## D0203 — Evals and vector-store failure surfaces are readable; rejected maxima report actual

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `EvalRun` (+counts/error), `EvalRunOutputItem`, `EvalSample`, `experimental::RunGraderResponse` (+metadata tree), `RunGraderErrors` accessors, `VectorStoreValidationError::InvalidMaxResults`
- Sources: the pinned run/grader failure payloads (14-field `RunGraderResponse.metadata.errors`, run `error`/`result_counts`, per-item status/sample) decode fully but were write-only on the typed surface, unlike `FineTuningJob`'s public fields and `Batch::errors()`; `InvalidMaxResults` stored the rejected value in a field named `maximum`.
- Decision: read accessors across the evals failure tree (Nullable fields surface as `Option`, `type` accessors named `kind()` per crate convention); the variant field renames to `actual` with unchanged Display.
- Impact: `openai-rs-types` Evals/Vector Stores surfaces (additive; enum field rename on a `#[non_exhaustive]` variant).
- Overrides: none
- Tests: `eval_run_result_counts_and_error_are_readable`, `output_item_status_sample_and_sample_error_are_readable`, `run_grader_metadata_and_error_flags_are_readable`, `max_results_bounds_report_the_rejected_actual_value`.

## D0204 — Custom-voice audio limit reports RequestPayloadTooLarge at send time

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `Error::RequestPayloadTooLarge`, `voices.rs::prepare_bounded`, `VoiceRequestError` docs
- Sources: the pinned 10 MiB prose cap; in-memory sources fail at construction with `VoiceRequestError::AudioTooLarge` while file/stream sources only become measurable after preparation, and that send-time failure previously reused `InvalidConfiguration` ("invalid client configuration"), which is a client-config category.
- Decision: new `Error::RequestPayloadTooLarge { limit_bytes }` for the send-time half; the two-phase split is documented on both error surfaces; the threshold constant stays single-sourced.
- Impact: `openai-rs-client` error enum (additive variant) and voices transport classification.
- Overrides: none
- Tests: `oversized_send_time_audio_reports_request_payload_too_large`.

## D0205 — Webhook post-verification decode errors are sanitized; prefixed secrets validate at construction

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `WebhookVerificationError::Decode`, `WebhookVerifier::new`/`decode_secret`, tolerance boundary tests, verifier docs
- Sources: reproduced leak — the serde error's Debug/Display embed rejected payload literals (SIP credentials could surface through error chains, the only path outside the D0125 redline); a `whsec_`-prefixed secret decoding to an empty key was accepted and empty-key HMAC forgeries verify; node's docs carry the clock-sync guidance the verifier lacked.
- Decision: `Decode` becomes `{ kind: syntax|discriminator|type, line, column }` with no source chain (the category is re-derived from the payload envelope, not the error text); construction decodes prefixed secrets eagerly and rejects empty/invalid decodings with `InvalidSecret`; the future-side `== tolerance` pass boundary joins the past-side lock; module docs state the replay purpose, the symmetric window, and the clock-sync advice.
- Impact: `openai-rs-client` webhook verification (breaking: `Decode` shape).
- Overrides: none
- Tests: `decode_failures_report_sanitized_class_and_position_without_payload_content`, `empty_or_malformed_prefixed_secrets_are_rejected_at_construction`, extended `sub_second_tolerances_are_rejected_instead_of_truncating_to_zero`.

## D0206 — Codex: typed error notifications, distinguished child exits, hardened error channel

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `protocol.rs` (ErrorNotification/TurnError/CodexErrorInfo), `app_server/client.rs` (exit reaping, line limit), `error.rs` (Json/ResponseDecode/RequestTimeout/RpcError.data), `direct/transport.rs` (status_error)
- Sources: pinned `v2/ErrorNotification`→`TurnError`→`CodexErrorInfo` (11 string + 5 object variants, including the `turn/start`-failure `activeTurnNotSteerable`); a crashed or killed child surfaced as a plain stdout EOF while the reserved `ChildExit` kind went unused; serde error text can embed payload fragments; `JSONRPCErrorError.data` accepts explicit null; D0144's payload-scale argument applies to JSONL frames.
- Decision: the error notification channel is fully typed (open enums + granular object variants, flatten extras, `Turn.error` narrowed to `TurnError`); stdout EOF reaps the child (bounded by the shutdown timeout) and folds the exit status — plus a truncated stderr tail — into a `ChildExit` terminal failure, and `terminate()` no longer discards already-reaped statuses; `Error::Json` renders neutrally with decode failures carrying the method name (`ResponseDecode`) and `RequestTimeout` gaining `method`; `RpcError.data` becomes three-state so explicit null round-trips; the default JSONL frame limit rises 4 MiB → 32 MiB (still bounded, configurable); direct `status_error` falls back to a sanitized `error.message` when the code is absent; the dead `UnsupportedDirectTransport` variant is removed.
- Impact: `openai-rs-codex` protocol/error/client surfaces (breaking: variant removal, field type changes; additive: typed error channel).
- Overrides: none
- Tests: `error_notification_decodes_and_serializes_the_pinned_shape`, `codex_error_info_string_branch_covers_the_pinned_domain_and_keeps_unknowns`, `codex_error_info_object_variants_match_the_pinned_wire_shape`, `failed_turn_error_is_typed_and_lossless`, `rpc_error_data_keeps_the_three_wire_states`, `json_and_decode_failures_keep_neutral_messages`, `child_exit_failure_displays_its_message`, `default_line_limit_matches_the_payload_scale`, `child_crash_exit_status_reaches_in_flight_requests`, `rpc_error_response_preserves_code_message_and_data`, `eof_mid_frame_fails_in_flight_requests`, `invalid_json_frame_fails_in_flight_requests`, `status_error_prefers_code_and_falls_back_to_sanitized_message`.

## D0207 — RMCP bridge: cancellation semantics, unsupported results, and model re-exports (round-4 item 4-41)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BridgeError`, `RmcpExecutor::call_cancellable`/`list_tools`, `ResultEncoding::CompactWhenPossible`, crate-root re-exports, `examples/rmcp_bridge.rs`
- Sources: rmcp 3.1.4 resolves peer-cancelled requests as `Cancelled` (both directions share the shape); cancel-notification delivery can fail after transport close, which previously masked the cancellation as `Transport`; SEP-2322 `InputRequired`/SEP-2663 task results are successful exchanges the bridge cannot drive; MCP makes cancel notifications optional; the compact flattening provably drops `resultType`/`_meta` (D0185 fields).
- Decision: `Cancelled` documents both initiators (no `initiator` field, preserving the variant surface); the cancel branch ignores notification-delivery failure and returns `Cancelled` (matching the timeout branch); `UnsupportedResult { kind }` separates input-required/task outcomes from executor failures; `list_tools` cancellation stays local by design (documented); the compact doc discloses the dropped fields; the crate root re-exports `rmcp::model::{Tool, CallToolResult, ContentBlock, JsonObject}` so facade-only users can implement the executor trait; a new example demonstrates the DispatchOutcome split.
- Impact: `openai-rs-rmcp` error surface (additive variant, cancel-classification change) and exports; facade example added.
- Overrides: none
- Tests: `cancellation_wins_over_a_failed_cancel_notification_delivery`, `input_required_results_report_an_unsupported_result_kind`, `unknown_function_is_rejected_before_any_execution`, `discover_propagates_list_tools_protocol_failure`.

## D0208 — Recorded positions: constraint errors stay count-only; the error preview stance

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CreateResponseConstraintError` shape; `BodyPreview` behavior; `StructuredError::Decode` messages
- Sources: the audit asked whether validation errors should carry item paths (`input[6]`) and whether the preview/Display truncation should be ledgered; both behaviors predate the audit rounds and carry deliberate privacy/simplicity trade-offs (variants intentionally omit values; the transport's `Error::Decode` already provides serde paths for decode failures).
- Decision: `CreateResponseConstraintError` stays count-only (positioning belongs to decode-side serde paths); the `BodyPreview` stance is hereby recorded — an 8 KiB cap, sensitive-key and prefix redaction, Display limited to status/code/request-id, Debug fully redacted, versus python's full `response.text` exposure; `StructuredError::Decode` similarly keeps serde's own location info without payload snippets.
- Impact: documentation of existing behavior.
- Overrides: none
- Tests: existing redaction and constraint tests.

## D0209 — follow_up_from drops conversation; previous_response_id documents the mutual exclusion

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `CreateResponseRequest::follow_up_from`, `previous_response_id`/`conversation` builders (GA+beta create and count faces)
- Sources: pinned `ResponseProperties.previous_response_id` — "Cannot be used in conjunction with `conversation`." — carried by both official SDKs' parameter docs; the follow-up helper copied `conversation` after setting `previous_response_id`, producing the rejected combination.
- Decision: the follow-up constructor copies only the seven stable prefix fields and never `conversation`; the mutual-exclusion sentence now appears on both builders in both directions across GA and beta create/count faces.
- Reason: conversation-mode callers invoking the continuation helper got a request the service rejects with 400.
- Impact: `openai-rs-types` Responses construction (behavior change on the helper; docs).
- Overrides: none
- Tests: `follow_up_from_does_not_carry_conversation`.

## D0210 — Beta Responses REST face sends the OpenAI-Beta preview header on every lane

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaResponses` REST operations (create/retrieve/delete/cancel/compact/input_items/input_tokens and both SSE lanes); `Transport::execute_optional_json_with_static_header`, `send_with_static_header`
- Sources: the pinned `?beta=true` routes declare the optional `openai-beta` header (enum `["responses_multi_agent=v1"]`); openai-python's `betas` parameter and openai-node's `betas` option send it on every beta method, streaming and delete included.
- Decision: all JSON, empty-or-JSON, and SSE beta lanes carry `OpenAI-Beta: responses_multi_agent=v1` via the static-header transport entries (two new entry points were added for the empty-or-JSON and raw-send lanes); the WebSocket face stays header-free (python exposes the preview REST-only). The header value is the enum's single member — the only expressible setting.
- Reason: the header is the documented preview gate in the official SDKs; three of nine operations previously lacked it with no escape hatch.
- Impact: `openai-rs-client` beta transport headers (additive wire header on delete and SSE lanes).
- Overrides: none
- Tests: `create_uses_beta_query_multi_agent_body_and_beta_header` and per-lane assertions in `beta_responses.rs` tests.

## D0211 — create_call sends a bare application/sdp request when no session is attached

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `Realtime::create_call`
- Sources: the pinned `POST /realtime/calls` requestBody lists both `multipart/form-data` and `application/sdp`; openai-python's `encode_multipart(raw_body_field="sdp")` and openai-node's `encodedMultipartFormRequestOptions(..., 'sdp')` both switch to the bare SDP body when it is the only field.
- Decision: an attached session keeps the pinned two-part multipart (sdp + session); an omitted session sends the SDP text with `Content-Type: application/sdp` (Accept stays `application/sdp`; 201/Location handling and the single-shot retry classification are unchanged). Unknown request keys are documented as not sent (the pinned encoding table defines only the two parts; decode stays lossless).
- Impact: `openai-rs-client` realtime call transport (wire form for the session-less case).
- Overrides: none
- Tests: `create_call_without_session_sends_a_bare_sdp_request`, existing `create_call_sends_multipart_and_returns_sdp_location`.

## D0212 — All three WebSocket recv loops share one failure posture

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaResponsesWebSocket`, `ResponsesWebSocket::recv`, `RealtimeWebSocket::recv`
- Sources: round-4's D0198 stated "both WS clients" while the crate has three; the beta loop dropped close frames, kept an explicit Pong write, and left the socket open after protocol errors; the GA loop left non-decode failures unretired.
- Decision: every transport/protocol failure (read error, oversized text, Binary/Frame rejection, keepalive timeout, probe write failure) retires the socket (`closed = true`); event decode failures keep it open (node parity); close frames are recorded and exposed via `close_code()`/`close_reason()` with the unframed-EOF-stays-None distinction; the beta loop's explicit Pong write was removed (tungstenite 0.29 auto-replies, per D0148).
- Impact: `openai-rs-client` beta/GA WebSocket failure semantics tightened; additive accessors.
- Overrides: extends D0198 to all three clients
- Tests: `beta_websocket_close_code_and_reason_survive_the_close_handshake`, `rejected_frame_retires_the_beta_socket`, `beta_event_decode_failure_keeps_the_socket_open`, `binary_frame_retires_the_responses_socket`.

## D0213 — Realtime connection targets reject empty strings

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `realtime_websocket_url`
- Sources: openai-node's `buildRealtimeURL` throws on empty model/callID targets; an empty target otherwise derives `?model=` / `?call_id=` onto the wire.
- Decision: the URL derivation rejects empty target values with `InvalidConfiguration`; enforcement sits at the derivation layer so `From<ModelId>` and direct enum construction are covered too (the named constructors stay infallible — their Debug shape is pinned by facade re-export tests).
- Impact: `openai-rs-client` realtime connect validation (additive).
- Overrides: none
- Tests: `websocket_url_rejects_empty_target_values`.

## D0214 — Usage image and web-search filters are typed enums (completes the D0178 family)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `UsageImageSource`, `UsageImageSize`, `UsageContextLevel`, `UsageQueryParams`
- Sources: the pinned `/organization/usage/images` `sources` (3 values) and `sizes` (5 values — including `1792x1792`, which differs from the generation-side `ImageSize` domain) and `/organization/usage/web_search_calls` `context_levels` (3 values) item enums; both official SDKs type them as Literal unions.
- Decision: three open string enums narrow the shared bag's vector fields; the usage-side image size keeps its own domain rather than aliasing the generation enum.
- Impact: `openai-rs-types` Admin usage query (breaking: field types).
- Overrides: none
- Tests: `usage_query_pins_image_and_web_search_filter_enums`, updated `admin_query_filters_match_openapi`.

## D0215 — Audit effective_at pins the four comparison keys

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AuditEffectiveAt`, `AuditLogListParams.effective_at`
- Sources: the pinned audit-logs `effective_at` object carries exactly gt/gte/lt/lte (integer); both official SDKs model the same four-key typed dict; the previous `BTreeMap<String, u64>` accepted arbitrary keys onto the wire.
- Decision: a dedicated four-field structure with per-bound builders; the deep-object encoder unchanged (`effective_at[gt]=…`).
- Impact: `openai-rs-types` Admin audit query (breaking: field type).
- Overrides: none
- Tests: `audit_effective_at_pins_the_four_comparison_keys`, `audit_effective_at_encodes_as_deep_object_bounds`.

## D0216 — Manual pagination getters share the D0147 cursor rule

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `next_after` on stored-chat (×2), container (×2), and admin (×2) page envelopes; `next_after_with` on the admin envelopes; `ChatCompletions::next_page`
- Sources: D0147 set last_id → last-item-id → fail-closed for the streaming channel only; the getters returned `Some("")` for empty cursors, which D0145 drops from the query, silently re-fetching the first page; python's page helpers advance via `data[-1].id`.
- Decision: every `next_after` filters empty cursors and falls back to the page's last item id (None when neither resolves); `next_page` builds on the getter and returns `None` instead of re-requesting; the admin envelopes gain `next_after_with(last_item_id)` for manual paging.
- Impact: `openai-rs-types` chat/containers/admin envelopes; `openai-rs-client` next_page paths.
- Overrides: extends D0147
- Tests: `stored_completion_and_message_pages_fall_back_to_the_last_item_id`, `container_and_file_pages_fall_back_to_the_last_item_id`, `admin_cursor_pages_fall_back_to_the_last_item_id`, `stored_list_next_page_falls_back_to_the_final_completion_id`, `stored_list_next_page_stops_without_a_resolvable_cursor`, `stored_messages_next_page_follows_the_same_cursor_fallback`, `container_page_stream_falls_back_to_the_last_container_id`, `container_file_page_stream_falls_back_to_the_last_file_id`.

## D0217 — Conversations and voice-consent list limits enforce only the schema-backed lower bound

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ConversationListLimit` handling (`InvalidListLimit.actual`), `ListVoiceConsentsParams.limit`
- Sources: both pinned `limit` parameters carry only `default: 20` (the 1..=100 range is prose); D0154/D0174 established the same treatment for Files/Batches/Vector Stores; D0203 fixed rejected maxima to report `actual`.
- Decision: the conversations ceiling is dropped (≥1 in builder and decode; variant field renamed to `actual`); voice consents gain the missing ≥1 floor at both serde boundaries (the chained client call site pins the infallible builder signature, so zero surfaces as an `EncodeQuery` failure before transport — the D0204 two-phase pattern).
- Impact: `openai-rs-types` Conversations/Voices query surfaces (breaking: variant field rename; additive error type).
- Overrides: none
- Tests: extended `request_validation_enforces_item_metadata_and_page_limits`, `consent_list_limit_enforces_the_schema_backed_minimum`.

## D0218 — Proxy posture: environment proxies never read; one explicitly declared proxy

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ClientBuilder::proxy`, `X509ClientBuilder::proxy`, `WorkloadIdentityAuth` (exchange coverage), the `no_proxy` call sites
- Sources: openai-node reads no environment proxies; openai-python honors `trust_env` by default — an unrecorded divergence; all four reqwest builders hard-coded `.no_proxy()` with no escape hatch for proxy-locked networks.
- Reason: an explicitly declared proxy is a chosen hop, compatible with the credentials-never-cross-invisible-hops stance (D0163's redirect rationale).
- Decision: the default stays `no_proxy()` (HTTP(S)_PROXY/ALL_PROXY are never honored); `ClientBuilder::proxy(Some(..))` is the single escape hatch and its value is propagated to the workload token exchange; the x509 builder gains its own `proxy` face (mTLS still terminates at the pinned origin through the CONNECT tunnel); Debug output redacts the proxy. The admin channel deliberately offers no proxy face (documented at its build site).
- Impact: `openai-rs-client` client surface (additive builder methods).
- Overrides: none
- Tests: `explicit_proxy_builds_and_none_restores_the_no_proxy_default`, `explicit_proxy_carries_traffic_so_no_proxy_no_longer_applies`, `builder_proxy_covers_the_token_exchange_too`, `explicit_proxy_face_is_accepted_and_redacted_from_debug`.

## D0219 — Client::with_request_timeout derives a budget-scoped client

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `Client::with_request_timeout`, `Inner`/`TransportDerivation`
- Sources: D0199 established `request_timeout` as a total budget (node parity) whose 600s default truncates long jobs; both official SDKs offer per-request timeout overrides, which the typed-first surface lacked.
- Decision: budgets belong to clients, not requests: the method derives a new `Client` from a stored construction blueprint (the transports are rebuilt to carry the overridden budget while sharing the connection pool and credentials); a zero budget fails closed immediately (`DeadlineExceeded`) since the non-fallible signature cannot reject it.
- Impact: `openai-rs-client` client surface (additive method).
- Overrides: none
- Tests: `with_request_timeout_narrows_only_the_derived_client`, `zero_derived_budget_fails_closed_immediately`.

## D0220 — Codex app-server: kill before locking the writer; writes join the request budget

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AppServerClient::terminate`/`request_value`/`notify`, `Error::RequestPayloadTooLarge`, `ThreadStartParams.cwd` and environment docs, `AppServerLimits::event_queue_capacity`
- Sources: terminate locked the writer before killing the child, so a blocked stdin write (full pipe) deadlocked teardown against the stdout reader — leaking the child and making a later `close()` report success without killing; the request timeout covered only the response wait, leaving writes unbounded; the `initialized` notification carried a pin-undefined `params: {}`; outbound frame overflow reused `InvalidConfiguration` (diverging from the platform-side D0204 category).
- Decision: terminate kills and reaps the child (bounded by the shutdown timeout) before taking the writer — the broken pipe releases every unbounded write (`notify`, `respond_*`); the request timeout wraps the write-plus-response exchange; `initialized` is method-only; oversized frames report `RequestPayloadTooLarge { limit_bytes }`; the child cwd (CODEX_HOME when `ThreadStartParams.cwd` is unset), the HOME-less environment allowlist, and the event-queue fail-stop posture are documented.
- Impact: `openai-rs-codex` shutdown/timeout/error surfaces (additive variant; behavior fixes).
- Overrides: none
- Tests: `close_releases_a_blocked_writer_by_killing_the_child_first`, `request_write_phase_shares_the_request_timeout_budget`, `server_request_responses_roundtrip_string_and_numeric_ids`, `oversized_outbound_frame_reports_request_payload_too_large`, `oversized_outbound_frame_has_a_dedicated_category`, handshake no-params assertion.

## D0221 — Webhook results carry the delivery id; verify declares the raw-body requirement

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `VerifiedWebhook` (`webhook_id` field/accessor, `from_verified` signature), verifier docs
- Sources: the official delivery contract — non-2xx or slow responses retry with exponential backoff for up to 72 hours, 3xx counts as failure, duplicates are possible, and `webhook-id` is the recommended idempotency key (node's docs spell all of this out); the verifier read the id for signature purposes and then discarded it; re-serialized JSON changes the signed bytes (node documents the raw-body requirement).
- Decision: the verified wrapper pins the delivery id behind `webhook_id()` (single-entry construction preserved; Debug stays silent); module docs state the delivery semantics; `verify`/`verify_at` warn that the payload must be the original request bytes taken before any JSON middleware.
- Impact: `openai-rs-types`/`openai-rs-client` webhook surfaces (breaking: `from_verified` gained a parameter).
- Overrides: none
- Tests: `verified_delivery_exposes_the_webhook_id_as_the_deduplication_key`, `verified_wrapper_keeps_the_webhook_id_through_mapping`, extended `verified_wrapper_never_debugs_the_body`.

## D0222 — RMCP discovery pagination is caller-bounded; result magnitude is types-side enforced

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ResponsesToolExecutor::list_tools`, `RmcpExecutor::list_tools`, `ResponsesToolBridge::discover`, `DispatchOutcome`, `encode_tool_result`
- Sources: rmcp 3.1.4's `list_all_tools` re-issues `tools/list` with each `nextCursor` until the server omits one — a traversal with no protocol-level bound; the pinned Responses API caps `function_call_output` strings at 10 MiB characters, and MCP media `data` arrives already base64-encoded, so envelope output grows by the full inflated payload.
- Decision: documented (no behavior change) — custom executors must return the complete page-merged tool set (first-page-only answers freeze a truncated catalog); `ExecutionControl`'s timeout/cancellation is the only bound on discovery, so `discover` callers should pass a bounded control (`unbounded()` is reserved for in-process executors); the encoder neither truncates nor checks the 10 MiB cap, which the types side enforces when the follow-up request is validated on the next turn.
- Impact: documentation of existing behavior; one cross-crate pin test.
- Overrides: none
- Tests: `oversized_rich_result_encodes_but_fails_next_turn_validation`.

## D0223 — Round-5 recorded positions (docs/decisions only)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: success-status lanes; containers DELETE Accept; X-Stainless headers; compression; connection pool/HTTP2; admin pagination surface; admin JSON content-type gate; `safety_identifier.blocked`; upload constraint docs; timeout docs
- Sources: python/node accept any 2xx while the platform lane requires the exact pinned status (multipart accepts 200+201, admin exactly 200); python sends `Accept: */*` on the containers deletes while the EmptyOrJson lane sends `application/json`; both official SDKs send X-Stainless platform/retry/read-timeout headers this crate omits entirely; both negotiate compression (this crate sends no `Accept-Encoding`); reqwest defaults (90s idle, unlimited per-host, h2-by-ALPN) differ from both baselines' pool settings; the admin channel offers no auto-pagination (python/node pages automatically); the admin lane enforces an exact `application/json` content type (and rejects a missing header) where the platform lane never checks; python alone types a `safety_identifier.blocked` webhook the pin lacks (node has 16 events, the pin 18); python docstrings carry the 64MB/8GB/1h upload constraints.
- Decision: all positions recorded as deliberate: exact-status fail-stop (with the multipart 201 tolerance noted); `application/json` on EmptyOrJson; no X-Stainless telemetry (the read-timeout hint is forgone with it); no compression negotiation (bandwidth-for-CPU); default reqwest pool/h2 posture; admin stays manual-paging (`next_after_with` being the helper) with its strict content-type gate as anti-misrouting hardening; the pinned 18-event webhook set stands with `Unknown` absorbing python-only discriminators; upload and timeout constraints now live in rustdoc.
- Impact: documentation only.
- Overrides: none
- Tests: existing lane/content-type/webhook tests.

## D0224 — Responses input-items limits fallible at ≥1; beta stream params pin stream=true (extends D0217/D0154)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ListResponseInputItemsParams::limit`, `BetaListInputItemsParams::limit` (`ListResponseInputItemsLimitError`), `BetaRetrieveResponseStreamParams.stream`
- Sources: the pinned input-items `limit` carries only `default: 20` (the 1..=100 range is prose), matching the D0154/D0174/D0217 family; the GA stream-params guard (`deserialize_true`) already rejected `stream=false` on decode while the beta twin accepted it, deferring the failure to a runtime content-type error.
- Decision: both input-items limit builders return `Result` and reject zero at the decode boundary too (prose ceilings not enforced); the beta stream params gain the same `stream=true` guard as GA. Completes the round-5 review's ledger gap for these breaking signature changes.
- Impact: `openai-rs-types` Responses/beta Responses list params (breaking: `limit()` builders now fallible).
- Overrides: none
- Tests: `input_item_list_limit_rejects_zero_on_build_and_decode`, `beta_input_item_list_limit_rejects_zero_on_build_and_decode`, `beta_retrieve_stream_params_pin_stream_true`.

## D0225 — Webhook signature candidates are bounded-verified; no slot-count rejection

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `decode_signatures` (`webhooks.rs`), `InvalidSignatureHeader` docs
- Sources: openai-node's `webhook-signature-amplification.test.ts` explicitly accepts a valid signature in slot 33 and after 1600 distinct invalid candidates; openai-python's `any(compare_digest)` has no cap; the 8 KiB joined-header bound (~170 `v1,<tag>` slots) is already the amplification limit. Round-6 item 6-01.
- Decision: the 32-candidate hard rejection and `MAX_SIGNATURE_CANDIDATES` are removed; every candidate within the header bound is evaluated at constant work (no short-circuit), keeping the 32-byte length filter, the invalid-candidate skip, and the zero-valid-candidate rejection.
- Reason: the fail-stop cap rejected deliveries both official SDKs verify.
- Impact: `openai-rs-client` webhook verification (behavior widened: 33rd+ valid candidates now verify).
- Overrides: revises D0205's implicit candidate-cap stance
- Tests: `a_valid_signature_in_slot_thirty_three_still_verifies`, `a_valid_signature_after_1600_invalid_candidates_still_verifies`, rewritten `rejects_replay_future_tamper_and_unusable_signature_lists`, plus the configuration fail-closed suite.

## D0226 — Codex app-server send surface completed; write-timeout fail-stop; extra Debug redacted

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ThreadStartParams`/`TurnStartParams` (+`SandboxPolicy`, `ApprovalsReviewer`, `SessionStartSource`, `NetworkAccess`, `W3cTraceContext`), `AppServerClient::with_trace_context`, `ConnectionFailureKind::WriteTimeout`, `redacted_extra_debug!`, `decode_notification` known list
- Sources: pinned 0.144.5 `v2/ThreadStartParams` (15 optional properties) and `v2/TurnStartParams` (turn-level `sandboxPolicy`/`approvalPolicy`/`approvalsReviewer`/`serviceTier` overrides), `v2/SandboxPolicy` four-branch tagged union with server-side defaults, `JSONRPCRequest.trace` optional `W3cTraceContext`; tokio `write_all` is not cancel-safe. Round-6 items 6-02/6-03/6-07/6-13.
- Decision: the two send DTOs model every pinned optional property (open enums, a `config` map escape hatch, server defaults expressed by omitting keys) and gain the flatten `extra` they uniquely lacked; W3C trace context is injected per-handle opt-in with three-state fields; a request timeout that fires before the write completes now tears the connection down (`WriteTimeout` terminal kind) since a half-written frame desynchronizes the JSONL stream, while response-late timeouts keep the connection; all thirty-one `extra` carriers (including `RpcError.data`) print only lengths/`<redacted>` in Debug; `error` joins the known-notification warn list.
- Impact: `openai-rs-codex` surface (additive fields/types; Debug output changes; breaking error-type removal noted in the round record).
- Overrides: none
- Tests: `thread_start_params_serialize_the_previously_missing_pinned_fields`, `turn_start_params_serialize_the_turn_level_overrides`, `sandbox_policy_matches_the_pinned_tagged_union`, `extra_carriers_debug_never_leaks_retained_values`, `error_notification_decode_failure_emits_warn`, `w3c_trace_context_serializes_the_pinned_wire_states`, `write_phase_timeout_fails_stop_the_half_written_connection`, `response_phase_timeout_keeps_the_connection_usable`, `trace_context_is_injected_only_into_opted_in_requests`.

## D0227 — Admin cursor empty-string family completed; delete discriminators typed; manifest pinned parity

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `AdminNextPage::next_cursor`, `UsageResponse::next_page`, fine-tuning events/checkpoints/permissions `next_after`; fifteen admin response DTO `object` fields; admin manifest tests
- Sources: D0145 already filtered empty cursors on `AdminCursorPage`/`AdminRequiredCursorPage` — five same-shaped getters were missed; the pinned `object` constants (python/node model Literal) while sibling discriminators were already open enums; every existing manifest guard was self-referential. Round-6 items 6-04/6-14/6-15.
- Decision: all five cursor getters return `None` for empty strings; fifteen DTO discriminators become open string enums (spend-alert/spend-limit each carry the organization+project pair); `admin_manifest_matches_pinned_operations_json` asserts bidirectional (operation_id, method, path) set equality against the pinned operations projection, replacing both hard-coded 119 counts with pin-derived counts.
- Impact: `openai-rs-types` admin/fine-tuning surfaces; `openai-rs-client` tests only.
- Overrides: extends D0145
- Tests: `admin_next_and_usage_page_cursors_drop_empty_strings`, `list_pages_drop_empty_cursors_when_more_remains`, `delete_and_resource_object_discriminators_are_pinned_open_enums`, `admin_manifest_matches_pinned_operations_json`.

## D0228 — Beta prompt-cached content pins its three construction branches; chat moderation policy is typed

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `BetaPromptCachedInputContent` construction + `BetaResponseInputConstraintError`; `ChatModerationConfig.policy`
- Sources: the pinned beta message-content unions are exactly input_text/input_image/input_file (the item-form-only `computer_screenshot` was constructible via the wide `Into<InputContent>` entry — D0167's beta gap); `CreateChatCompletionRequest.moderation` is the same `ModerationParam` the Responses host already types. Round-6 items 6-05/6-11.
- Decision: the prompt-cached content gains named `text`/`image`/`file` constructors only, and every beta request `validate()` rejects a decoded `computer_screenshot` branch through the new beta envelope error (decode stays the D0142 lossless four-branch bridge); chat `policy` reuses `responses::ModerationPolicy` with typed builders, and the `with_policy` escape hatch now requires its serialization to match the pinned shape exactly (out-of-domain members error rather than drop).
- Impact: `openai-rs-types` beta Responses construction (breaking: `new` removed; beta `validate()` error types unified) and Chat moderation field type.
- Overrides: extends D0167/D0142
- Tests: `beta_prompt_cached_content_pins_the_three_official_branches`, `beta_prompt_cached_computer_screenshot_decode_stays_lossless_and_validate_rejects`, `chat_moderation_policy_mirrors_the_pinned_moderation_param`.

## D0229 — Multipart sources redact metadata; lanes trace real operation identities

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ReplayableMultipartSource` Debug, the four multipart send lanes' `operation.id`/`http.route`, `from_path`/`Uploads::add_part` docs
- Sources: the one-shot source already redacted `file_name`/`media_type` while the replayable source printed them (a Path source's explicit file name is typically the basename of the redacted path); the download lane hardcoded `http.route = "/download"` and the lanes carried transport names instead of operation ids, unlike the JSON lanes. Round-6 items 6-06/6-16.
- Decision: replayable source Debug redacts `file_name`/`media_type` (existence preserved); every lane records the caller-supplied pinned operation id and the download route derives from the path segments; `from_path` documents the regular-files-only, symlink-rejecting stance and `add_part` mirrors the retry-identity documentation.
- Impact: `openai-rs-types` files/media Debug output; `openai-rs-client` multipart internals (private signatures).
- Overrides: none
- Tests: `multipart_source_debug_redacts_file_name_and_media_type`, `image_edit_multipart_debug_redacts_source_file_names_and_media_types`, `replayable_form_lane_records_real_operation_id`, `one_shot_form_lane_records_real_operation_id`, `download_lane_records_real_operation_id_and_route_template`.

## D0230 — Containers limits and translation secret lifetime join their families; `#/` joins the root-reference rejection

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ContainerListLimit`, `RealtimeTranslationClientSecretExpiration`, `structured.rs` root-reference classification
- Sources: both container `limit` parameters carry only the prose 1..=100 range (the third D0217-family miss, zero previously passing); the translation secret lifetime shares the GA client-secret pin schema yet hard-failed on decode with a u16 cap; `#/` is the second spelling of the document-root reference and passed through the bare sole-key path. Round-6 items 6-08/6-09/6-12.
- Decision: container limits enforce ≥1 at both serde boundaries (newtype, D0204 two-phase reporting); translation seconds become `Omittable<i64>` with the 10..=7200 range moved into the request `validate()` (D0036/D0169 pattern); `"#"` and `"#/"` both report `RecursiveReference`.
- Impact: `openai-rs-types` Containers/Realtime/Structured surfaces (breaking field-type changes noted in the round record).
- Overrides: extends D0217, D0036/D0169, D0143
- Tests: `container_list_limits_enforce_the_prose_backed_minimum_of_one`, `translation_secret_lifetime_range_is_opt_in_validate`, `empty_pointer_root_reference_reports_recursion_in_both_forms`.

## D0231 — The tracing facade is local-only, six-field spans, one retry field name

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `trace.rs` helpers; span declarations across transport/admin/x509/multipart/codex-direct; client/rmcp/codex crate docs; README; `reqwest` re-export
- Sources: round-6 items 6-17/6-18/6-21 (the engineering gaps after remote commit b6b01a6 introduced tracing); openai-python logs retries at INFO and openai-node emits no SDK logging; OTel HTTP semconv informed field naming only.
- Decision: tracing is a local facade — unconditional dependency, no feature gate, no public hooks or subscriber, no network telemetry (README states this). Each outbound HTTP lane emits exactly one debug span `openai.http_request` with a fixed six-field whitelist (`operation.id`, `http.request.method`, `http.route` templates, `http.response.status_code`, `openai.request_id`, `retry.count`); events are the retry WARN (with `retry.count`/`retry.delay_ms`/`retry.reason`) and the deadline WARN — WARN rather than python's INFO because both change observable latency, and node has no counterpart; the 401 invalidation pair keeps "and retrying" only on lanes that actually replay. The retry counter is named `retry.count` everywhere. Route templates evaluate lazily (closure form), so disabled debug spans allocate nothing. Never recorded: credentials, full URLs/query strings, path values, bodies, stream deltas/events — SSE and WS consumption run outside the span scope. rmcp keeps its flat snake_case fields (`rmcp.tool_dispatch` four fields) and codex records `codex.app_server.connection`/`codex.app_server.rpc`/`codex.direct.sse`. `reqwest` is re-exported at the client root and facade for nameability only; constructing a `Proxy` still requires a same-major direct dependency.
- Impact: documentation plus crate-private helpers; one event field rename (`retry.attempt`→`retry.count`); additive re-export.
- Overrides: none
- Tests: `http_retry_emits_warn_with_retry_count`, `auth_refresh_messages_match_their_lanes_retry_behavior`, `lazy_route_template_is_skipped_when_debug_spans_are_disabled`, `sse_stream_deltas_never_enter_tracing`, `admin_lane_span_records_six_fields_without_credentials`, `x509_lane_span_records_six_fields_without_bearer_tokens`, `direct_trace_tests::direct_lane_span_keeps_shape_without_leaking_credentials`, extended `dispatch_span_records_mcp_name`, `reqwest_proxy_is_nameable_through_the_facade`.

## D0232 — Round-6 recorded positions (docs/tests/decisions only)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: builder/limit/manifest rustdoc; JSONL and hyperparameter boundary tests; close-reason docs; batch budget docs; codex TOCTOU and Windows snapshot stances; D0154-family wording
- Sources: round-6 items 6-10/6-19/6-20/6-22/6-23 and the accompanying audit evidence.
- Decision: AdminClientBuilder knobs document defaults and the total-budget semantics; AdminListParams carries the limit-domain matrix with the shared-bag stance; the operations module lists the three once-only mint endpoints; the admin proxy comment no longer implies an unavailable escape hatch; JSONL line/byte/blank boundaries, FT reinforcement hyperparameter boundaries, and the webhook configuration fail-closed suite are test-locked; `submit_jsonl_path` documents its no-validation stance and the embeddings 50k cross-request cap; `close_reason()` documents that coded closes may carry empty reasons (unframed EOF stays `None`); the realtime single-shot lane emits the no-retry invalidation message; recorded positions — codex spawn hash→spawn TOCTOU accepted under the single-machine threat model, the Windows path-source snapshot is weaker (len+mtime), the batch 200 MB budget is decimal fail-closed and overridable, AdminInner holds the bearer header for the client lifetime (both baselines store plaintext keys), admin convenience facades are samples over the complete generic `request::<O>()` surface (D0151/D0164 extended), the realtime call-control Accept/no-body stance joins D0223, and D0154/D0174/D0217's "schema-backed lower bound" wording is corrected to "prose-backed".
- Impact: documentation and test coverage only.
- Overrides: corrects D0154/D0174/D0217 wording
- Tests: the round-6 test additions cited above.

## D0233 — Round-6 review addenda

- Status: accepted
- Reviewed: 2026-08-31
- Scope: D0225/D0227/D0229 cross-references; UsageQueryParams field docs
- Sources: the round-6 review's four observations.
- Decision: D0225's revision also supersedes D0187's thirty-two-valid-candidate branch description; D0227's impact is breaking (fifteen `pub object` fields changed type from `String` to open enums); core-domain multipart lane `operation.id` values are the codegen type-name spelling (PascalCase, matching the JSON lanes' `stringify!(TypeName)` convention), not the pinned operationId's original camelCase — admin-domain ids match the pin verbatim; `UsageQueryParams` fields now document the superset stance, the exclusive `end_time` bound, the bucket-width-dependent `limit` defaults, and the endpoint-specific filters.
- Impact: documentation and ledger hygiene only.
- Overrides: notes on D0187, D0225, D0227, D0229
- Tests: none (documentation).

## D0234 — Workload-identity token exchange is cancellation-safe; clippy gates cover default features

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `WorkloadIdentityAuth::token` (`spawn_refresh`), `trace::http_request_span` and `transport::execute_optional_json_with_static_header` cfg gates, docs/development.md gate list
- Sources: round-7 items 7-01/7-02 — the inline exchange leaked the single-flight slot when the caller's future was dropped (outer timeout/select), permanently blocking every later `token()` on the un-timed `notified().await`; x509's `TokenManager::lease` already used the detached-task pattern; the round-6 tracing/optional-lane helpers produced dead_code warnings under the default feature set that the all-features-only gate list could not see (development.md claimed default coverage it did not have).
- Decision: workload-identity adopts the x509 spawn pattern (detached task runs the exchange and unconditionally finishes the refresh, waking waiters; callers only register); the two helpers gain precise cfg gates matching their callers; the gate list adds default- and minimal-feature clippy commands and the coverage claim is corrected.
- Impact: `openai-rs-client` workload-identity internals; documentation.
- Overrides: none
- Tests: `cancelled_first_token_call_does_not_leak_the_refresh_slot`.

## D0235 — Webhook verification signs once and compares in constant time

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `WebhookVerifier::verify_at` candidate loop
- Sources: round-7 item 7-04 — node's bounded path (`selectMatchingSignature`) signs the payload once and compares candidate tags with a constant-time XOR, verifying only the selected candidate; the previous per-candidate HMAC loop allowed a public unauthenticated endpoint to force up to 182 × 16 MiB ≈ 2.9 GiB of hashing per request (candidates need only decode to 32 bytes, not be valid signatures).
- Decision: the expected tag is computed once; every candidate inside the 8 KiB header bound is compared with a full 32-byte XOR-and-fold (no short-circuit, no count rejection — D0225's acceptance semantics unchanged); the selected candidate is verified once more through the HMAC implementation as defense in depth. Total cryptographic work per delivery: one signature plus one verify.
- Impact: `openai-rs-client` webhook verification (performance/DoS posture only; verdicts unchanged).
- Overrides: refines D0225's work description
- Tests: `the_worst_usable_candidate_count_stays_at_bounded_hmac_work`, existing slot-33/1600 tests.

## D0236 — Codex direct lanes: 600s non-streaming budget knob, streaming unbudgeted; SSE decoder matches the platform

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `DirectCodexResponsesClient` (timeout knob, lane semantics), `direct/sse.rs`, `dispatch_sse_items`
- Sources: round-7 items 7-03/7-07 — reqwest 0.12's client-level timeout covers streaming bodies end-to-end and has no per-request "None" override, so the hardcoded 120s truncated every long turn (the platform's 600s knob and D0199 stance existed only in the client crate); the private SSE decoder mishandled lone CRs, stream-start BOMs, and continued dispatching after a decode failure, with a 4 MiB event cap contradicting D0144/D0226.
- Decision: the client carries no total timeout (10s connect only); non-streaming `create` applies `request_timeout` per request (default 600s, `with_request_timeout` knob, zero = immediate timeout per the platform stance); streaming runs unbudgeted (bounded by SSE terminators, EOF, decoder limits, or caller drop — no read-idle timeout, since long silent turns are legitimate); the private decoder is rewritten as a WHATWG byte state machine (lone CR terminates, split CRLF counts once, stream-start BOM stripped, exact `[DONE]` match, line/event limits split with `>` comparisons, default 32 MiB, `with_sse_limits` knob, explicit empty `data:` dispatches); decode failures stop dispatch after yielding the error once (fail-stop, D0194 parity).
- Impact: `openai-rs-codex` direct surface (additive knobs; default non-streaming budget 120s→600s; `[DONE]` with whitespace and silent empty-`data` frames now error instead of pass).
- Overrides: none
- Tests: the eleven direct tests cited in the round record.

## D0237 — ThreadItem is typed; apiKey login joins the typed login methods

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `ThreadItem` (18 branches) + ten supporting enums, `ItemLifecycleNotification.item`, `Turn.items`, `AppServerClient::account_login_api_key`
- Sources: pinned 0.144.5 `v2/ThreadItem` oneOf (the protocol's main content surface was raw `Value`); `v2/LoginAccountParams` apiKey branch `{type, apiKey}` was unreachable because `request`/`request_value` are private. Round-7 items 7-05/7-06.
- Decision: ThreadItem becomes an open tagged union — every branch models its pinned core fields with flatten extra and redacted Debug; unknown tags, missing/non-string tags, non-object payloads, and malformed known branches all degrade to `Unknown(Value)` losslessly (the outer notification never fails to decode); nested unions are closed per the pin with whole-item degradation for future nested tags. The apiKey login takes a `SecretString`, exposes it only inside the single request frame, retains nothing on the client, and never echoes it in errors or Debug.
- Impact: `openai-rs-codex` protocol surface (additive typing; `Turn.items`/lifecycle notifications gain typed access).
- Overrides: none
- Tests: `thread_item_decodes_and_round_trips_every_pinned_branch`, `thread_item_unknown_tags_and_malformed_branches_stay_lossless`, `thread_item_status_enums_decode_known_and_unknown_values`, `turn_and_item_lifecycle_carry_the_typed_thread_item`, `fake_child_api_key_login_sends_the_pinned_branch_and_never_echoes_the_key`.

## D0238 — Admin query arrays use bracketed keys (pin spelling + both SDK runtimes)

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `append_query_value` array branch, the five audit-log filters
- Sources: round-7 item 7-10 — the pinned spec spells the five audit filters `project_ids[]`/`event_types[]`/`actor_ids[]`/`actor_emails[]`/`resource_ids[]`, and both official SDKs override their serializers at the client level to brackets (python `_client.py` Querystring(brackets); node `qs.stringify(arrayFormat: 'brackets')`); D0059's repeat-style decision had cited only the `_qs.py` module default and the bare parameter names, missing both runtime overrides. The admin channel is corrected; the platform channel (python's client-level override applies there too) is left for a maintainer decision since no pinned parameter name carries brackets there.
- Decision: admin-channel arrays emit `name[]` (audit filters thereby match the pin's own spelling); the encoder comment cites the client-level overrides; tests assert the bracketed keys.
- Impact: `openai-rs-client` admin query encoding (wire-visible key change).
- Overrides: revises D0059's array-style finding (admin channel)
- Tests: updated `query_encoder_supports_arrays_null_and_deep_objects`, `audit_logs_loopback_encodes_effective_at_bounds_and_falls_back_to_last_item`.

## D0239 — Keepalive counts only polled time; WS send failures retire the socket; InitialConnect retries REST statuses

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `RealtimeKeepaliveState` (`last_poll` re-anchoring), the three WS clients' send paths, `derive_websocket_url`, `retryable_connect_error`
- Sources: round-7 items 7-08/7-09/7-22 — the silence window anchored on the last inbound frame, so a recv pause longer than the window killed a healthy connection on the first resumed tick with zero probes sent; send-path write failures left `is_closed()` false contrary to the D0198/D0212 posture; GA/beta URL derivation cleared base-query parameters while realtime preserved them (three copies of near-identical code); InitialConnect retried only Io/Tls, dropping the handshake statuses REST retries (408/429/5xx), with zero parameter documentation.
- Decision: the silence window counts only time spent awaiting recv — a poll gap of at least one ping interval re-anchors the window at resume; send transport failures retire the socket (local validation failures do not, as the connection was never touched); one `derive_websocket_url` helper serves all three clients, GA/beta now preserve base query like realtime (realtime additionally drops base fragments), and beta still never adds `beta=true`; InitialConnect additionally retries HTTP 408/429/5xx handshakes (409 and `x-should-retry` remain REST-only — handshakes replay no mutation), with max_retries/delay semantics documented.
- Impact: `openai-rs-client` WebSocket surfaces (keepalive no longer punishes paused pollers — the documented contract; gateway base URLs with query parameters now behave identically across the three clients, a behavior correction).
- Overrides: extends D0171/D0198/D0212
- Tests: `keepalive_reanchors_when_recv_polling_resumes_after_a_gap`, `send_write_failure_retires_{the_realtime_socket,the_responses_socket,the_beta_socket}`, `retryable_connect_error_covers_the_rest_retry_statuses`, `initial_connect_retries_a_503_handshake_rejection`, `initial_connect_retries_429_rejections_until_the_budget_is_spent`, `beta_initial_connect_retries_a_503_handshake_rejection`, updated URL derivation tests.

## D0240 — Resource replay carries resource-only fields through extra; legacy typestate decode guards mode-owned keys

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `Response::to_input_items` (ten branches), `ConversationMessage::to_response_input_item`, `StoredInputMessage::with_retained_extra`, legacy `CreateCompletionRequest`/`CreateStreamingCompletionRequest` decode
- Sources: round-7 items 7-11/7-12/7-14 — resource→input conversion took three different wire shapes for `created_by` (dropped, kept via shared struct, passed via conversation round-trip); the rebuilt user/system/developer branches discarded top-level extras while the assistant branch kept them; the legacy typestate silently dropped the other mode's keys on decode (chat's guard existed), breaking round-trips.
- Decision: D0030's "no sendable copy" is scoped to typed fields/constructors — replay paths are lossless: `created_by` merges into the input item's extra across all ten branches and the rebuilt message branches retain the source extras; the legacy wire structs capture the opposing mode's keys (null counts as present) and reject them with explicit errors mirroring chat.
- Impact: `openai-rs-types` Responses/Conversations replay semantics; legacy decode strictness (breaking: previously-accepted invalid combinations now error).
- Overrides: clarifies D0030-3's scope
- Tests: `to_input_items_replays_resource_only_created_by_into_extra`, `user_message_conversion_retains_top_level_extra_fields`, `accumulator_reduces_part_level_lifecycle_across_content_indexes`, `request_decode_rejects_mode_owned_field_combinations`.

## D0241 — Upload part ceiling, object-only batch bodies, search-empty guard, container file_id, speech voice bridge

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `MAX_UPLOAD_PART_BYTES`, `Uploads::add_part*` length checks, `BatchJsonlError::NonObjectBody`, `BatchSubmissionError::Io` mapping, `VectorStoreSearchRequest::validate`, `CreateContainerFileUploadRequest.file_id`, `SpeechVoice::custom`/`From<&VoiceId>`
- Sources: round-7 items 7-16/7-17/7-18 — the uploads documentation stated the 64 MB part ceiling but never enforced it locally although the replay lane freezes the length at prepare; non-object batch bodies passed every local check and only failed asynchronously on the server; `Texts(vec![])` was constructible past the minItems guard; the pinned multipart container-file schema (and both SDKs) carry an optional `file_id` form field; custom-voice ids had no bridge into `SpeechVoice::Custom`.
- Decision: a public 64 MiB constant and pre-transport `RequestPayloadTooLarge` rejection on the replay lane (path sources measured with the same freeze semantics as prepare; one-shot checked only when a length is declared); batch bodies must encode to JSON objects (`NonObjectBody` with the line number); JSONL writer IO errors map to the `Io` submission variant (other rule violations stay structured); `VectorStoreSearchRequest::validate()` closes the direct empty-array hole; the container upload gains an optional string `file_id` (distinct from the JSON lane's `FileId`) sent as a text part; `SpeechVoice::custom` plus a custom-voice-gated `From<&VoiceId>` bridge the voice APIs (one-way — a decoded Custom id need not be a Custom Voice resource).
- Impact: `openai-rs-types` files/batches/vector-stores/containers/media surfaces; `openai-rs-client` files/batches/containers (breaking: `add_part_one_shot` takes the source directly).
- Overrides: none
- Tests: the round-7 additions cited in the group reports.

## D0242 — Alpha grader DTOs are single-tracked; audit parity derives from the pin; codex extras conflict-check; structured rejects malformed keyword shapes; embeddings decode

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `fine_tuning::experimental_graders` (endpoint DTOs removed), the audit parity test (pinned derivation), codex `validate_extra`/`Error::ExtraFieldConflict`, structured malformed-keyword errors, `EncodedEmbedding::decode_f32_vec`
- Sources: round-7 items 7-19/7-20/7-21 — the unwired FT-side grader DTOs modeled `token_usage` as `Nullable<u64>` and could not decode the pinned official example (object form) while `evals::experimental` was correct; the 147-event/55-payload parity guard was self-referential; codex's public extra maps could shadow typed keys in serialized output (serde flatten does not deduplicate); structured silently skipped `items` arrays, non-array `anyOf`, and non-object `$defs`; base64 embeddings had no decode helper.
- Decision: the alpha run/validate endpoint DTOs live only in `evals::experimental` (the FT module keeps the grader types reinforcement uses); the audit test derives the event enum and payload keys from the pinned OpenAPI via include_str and asserts both directions (the binary-size cost is test-only, matching the realtime precedent); codex send paths validate that extras do not collide with typed keys (activating the kernel conflict check, which was previously dead) and report `ExtraFieldConflict`; the malformed keyword shapes return `UnsupportedKeyword` with paths; `decode_f32_vec` validates little-endian f32 length.
- Impact: `openai-rs-types` fine-tuning (breaking: endpoint DTOs removed, no callers), admin tests, structured/core surfaces; `openai-rs-codex` send validation (new error variant).
- Overrides: none
- Tests: the round-7 additions cited in the group reports.

## D0243 — Codex facade alias covers both features; rmcp cancel delivery is bounded

- Status: accepted
- Reviewed: 2026-08-31
- Scope: facade `codex` alias cfg, feature-status documentation, rmcp `CANCEL_DELIVERY_TIMEOUT` + trait/catalog docs + paginated ProbeServer
- Sources: round-7 items 7-24/7-25 — enabling `experimental-codex-direct` compiled the codex crate without exposing anything through the facade (the alias was gated on `codex-app-server` only); rmcp's cancel-delivery await was unbounded, so a wedged write path held dispatch past its deadline forever (rmcp's own internal timeout path is equally unbounded); the tool adaptation dropped fields silently and the executor contract was undocumented.
- Decision: the codex alias is visible under either feature (documented, tested); the direct feature's users are still pointed at a direct dependency for the full surface; rmcp's cancel-notification delivery is bounded at one second (timeout keeps the `let _ =` semantics and returns `Cancelled`/`Timeout` as decided); the trait documents control obligations, the "decoded to a JSON object" wording, and the error-variant guidance; the catalog documents its dropped-fields list with the `mcp_tool()` escape; the ProbeServer paginates `tools/list` and a stalled discovery e2e is covered.
- Impact: facade gating; rmcp documentation and bounded cancellation; test fixtures.
- Overrides: none
- Tests: `codex_alias_is_nameable_under_either_codex_feature`, `cancellation_returns_promptly_when_the_cancel_write_is_wedged`, `discovery_deadline_ends_a_stalled_tools_list_traversal`, `discovery_merges_every_paginated_tools_list_page`.

## D0244 — Round-7 recorded positions

- Status: accepted
- Reviewed: 2026-08-31
- Scope: webhook base64 strictness; secret construction buffers; beta agent extras; validate_stream_id dedup; streaming retry documentation; with_request_timeout scope; platform array-style finding
- Sources: round-7 audit items 7-15/7-23/7-26 and the residual observations.
- Decision: webhook candidate and secret decoding stays strict-canonical (real deliveries are canonical; node's tolerance is a `Buffer.from` side effect, not a contract); secret construction's intermediate String buffers are an accepted window (secrecy's `From<String>` semantics; the holder itself zeroizes on drop); BetaAgent stays a strict single-field leaf pending a need for agent metadata forward-compat (noted, not changed); the duplicated validate_stream_id was not deduplicated this round (the shared helper landed for URL derivation; the validator remains two copies pending a maintainer preference); streaming methods document that no retry happens after the handshake (transport errors are terminal) and the request budget excludes credential acquisition (token exchanges carry their own budgets — documented); the platform-channel array style repeats (python's client-level brackets override applies there too, but no pinned platform parameter name carries brackets, so a change needs a maintainer decision and a live-traffic confirmation).
- Impact: documentation and ledger only.
- Overrides: none
- Tests: existing suites.

## D0245 — Test-gap round: fixtures wired, pin-derived parities extended, retry/WS/codex error lanes covered

- Status: accepted
- Reviewed: 2026-08-31
- Scope: testdata fixtures; UsageResult/audit/ThreadItem-adjacent parities; chat/media stream-event positive coverage; retry truth tables and error lanes; WebSocket handshake-timeout/401/oversize lanes; codex error paths; rmcp schema policies; facades for spend alerts/limits; ledger name sync; fuzz gate
- Sources: round-8 audit (问题8.md) — five of six testdata fixtures were dead files while D0007/OVR-0006's claimed empty-list guard did not exist in any form; seventeen chat request fields and the final usage chunk (both lanes) had zero wire coverage; ~32 GA stream-event tags, the beta wrapper's item branch, seven UsageResult branches, and five codex notification branches had no positive tests; the retry matrix's x-should-retry/408/409/transport-error/DeadlineExceeded quadrants, three WebSocket handshake-timeout/401-refresh/oversize lanes, and four codex public error paths were untested; fuzz had no execution path and trybuild pinned only two of twelve feature boundaries.
- Decision: the four dead JSON fixtures are include_str!-wired (empty listFiles pins the required cursor ids — the OVR-0006 guard now exists — plus retrieveFile status and both mcp-approval shapes; the downloadFile fixture.toml stays as provenance metadata since its binary is already wired); pin-derived parities extend to the eleven UsageResult branches (from discriminators.json + the pinned OpenAPI) and to field-level audit payload probes (positive pin-derived fixtures plus 120 mismatched-value reverse probes; the unreachable `{}`-probe error branch is gone); chat gains a kitchen-sink round-trip with a key-set assertion, the nullable builder family, populated logprobs, and the empty-choices usage chunk on both lanes; the GA stream-event suite gains positive fixtures for the previously untested tags (hosted-tool lifecycles, delta families, queued/in_progress) plus the four annotation branches; kernel's three adversarial discriminator shapes are pinned; beta gains the moderation resource fixture, background-poll loopback, non-SSE content-type gates on both lanes, and a wire-level lane send; the retry lanes gain x-should-retry/408/connect-failure/deadline e2e's; the WebSocket clients gain hanging-handshake timeouts (with InitialConnect retry counting), a 401-refresh-retry lane, and oversize-send/keep-alive-local-validation halves; codex gains the queue-full, pending-capacity, orphan-response, and invalid-message paths plus the five notification branches, the OAuth success chain with a local JWKS, jwt multi-audience/namespace/nbf variants, and keyring/validate-body rejections; rmcp pins the Preserve policy and the invalid-name fallback; spend alerts/limits gain full facades (sixteen operations) with loopbacks; four pagination glues, chatkit item pages, and the SessionUpdate nested validate gain tests; the ledger's stale test names are synced; the fuzz crate joins the stable gate list via a manifest-path check and the trybuild suite grows four feature-boundary negatives.
- Impact: tests, fixtures, and documentation only, plus two additive admin facades; no wire behavior change.
- Overrides: none
- Tests: the round-8 additions cited in the group reports (workspace total 1006 → 1101).

## D0246 — Round-10 closeout: direct-lane error hygiene, facade completion, family parities, recorded positions

- Status: accepted
- Reviewed: 2026-08-31
- Scope: `DirectError::Json`; the shared `validate_stream_id`; `BetaResponse::output_text`; evals `reasoning_effort` nullability; chat `prediction` coverage; multipart empty-string guard; structured malformed-root guards; four spend facades; the rmcp proxy disclosure; and the round-10 recorded positions
- Sources: round-10 audit (问题10.md) — the direct lane's Json variant still quoted serde payload fragments (D0206 had neutralized the app-server side only); the last byte-duplicated validator lacked both a dedup and rejection tests; BetaResponse lacked the GA-parity text accessor; the pinned `ReasoningEffort` is anyOf[enum, null] yet the three evals sampling params modeled it non-nullable (FT was correct — the run resource could fail to decode a null echo); chat's nullable-fields test list omitted `prediction` against its own "every" claim; the D0163 empty-string decision had no regression pin; structured's non-object root and non-object ref-target branches were untested; the four round-8 spend facades were unreachable through the facade (the 2-34 family recurring); rmcp's streamable-HTTP transport builds its own reqwest 0.13 client that honors environment proxies, contradicting the crate-wide no-env-proxy posture without disclosure.
- Decision: the direct Json Display is neutralized (serde errors remain as `source` for diagnostics); one shared pub(crate) `validate_stream_id` serves both WebSocket faces with rejection tests through the real send path; `BetaResponse::output_text()` aggregates Stable and agent-message text exactly like GA; the three evals sampling params take `Omittable<Nullable<ReasoningEffort>>` with null builders and a full-chain null echo test; chat's nullable list and kitchen-sink cover `prediction`; the multipart empty-string decision is test-pinned; structured's two malformed-root branches are test-pinned; the four spend resource types are facade re-exported with a nameability test; the rmcp crate documents its env-proxy posture, the dual reqwest stacks, the auth-feature token-hop implication, and the stdio/with-client mitigations. Recorded positions (no code change): D0236's eleven direct tests are now named (six sse, five transport); assert_wire's 21-type list is a deliberate representative sample, not an unfinished target; the prompt-cache param triplicate's extra-flatten asymmetry mirrors the chat-domain convention; the default multipart filename difference ("file" vs python's "upload") is arbitrary; the direct SSE decoder omits a data-line cap (byte limits bound it) and the direct stream lane has no header-phase budget (callers wrap with their own timeout); AdminClient has no with_request_timeout derivation surface (documented as asymmetry); the fuzz crate cannot inherit unsafe_code=forbid (libfuzzer-sys); the beta item_reference agent metadata stays unconstructible (UnknownTaggedObject is the escape hatch); transcription_session.updated stays untyped (the pin's anyOf excludes it — it always decodes as Unknown); the spend loopbacks cover 14 of 16 wire paths; codex notification typing stays at ten of sixty-eight methods (mirroring the request face; a pin-derived parity test is the only worthwhile hardening).
- Impact: behavior fixes on the evals nullability (decode-hardening) and direct error display; additive accessors, re-exports, and tests; documentation.
- Overrides: none
- Tests: the round-10 additions cited in the group reports (workspace total 1101 → 1119).

## D0247 — Strict schemas drop the root $schema declaration; Prompt.variables is three-state; tool names pin at 128

- Status: accepted
- Reviewed: 2026-09-01
- Scope: `normalize_strict_schema` root handling, `PromptReference.variables`, `validate_name` length split, `FunctionTool::for_type` name validation
- Sources: round-11 audit (问题11.md items 11-01/11-02/11-05) — schemars 1.2.2's `schema_for!` (draft 2020-12 generator) unconditionally inserts a root `"$schema"` key with no removal transform, and the normalizer's keyword list let it reach the wire on all three public entry points (`StructuredOutput::new`, `TypedFunction::new`, `FunctionTool::for_type`); the ecosystem strips it (node's zod openai target) or never produces it (pydantic), and strict endpoints reject unknown root keys; the pinned `Prompt.variables` is `anyOf [map, null]` (python `Optional[Dict]`, node nullable) yet the field was a bare map so a `variables: null` echo failed the whole Response decode (D0034 fixed the sibling `version` only); the pinned function-tool name bound is 1..=128 while `validate_name` capped every path at 64 (the text-format bound) and `FunctionTool::for_type` validated nothing.
- Decision: the root `$schema` declaration key is stripped at the `normalize_strict_schema` entrance (root only — schemars emits it once at the document root), recorded as the single documented exception to D0129's never-silently-drops contract; non-root occurrences keep ordinary keyword handling. `PromptReference.variables` becomes `Omittable<Nullable<BTreeMap<..>>>` with a `variables_null()` builder. Name validation splits by surface: `MAX_RESPONSE_FORMAT_NAME_CHARS = 64` for text-format names, `MAX_FUNCTION_TOOL_NAME_CHARS = 128` for `TypedFunction`/`ToolRegistry`/`FunctionTool::for_type` (the `for_type` path gains validation it previously lacked).
- Impact: `openai-rs-types` structured output and Responses surfaces (behavior fix on the wire — generated strict schemas no longer carry `$schema`; the variables field type is breaking).
- Overrides: documents the sole D0129 exception
- Tests: `schemars_root_dollar_schema_never_reaches_the_wire`, `hand_written_root_dollar_schema_is_stripped`, `prompt_reference_variables_send_and_decode_official_null`, `function_tool_names_accept_65_to_128_characters`, `response_format_names_stay_capped_at_64_characters`, `tool_registry_follows_the_128_char_tool_name_pin`, `function_tool_for_type_enforces_the_128_char_name_pin`.

## D0248 — Beta create validates its own context_management; streaming gains a validate entry

- Status: accepted
- Reviewed: 2026-09-01
- Scope: `BetaCreateResponseRequest::validate`, `BetaCreateStreamingResponseRequest::validate`, `validate_beta_context_management`
- Sources: round-11 item 11-03 — the beta struct stores `context_management` outside the embedded GA base, so `base.validate()` never saw it and the two GA checks (non-empty array, `compact_threshold >= 1000`) were unreachable on the beta channel despite its "alongside the GA constraints" doc; GA's builder macro exposes `validate()` on both typestates while the beta streaming type had none.
- Decision: the two GA constraints replay on the beta entrance wrapped through the existing `CreateResponseConstraintError` variants (the threshold is read from the shared wire encoding since the field is private to the GA module; null/omitted skip matches GA); the streaming typestate gains a `validate()` that delegates to the non-streaming one.
- Impact: `openai-rs-types` beta validation surface (additive).
- Overrides: none
- Tests: `create_and_streaming_validate_enforce_beta_context_management_bounds`.

## D0249 — Files gains wait_for_processing and the for_files preset; admin digest constants replaced by a row projection parity; catalog's ghost execution field removed; the rmcp alias covers every rmcp feature; the SSE no-leak test retries

- Status: accepted
- Reviewed: 2026-09-01
- Scope: `Files::wait_for_processing`, `PollOptions::for_files`, the deleted admin SHA-256 constants plus a row-projection parity test, the rmcp catalog dropped-fields list, the facade rmcp alias cfg, the SSE tracing no-leak test
- Sources: round-11 items 11-04/11-06/11-07/11-08/11-09 — both official SDKs ship `wait_for_processing` (terminal set processed/error/deleted with `deleted` expressible only through the open enum's Unknown, and `error` being a resource terminal that returns the object rather than an error; defaults 5s/30min); the two admin manifest digest constants froze at the pin-introduction hash with zero consumers while the runtime parity test already guards the live file; the catalog's dropped-fields list cited an `execution` field that exists in neither the locked rmcp 3.1.4 `Tool` nor the MCP 2026-07-28 schema (a 2025-11-25 revision leftover, breaking the `mcp_tool()` escape promise for that entry); the facade rmcp alias was gated on `rmcp` alone so the server/auth-only feature combinations compiled the crate without exposing anything (the 7-24 pattern); the SSE no-leak test raced the process-level callsite cache in parallel runs (the same root cause rmcp's dispatch-span tests hit in 6-18).
- Decision: `wait_for_processing` rides the shared poller with the terminal set Processed | Error | unknown "deleted", returning the file on error terminals and `DeadlineExceeded` on timeout, with a `for_files()` preset (5s/30min) added to the poll consumers; the stale digests are deleted and a stronger row-by-row projection parity test (id/method/path/modes/statuses/content-types/schema-refs, sorted and fully compared against the pinned spec) takes over provenance; the catalog list drops `execution` and now claims exhaustiveness against the locked rmcp shape; the facade alias cfg becomes `any(rmcp, rmcp-server, rmcp-server-stdio, rmcp-auth)`; the no-leak test adopts the sixteen-attempt re-arm pattern with a keep-alive queue server (assertion strength unchanged, five repeated runs green).
- Impact: `openai-rs-client` Files/poll (additive), `openai-rs-types` admin (breaking only for the two zero-consumer constants), rmcp documentation, facade gating (a pure widening), test stability.
- Overrides: none
- Tests: `wait_for_processing_resolves_every_terminal_status`, `wait_for_processing_times_out_with_the_last_observed_status`, `wait_for_processing_accepts_for_files_preset_options`, `for_files_preset_matches_the_official_wait_for_processing_defaults`, `operation_manifest_rows_match_the_pinned_spec_projection`, `rmcp_alias_is_nameable_under_any_rmcp_feature`, `rmcp_alias_is_nameable_through_the_facade_under_any_rmcp_feature`, the hardened `sse_stream_deltas_never_enter_tracing`.

## D0250 — Round-11 recorded positions

- Status: accepted
- Reviewed: 2026-09-01
- Scope: bare root refs; Certificate.active; beta retrieve's non-streaming params; speech stream_format; skill reference validation; webhook docs provenance; the duplicate realtime.call.incoming; RpcId; the executor's auth-only import
- Sources: round-11 audit observations (问题11.md items 11-10 and the per-domain notes).
- Decision: the bare `"$ref": "#"` rejection stands (D0143/D0230) despite the official docs using it in recursion examples — the tension is recorded for reopening when real recursion demand appears; `Certificate.active` stays a permissive union field (the shared DTO spans pin-required and optional shapes); beta non-streaming retrieve deliberately omits the stream-only parameters; `CreateSpeechRequest` keeps `stream_format` internal (the default equals omission — a builder may be added on demand); container skill references keep serde-lossless without opt-in validation (python validates nothing either); the webhooks module doc's delivery-semantics provenance now cites the official platform guide rather than node's docs; the second `realtime.call.incoming` model stays (the realtime feature cannot depend on the webhooks feature — the divergence is documented); `RpcId`'s u64 domain stays (1-R3, fail-closed on negatives); the executor's `std::time::Duration` import warning under auth-only feature combinations is noted for a follow-up touch.
- Impact: documentation and ledger only.
- Overrides: none
- Tests: existing suites.
