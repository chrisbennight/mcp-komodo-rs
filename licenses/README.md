# Dependency notices

`python3 scripts/package_notices.py` collects license, notice, copying, and
copyright files from the locked Linux Cargo dependency graph, including nested
vendor files and build/test dependencies. It does not infer which dependencies
are linked into the binary. The ZIP includes an index with declared licenses,
file hashes, and the Cargo.lock hash. CI retains the ZIP, index, and checksum.

Some crate archives omit license files. `overrides.json` records copies from
their exact published source revision, obtained from `.cargo_vcs_info.json`.
Each override is tied to a package version and revision; an upgrade needs a
new review. The sse-stream manifest offers MIT or Apache-2.0 but supplies neither
text, so its bundle includes the canonical Apache-2.0 text and source attribution.
The rmcp upstream license describes a licensing transition; its entire notice
is preserved rather than replacing it with the manifest's shorter declaration.

This archive covers Rust package notices. It does not replace review of native
library obligations or the base image's distribution terms. Keep the source
SBOM, OS SBOM, image scan and source revision alongside it. Before a public
release, retain these files in a durable release archive and review the
[release prerequisites](../docs/releases.md#repository-setup-and-publication-prerequisites).
