# Development guide

## Toolchain

The workspace uses Rust edition 2024 and declares Rust 1.88.0 as its minimum
supported Rust version (MSRV). `rust-toolchain.toml` pins the local toolchain and
includes `rustfmt` and Clippy.

```console
rustup show active-toolchain
rustc --version
cargo --version
```

## Required checks

Run these from the workspace root before opening a pull request:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo check -p openai-rs-sdk --all-targets --no-default-features --locked
cargo check --workspace --all-targets --all-features --locked
cargo +1.88.0 check --workspace --all-targets --locked
cargo run --locked -p xtask -- check
cargo deny --all-features check
```

The checks intentionally cover default features, no-default-features, and all
features. `all-features` is a compile/test pressure test; it does not change the
stability level of experimental features.

## `xtask check`

`cargo run -p xtask -- check` is the repository consistency entry point. It
validates checked-in provenance, hashes, operation/schema inventories, and any
registered generated artifacts without updating them. The command must return
a failure if it would change a tracked artifact or if a pinned input no longer
matches its recorded hash. M0 currently checks four registered contract
projections for zero diff, in addition to the vendored OpenAPI and Codex schema
provenance.

Refresh commands, when added, must be separate and explicit. Normal builds,
tests, and `xtask check` must not fetch a moving specification.

## Generated files

- Do not hand-edit generated wire types or generated schema bundles.
- Change the pinned input, lowering rules, or an explicit override instead.
- Include the source revision, hash, generator version, and generated diff in
  the review.
- Keep fixtures small enough to review and never include real credentials,
  prompts, account IDs, or customer data.

## Tests

New wire types should have positive and negative fixtures. Tagged unions need a
fixture for every known discriminator plus unknown-tag behavior where the
contract is forward-compatible. Known tags with malformed payloads must fail;
they must not silently fall back to an unknown variant.

Network tests use local scripted servers. The default test suite must not
require a live OpenAI or ChatGPT credential. Real subscription or Platform
smoke tests are manual and explicitly opted in.

## Dependency policy

Prefer crates.io releases pinned through `Cargo.lock`. New git dependencies need
an explicit rationale and a source-policy update. Run `cargo deny` after any
dependency change and document unavoidable duplicate major versions.

The project does not accept dependencies on `sub2api`, nor code derived from its
gateway, account-pool, billing, scheduling, or proxy implementation.

## Pull requests

Keep changes scoped and include:

- the behavior or contract being changed;
- upstream provenance when wire behavior changes;
- tests for success, failure, unknown-value, and secret-redaction paths as
  applicable; and
- any feature, MSRV, or public API impact.
