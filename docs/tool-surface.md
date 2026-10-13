# Tool surface

For uncertain write results, use the
[operation-specific reconciliation guide](reconciliation.md). Metadata exports
describe the tools; operators configure gateway authorization, disclosure,
and approval policy separately. The [architectural decisions](../DECISIONS.md)
define ownership. The library publishes factual MCP behavior and does not assign
risk, sensitivity, roles, or approval requirements.

For smaller catalogs, on-demand schemas, and compatible result presentation, see
[discovery efficiency](discovery-efficiency.md).

## Available tools

Read-only tools:

- `system.status`;
- `servers.search` and `servers.status`;
- `servers.networks.search` and `servers.networks.read`;
- `stacks.search`, `stacks.status`, and `stacks.diagnostics`;
- `deployments.search` and `deployments.status`;
- `builds.search` and `builds.status`;
- `repos.search` and `repos.status`; and
- `operations.search` and `operations.status`.

Additional read-only tools for stack configuration and content:

- `stacks.config.read` — complete ordinary configuration and a compatibility summary;
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

Search output is locally filtered, sorted, and selected by the caller's positive
limit, which defaults to 20.
Resource pages never advertise an offset beyond 10000. `truncated: true` means
matching items remain beyond that supported range; narrow the query. Each
request fetches the bounded upstream inventory before local filtering, so an
upstream body larger than two MiB still fails rather than silently truncating.
`operations.search` accepts the upstream unsigned page index and fetches one
bounded page per request, without imposing a history horizon. Searches return `nextOffset` for
remaining matches within the same page; follow it before `nextPage`, then reset
offset to zero when changing pages. Page contents can change as new operations
arrive, so these cursors are not snapshot guarantees. `operations.status`
resolves one operation by its stable id rather than a moving page (its former
`page` argument is accepted but ignored for one release). `stacks.diagnostics`
returns bounded operational signals — state, Docker status text, missing Compose
file paths, and per-service image and update flags, with 250 entries per collection
by default. Content reads return bounded configuration, Compose, environment,
commands, and logs as reported by Core. Custom webhook tokens use native
file transfer outside model context; a stack that inherits
its webhook secret reports `source: "inherited"` with `valueAvailable: false` and
never fetches the shared value. `stacks.logs.tail` accepts 1-5000 lines and up to
50 named services. Upstream response bodies are streamed into a two-mebibyte
maximum before deserialization. Named typed models represent supported Core
facts; normalization does not discard addresses, public keys, identity,
or repository provenance based on presumed sensitivity.

The typed configuration and content writes send only the fields they set through
Komodo's partial `UpdateStack` (or `WriteStackFileContents`) — no read-modify-
write and no arbitrary JSON — validate their complete bounded input before the
upstream call, and are never retried. They return the target, the applied field
names, and any reconciliation id, never the submitted contents or secret.
Komodo may retain submitted configuration or credentials in its own traces or
operation history. Tool descriptions disclose this upstream behavior.

Generic create/update/delete is absent. Governed configuration changes use
narrow partial-update tools rather than read-modify-write over full resource
objects. Ordinary metadata and content remain available through typed reads;
the operator governs their disclosure. Literal credential values use native
file transfer outside model context and must never appear in MCP diagnostics or
approval records. See [the decisions](../DECISIONS.md) and the contracts below.

## Read contract

Resource search and status tools retain their normalized health summaries and
include `resource`, a typed Core list record with id, type, name, template flag,
tags, and resource-specific `info`. Fields inside `resource` use Core's field
names. Server facts include network addresses, public keys, region, alerts, and
available statistics. Stack, deployment, build, and repository facts include
placement, image, source, branch, and commit provenance. Fields introduced in
newer Core releases are optional when older versions do not report them.

`servers.networks.search` selects one server by name or id and returns its typed
Docker network inventory. An optional `query` matches network names or ids,
ignoring ASCII case and surrounding query whitespace. Results are sorted by
name and id. A `window` defaults to offset zero and 250 records; follow
`itemsPage.nextOffset` to reach the remaining filtered inventory. Every request
fetches a fresh bounded inventory, so continuation is not a snapshot guarantee.
List facts include driver, scope, IPv6 enablement, IPAM subnet and gateway, and
attachment flags when Core reports them.

