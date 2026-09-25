# Compatibility

The wire contract targets Komodo Core 2.1.2 at upstream commit
[`20b9d16d4bbab934c6274477c68b032bae4b61b9`](https://github.com/moghtech/komodo/commit/20b9d16d4bbab934c6274477c68b032bae4b61b9).

The client sends the documented envelope form to `/read`, `/write`, and
`/execute`, using the request variant in `type` and typed arguments in `params`.
Reads and typed writes (`UpdateStack`, `WriteStackFileContents`) go to `/read`
and `/write`; action executions go to `/execute`. The allowlisted response
structures are intentionally smaller than Komodo's types; Serde drops unknown
fields, and write responses that echo submitted configuration are discarded
entirely. New upstream fields therefore do not automatically expand the MCP
information surface.

Operation search and status preserve only the resource target's type and id
from Core's tagged `ResourceTarget`; unknown fields remain excluded. Core 2.1.2
reports `Queued`, `InProgress`, and `Complete` update states. The MCP `outcome`
distinguishes pending from completed results, and `success` is now nullable until
completion. Consumers that assumed a boolean for pending operations must use
the explicit outcome. An unrecognized state or target does not establish success
or caller-level correlation. See [write reconciliation](reconciliation.md).

Stack stop uses the pinned upstream
[`StopStack` request](https://github.com/moghtech/komodo/blob/20b9d16d4bbab934c6274477c68b032bae4b61b9/client/core/rs/src/api/execute/stack.rs)
with `stack` set to the resolved resource id, `services: []` to stop all
services, and `stop_time: null` to retain Komodo's default termination timeout.
This invokes `docker compose stop` and preserves the stack's containers.

Compatibility must be rechecked before changing the deployed Komodo minor
version. At minimum, verify the request variants, parameter names, list-item
state fields, update pagination, operation receipt id encoding, and API-key
headers against the exact target tag. Run loopback wire tests with adversarial
sentinel values in every excluded field.

The MCP protocol version is `2025-11-25`, served through stateless Streamable
HTTP at `/mcp`, or local status-only stdio with `--stdio`. The stdio integration
test launches the real executable, negotiates this protocol version, lists tools,
and checks status calls and denied operations against a loopback fake Core.

| Component | Supported contract | Evidence |
| --- | --- | --- |
| Komodo Core | 2.1.2 request/response shapes | Loopback wire tests; no live deployment certification |
| MCP | 2025-11-25 | Pinned rmcp SDK and binary stdio integration test |
| HTTP access | Gateway bearer plus verified identity JWT | Production-router status call with loopback JWKS/Core and rejected bearer/JWT tests |
| Local access | Status-only stdio, process-owner trust | Binary initialization, discovery, call and denial tests |
| Container build target | Linux amd64 | Source-image liveness smoke test |
| Source builds | Pinned Rust toolchain on Linux | Hosted Ubuntu checks; other operating systems unverified |
