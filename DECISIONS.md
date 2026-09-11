# Decisions

## Typed intents instead of a raw API proxy

The MCP surface owns named operations with exact input and output schemas. It
does not accept a Komodo endpoint, request variant, query document, or arbitrary
JSON. This improves discovery and makes the information-flow and authorization
contract reviewable per tool.

## Separate read and administrative identities

Every handler is wired to exactly one of two Komodo clients. Read tools cannot
accidentally gain mutation authority; mutations cannot silently fall back to
the read credential. Both credential pairs arrive through the process
environment at startup. Operators may use any secret provider that can supply
that environment; the server has no Infisical dependency or credential-file
interface.

## Governed sensitive capabilities

Configuration, Compose, environment, logs, commands, and secrets are sensitive
data, but sensitivity alone is not a design reason to remove a capability.
Bounded typed tools can expose these values when their annotations make the
information flow explicit and the gateway authorizes the caller and disclosure.

Sensitive tools require:

- a narrow input and output schema with explicit bounds;
- accurate behavior, input-sensitivity, and result trust annotations;
- gateway-owned risk, role, and approval policy;
- tests proving values do not enter MCP or gateway telemetry and audit records;
  and
- a disclosure when Komodo may retain submitted values in its own traces or
  operation history.

Generic create/update/delete remains excluded because an arbitrary resource
payload is not a reviewable intent. Stack configuration writes use Komodo's
typed partial-update semantics directly. They do not read a full resource,
await caller or network work, and then write the stale object back. When
Komodo supplies no atomic revision precondition, same-field concurrent writes
are documented as last-writer-wins rather than hidden behind a racy client-side
check.

## Status reads minimize exposure

Ordinary status tools use minimal list or stable single-operation endpoints.
Full-resource and log endpoints are reserved for separately named sensitive
tools whose policy and result handling reflect the additional exposure. Docker
inspect endpoints remain outside the contract.

## Gateway policy is deny-by-default for mutations

Read-only low-risk tools use the authenticated-user baseline. Side-effecting
tools require `komodo-admin`, and an explicit Cedar forbid denies those tools to
every principal lacking that group. This remains true even if another policy
grants a broad administrator role.

## MCP annotations describe behavior; the gateway owns policy

Standard MCP annotations and the experimental action-metadata extension
draft (published under the official MCP extensions organization) describe
read-only behavior, idempotency, destructive or consequential outcomes, input
destinations, and review requirements. Result trust annotations describe
sensitive and untrusted output. These values are claims for the gateway to
validate and enforce, not authorization decisions.

The gateway catalog remains authoritative for risk because risk depends on the
deployment, caller, target, and policy environment. The legacy
`side_effects` manifest field is replaced by MCP behavior annotations for this
server. The legacy `pii` field is replaced by result-level sensitive and
untrusted annotations instead of a new custom data taxonomy.