`servers.networks.read` takes a server selector and a network name or id. It
returns the server identity and a typed network record, including IPAM address
ranges and auxiliary addresses, driver options, labels, and attached endpoint
identifiers and MAC, IPv4, and IPv6 addresses. Network record fields use snake
case; missing facts remain null rather than becoming empty or false. Both tools
use read credentials, preserve ordinary addressing facts without sensitivity
filtering, and apply the shared upstream response bound. Input validation
finishes before resource resolution. Network data remains untrusted and never
becomes a local command.

Search inputs accept an optional 128-byte ASCII-case-insensitive substring, a bounded
offset, and a positive limit. Results are sorted by normalized name and
id before slicing. Status selectors accept an exact name or id and fail on
ambiguity.

Input limits are checked before upstream reads, including resource resolution.
Invalid requests therefore fail locally even when Komodo is unavailable. Query
length and control characters are checked before whitespace trimming; ordinary
leading and trailing spaces are then removed, and an empty query matches all.
Selectors remain exact inputs without trimming.

Published input schemas include numeric ranges, collection sizes, and necessary
string-length bounds and control-character exclusions. JSON Schema counts characters, while runtime string limits
count UTF-8 bytes: 128 for selectors, operation ids, service names, and queries;
512 for paths; and 262144 for written content, commands, and custom webhook
secrets. Field descriptions state these byte limits. Unicode remains supported,
but a multibyte value can satisfy `maxLength` and still exceed its byte budget.
Runtime validation also checks control characters and whether a partial intent
actually supplies a change. No schema check replaces that validation.

`operations.search` returns a bounded page of Komodo's update list, while
`operations.status` resolves one operation by its stable id through Komodo
`GetUpdate` (its former `page` argument is accepted but ignored for one
release). Both return id, operation kind, start timestamp, raw status, explicit
`outcome`, nullable `success`, and a typed resource target when available.
Accepted/running/unknown operations have `success: null`; only completed
operations report true or false. A target identifies a resource, not the caller
that submitted an operation. See [reconciliation](reconciliation.md).
`stacks.diagnostics` adds Docker status text, missing
Compose file paths, per-service image and update signals, and deploy-vs-latest
drift. Operation search and status preserve initiating user identity, version,
commit data, and operation-specific data when Core supplies them. A list record may lack detail-only fields;
use `operations.status` with its stable id for those facts. Resource target kinds
remain open strings so new Core resource kinds retain their correlation. Command
logs remain available through `operations.logs.read`. These named projections
are semantic contracts, not disclosure policy.

