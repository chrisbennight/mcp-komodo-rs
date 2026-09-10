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
HTTP at `/mcp`.
