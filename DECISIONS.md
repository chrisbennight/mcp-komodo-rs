# Decisions

## Typed intents instead of a raw API proxy

The MCP surface owns named operations with exact input and output schemas. It
does not accept a Komodo endpoint, request variant, query document, or arbitrary
JSON. This improves discovery and makes operational meaning and information
flow reviewable per tool.

## Separate read and administrative identities

In the HTTP profile, each handler uses the read or administrative Komodo client.
Read tools cannot gain mutation authority; mutations cannot fall back to the
read credential. Credential availability should reflect an operation's upstream
authority rather than a judgment about the sensitivity of its result. Process
credentials arrive through the environment at startup. Operators may use any
secret provider that supplies that environment; the server has no Infisical
dependency.

## Semantic capability and external policy

The library describes named operations, their factual effects, and upstream
facts. The operator's gateway owns access, disclosure, risk, approval, and role
policy. Ordinary configuration, logs, Compose, environment content, command
descriptions, network addresses, paths, public keys, and resource metadata do
not acquire a blanket secret classification from this library.

Typed contracts, input validation, authentication, appropriate upstream
credentials, cancellation, and bounded request costs protect runtime integrity.
They do not justify silently discarding supported facts or imposing categorical
capability bans. An unsupported upstream operation is a coverage question;
presumed sensitivity is not an architectural prohibition.

Credential values are actual passwords, tokens, API keys, or equivalent
authentication material. Those values require governed file transfer outside
model context in both directions. Preserve credential operations and metadata
such as identifiers, references, source, and existence. Document upstream
Komodo retention accurately. Avoid speculative scanning of ordinary documents
for hypothetical credentials.

Generic create/update/delete remains excluded because an arbitrary resource
payload is not a reviewable intent. Stack configuration writes use Komodo's
typed partial-update semantics directly. They do not read a full resource,
await caller or network work, and then write the stale object back. When
Komodo supplies no atomic revision precondition, same-field concurrent writes
are documented as last-writer-wins rather than hidden behind a racy client-side
check.

## Summaries preserve access to facts

Compact status views and caller-selected searches or projections can make
operational work easier. Supported facts omitted from a summary should remain
available through an appropriate typed view. Resource limits bound each request
and preserve useful continuation rather than limiting which records can ever
be reached. Errors preserve actionable non-credential information.

## MCP annotations describe behavior; the gateway owns policy

MCP annotations describe factual behavior such as read-only operation,
idempotency, destructiveness, and interaction with external systems. Upstream
content remains untrusted data. These descriptions support an operator's policy
without assigning risk tiers, PII judgments, approval requirements, or local
role names. Catalog validation should detect semantic and schema drift while
allowing legitimate operator policy differences.

Production connection settings, gateway manifests, and applied policy belong
in operator-owned deployment repositories or control planes. This repository
owns application source, image publication, and tests of its semantic contract.
