# Architecture

## Request path

The MCP gateway authenticates the caller, applies Cedar authorization, mints a
short-lived `X-MCP-Identity` JWT audienced to `komodo`, and calls the private
Streamable HTTP endpoint with the per-upstream bearer. The server independently
verifies the bearer, Host and Origin policy, JWT signature, issuer, audience,
times, subject, and the configured exact gateway actor before MCP dispatch.

Identity tokens may be issued at most 240 seconds before verification, allow
30 seconds of clock skew, and must have a positive lifetime no longer than 300
seconds. Their expiration may be no more than 330 seconds in the future. These
bounds prevent an otherwise valid signed token from becoming a long-lived
credential.

`komodo-mcp` parses the exact Rust-derived input schema and validates runtime
bounds before choosing a read-only or administrative API client. `komodo-api`
then sends one typed Komodo envelope over a proxy-free, redirect-free client.
Responses are streaming-capped before deserialization and normalized into
typed fields. Ordinary tools allowlist non-sensitive status fields; separately
governed tools may return bounded sensitive content with result trust
annotations. The caller receives `structuredContent` with the JSON text
compatibility representation supplied by the MCP library.

## Network boundary

One supported deployment layout uses two private networks:

- a Komodo network, used only to reach Komodo Core; and
- a separate ingress network, used only by the MCP gateway.

The gateway does not join Komodo's network, and the MCP service publishes no
host port or public reverse-proxy route. This keeps upstream credentials and direct Komodo
access out of the gateway container while preserving a single authenticated
MCP ingress.

## Secret flow

The operator's secret provider injects the read key/secret, admin key/secret,
and current or optional previous gateway bearer into the process environment
at startup. Infisical and Compose are optional deployment choices.
Secret values do not appear in the Compose file, Komodo stack configuration,
command arguments, logs, or health output. Host users with Docker inspection
access remain inside the trusted infrastructure boundary and can inspect a
container's environment.

An explicitly authorized secret-read tool may return a stack-specific value to
its caller. The value must not be copied into MCP or gateway logs, errors,
metrics, approvals, or audit payloads. An inherited Core secret is reported as
unavailable because the existing Komodo API does not expose its effective
value; the MCP server does not bypass Komodo through Infisical or direct
control-plane access.

Values sent to typed Komodo write APIs may be retained by Komodo in its own
traces or operation history. That upstream behavior is disclosed in affected
tool descriptions and approvals. The MCP server and gateway do not create
additional payload copies.

## Failure semantics

Idempotent reads retry once only on an unavailable transport. Mutations are
sent exactly once. An unavailable response after a mutation is an ambiguous
outcome. When no receipt is available, the error names the stable target and
the relevant read tool without echoing submitted values. Follow the
[operation-specific reconciliation guide](reconciliation.md) instead of
replaying the intent. Upstream response bodies are never returned as errors.
