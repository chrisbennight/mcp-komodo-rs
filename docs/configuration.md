# Configuration reference

Settings are read from the process environment at startup. The server does not
read `.env` or secret files. Use your service manager or secret provider to
supply values without putting credentials in command arguments or source files.
Restart the process after changing configuration.

## Select a profile

Start with `--stdio` for local status tools. This mode loads only the read key,
read secret, upstream URL, and upstream timeout. It opens no listener and does
not load admin credentials or gateway settings. The parent MCP client owns
access to the process; see the [quickstart](quickstart.md).

Starting without a mode flag selects the full HTTP gateway profile. All settings
marked required below apply to that profile. Listener and gateway variables do
not change stdio limits.

| Capability | Local status-only stdio | Full HTTP gateway profile | Access boundary |
| --- | --- | --- | --- |
| Ordinary status and search | Available | Available when admitted by the gateway | Restrict the read service user to the intended resources. |
| Diagnostics, logs, configuration, Compose, environment, and commands | Excluded, including direct calls by name | Separately named sensitive reads | Gateway authorization must permit the specific sensitive capability. |
| Custom webhook secret | Excluded | Governed secret read/write tools | Secret values and their source/presence metadata have different exposure rules. |
| Deploy, restart, stop, build, cancel, and repository pull | Excluded | Named mutation tools | Gateway `komodo-admin` membership and applicable approval policy. |
| Configuration, content, file, and webhook writes | Excluded | Typed partial intents | Administrative upstream identity, gateway authorization, and consequential-write approval. |

HTTP defaults to the full catalog. Select a smaller process-wide catalog with
`--tool-profile status`, `--tool-profile read-only`, or `--tool-profile operations`:

| HTTP tool profile | Included capabilities |
| --- | --- |
| `status` | Ordinary status/search and webhook-source metadata; no sensitive content or writes. |
| `read-only` | All reads, including separately governed logs, configuration, and custom secrets. |
| `operations` | Ordinary reads plus deploy, restart, stop, build, cancel, and repository pull. |
| `full` | Every typed read, action, and configuration write. |

