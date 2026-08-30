# Contributing to openai-rs

Thank you for helping build `openai-rs`. The project is pre-release, so small,
well-tested contract changes are more useful than broad surface-area additions.

## Before you start

- Read [README.md](README.md) for the current capability boundary.
- Read [docs/architecture.md](docs/architecture.md) before changing
  authentication, transport, redirect, or Codex behavior.
- Check [docs/feature-status.md](docs/feature-status.md); a Cargo feature may be
  a scaffold rather than an implemented API.
- For large public API or generation changes, open an issue first so the wire
  contract and compatibility approach can be agreed on.

## Development setup

Install Rust through `rustup`. The workspace pins Rust 1.88.0, edition 2024,
Rustfmt, and Clippy in `rust-toolchain.toml`.

```console
rustup show active-toolchain
cargo build --workspace
```

Do not place OpenAI API keys, ChatGPT credentials, Codex access tokens, browser
cookies, or app-server transport tokens in the repository. Tests must use local
scripted servers and synthetic secret markers.

## Required checks

Run the local quality checks:

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

See [docs/development.md](docs/development.md) for the purpose of each command.

## Wire-contract changes

When a change affects serialized requests, responses, events, headers, status
handling, or authentication:

1. Cite the pinned upstream specification, official documentation, or audited
   source revision that establishes the behavior.
2. Update the generator or explicit override rather than hand-editing generated
   output.
3. Add positive and negative fixtures, including required/null/omitted behavior
   and unknown-value handling where applicable.
4. Verify that known tagged-union variants with malformed payloads fail instead
   of falling back to an unknown variant.
5. Test secret redaction in errors and `Debug` output whenever credentials or
   user content cross the changed code path.

Normal builds and `xtask check` must not download moving specifications. Refresh
operations are explicit maintainer actions and must produce a reviewable source,
hash, and generated diff.

## Feature and trust boundaries

- Standard Platform clients must not accept Codex or ChatGPT subscription
  credentials.
- Administration credentials and operations remain separate from ordinary
  Platform operations.
- Experimental direct Codex support must stay host-locked, operation-locked,
  disabled by default, and outside the `full` feature.
- RMCP tools are explicitly registered or allowlisted; paid OpenAI operations
  are never exposed automatically.
- Dependencies on `sub2api`, or code derived from its gateway, account-pool,
  billing, scheduling, or proxy implementation, are out of scope.

## Pull requests

Keep commits focused. A pull request should explain:

- the user-visible or wire-level behavior;
- the authoritative source and pinned revision when relevant;
- tests added for success, failure, compatibility, and redaction;
- feature, MSRV, dependency, and public API impact; and
- whether generated files changed and how they were reproduced.

Do not mix unrelated formatting or generated churn into a contract change.

## Licensing

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project is licensed under the project's dual
MIT/Apache-2.0 terms, without additional restrictions.
