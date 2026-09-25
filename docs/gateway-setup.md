# Gateway deployment

The HTTP server requires a gateway that implements the identity and policy
contract below. An ordinary MCP client cannot connect directly using only a
Komodo API key. The service does not implement OAuth discovery or interactive
login. This guide describes integration prerequisites, not a standalone client
quickstart. For a local client, use the [status-only quickstart](quickstart.md).

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

Export the standard MCP catalog for a gateway that consumes tool schemas and
annotations:

```sh
cargo run --locked -p komodo-server -- --emit-tools-json
```

The gateway-specific connection and tool-risk scaffold is also available:

```sh
cargo run --locked -p komodo-server -- --emit-gateway-manifest
```

This scaffold is not ready to publish: its supported gateway requires an
`approved_behavior_hash` for each tool, obtained from that gateway's live
manifest-change preview. Configure its URL and bearer secret reference for your
deployment, then follow your gateway's admission procedure. Do not maintain a
separate hand-edited copy of the Rust tool classifications.

The scaffold uses the gateway's `low`, `medium`, and `high` risk vocabulary.
Consequential configuration, content, command, and secret writes use `high`,
its highest supported level. Their sensitivity and review requirements remain
separate behavior claims; a risk label alone is not an authorization or an
approval policy.

Before publishing, compare the complete generated tool-name set with the live
upstream and gateway catalog. Inspect every missing tool, changed schema, and
behavior-hash mismatch. Use the gateway preview to obtain and review current
behavior hashes; a hash is an approval record, not a value to copy blindly to
make a mismatch disappear. Re-preview the complete intended manifest and
resolve quarantine findings before requesting publication approval. Preserve
unrelated manifests and use the control plane's current revision precondition.

When using a reduced HTTP capability profile, pass the same `--tool-profile`
value to both metadata export commands and the serving process. A full scaffold
does not describe a restricted deployment. The gateway still applies user policy
and approvals to every tool admitted by the selected profile.

Neither export contains an authorization or approval policy. You must configure
that policy in the gateway. Permit ordinary status reads only for authenticated
users, require `komodo-admin` for mutations, and separately govern sensitive
reads and approval requirements. Enforce a deny rule for unauthorized writes
even if another policy grants broad administrator access. Standard MCP
annotations and experimental metadata describe behavior; they grant no
permissions. The server verifies gateway identity but does not implement these
per-user policy decisions itself.

## Verify the connection

First check `/healthz` or run `komodo-mcp-rs --healthcheck`. Success proves only
that the process is serving requests. Through your configured gateway and MCP
client, initialize the connection, list tools, then call `system.status` with
`{}`. This is the first check that exercises upstream read credentials. Confirm
an unauthorized identity cannot invoke a mutation before enabling operational
use. Test mutations only against disposable resources. Follow
[write reconciliation](reconciliation.md) when no usable receipt is returned;
never automatically repeat a timed-out write.

An HTTP 401 from `/mcp` can indicate a Host/Origin mismatch, bearer mismatch,
unverifiable identity token, stale timestamps, or JWKS failure. Inspect the
configuration names and clock synchronization without logging tokens. A Komodo
authentication/permission error after a valid MCP call concerns the upstream
service user. A healthy process does not rule out either problem.

## Validate deployed catalog coverage and classification

Export expectations from the exact source image being deployed, selecting its
configured profile. This command needs no runtime credentials:

```sh
komodo-mcp-rs --emit-gateway-contract-json --tool-profile full > source-contract.json
```

Using the gateway's authorized discovery tools, collect every Komodo tool into
an operator-local JSON projection with a `tools` array. Each entry must contain
its fully qualified `name` and `governance` object with `risk`, `side_effects`,
`pii`, `requires_approval`, and `requires_approval_known`. Fetch all pages and use
a caller entitled to discover the complete configured profile; a restricted
caller's smaller catalog is not evidence of deployment drift. Do not include
connection configuration, credentials, or tool arguments in this projection.

```sh
python3 scripts/check_gateway_catalog.py source-contract.json observed-catalog.json \
  --approval-mode per_call
```

Select the deployment's actual `approval_mode` explicitly. `per_call` expects
the source review requirement to remain an imported approval floor;
`policy_only` records the deliberate choice to govern approvals through policy.
The validator refuses missing or extra tools, duplicate identities, unknown
approval classification, and risk/effect/sensitivity drift. A zero exit status
proves only agreement of this complete projection. It does not prove execution
authorization, caller-specific exemptions, or Cedar approval overlays. Exercise
those separately with controlled identities and non-production targets. The
gateway's reviewed behavior hashes continue to enforce schema and description
drift; this projection does not replace contract review or approve anything.
