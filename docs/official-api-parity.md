# Official API parity additions — 2026-09-05

The implementation now preserves service-account key expiry, bounds WIF credential
waits by request deadlines, and exposes current Responses diagnostics and async flags.
It adds Safety Alerts retrieval and the ten deprecated Videos operations.

## Supported additions

| Surface | Behavior and evidence |
|---|---|
| Service-account API keys | `expires_in_seconds` distinguishes omission, null and a lifetime; `expires_at` is typed on creation responses. Limits are available through opt-in `validate()`. [Official definition](https://developers.openai.com/api/reference/resources/admin/subresources/organization/subresources/projects/subresources/service_accounts/subresources/api_keys/methods/create). |
| Responses GA and beta | Function/custom tools and their calls expose `with_async`/`is_async`; function tools also retain the `asynchronous` builder and preserve future fields. Cache options expose `comparison_response_id`, including null. Responses expose all four cache-diagnostic variants and typed safety error details. Typed Structured Outputs helpers preserve supported recursive schema references. [Official definition](https://developers.openai.com/api/reference/resources/responses/methods/create). |
| Safety Alerts | `client.safety().alerts().retrieve(id)` and `client.safety_alerts().retrieve(id)` send a project-authenticated GET, percent-encode the opaque id, retain required-nullable `reason`, and expose metadata and API errors. Existing webhook `SafetyAlertId` values work as arguments. [Official definition](https://developers.openai.com/api/reference/resources/safety/subresources/alerts/methods/retrieve). |
| Videos | Enable `legacy-videos`. `client.videos()` supports create, list, retrieve, delete, download_content, remix, edit, extend, create_character and get_character. Uploads use multipart; existing asset references use the documented JSON encoding. Binary downloads preserve `variant` and response metadata. [Official reference](https://developers.openai.com/api/reference/resources/videos/methods/create). |
| WIF deadlines | Credential acquisition shares the logical HTTP budget or each WebSocket attempt's connect budget. A detached refresh also has a finite lifetime and releases the singleflight slot on expiry. |

The frozen OpenAPI remains byte-for-byte intact. Reviewed source additions live in
`spec/contracts/documentation-additions.json`; regenerated inventory, schema IR,
nullability and discriminator artifacts identify both sources. The supplement is a
reviewed transcription of current documentation, not a purported new upstream release.
Its application fails if a member already exists, requiring deliberate reconciliation
when the upstream pin is refreshed.

## Lifecycle and verification boundaries

The frozen OpenAPI inventory contains 288 client operations: 264 verified, 23
omitted, and one quarantined. Safety Alerts retrieval is recorded once in the
separate `documented_operations` manifest section. The two sources therefore
describe 289 client operations, with 265 implemented in total. The 18 verified
webhook contracts are counted separately. Sora is scheduled to shut down on
September 24, 2026; `legacy-videos` does not override that shutdown.
Assistants reached its August 26, 2026 shutdown. Image variation depends on the
DALL-E 2 model listed as removed on May 12, 2026, so it remains quarantined despite
its reference page. [Official deprecations](https://developers.openai.com/api/docs/deprecations).

Regression checks exercise actual loopback HTTP requests and lossless Serde fixtures.
They do not create paid resources or assert real-account permissions, service availability,
or complete parity for every possible payload. Known malformed tags remain errors;
future tags and fields remain preserved. Older documentation samples that contradict
their own field definitions do not silently weaken the required-field contracts.

## Historical validation record — 2026-09-05, before the merge

The following results are retained from the local additions before their merge
with the upstream `0.2.0` changes. They ran on Rust 1.88.0, the workspace MSRV,
and do not establish validation of the merged working tree:

| Check | Result |
|---|---|
| `cargo test --workspace --all-features --locked` | 1,272 passed, zero failed, two existing ignored tests |
| `cargo clippy --workspace --all-features --all-targets --locked -- -D warnings` | Passed |
| `cargo run --locked -p xtask -- check` | Frozen-source integrity and all five generators passed |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| Default workspace; facade without default features; minimal Videos/WIF; beta Responses/WIF | All four feature configurations compiled |
| Original independent defect probes | Expiry and async fields retained; a 20 ms WIF request ended with `DeadlineExceeded` in approximately 23 ms, previously approximately 203 ms |

In that historical run, replaying the original 514 REST/schema examples and 322 event examples introduced
no new decoding failures. Twenty additional official Safety/Videos examples yielded
14 successful decodes and six schema conflicts: four older video examples omit
required `completed_at`, and two list examples contain jobs without required
`progress`. These illustrative examples are not evidence that the live API omits
those fields. The implementation retains the documented required/nullable
distinction; accepting every historical example is not the acceptance criterion.
