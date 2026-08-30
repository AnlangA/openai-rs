# Security policy

## Supported versions

`openai-rs` is pre-release and has not published a supported stable version.
Security fixes are made on the current development branch. No compatibility or
backport SLA is promised until the first supported release is documented here.

## Reporting a vulnerability

Please do not disclose a suspected vulnerability in a public issue, discussion,
pull request, test fixture, or log.

Use GitHub's private **Report a vulnerability** form for this repository:

<https://github.com/AnlangA/openai-rs/security/advisories/new>

If the private form is unavailable, open a public issue containing only a
request for a private maintainer contact channel. Do not include exploit steps,
credentials, account identifiers, prompts, model output, or other sensitive
details in that issue.

Include, when possible:

- the affected revision, crate, feature set, and platform;
- a concise impact statement;
- minimal reproduction steps using synthetic credentials and local endpoints;
- whether the issue crosses a credential, host, redirect, process, or protocol
  boundary; and
- any suggested mitigation.

Maintainers will acknowledge reports on a best-effort basis. Because the project
is pre-release, there is currently no guaranteed response or disclosure
timeline. Please coordinate public disclosure after a fix or mitigation is
available.

## High-priority security boundaries

Reports are especially useful when they involve:

- credentials being sent to an unexpected host or across a redirect;
- confusion between Platform, administration, ChatGPT/Codex, or app-server
  transport credentials;
- secret, prompt, tool-argument, model-output, signed-URL, or MCP-content leaks
  in logs, errors, serialization, or `Debug` output;
- unsafe callback binding, OAuth state/PKCE validation, token refresh, or token
  storage behavior;
- request validation bypass through malformed or unknown tagged-union variants;
- unbounded JSON, SSE, WebSocket, multipart, subprocess, or queue behavior; or
- an experimental Codex operation escaping its host/operation allowlist.

## Secrets in tests

Never submit a real OpenAI API key, ChatGPT token, Codex access token, browser
cookie, organization/account ID, private certificate, or app-server transport
secret. Use conspicuous synthetic markers so tests can assert that secret values
do not appear in logs or errors.

