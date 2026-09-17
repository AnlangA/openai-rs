# Upstream merge and reliability improvements — 2026-09-17

## Merge baseline

The local `main` branch was fast-forwarded from `7516d98` to GitHub's
`9703a80`, incorporating 12 commits and the `0.2.0` release. All 38 paths
with existing local changes were backed up before merging. The backup and
the named Git stash are retained. The initial merge phase did not create a
commit or push to a remote.

The merge preserves upstream diagnostics, Rustdoc requirements, protocol
fixes, and the existing local API additions. Duplicate Safety definitions,
function-tool `async` fields, and workload refresh fields were reconciled.
Function tools support both `asynchronous()` and `with_async()` through
one serialized field. Safety timestamps continue to accept fractional seconds.

## Reliability and consistency

- Platform SSE clients now deliver complete events before reporting malformed
  input later in the same HTTP chunk. Responses, Chat Completions, legacy
  Completions, and media streams use the new `push_with_flushed` entry point.
  The existing public `push` signature remains compatible. Regression tests
  cover every two-chunk split, invalid UTF-8, and configured resource limits.
- Direct Codex SSE stops parsing after `[DONE]`, including malformed bytes
  already present in the same chunk. Tests cover LF, CRLF, and CR framing.
- Workload identity refreshes release their shared refresh slot after provider
  panics, including cancellation-time cleanup. Other requests can obtain a
  fresh token afterward. Cache hits also avoid copying the cached secret string
  before constructing the authorization header.
- Contract generation computes supplement fingerprints from the bytes actually
  read at runtime. Added routes are checked against the documented-operation
  registry, including duplicates and aliases of pinned parameterized paths.
- The frozen inventory remains 288 client operations and 18 webhook operations.
  Its 264 verified client operations and the separately documented Safety route
  represent 265 implemented operations. Safety is counted once. All five
  generated artifacts were rebuilt from their inputs, and outdated test
  references in the implementation registry were corrected.

## Validation

Validation used Rust 1.88.0, locked dependencies, and the local dependency cache.

| Check | Result |
| --- | --- |
| Final workspace tests, all features, including documentation examples | 1,314 passed, zero failed, one existing ignored smoke test |
| Workspace Clippy, all targets and features, warnings denied | Passed on the final source |
| Default facade and minimal rustls client Clippy | Passed |
| Facade without default features; all workspace targets/features on MSRV | Passed |
| Fuzz target compilation | Passed |
| Strict Rustdoc, all features and no default features | Passed |
| No-default-feature documentation examples | 8 passed |
| Contract generator and source consistency | 29 generator tests passed; five artifacts reproduce without changes |
| Dependency advisories, bans, licenses, and sources | Passed using the cached advisory database |
| Formatting, whitespace, and Git conflict state | Passed; no unresolved paths |

The one existing ignored smoke test requires an explicitly selected, audited
local Codex binary.

Tests use local fixture servers and synthetic credentials. They do not establish
live API availability or account permissions.
