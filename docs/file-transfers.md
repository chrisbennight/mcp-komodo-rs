# Literal credentials and native file transfer

The semantic interface exposes credential operations while keeping literal
password and token values outside model context. Ordinary addresses, public
keys, configuration, Compose, environment, commands, and logs are not credential
values merely because they are sensitive or could contain one. The library does
not scan arbitrary content for presumed secrets.

## Webhook tokens

`stacks.webhook.secret.read` reports `source`, `valueAvailable`, and `file`.
For a custom token, `file` is native MCP metadata containing a URI, name, media
type, and size, without token bytes or transfer headers. For inheritance,
`valueAvailable` is false and `file` is null: Core's stack API does not return
its shared token.

`stacks.webhook.secret.write` accepts an explicit source choice:

```json
{
  "selector": "example-stack",
  "secret": {
    "source": "custom",
    "file": {"uri": "mcp-file://gateway/00000000-0000-0000-0000-000000000000"}
  }
}
```

The example URI is a placeholder. Use the exact URI returned by native upload.
The input declares `x-mcp-file` with upload mode, so the gateway delivers
caller-owned bytes and substitutes the upstream file reference. The completed
file must contain nonempty UTF-8 token text within the write-content byte bound.
Inline token arguments are rejected without a fallback.

To restore inheritance, supply `"secret": {"source": "inherited"}`. This sends
Core's inheritance representation without fetching its shared token. Neither
choice deploys the stack. Writes are never replayed; reconcile the source with
`stacks.webhook.status` and custom values through authorized native download.
Core may retain submitted tokens in its configuration and operation history.

## Configuration history

`operations.search`/`operations.status` return previous/current Core history.
Ordinary TOML stays exact inline. `toml` replaces known credential values;
`credentialFields` lists their paths; `originalFile` carries the exact original
outside model context as `operation-before.toml` or `operation-after.toml`
(fixed names).

Typed credentials include server/URL-builder passkeys, webhook tokens, URL
passwords, Slack/Discord webhook tokens, and Pushover application/user tokens.
Empty inheritance, addresses, account/secret-name references, environment,
Compose, and commands remain available. Custom/Ntfy text has no keyword or
query-name scan.

Both documents of every selected record are parsed before publishing files;
only the selected page publishes originals. Invalid TOML leaves operation
metadata available and reports `formatError` with a fixed message and numeric
location. Its `toml` is empty because no safe inline projection is available;
the exact unparsed original is file-only. Missing native support or capacity
sets `originalFileUnavailable`: ordinary facts remain available and credentials
stay file-only. Core may retain credentials in configuration and history.

## Native transport boundary

`files/authorizeUpload` and `files/authorizeDownload` require the gateway bearer
and verified identity JWT. The verified subject owns the file. Requests declare
HTTP file support in `_meta.io.modelcontextprotocol/clientCapabilities.files`.

Authorization returns an HTTP PUT or GET descriptor for the verified request
host. The trusted runtime keeps its short-lived header and bytes outside model
context. HTTP matches the private control plane; the gateway governs network
destinations. Forwarded headers and model-supplied URLs cannot select them.

The byte endpoints accept a fresh, direction-specific capability grant. Uploads
are immutable after completion and optional declared size and SHA-256 digest
are checked before completion. A tool consumes a completed upload once, and a
download consumes its delivery copy once. Reauthorizing a download replaces the
previous grant. Foreign, incomplete, expired, or consumed files are unavailable.
Credentials and grants never enter diagnostic output or tool error messages.

The in-memory store bounds entries and bytes, including active downloads. Uploads
retain the existing 256 KiB write bound; downloads are bounded to 2 MiB. Files
expire after five minutes, grants after one minute, and expired storage is
cleared periodically and on shutdown. Stored byte buffers are zeroized when
discarded. The byte routes share the MCP request concurrency, deadline, and
shutdown limits through response delivery. Downloads return `Cache-Control: no-store` and declare
the credential file's `secret` retention hint to the gateway.

Other transports need a native file adapter to carry credential values. Their
absence is reported explicitly; it does not cause tokens to be returned inline
or remove ordinary read capabilities.
