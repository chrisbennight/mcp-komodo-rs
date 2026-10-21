# Compatibility

The wire contract targets Komodo Core 2.1.2 at upstream commit
[`20b9d16d4bbab934c6274477c68b032bae4b61b9`](https://github.com/moghtech/komodo/commit/20b9d16d4bbab934c6274477c68b032bae4b61b9).
Resource and operation metadata models also cover the fields defined by
[Core 2.3.3](https://github.com/moghtech/komodo/tree/v2.3.3/client/core/rs/src/entities),
including server statistics and resource placement names. This source comparison
and loopback evidence do not certify a live deployment.

Stack configuration and Compose provenance cover the ordinary fields in the
[Core 2.3.3 stack model](https://github.com/moghtech/komodo/blob/780ac68b992094a9fccd5fffb760e0c84fd3c3d1/client/core/rs/src/entities/stack.rs).
Missing newly supported fields remain null. Command reads and writes preserve
the upstream `SystemCommand.shell_mode` setting. The dedicated webhook-token
operation continues to use native file transfer.

Network reads follow the documented Core 2.3 request rename:
[`ListNetworks` and `InspectNetwork`](https://github.com/moghtech/komodo/blob/780ac68b992094a9fccd5fffb760e0c84fd3c3d1/client/core/rs/src/api/read/docker.rs).
Core releases before 2.3 use `ListDockerNetworks` and `InspectDockerNetwork`.
The adapter selects the named request after reading Core's version; an
unparseable version fails explicitly. Network request selection shares version
metadata across client clones for one minute, coordinating concurrent refreshes;
an upgrade is observed after expiration. `system.status` still makes a fresh
version request to establish reachability. Loopback tests verify both release forms
and the fields in the
[Core 2.3.3 network model](https://github.com/moghtech/komodo/blob/780ac68b992094a9fccd5fffb760e0c84fd3c3d1/client/core/rs/src/entities/docker/network.rs).

Core uses `mogh_error` 1.0.4 for HTTP failures. Its
[`Serror` source](https://docs.rs/mogh_error/1.0.4/src/mogh_error/serror.rs.html)
defines the `error` and `trace` diagnostic fields retained by this adapter. HTTP
status and the named request accompany those fields. UTF-8 non-JSON failures are
represented as diagnostic text under the same upstream response bound.

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
History documents remain available through operation search and status. Their
typed credential fields follow the Core 2.3.3
[resource models](https://github.com/moghtech/komodo/tree/780ac68b992094a9fccd5fffb760e0c84fd3c3d1/client/core/rs/src/entities).
Core's [TOML export](https://github.com/moghtech/komodo/blob/780ac68b992094a9fccd5fffb760e0c84fd3c3d1/bin/core/src/api/read/toml.rs)
retains these literal values, so their exact originals use native file transfer.
Ordinary documents remain inline without a speculative content scan. Core 2.1.2
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
| Network reads | Versioned 2.1.2 and 2.3.3 named requests and typed addressing facts | Loopback wire tests and MCP discovery, detail, and pagination regressions |
| MCP | 2025-11-25 | Pinned rmcp SDK and binary stdio integration test |
| HTTP access | Gateway bearer plus verified identity JWT | Production-router status call with loopback JWKS/Core and rejected bearer/JWT tests |
| Local access | Selected stdio capability profile, process-owner trust | Binary initialization, discovery, content read and mutation denial tests |
| Container build target | Linux amd64 | Source-image liveness smoke test |
| Source builds | Pinned Rust toolchain on Linux | Hosted Ubuntu checks; other operating systems unverified |