Operation search and status expose `previousConfiguration` and
`currentConfiguration` when Core supplies history. Each includes `toml`,
`credentialFields`, and native `originalFile` metadata. Ordinary facts stay
inline; literal credentials use exact native originals with explicit transfer
unavailability. See
[history credential fields](file-transfers.md#configuration-history).

Content reads expose typed configuration and content under the bounded upstream
response limit. `stacks.config.read` retains its summary and adds `configuration`
with Core's ordinary configuration fields, including placement, repository and
account references, update settings, file dependencies, command wrappers, inline
Compose, environment text, and pre/post-deploy commands. The nested object uses
Core's snake_case field names. Newly supported fields are null when absent upstream;
reported false, empty text, and empty lists remain distinct from absence. The literal
`webhook_secret` field is excluded from this object and is reachable through its
native-file tool. No content scanner classifies configuration text as a credential.
`stacks.compose.read` returns a page of configured, latest, or deployed files, plus
`provenance` containing cached and deployed project, commit, service, resolved
Compose configuration, missing-file, and remote-error facts. Remote file records
also retain their service dependencies and required action. `stacks.environment.read`
and `stacks.commands.read` return environment text and pre/post-deploy commands;
commands retain Core's shell-mode setting. Command writes accept optional
`shell_mode`; omitting it uses Core's default for the replacement command.
`stacks.webhook.status` reports whether the webhook is enabled, whether it forces
deploy, and whether its secret is custom or inherited. `stacks.webhook.secret.read`
returns native file metadata for a custom token; a stack that inherits Komodo Core's shared secret
instead reports `source: "inherited"` with `valueAvailable: false` and no file,
and the shared token is never fetched. `stacks.logs.tail` tails container logs
(1-5000 lines, up to 50 named services) and `operations.logs.read` returns an
operation's per-stage command logs. Per-file and per-record pages are bounded,
and results mark Core-provided content as `untrusted` without classifying sensitivity.

Literal token values use [native file transfer](file-transfers.md). Ordinary
configuration, Compose, environment, commands, and logs retain their content
contracts; they are not treated as credentials by a speculative scanner.

### Collection completeness and continuation

`stacks.diagnostics`, `stacks.config.read`, `stacks.compose.read`, and
`operations.logs.read` accept `window: {"offset":0,"limit":250}`. Both fields
are optional within the optional window: offset defaults to zero and limit to
250. Limits must be positive and offsets are 0–2097152. Larger limits select
more of the current bounded response without additional upstream requests.
Invalid windows are rejected
before any upstream read. Omission retains the first-page behavior.

Each collection has adjacent page metadata: `servicesPage`, `missingFilesPage`,
`filePathsPage`, `filesPage`, or `logsPage`. It reports `available`, `returned`,
`totalObserved`, `truncated`, and `nextOffset`. Follow each collection's next
offset in a later request; a diagnostics window applies independently to its
service and missing-file lists. `truncated` means some observed records are
outside the current page, including earlier records. A final continuation page
can therefore have `truncated: true` and `nextOffset: null`.

Unavailable latest/deployed Compose contents report `available: false` and
`totalObserved: null`. Available empty contents report true and zero instead.
An empty configured inline source is an available empty collection. These values
describe Core's response, not whether files exist on a host. Continuations fetch
fresh bounded responses and do not provide a stable snapshot. Concurrent changes
can shift offsets; reconcile important results against resource/operation IDs.

Pagination does not shorten an individual file or log stage. Log-stage record
limits differ from the line count accepted by `stacks.logs.tail`. The upstream
two-MiB response limit still applies before local pagination; an oversized source
fails explicitly and is never presented as a complete page.

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
| `stacks.webhook.secret.write` | Sets a custom token from a completed native credential file, or explicitly restores inheritance. The shared Core token remains unavailable through the stack API. | `stacks.webhook.status`, or `stacks.webhook.secret.read` for an authorized custom-value check |

Inline Compose is stored configuration text. Repository and host-file modes use
files selected by Core instead. The configured, latest, and deployed Compose
views answer different questions: desired input, the latest content observed by
Core, and content associated with its deployed state. None substitutes for live
application-health evidence.

The operator decides which changes require approval and what ordinary context
its review presents. Relevant operation facts include the target, affected
fields, replacement or clearing intent, source mode, later deployment effects,
and upstream retention. Literal credential values must remain outside model
context, approval records, and diagnostics.

### Validation and reconciliation

Mutation tools accept one exact resource selector and an operation-specific
typed intent. They do not accept arbitrary JSON, endpoint names, wildcard
actions, or generic resource objects. The result is a minimal receipt suitable
for later reconciliation and never echoes submitted configuration or credentials.

Annotations are assigned from each operation's behavior instead of treating
every mutation as destructive and non-idempotent. Operators own risk,
authorization, disclosure, roles, and approval policy in their gateway. The
library does not request review or prescribe the operator's policy. Verify the
deployment's chosen policy before enabling tools; metadata alone does not grant
access or enforce approval.

Mutation calls are never retried automatically. A transport failure does not
prove that Komodo rejected the request.

`stacks.stop` stops all services of an existing stack selected by exact name or
id, preserving its containers. It uses Komodo's default termination timeout
and returns an operation receipt. Stopping services interrupts availability;
operators decide applicable roles and approvals outside the library. Repeated
stops have an idempotent stopping effect, but calls are never retried; reconcile
an ambiguous outcome through `operations.search` and `operations.status` before
deciding whether to submit another operation.

Configuration writes use Komodo's typed partial-update semantics directly.
`stacks.config.patch` sets structural fields (file mode, run directory, file
paths, env-file path); `stacks.compose.write`, `stacks.environment.write`, and
`stacks.commands.write` replace inline Compose, environment, and pre/post-deploy
commands through partial `UpdateStack`; `stacks.webhook.update` sets the webhook
enabled and force-deploy flags and `stacks.webhook.secret.write` sets a custom token from native file transfer
or explicitly selects inheritance through the same partial update; and `stacks.file.write` writes
one file through `WriteStackFileContents`. They never perform a full-resource
read-modify-write cycle, send only the fields they set, validate their complete
bounded input before the upstream call, and return the target, applied field
names, and any reconciliation id — never the submitted contents or secret.
`stacks.file.write` includes the selected path in its receipt. Same-field
concurrent updates are last-writer-wins when Komodo
offers no revision precondition. Komodo may retain submitted configuration,
content, commands, or secret values in its own traces or operation history;
affected tool descriptions disclose that upstream behavior.

## Upstream failure details

An unsuccessful Core response retains its HTTP status, named request, and bounded
diagnostic in the MCP error's `data.upstream`. Core's `error` and `trace` fields
remain ordinary untrusted data. A UTF-8 text response uses the same diagnostic
shape with an empty trace; an empty or non-UTF-8 response has a null diagnostic.
The shared response limit applies before parsing. Diagnostic values never enter
server logs, metrics, or Rust error formatting.

Only literal values explicitly known from the request's API credentials or
`UpdateStack.webhook_secret` receive the credential boundary. If a diagnostic
echoes one of these values, its ordinary projection separates text around that value and reports
`credentialValuesPresent: true`; `diagnosticFile` holds native metadata for the
exact original response. Ordinary paths, addresses, configuration references, and
text mentioning passwords or tokens do not trigger a speculative content scan.
When native transfer or capacity is unavailable, `diagnosticFileUnavailable`
explains that limitation while preserving the ordinary diagnostic projection.
The original credential bytes never fall back into context. Changed text is an
object with `segments`, rather than joining fragments that could reconstruct a
credential. At most 256 segments per text field remain inline; an explicit
`diagnosticUnavailable` reason directs the caller to the native original when
that bound is exceeded. Received HTTP status and request survive interrupted or
oversized failure bodies with an explicit body-availability reason.

Credential-bearing failures use a standard MCP tool-error result (`isError:
true`) whose structured content contains `code`, `message`, and `data`. The
output schema includes that form, allowing the gateway to import its native file.
Other failures retain JSON-RPC error responses.

Submitted mutations retain their uncertain-outcome and reconciliation facts
alongside the diagnostic. Authentication, authorization, and not-found responses
remain distinct. Reads retry a server-unavailable response at most once; mutations
are never replayed. A diagnostic is evidence, not permission to retry a write.

## Metadata ownership

Standard MCP annotations describe whether an operation reads, destroys,
is idempotent, or interacts with an open world. Current reads are read-only,
non-destructive, idempotent, and closed-world. Mutation annotations reflect the
specific operation:

| Operations | Destructive | Idempotent | Open world |
|---|---:|---:|---:|
| stack/deployment deploy, repository pull | yes | no | yes |
| stack/deployment restart | no | no | no |
| stack stop | yes | yes | no |
| build run | no | no | yes |
| build cancel | yes | yes | no |
| stack config/content/file/webhook writes | yes | no | no |

The library publishes no action-metadata, risk, PII, sensitivity, review, or role
claims. Result-level `io.modelcontextprotocol/trust-annotations` reports
`untrusted: true` for Core-provided data. This is an origin trust fact, not a
disclosure or authorization rule.

`--emit-gateway-manifest` produces an incomplete connection and tool-name
scaffold. The operator supplies risk and gateway-observed behavior hashes and
chooses disclosure, roles, and approvals outside this repository.
`--emit-gateway-contract-json` preserves standard MCP declarations for
[deployment drift checks](gateway-setup.md#validate-deployed-catalog-coverage-and-declarations).
Those checks compare source-owned schemas, descriptions, and factual behavior;
they do not impose operator policy or replace the gateway's reviewed admission.

## Current implementation coverage

Beyond the typed network reads, there are no tools for container or Swarm inspection, container or server
terminals, unrestricted shell execution, arbitrary actions or procedures,
providers, credential stores, permissions, onboarding keys, ResourceSync
apply, raw API forwarding, or generic resource create/update/delete. This is
the current coverage, not a prohibition on additional named typed capabilities.
Configuration and logs are existing capabilities. Missing functionality is a
coverage question; presumed sensitivity does not establish an architectural ban.
