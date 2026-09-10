# Security policy

Report vulnerabilities privately to the repository owner. Do not include live
credentials, tokens, Komodo configuration, logs, or Docker runtime data in an
issue, pull request, test fixture, review comment, command argument, or
diagnostic output.

Security-sensitive surfaces include:

- `crates/komodo-api`, which holds upstream credentials and normalizes Komodo
  responses;
- `crates/komodo-mcp`, which defines model-visible schemas and mutation
  annotations;
- `crates/komodo-server/src/auth.rs`, which authenticates the gateway and
  caller identity;
- `Dockerfile` and `.gitea/workflows`, which form the supply-chain boundary;
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
