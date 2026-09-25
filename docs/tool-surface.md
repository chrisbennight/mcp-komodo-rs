# Tool surface

For uncertain write results, use the
[operation-specific reconciliation guide](reconciliation.md). Metadata exports
describe the tools; gateway authorization and sensitive-read approvals remain
separate requirements.

## Available tools

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
Resource pages never advertise an offset beyond 10000. `truncated: true` means
matching items remain beyond that supported range; narrow the query. Each
request fetches the bounded upstream inventory before local filtering, so an
upstream body larger than two MiB still fails rather than silently truncating.
`operations.search` page indices are capped at 100. Searches return `nextOffset` for
remaining matches within the same page; follow it before `nextPage`, then reset
offset to zero when changing pages. Page contents can change as new operations
arrive, so these cursors are not snapshot guarantees. `operations.status`
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
and use `high`, the gateway's highest supported risk level, in the manifest
scaffold; `stacks.webhook.update` toggles only flags, so its input is operational
and its risk is also `high`. Komodo may retain a
submitted payload in its own traces; that is disclosed at the approval boundary
while MCP and gateway storage remain payload-free.

Generic create/update/delete is absent. Governed configuration changes use
narrow partial-update tools rather than read-modify-write over full resource
objects. Sensitive values may be model-visible when a caller explicitly invokes
an authorized sensitive capability, but they must not be copied into MCP or
gateway logs, errors, metrics, approval records, or audit payloads. Komodo may
independently retain values submitted through its API; such tools must disclose
that upstream behavior. See [the decisions](../DECISIONS.md) and the contracts below.

## Read contract

Ordinary status tools expose only resource ids, names, normalized state,
derived health, counts, versions, timestamps, and update availability. A
separately named sensitive read may expose bounded configuration or diagnostic
content when its schema, annotations, gateway policy, and result handling make
that disclosure explicit.

Search inputs accept an optional 128-byte case-insensitive substring, a bounded
offset, and a limit from 1 through 50. Results are sorted by normalized name and
id before slicing. Status selectors accept an exact name or id and fail on
ambiguity.

`operations.search` returns a bounded page of Komodo's update list, while
`operations.status` resolves one operation by its stable id through Komodo
`GetUpdate` (its former `page` argument is accepted but ignored for one
release). Both return id, operation kind, start timestamp, success, and status.
`stacks.diagnostics` is a sensitive read that adds Docker status text, missing
Compose file paths, per-service image and update signals, and deploy-vs-latest
drift. Ordinary status responses continue to omit logs, commands, stdout,
stderr, operator/user identity, commit data, and previous/current TOML snapshots.

Governed sensitive reads expose bounded configuration and content through
their own named tools rather than a raw proxy. `stacks.config.read` returns the
configuration shape — file mode, run directory, file paths, env-file path,
webhook flags, and presence flags — without raw Compose, environment, commands,
or the webhook secret. `stacks.compose.read` returns Compose files for a
configured, latest, or deployed source. `stacks.environment.read` and
`stacks.commands.read` return the raw environment and pre/post-deploy commands.
`stacks.webhook.status` reports whether the webhook is enabled, whether it forces
deploy, and whether its secret is custom or inherited. `stacks.webhook.secret.read`
returns a custom secret value; a stack that inherits Komodo Core's shared secret
instead reports `source: "inherited"` with `valueAvailable: false` and no value,
and the shared secret is never fetched. `stacks.logs.tail` tails container logs
(1-5000 lines, up to 50 named services) and `operations.logs.read` returns an
operation's per-stage command logs. Per-file and per-record lists are bounded,
and every content-bearing read labels its result `sensitive` and `untrusted`.

## Mutation contract

### Choose the intended configuration effect

Configuration and deployment are separate steps. Check the stack's source mode
and desired change before selecting a write. A successful partial configuration
update records the supplied fields; it does not prove the running application
has adopted them. Inspect the receipt and reconcile an uncertain result before
deciding whether to deploy or submit another write.

| Tool | Replacement and clearing | Read to reconcile |
| --- | --- | --- |
| `stacks.config.patch` | Only supplied structural fields change. An empty `file_paths` list clears the list; individual path values must be nonempty. | `stacks.config.read` |
| `stacks.compose.write` | Replaces the complete inline Compose text; empty clears it. It does not switch away from a repository or host-file source. | `stacks.compose.read` with `source: "configured"` |
| `stacks.environment.write` | Replaces the entire environment text; it does not merge individual variables. Empty clears the configured text. | `stacks.environment.read` |
| `stacks.commands.write` | Replaces each supplied command object, leaving omitted pre/post commands unchanged. Empty command text clears that command. Stored commands run later with upstream deployment privileges. | `stacks.commands.read` |
| `stacks.file.write` | Writes a complete file in repository or files-on-host mode; empty content leaves an empty file. Core enforces the applicable mode and path rules. | `operations.status` using the returned operation ID |
| `stacks.webhook.update` | Only supplied flags change; their values affect subsequent webhook handling. | `stacks.webhook.status` |
| `stacks.webhook.secret.write` | Replaces the custom value with a nonempty secret. Clearing it to restore inheritance is not supported. The shared Core secret remains unavailable through this server. | `stacks.webhook.status`, or `stacks.webhook.secret.read` for an authorized custom-value check |

Inline Compose is stored configuration text. Repository and host-file modes use
files selected by Core instead. The configured, latest, and deployed Compose
views answer different questions: desired input, the latest content observed by
Core, and content associated with its deployed state. None substitutes for live
application-health evidence.

