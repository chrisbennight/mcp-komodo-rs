# Gateway deployment

The HTTP server requires a gateway that implements the identity and policy
contract below. An ordinary MCP client cannot connect directly using only a
Komodo API key. The service does not implement OAuth discovery or interactive
login. This guide describes integration prerequisites, not a standalone client
quickstart.

## Prepare Komodo access

Use the [tested Komodo contract](compatibility.md). Create separate service-user
API credentials for reads and writes. Restrict each user to the resources it
needs. The read identity needs access to status and any explicitly enabled
sensitive read endpoints. It must have no execution or configuration-write
permissions. The write identity needs only the resource permissions for enabled
deploy/restart/stop/build/pull or configuration intents; it must not be a global
administrator merely to simplify setup. Resource permissions and API-key scopes
must both permit the operation.

Exact permission combinations vary by tool and upstream version. Verify them
against a disposable Komodo instance before enabling that tool. There is no
claim that the server provisions or audits these users automatically.

## Configure the gateway and server

Supply the variables in the [configuration reference](configuration.md), using
your secret provider for credential values. Place the server on networks that
reach Core and the gateway without publishing it to the Internet. The example
network layout in [architecture](architecture.md#network-boundary) is optional;
the authentication requirements are not.

The gateway sends both `Authorization: Bearer …` and `X-MCP-Identity` on every
`/mcp` request. The latter is an Ed25519 JWT with a recognized `kid`, the configured
issuer and actor, audience `komodo`, and bounded timestamps. The Host header must
match the server's allowlist. The gateway owns user authentication, tool-level
authorization, approval, and safe audit storage. Membership in a network is not
authorization.

Generate the tool catalog with:

```sh
cargo run --locked -p komodo-server -- --emit-gateway-manifest
```

Import it using your gateway's deployment procedure. Do not edit the generated
tool classifications separately from the Rust registry. Standard MCP annotations
and experimental metadata describe behavior; they grant no permissions. The
reference policy allows ordinary status reads for authenticated users, requires
`komodo-admin` for mutations, and separately governs sensitive reads and approval
requirements. Enforce a deny rule for unauthorized writes even if another policy
grants broad administrator access.

## Verify the connection

First check `/healthz` or run `komodo-mcp-rs --healthcheck`. Success proves only
that the process is serving requests. Through your configured gateway and MCP
client, initialize the connection, list tools, then call `system.status` with
`{}`. This is the first check that exercises upstream read credentials. Confirm
an unauthorized identity cannot invoke a mutation before enabling operational
use. Test mutations only against disposable resources.

An HTTP 401 from `/mcp` can indicate a Host/Origin mismatch, bearer mismatch,
unverifiable identity token, stale timestamps, or JWKS failure. Inspect the
configuration names and clock synchronization without logging tokens. A Komodo
authentication/permission error after a valid MCP call concerns the upstream
service user. A healthy process does not rule out either problem.
