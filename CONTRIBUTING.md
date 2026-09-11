# Contributing

Start with an issue describing the user problem and an example of the desired
result. For a bug, include the server version, Komodo version, transport, and a
minimal reproduction using synthetic data. Follow [SECURITY.md](SECURITY.md)
for vulnerabilities; do not post a credential or a real configuration payload.

## Build and check

Install Rust through rustup, Python 3, and Git. The checked-in toolchain file
selects the supported Rust version and components. Docker is needed only for
the image test.

```sh
git clone https://github.com/chrisbennight/mcp-komodo-rs.git
cd mcp-komodo-rs
cargo build --workspace --locked
cargo run --locked -p komodo-server -- --help
```

The repository is private during preparation, so cloning currently requires
access. Cargo uses public dependencies; no lab registry is needed. Run all
[development checks](README.md#development) before submitting a pull request.
Use `bash scripts/test_image.sh` to build and smoke the container. Tests use
loopback fakes and synthetic credentials; never point them at a live deployment.

## Change design

Keep a change focused on one user-visible outcome. Add a regression test for a
behavior change and update the relevant documentation. Use plain language to
explain what happens, what callers must provide, and how failures are handled.
Include a concrete example where it clarifies the contract. Avoid promotional
claims or instructions that cannot be run from the documented prerequisites.

The [architecture](docs/architecture.md) explains the crate boundaries.
The [tool contract](docs/tool-surface.md) defines bounds, sensitivity, and
mutation semantics. New tools must have typed schemas, bounded responses,
accurate annotations, and an explicit authorization and reconciliation contract.
Do not turn an upstream API into a generic proxy or remove a governed operation
solely because its data is sensitive.

Fill the pull-request template with the problem, resulting behavior, relevant
checks, and remaining limitations. Review comments should explain the violated
contract or user impact. Maintainers review CI and AERB results on the current
head before merging. For editor/agent-specific rules see [AGENTS.md](AGENTS.md).
