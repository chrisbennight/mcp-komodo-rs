# Local status-only quickstart

Use `--stdio` when a local MCP client should inspect Komodo status without a
gateway. The client starts the server as its child process and owns access to
its stdin, stdout, and environment. No HTTP listener is opened in this mode.
Use a read-only Komodo service user restricted to the resources you intend to
make visible to that client. Status names and values still reveal operational
information and should be treated as untrusted data by the model.

## Build

Install Git and Rust through rustup, then:

```sh
git clone https://github.com/chrisbennight/mcp-komodo-rs.git
cd mcp-komodo-rs
cargo build --release --locked -p komodo-server
```

The toolchain file selects the supported Rust version. During preparation,
repository access is required. The resulting binary is
`target/release/komodo-mcp-rs`; use its absolute path in your client.

## Connect a client

Configure a stdio MCP server using your client's command/argument interface:

```json
{
  "mcpServers": {
    "komodo-status": {
      "command": "/absolute/path/to/komodo-mcp-rs",
      "args": ["--stdio"]
    }
  }
}
```

`mcpServers` is a common client configuration shape; consult your client's
documentation for its file location and secret-provider interface. Supply
`KOMODO_MCP_READ_API_KEY`, `KOMODO_MCP_READ_API_SECRET`, and
`KOMODO_MCP_UPSTREAM_URL` to the child process through that interface. The
upstream URL is the Core base URL, for example `https://komodo.example.com/`.
Do not paste credentials into the command, arguments, or a committed JSON file.
The server does not load dotenv files itself.

No administrative key, gateway bearer, issuer, actor, or JWKS URL is needed.
The HTTP gateway profile remains available by starting the executable without
`--stdio`; its authentication requirements are unchanged. Never bridge local
stdio to an unauthenticated network listener.

## First call

Connect the client, initialize MCP, and list tools. Call `system.status` with
`{}`. A result with `reachable: true` and the Core version confirms the process
can authenticate to the upstream read API. Then try `stacks.search` with
`{"limit": 10}`. Sensitive configuration, logs, diagnostics, secrets, and all
writes are absent from this profile and rejected even if called by name.

The integration test exercises the real binary through stdin/stdout, performs
initialization and tool discovery, calls status against a loopback fake Core,
and checks that sensitive and mutating tools are denied:

```sh
cargo test --locked -p komodo-server --test stdio
```

This proves the protocol path without touching a live Komodo installation. It
does not certify a particular third-party client's configuration UI or a newer
Core version. See [compatibility](compatibility.md).

## Limits and failures

Input uses newline-delimited JSON with at most 64 KiB per frame, including its
newline. Malformed frames, duplicate pending request IDs, or more than 32
pending requests close the connection. Partial frames survive normal task
cancellation. Initialization and output each have a 30-second deadline.
Upstream requests use `KOMODO_MCP_UPSTREAM_TIMEOUT_SECONDS` (default 20, range
1–120 seconds); an unavailable read may retry once. No mutation is available.

EOF on stdin ends the process. stdout contains only protocol messages.
Diagnostics are deliberately limited to static startup failures on stderr;
peer input and credential values are not logged. A startup failure usually
means missing read credentials, an invalid upstream URL, or an incomplete MCP
initialization. Correct the client configuration and restart the child process.
