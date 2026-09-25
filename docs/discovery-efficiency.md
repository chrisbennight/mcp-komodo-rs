# Discovery and result context

Choose a [capability profile](configuration.md#select-a-profile) for the process,
then use gateway discovery to load names and descriptions before requesting the
schemas of selected tools. The server caches immutable catalog definitions and
shares schema allocations. It never caches credentials, resource inventories,
or sensitive tool results in that catalog.

With the connected gateway, request `komodo.searchTools` in `operations` mode
with `detail: "nameDescription"` and a bounded limit. Load selected input/output
type handles in `types` mode. These are gateway discovery capabilities; the MCP
server still publishes standard `tools/list`. Discovery visibility and process
profiles do not replace execution authorization.

MCP results retain both structured JSON and compatibility text for supported
clients. A consuming client should select structured content when supported and
avoid sending its equivalent text to the model again. Removing compatibility
text in this server would change the supported-client contract.

Keep large governed content in gateway artifacts where that integration is
available. References must preserve the original sensitivity, caller access,
expiration, and audit rules; a URI alone is not an authorization mechanism.
This server does not create gateway artifacts or promise their retention.
Its typed content reads remain available with bounded upstream responses.

## Reproduce local measurements

Build the desired revision and use a temporary environment with the official
`tiktoken` package. The measurement script defaults to the tokenizer for
`gpt-4o-mini-2024-07-18`; it makes no model API request. Token counts describe
the exact exported text, excluding message framing and gateway wrappers, and
are not API billing measurements. See the [official token-counting guide](https://developers.openai.com/cookbook/examples/how_to_count_tokens_with_tiktoken).

```sh
cargo build --workspace --locked
python3 scripts/measure_catalog.py /absolute/path/to/komodo-mcp-rs
cargo run --locked -p komodo-mcp --example catalog_timing
```

The Python command reports full/profile catalog bytes, actual tokenizer counts,
name/description projection costs, and local process export latency. The Rust
example separately reports first construction and repeated in-process catalog
latency. Compare the same build profile, machine, and load; neither command
measures gateway transport latency or proves model selection accuracy. Fixed
task regressions cover status lookup, a sensitive environment read, stopping a
stack, and writing its environment under the intended profiles. Client result
deduplication and artifact access checks require their owning integration tests.
