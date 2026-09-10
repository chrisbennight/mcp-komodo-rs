## Change type

- [ ] Tool contract / behavior
- [ ] Authentication / authorization boundary
- [ ] Komodo API compatibility
- [ ] Container image or runtime
- [ ] Workflow / CI
- [ ] Dependency update
- [ ] Documentation only

## Design goal

What this change achieves and why.

## Acceptance criteria

- Observable behavior that defines success.

## Change narrative

### What changed and why

Describe the code, schema, normalization, policy classification, and
documentation changes.

### Security and information flow

Describe caller authentication, gateway authorization, read/admin credential
selection, upstream requests, model-visible fields, and excluded sensitive
fields.

### Upstream assumptions

Record the targeted Komodo release and primary upstream references.

## Risk assessment

### Secrets and exposure

Explain changes to secrets, configuration/log/runtime-data exposure, PII, or
network reachability.

### Mutation semantics

Explain validation, idempotency, retry behavior, ambiguous outcomes, and the
`komodo-admin` gate. State `Not applicable` for read-only changes.

### Open questions

List unresolved assumptions or state `None`.

## Non-goals

State what this pull request deliberately leaves out.
