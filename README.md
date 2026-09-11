# mcp-komodo-rs

`mcp-komodo-rs` is a Rust Model Context Protocol server for bounded
Komodo operations. It is designed for the homelab MCP tool-search gateway and
is not a general Komodo API proxy.

The repository is named `mcp-komodo-rs`; the executable remains
`komodo-mcp-rs` for compatibility. Start with the
[local status-only quickstart](docs/quickstart.md) for a stdio MCP client without
a gateway or administrative credentials. The HTTP profile described below
provides the full governed tool surface through a gateway.

The current surface exposes operational status plus a small set of typed
deploy, restart, stop, build, cancel, and pull intents. Sensitive operations are not
categorically excluded: configuration, Compose, environment, logs, and
secret-handling tools may be added when they have bounded typed schemas,
accurate MCP annotations, gateway authorization, and audit-safe handling.

The server will not expose a raw Komodo proxy, arbitrary JSON or actions,
Docker inspection, Periphery configuration, terminal or shell access, or
unbounded upstream data. Those interfaces cannot be governed with the same
precision as named typed operations.

## Authorization model

Every `/mcp` request requires both the private rotating gateway bearer and a
gateway-signed `X-MCP-Identity` JWT. The JWT is verified against bounded JWKS
data with pinned issuer, `aud = komodo`, and
the configured gateway actor in `act.sub`.

The gateway is the policy authority. Standard MCP hints plus the experimental
action-metadata and result trust-annotation namespaces describe behavior and
result handling as claims; they do not grant access:

- authenticated users may invoke low-risk, read-only status tools;
- every side-effecting tool requires membership in `komodo-admin`; and
- a Cedar forbid rule prevents other groups, including broad MCP
  administrators, from bypassing that requirement.

The server also uses two distinct Komodo service-user credentials. Read tools
can only use the read identity; mutation tools can only use the narrowly
privileged administrative identity. Infisical injects credentials and gateway
bearers into the process environment at container creation.

## Tool surface

Read-only tools:

- `system.status`;
- `servers.search` and `servers.status`;
- `stacks.search`, `stacks.status`, and `stacks.diagnostics`;
- `deployments.search` and `deployments.status`;
- `builds.search` and `builds.status`;
- `repos.search` and `repos.status`; and
- `operations.search` and `operations.status`.

Governed reads over stack configuration and content (content-bearing results
are labeled sensitive and untrusted; `stacks.webhook.status` returns only
metadata and is not sensitive):

- `stacks.config.read` — configuration shape and presence flags, no raw payloads;
- `stacks.compose.read` — Compose contents for a configured, latest, or deployed source;
- `stacks.environment.read` and `stacks.commands.read`;
- `stacks.webhook.secret.read`;
- `stacks.logs.tail` and `operations.logs.read`; and
- `stacks.webhook.status` — enabled, force-deploy, and secret-source metadata only.

Administrative tools:

- `stacks.deploy`, `stacks.restart`, and `stacks.stop`;
- `deployments.deploy` and `deployments.restart`;
- `builds.run` and `builds.cancel`;
- `repos.pull`; and
- typed configuration/content writes: `stacks.config.patch`,
  `stacks.compose.write`, `stacks.environment.write`, `stacks.commands.write`,
  and `stacks.file.write`; and
- typed webhook writes: `stacks.webhook.update` (enabled and force-deploy flags)
  and `stacks.webhook.secret.write` (custom secret).

Search output is locally filtered, sorted, and capped at 50 items.
`operations.search` page indices are capped at 100, while `operations.status`
resolves one operation by its stable id rather than a moving page (its former
`page` argument is accepted but ignored for one release). `stacks.diagnostics`
returns bounded operational signals — state, Docker status text, missing Compose
file paths, and per-service image and update flags, each list capped at 250
entries — and is classified sensitive so its results are labeled accordingly.
The governed sensitive reads return bounded configuration, Compose, environment,
command, log, and webhook-secret content labeled sensitive; a stack that inherits
its webhook secret reports `source: "inherited"` with `valueAvailable: false` and
never fetches the shared value. `stacks.logs.tail` accepts 1-5000 lines and up to
50 named services. Upstream response bodies are streamed into a two-mebibyte
maximum before deserialization. Only allowlisted fields are represented in Rust,
so unrequested upstream fields are discarded before they can reach MCP output.

The typed configuration and content writes send only the fields they set through
Komodo's partial `UpdateStack` (or `WriteStackFileContents`) — no read-modify-
write and no arbitrary JSON — validate their complete bounded input before the
upstream call, and are never retried. They return the target, the applied field
names, and any reconciliation id, never the submitted contents or secret. Writes
that accept paths, content, or the webhook secret classify their input sensitive
and are `critical` (content, command, and secret writes) or `high` (structural
config patches) in the gateway catalog; `stacks.webhook.update` toggles only
flags, so its input is operational and its risk `high`. Komodo may retain a
submitted payload in its own traces; that is disclosed at the approval boundary
while MCP and gateway storage remain payload-free.

Generic create/update/delete is absent. Governed configuration changes use
narrow partial-update tools rather than read-modify-write over full resource
objects. Sensitive values may be model-visible when a caller explicitly invokes
an authorized sensitive capability, but they must not be copied into MCP or
gateway logs, errors, metrics, approval records, or audit payloads. Komodo may
independently retain values submitted through its API; such tools must disclose
that upstream behavior. See [the decisions](DECISIONS.md) and
[tool contract](docs/tool-surface.md).

## Configuration

Required runtime variables:

| Variable | Purpose |
| --- | --- |
| `KOMODO_MCP_READ_API_KEY` | Read-only Komodo service-user key |
| `KOMODO_MCP_READ_API_SECRET` | Read-only Komodo service-user secret |
| `KOMODO_MCP_ADMIN_API_KEY` | Narrow administrative Komodo key |
| `KOMODO_MCP_ADMIN_API_SECRET` | Narrow administrative Komodo secret |
| `KOMODO_MCP_GATEWAY_BEARER_CURRENT` | Current private gateway bearer |
| `KOMODO_MCP_IDENTITY_JWKS_URL` | Gateway Ed25519 JWKS endpoint |
| `KOMODO_MCP_IDENTITY_ISSUER` | Exact gateway identity-token issuer |
| `KOMODO_MCP_IDENTITY_ACTOR` | Exact gateway identity-token actor subject |

Optional variables and safe defaults are documented in
[`.env.example`](.env.example). The previous gateway bearer may be supplied by
`KOMODO_MCP_GATEWAY_BEARER_PREVIOUS` during rotation.

## Development

Rust 1.96 is pinned. Required local gates are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
python3 scripts/check_docs.py
python3 -m unittest discover -s scripts/tests
```

The gateway manifest is generated from the same registry as MCP `tools/list`:

```sh
cargo run --locked -p komodo-server -- --emit-gateway-manifest
```

The implementation contract targets Komodo 2.1.2. Compatibility references
are recorded in [the compatibility document](docs/compatibility.md). Deployment
configuration and secret-provider references belong in the operator's deployment
repository, not this source repository. See [builds and releases](docs/releases.md)
for GitHub checks, image publication, and the remaining release prerequisites.
