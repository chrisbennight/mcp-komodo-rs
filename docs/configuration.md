# Configuration reference

Settings are read from the process environment at startup. The server does not
read `.env` or secret files. Use your service manager or secret provider to
supply values without putting credentials in command arguments or source files.
Restart the process after changing configuration.

## Komodo connection

| Variable | Default | Contract |
| --- | --- | --- |
| `KOMODO_MCP_UPSTREAM_URL` | `http://core:9120/` | Core base URL, with no path other than `/`, user information, query, or fragment |
| `KOMODO_MCP_READ_API_KEY` | Required | Read service-user API key |
| `KOMODO_MCP_READ_API_SECRET` | Required | Read service-user API secret |
| `KOMODO_MCP_ADMIN_API_KEY` | Required | Separate administrative service-user API key |
| `KOMODO_MCP_ADMIN_API_SECRET` | Required | Administrative service-user API secret |
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
| `KOMODO_MCP_LOG_LEVEL` | `info` | Valid tracing EnvFilter expression; keep production logging at `info` or stricter |

Host and Origin lists are trimmed, lowercased, and empty entries removed. Host
matching is case-insensitive; incoming Origin is compared exactly to the
normalized list. List values are not wildcards. Optional numeric values use
their defaults when absent or blank. Most ordinary string settings are trimmed;
credential values and the identity actor are not.

The HTTP request deadline and upstream timeout are independent. A timeout during
a write can leave an uncertain outcome; do not repeat the write automatically.
See [failure semantics](architecture.md#failure-semantics).

## Command-line modes

`--emit-gateway-manifest` prints the generated registry without loading runtime
credentials. `--healthcheck` loads only host and port, performs a local
`/healthz` request with a two-second deadline, and exits. It does not authenticate
to the gateway or contact Komodo. `--help` and `--version` are also available.
