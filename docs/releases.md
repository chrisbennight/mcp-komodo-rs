# Builds and releases

The public project name is `mcp-komodo-rs`. The binary and existing Rust crate
names are unchanged; containers still run `/komodo-mcp-rs`.

## Validation

GitHub Actions runs the Rust and documentation checks listed in
[the development instructions](../README.md#development). Pull requests then
build the source image and check its liveness endpoint without access to Komodo
or the gateway. The smoke container has no network, published port, or real
credentials. A healthy result confirms liveness, not upstream connectivity.

Run the image check locally with Docker available:

```sh
bash scripts/test_image.sh
```

CI uses public dependency sources. The Dockerfile still accepts an optional
`CRATES_INDEX_URL` for operators with a Cargo mirror, but GitHub CI does not
require one. Do not put credentials in a mirror URL or Docker build argument.

## Container publication

In `chrisbennight/mcp-komodo-rs`, pushes to `main` and version tags run the
source checks, build the image, and smoke it before publishing to
`ghcr.io/chrisbennight/mcp-komodo-rs`. The publication job alone has
`packages: write`; its registry login receives the job's `GITHUB_TOKEN` through
stdin. Pull requests, forks, and manual validation runs do not publish.

| Source | Image tags |
| --- | --- |
| `main` push | `sha-<full commit SHA>` and `latest` |
| `vMAJOR.MINOR.PATCH` tag | `sha-<full commit SHA>` and the exact version tag |
| Prerelease such as `v0.2.0-rc.1` | `sha-<full commit SHA>` and the exact prerelease tag |

Version tags cannot contain build metadata (`+...`) because Docker tags do not
accept it. Version-tag builds do not move `latest`. Use a registry digest for
an immutable deployment reference; a tag alone is not an immutability guarantee.
Publishing several tags is not atomic. If a push fails, check the registry and
workflow before rerunning it rather than assuming nothing was published.

The current workflow builds `linux/amd64` on a GitHub-hosted Ubuntu runner.
Other platforms, release binaries, SBOMs and provenance attestations need
separate implementation and verification.

## Repository setup and publication prerequisites

Workflow files do not configure repository permissions or protection. Before
merging, require the `test`, `image`, and `pr-review/gate` checks on the current
pull-request head. Confirm their displayed names after the first run. Install
and authorize the AERB GitHub App for review status publication; the checked-in
review policy is in `.github/pr-review`. Keep dependency update tooling enabled
for the repository and assess advisory and dependency-review coverage separately.

The former private dependency threat-gate action is not part of GitHub CI.
Removing its unavailable integration does not establish equivalent public
advisory or malware scanning. That replacement remains a release prerequisite.

Repository visibility and GHCR package visibility are separate settings. Keep
both private during preparation. Before advertising a public image, deliberately
publish the package and verify that a user with no registry credentials can
pull the documented digest. Complete the source/dependency review, security
reporting setup and runnable onboarding instructions before a public release.
