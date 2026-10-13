# Compatibility

The wire contract targets Komodo Core 2.1.2 at upstream commit
[`20b9d16d4bbab934c6274477c68b032bae4b61b9`](https://github.com/moghtech/komodo/commit/20b9d16d4bbab934c6274477c68b032bae4b61b9).
Resource and operation metadata models also cover the fields defined by
[Core 2.3.3](https://github.com/moghtech/komodo/tree/v2.3.3/client/core/rs/src/entities),
including server statistics and resource placement names. This source comparison
and loopback evidence do not certify a live deployment.

The client sends the documented envelope form to `/read`, `/write`, and
`/execute`, using the request variant in `type` and typed arguments in `params`.
Reads and typed writes (`UpdateStack`, `WriteStackFileContents`) go to `/read`
and `/write`; action executions go to `/execute`. Response structures represent
named typed contracts. Resource search and status include the supported Core
list record alongside the normalized summary. Unknown new upstream fields need
an explicit compatibility update; presumed sensitivity does not justify excluding
supported facts. Write responses that echo submitted configuration remain
acknowledgments; callers use the corresponding typed read to observe state.

Operation search and status preserve the resource target's type and id from
Core's tagged `ResourceTarget`, including future resource kinds. They also retain
initiating identity, version, commit, and operation-specific data when reported.
The API adapter retains configuration-history documents for the credential
transfer work; MCP document delivery is still pending. Core 2.1.2
reports `Queued`, `InProgress`, and `Complete` update states. The MCP `outcome`
distinguishes pending from completed results, and `success` is now nullable until
completion. Consumers that assumed a boolean for pending operations must use
the explicit outcome. An unrecognized state does not establish success. A target
correlates a resource rather than a caller. See [write reconciliation](reconciliation.md).

Stack stop uses the pinned upstream
[`StopStack` request](https://github.com/moghtech/komodo/blob/20b9d16d4bbab934c6274477c68b032bae4b61b9/client/core/rs/src/api/execute/stack.rs)
with `stack` set to the resolved resource id, `services: []` to stop all
services, and `stop_time: null` to retain Komodo's default termination timeout.
This invokes `docker compose stop` and preserves the stack's containers.

Compatibility must be rechecked before changing the deployed Komodo minor
version. At minimum, verify the request variants, parameter names, list-item
state fields, update pagination, operation receipt id encoding, and API-key
headers against the exact target tag. Loopback wire tests should verify retained
facts and literal credential boundaries using synthetic values.

The MCP protocol version is `2025-11-25`, served through stateless Streamable
HTTP at `/mcp`, or local stdio with `--stdio`. The stdio integration
test launches the real executable, negotiates this protocol version, lists tools,
and checks status and content reads without admin credentials and excluded mutations against a loopback fake Core.

| Component | Supported contract | Evidence |
| --- | --- | --- |
| Komodo Core | 2.1.2 request/response shapes | Loopback wire tests; no live deployment certification |
| Resource and operation metadata | 2.1.2 and 2.3.3 list/detail fields | Version-pinned source comparison and loopback metadata regressions |
| MCP | 2025-11-25 | Pinned rmcp SDK and binary stdio integration test |
| HTTP access | Gateway bearer plus verified identity JWT | Production-router status call with loopback JWKS/Core and rejected bearer/JWT tests |
| Local access | Selected stdio capability profile, process-owner trust | Binary initialization, discovery, content read and mutation denial tests |
| Container build target | Linux amd64 | Source-image liveness smoke test |
| Source builds | Pinned Rust toolchain on Linux | Hosted Ubuntu checks; other operating systems unverified |
