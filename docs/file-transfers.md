# Native credential file transport

The HTTP service transfers bounded credential bytes outside model context.

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