Before approval, present the target, affected fields, replacement or clearing
intent, source-mode implications, later execution/deployment consequences, and
the fact that Komodo may retain submitted content. Exclude values, secret
hashes, raw commands, and content from approval and audit records. An upstream
retention disclosure does not authorize retaining those values in MCP logs.

### Validation and reconciliation

Mutation tools accept one exact resource selector and an operation-specific
typed intent. They do not accept arbitrary JSON, endpoint names, wildcard
actions, or generic resource objects. The result is a minimal receipt suitable
for later reconciliation and never echoes sensitive input.

Annotations are assigned from each operation's behavior instead of treating
every mutation as destructive and non-idempotent. The gateway catalog owns
risk and authorization; every mutation continues to require `komodo-admin`.
Gateway deployment policy must enforce interactive, argument-bound approval
for tools whose metadata requests review. Verify that effective policy before
enabling these tools; metadata alone does not enforce approval.

Mutation calls are never retried automatically. A transport failure does not
prove that Komodo rejected the request.

`stacks.stop` stops all services of an existing stack selected by exact name or
id, preserving its containers. It uses Komodo's default termination timeout
and returns an operation receipt. It requires `komodo-admin` and requests
interactive review because stopping services interrupts availability. Repeated
stops have an idempotent stopping effect, but calls are never retried; reconcile
an ambiguous outcome through `operations.search` and `operations.status` before
deciding whether to submit another operation.

Configuration writes use Komodo's typed partial-update semantics directly.
`stacks.config.patch` sets structural fields (file mode, run directory, file
paths, env-file path); `stacks.compose.write`, `stacks.environment.write`, and
`stacks.commands.write` replace inline Compose, environment, and pre/post-deploy
commands through partial `UpdateStack`; `stacks.webhook.update` sets the webhook
enabled and force-deploy flags and `stacks.webhook.secret.write` sets the custom
webhook secret through the same partial update; and `stacks.file.write` writes
one file through `WriteStackFileContents`. They never perform a full-resource
read-modify-write cycle, send only the fields they set, validate their complete
bounded input before the upstream call, and return the target, applied field
names, and any reconciliation id — never the submitted contents or secret.
Content, command, secret, structural, and webhook-flag writes use `high` in the
gateway manifest scaffold. Each write's input sensitivity is declared through action-metadata, and
only `stacks.file.write` labels its result sensitive because it echoes a path. Same-field concurrent updates are last-writer-wins when Komodo
offers no revision precondition. Komodo may retain submitted configuration,
content, commands, or secret values in its own traces or operation history;
affected tool descriptions and approval prompts must disclose that upstream
behavior.

## Metadata ownership

Standard MCP annotations and the experimental
`io.modelcontextprotocol/action-metadata` namespace describe behavior and input
sensitivity. The pinned Rust MCP type cannot retain extension members inside
`ToolAnnotations`, so Komodo publishes that namespace through `Tool._meta`; the
gateway normalizes it to the same reviewed contract.

Current read tools are read-only, non-destructive, idempotent, closed-world,
benign, and do not request review. Mutation metadata is operation-specific:

| Operations | Destructive | Idempotent | Open world | Requests review |
|---|---:|---:|---:|---:|
| stack/deployment deploy, repository pull | yes | no | yes | yes |
| stack/deployment restart | no | no | no | no |
| stack stop | yes | yes | no | yes |
| build run | no | no | yes | no |
| build cancel | yes | yes | no | yes |
| stack config/content/file/webhook writes | yes | no | no | yes |

All mutations have a consequential outcome. These are behavior claims, not
gateway permissions; catalog-owned policy remains authoritative. Writes that
accept paths, content, or the webhook secret declare sensitive input through
`action-metadata`, so the gateway classifies their arguments accordingly;
`stacks.webhook.update` toggles only flags and declares operational input.

Result-level `io.modelcontextprotocol/trust-annotations` label sensitive and
untrusted output. Most normalized results claim `sensitive: false` and
`untrusted: true`: they exclude sensitive fields but still contain
upstream-generated strings. A read that surfaces operator-sensitive signals
claims `sensitive: true` instead — `stacks.diagnostics` does, because it returns
missing-file paths and Docker status text — and its catalog entry carries the
matching `pii` classification and `returnMetadata.sensitivity: "sensitive"`
action metadata. The governed configuration, Compose, environment, command, log,
and webhook-secret reads set the same labels rather than inheriting the default.
The gateway treats annotations as claims, incorporates them into tool
identity and drift checks, and applies catalog-owned risk, release, and
authorization policy.

The Komodo `side_effects` and `pii` classification is no longer projected into
the gateway manifest. The sidecar's registry drives the MCP annotations —
behavior hints replace `side_effects`, and result-level sensitivity and trust
labels replace `pii` — and `--emit-gateway-manifest` now renders an
annotation-native scaffold (`classification_mode: mcp_annotations`, gateway-owned
`risk` only, no per-tool `side_effects`/`pii`) that an operator completes with
the per-tool behavior hash the gateway observes from the live server. Risk
remains gateway-owned rather than moving into MCP annotations.

## Hard exclusions

There are no tools for Docker or Swarm inspection, container or server
terminals, unrestricted shell execution, arbitrary actions or procedures,
providers, credential stores, permissions, onboarding keys, ResourceSync
apply, raw API forwarding, or generic resource create/update/delete. Sensitive
configuration and logs are governed capabilities, not members of this
exclusion list.
