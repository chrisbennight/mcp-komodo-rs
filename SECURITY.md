# Security policy

Use GitHub's [private vulnerability report form](https://github.com/chrisbennight/mcp-komodo-rs/security/advisories/new)
when it is enabled for your account and this repository. If the form is
unavailable, do not create a public issue containing vulnerability details;
the maintainer must enable private reporting before public release. This route
has not yet been verified during migration.

Do not include live
credentials, tokens, Komodo configuration, logs, or Docker runtime data in an
issue, pull request, test fixture, review comment, command argument, or
diagnostic output.

Include the affected commit or release, the violated security boundary,
synthetic reproduction steps, and the observed impact. Do not run a proof of
concept against a deployment you do not own or administer. No response-time
guarantee or bug bounty is offered.

## Supported versions

During preparation, security fixes target the current `main` branch. There are
no supported stable release lines yet. Release tags and a supported-version
table will be added when stable releases are published; old snapshots should
not be assumed to receive backports.

Security-sensitive surfaces include:

- `crates/komodo-api`, which holds upstream credentials and normalizes Komodo
  responses;
- `crates/komodo-mcp`, which defines model-visible schemas and mutation
  annotations;
- `crates/komodo-server/src/auth.rs`, which authenticates the gateway and
  caller identity;
- `Dockerfile`, `.github/workflows`, and the image validation/publication
  scripts, which form the supply-chain boundary;
  and
- generated gateway classifications, which must remain exact with
  `tools/list`.

Sensitive MCP capabilities must use bounded typed schemas and accurate tool and
result annotations. The gateway, not the upstream server's annotations, owns
risk classification, authorization, approval, disclosure, and audit policy.
Annotations are security-relevant claims and disappearance or drift must fail
closed at the gateway.

Secret and sensitive values may cross the MCP response boundary only for an
explicitly authorized capability. They must never enter this server's logs,
errors, metrics, health output, or operation receipts. Komodo Core may retain
values submitted through its own API in traces or operation history; tools that
send such values must disclose that upstream limitation before approval.

The unauthenticated `/healthz` endpoint reports only service status and build
version and never calls Komodo. The `/mcp` endpoint is private, bearer-protected,
identity-verified, body-bounded, concurrency-bounded, and deadline-bounded.
