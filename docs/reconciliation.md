# Reconciling a write

Never automatically repeat a write after a timeout, disconnected response,
oversized response, or incompatible response. Komodo may have accepted it even
when this server cannot return a receipt. A successful transport response also
does not prove a queued operation has completed.

## Receipt available

Execution tools and `stacks.file.write` normally return an `operationId`.
Call `operations.status` with `{"operation_id":"<operationId>"}`. The lookup
uses that stable ID. Check the completion status and success flag, not just
the presence of a receipt. Read operation logs only through the separately
authorized sensitive tool when needed.

## No receipt available

An uncertain upstream mutation error includes `outcome: "unknown"`,
`retrySafe: false`, the resolved stable `target`, and a `reconcileWith` tool
name. It never includes submitted configuration, paths, commands or secrets.
The error also sets `operatorVerificationRequired: true`: the suggested read
is evidence for an operator, not an automatic authorization to retry.
Failure while resolving a selector occurs before the write and does not carry
this uncertain-mutation marker.

| Write | Evidence to inspect when no receipt is available |
| --- | --- |
| Stack deploy/restart/stop | `operations.search`, then `operations.status` for a positively identified receipt; `stacks.status` for current resource state |
| Deployment deploy/restart | Operation search/status and `deployments.status` |
| Build run/cancel | Operation search/status and `builds.status` |
| Repository pull | Operation search/status and `repos.status` |
| `stacks.file.write` | Operation search/status and operator inspection of the intended file in Core; no generic filesystem read is exposed |
| `stacks.config.patch` | `stacks.config.read` gives shape/presence metadata; an operator must check exact structural values through Core when that metadata is insufficient |
| `stacks.compose.write` | Authorized `stacks.compose.read` with the configured source; compare only the intended contents |
| `stacks.environment.write` | Authorized `stacks.environment.read`; compare only the intended environment |
| `stacks.commands.write` | Authorized `stacks.commands.read`; compare only the fields submitted |
| `stacks.webhook.update` | `stacks.webhook.status`; compare enabled/force-deploy flags |
| `stacks.webhook.secret.write` | Authorized `stacks.webhook.secret.read`; custom value comparison must not enter logs or audit records |

Use `target` as the `selector` for stack reads. `operations.search` filters
operation ID or kind, not target; its current metadata cannot prove which
same-kind operation belongs to the failed call. A timestamp match or a running
resource is not sufficient proof. If Core cannot establish the result, keep the
outcome unknown and have an operator decide the next intent.

Readback may contain sensitive information and requires its own authorization;
permission to write does not automatically permit reading secrets or logs.
Do not persist comparisons or submitted values in telemetry. Concurrent
same-field writes are last-writer-wins in the upstream partial-update API;
observing the intended value later does not prove which caller wrote it.
The server adds no local idempotency ledger and makes no exactly-once guarantee.