Excluded tools are hidden from discovery and rejected when called directly.
Profiles limit the whole process; they do not replace user authorization or
approval policy. HTTP startup still requires the documented separate read/admin
credentials for every profile. Local stdio always keeps its status-only boundary
and cannot be combined with `--tool-profile`.
HTTP always requires both the rotating gateway bearer and a verified identity
JWT. A visible tool or an MCP annotation does not itself grant permission.
For a configuration task, select the intended [write consequence](tool-surface.md#choose-the-intended-configuration-effect)
before changing values or deciding to deploy.

## Komodo connection

| Variable | Default | Contract |
| --- | --- | --- |
| `KOMODO_MCP_UPSTREAM_URL` | `http://core:9120/` | Core base URL, with no path other than `/`, user information, query, or fragment |
| `KOMODO_MCP_READ_API_KEY` | Required | Read service-user API key |
| `KOMODO_MCP_READ_API_SECRET` | Required | Read service-user API secret |
| `KOMODO_MCP_ADMIN_API_KEY` | Required in HTTP mode | Separate administrative service-user API key |
| `KOMODO_MCP_ADMIN_API_SECRET` | Required in HTTP mode | Administrative service-user API secret |
| `KOMODO_MCP_UPSTREAM_TIMEOUT_SECONDS` | `20` | Integer, 1–120 seconds per upstream request |

Credential values must contain 1–16384 UTF-8 bytes and no CR or LF. They are
preserved exactly, including spaces. HTTPS is supported. Plain HTTP is accepted
only for loopback/private IP literals, `localhost`, or single-label hosts such
as `core`. DNS names are classified by spelling, not resolved to prove network
isolation. Use HTTPS across untrusted networks. Redirects and environment proxy
settings are not used for Komodo or JWKS requests.

## Gateway authentication

| Variable | Default | Contract |
| --- | --- | --- |
| `KOMODO_MCP_GATEWAY_BEARER_CURRENT` | Required | 32–16384 UTF-8 bytes, no ASCII whitespace |
| `KOMODO_MCP_GATEWAY_BEARER_PREVIOUS` | Absent | Optional rotation bearer, same bounds and distinct from current; empty means absent |
| `KOMODO_MCP_IDENTITY_JWKS_URL` | Required | Ed25519 JWKS URL, HTTPS or the private/loopback HTTP forms above; no user information, query, or fragment |
| `KOMODO_MCP_IDENTITY_ISSUER` | Required | Nonempty exact JWT issuer, trimmed during configuration loading |
| `KOMODO_MCP_IDENTITY_ACTOR` | Required | Exact `act.sub`, 1–256 bytes, no control characters or leading/trailing whitespace; never normalized |

JWKS fetches have a fixed three-second timeout, five-minute cache lifetime, and
64 KiB response limit. Identity-token timing and claim requirements are in
[the architecture](architecture.md#request-path). A previous bearer remains
valid until removed from configuration and the process restarted. Generate
bearers with a trusted secret manager or platform cryptographic random generator.
The example placeholders are not credentials to deploy.

## Listener and request limits

| Variable | Default | Contract |
| --- | --- | --- |
| `KOMODO_MCP_HOST` | `0.0.0.0` | Listen IP; the executable parses a socket address, not a DNS name |
| `KOMODO_MCP_PORT` | `8000` | Integer, 1–65535 |
| `KOMODO_MCP_ALLOWED_HOSTS` | `komodo-mcp,komodo-mcp:8000,localhost,127.0.0.1` | Comma-separated exact HTTP Host values, including a port when sent |
| `KOMODO_MCP_ALLOWED_ORIGINS` | Empty | Comma-separated allowed Origin values; absent Origin is accepted, any supplied Origin must match |
| `KOMODO_MCP_REQUEST_TIMEOUT_SECONDS` | `30` | Integer, 1–120 seconds |
| `KOMODO_MCP_MAX_CONCURRENT_REQUESTS` | `32` | Integer, 1–256 |
| `KOMODO_MCP_MAX_BODY_BYTES` | `1048576` | Integer, 1024–4194304 bytes |
| `KOMODO_MCP_LOG_LEVEL` | `info` | Tracing EnvFilter expression restricted to the `komodo_server::diagnostics` target; cannot enable dependency logs or protocol payloads |

Host and Origin lists are trimmed, lowercased, and empty entries removed. Host
matching is case-insensitive; incoming Origin is compared exactly to the
normalized list. List values are not wildcards. Optional numeric values use
their defaults when absent or blank. Most ordinary string settings are trimmed;
credential values and the identity actor are not.

HTTP diagnostics are JSON on stderr. At `info`, they report listener startup;
`debug` and `trace` additionally report HTTP completion status and duration.
Only the dedicated diagnostic target is eligible for output. Filter directives
such as `rmcp=trace` cannot enable SDK messages, request URLs or headers, tool
arguments, or upstream/result content. `off` disables diagnostics. Stdio installs
no diagnostic subscriber and reserves stdout for protocol responses.

The HTTP request deadline starts at admission, before identity verification and
body parsing, and covers resource resolution, read retries, and tool execution.
Each upstream attempt also has its own timeout; it cannot extend that deadline.
The concurrency limit includes detached MCP handlers until their work ends.
Excess requests receive HTTP 503 without an unbounded waiting queue.

Expiry or a dropped HTTP request cancels local handler work. A deadline check
immediately before administrative submission prevents a completed resolution
read from starting a write after cancellation. Cancellation cannot undo a write
already submitted to Komodo; even an HTTP 408 or disconnect can leave an uncertain
outcome. Do not repeat it automatically. See [write reconciliation](reconciliation.md).

HTTP mode handles SIGINT and, on Unix, SIGTERM. Shutdown stops admission, cancels
local requests, and allows up to five seconds for HTTP connections to drain.
`/readyz` returns HTTP 503 during shutdown; `/healthz` remains an independent
liveness endpoint. Readiness indicates whether this process accepts work, not
whether Komodo is healthy.

## Command-line modes

Mode flags are mutually exclusive:

- `--stdio` serves the local status profile over stdin/stdout. Frames are limited
  to 64 KiB, with at most 32 pending requests. Initialization and output each
  have a 30-second deadline. See [stdio limits](quickstart.md#limits-and-failures).
- `--emit-tools-json` prints the full standard MCP `tools/list` catalog without
  loading runtime credentials. Add `--tool-profile` to inspect the selected
  capability catalog. It includes schemas and annotations.
- `--emit-gateway-contract-json` prints source coverage and classification
  expectations for the [deployment validator](gateway-setup.md#validate-deployed-catalog-coverage-and-classification), without runtime credentials.
- `--emit-gateway-manifest` prints a gateway-specific connection and tool-risk
  scaffold without runtime credentials; `--tool-profile` selects the same subset
  as HTTP discovery. It contains no authorization or approval
  policy and needs gateway-computed behavior hashes before publication; see
  [gateway deployment](gateway-setup.md#configure-the-gateway-and-server).
- `--healthcheck` loads only host and port, performs a local `/healthz` request
  with a two-second deadline, and exits. It does not authenticate to the gateway
  or contact Komodo. It is for HTTP mode, not stdio.

`--help` and `--version` are also available.
