# Native credential file transport

The HTTP service provides bounded native file transfer for credential bytes
inside the verified runtime boundary. Tool-specific credential contracts are
documented with the tools that use this transport.

## Native transport boundary

The HTTP transport implements `files/authorizeUpload` and
`files/authorizeDownload`. These MCP requests require the same rotating gateway
bearer and verified identity JWT as other `/mcp` requests. File ownership comes
from the verified subject, never a tool argument or claimed file owner. Requests
declare file support in `_meta.io.modelcontextprotocol/clientCapabilities.files`.

Native authorization returns a bounded HTTP PUT or GET descriptor for the
verified request host. A trusted runtime consumes its short-lived transfer
header directly; the descriptor and bytes must stay outside model context.
The private service uses HTTP, matching its deployed control plane; the gateway
governs the permitted network destination and transport. Forwarded headers and
model-supplied URLs do not select a transfer destination.

The byte endpoints accept a fresh, direction-specific capability grant. Uploads
are immutable after completion and optional declared size and SHA-256 digest
are checked before completion. A tool consumes a completed upload once, and a
download consumes its delivery copy once. Reauthorizing a download replaces the
previous grant. Foreign, incomplete, expired, or consumed files are unavailable.
Credentials and grants never enter diagnostic output or tool error messages.

The ephemeral in-memory store admits bounded entries and total bytes. Uploads
retain the existing 256 KiB write bound; downloads are bounded to 2 MiB. Files
expire after five minutes, grants after one minute, and expired storage is
cleared periodically and on shutdown. Stored byte buffers are zeroized when
discarded. The byte routes share the MCP request concurrency, deadline, and
shutdown limits. Native downloads return `Cache-Control: no-store` and declare
the credential file's `secret` retention hint to the gateway.

Other transports need a native file adapter to carry credential values. Their
absence is reported explicitly; it does not cause tokens to be returned inline
or remove ordinary read capabilities.
