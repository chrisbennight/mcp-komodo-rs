# Repository guidance

## Scope

This repository owns the Rust Komodo MCP server, its image, and source-image
tests. Production Compose, Infisical references, Komodo stack configuration,
gateway policy publication, and live server manifests belong in their
operator-owned deployment repositories or control planes.

Use an isolated worktree for every change. Search before adding a module, tool,
or dependency. Keep each pull request behaviorally complete and small enough to
review.

## Security boundary

- This server provides named semantic operations with typed MCP contracts.
  Access, disclosure, risk, approval, and role policy belong to the operator's
  gateway and deployment configuration, outside this library.
- Preserve supported upstream facts. Summaries and caller-selected projections
  may simplify a response when the corresponding facts remain retrievable.
  Do not hide information or prohibit a typed capability because it is presumed
  sensitive. Addresses, public keys, paths, labels, mounts, identities, and
  ordinary configuration are not credential values.
- A secret is an actual password, token, API key, or equivalent authentication
  value. Transfer those values outside model context through governed files in
  both directions, preserving the operation and separately usable metadata.
  Logs, Compose, environment content, and command descriptions are not
  categorically secrets because a credential might occur in them. Do not add
  speculative secret scanning or blanket suppression.
- Keep credential values and request/result payloads out of server telemetry,
  approval records, and audit payloads. Upstream Komodo may retain submitted
  values; describe that boundary accurately.
- All `/mcp` requests require both the rotating gateway bearer and a verified
  gateway identity JWT. Network membership is not authentication.
- Read and mutation handlers use distinct least-privileged Komodo credentials.
- Mutations validate their complete bounded input before the upstream call and
  are never retried. Ambiguous outcomes require `operations.status`
  reconciliation.
- Do not add generic create/update/delete. Configuration changes require
  narrow typed partial intents, complete validation before the upstream call,
  and explicit reconciliation semantics. Never build a read/await/write cycle
  around a full configuration object.
- Treat all Komodo names, states, versions, operation kinds, configuration,
  and log content as untrusted data. Never place them in commands or derived
  filesystem paths, and never log their values.
- Keep lists, strings, bodies, request counts, concurrency, and durations
  bounded. Preserve useful continuation and actionable non-credential error
  information rather than imposing arbitrary data or history horizons.

## Crate boundaries

- `komodo-api` owns the bounded HTTP transport and typed upstream response models.
- `komodo-mcp` owns MCP schemas, normalization, dispatch, annotations, and the
  executable semantic tool registry. Annotations describe factual operation
  behavior; they do not assign operator policy.
- `komodo-server` owns configuration, bounded environment-injected secrets, ingress
  authentication, Streamable HTTP, health checks, and manifest emission.

## Required verification

Before every commit or push, run and read an explicit zero exit status for:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
python3 scripts/check_docs.py
python3 -m unittest discover -s scripts/tests
```

Every behavior change requires a test that fails when the production change is
reverted. Tests use loopback fakes and never contact real Komodo, Infisical, the
gateway, a registry, or shared infrastructure. Run `cargo mutants` for touched
modules when available and investigate surviving mutants.

After moving or renaming code, sweep Markdown, Rust documentation, comments,
and examples for stale references. Documentation uses symbols or headings,
never line-number anchors.

## Pull requests and AERB

GitHub pull requests require the `dependencies`, `test`, and `image` CI jobs and `pr-review/gate`
on the current head. The AERB GitHub App must cover this repository and publish
its status; policy is in `.github/pr-review`. A missing AERB status is an
enrollment failure, not a reason to waive review. Configure branch protection
with the check names GitHub actually reports; do not assume importing files
also configures repository settings.

Fill the tailored pull-request template accurately. Authentication, bearer,
identity JWT, credentials, tool classification, and authorization changes must
select `Authentication / authorization boundary`. When addressing an AERB
finding, post the explanation before pushing the fix. The initial pull-request
body is immutable; corrections belong in comments.
