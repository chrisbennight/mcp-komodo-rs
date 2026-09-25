# Builds and releases

The public project name is `mcp-komodo-rs`. The binary and existing Rust crate
names are unchanged; containers still run `/komodo-mcp-rs`.

## Validation

GitHub Actions runs the Rust and documentation checks listed in
[the development instructions](../CONTRIBUTING.md#build-and-check). Pull requests then
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
`packages: write`, `id-token: write`, and `attestations: write`; its registry login
receives the job's `GITHUB_TOKEN` through stdin. Pull requests, forks, and manual
validation runs do not publish.

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
Other platforms and standalone release binaries are not published yet.

The test job also packages Rust dependency license notices, including nested
vendor notices, from the locked Linux graph. It retains
`dependency-notices.zip`, its index, and a SHA-256 checksum for 90 days under
`dependency-notices-<source SHA>`. Missing notices fail the test job before
publication. Reviewed exact-version overrides cover crate archives that omit
their license files; see [notice provenance](../licenses/README.md).

## Dependency and image evidence

The `dependencies` job runs Cargo Deny against the supported Linux target,
including development dependencies. It checks RustSec advisories, licenses,
and source registries. Vulnerability exceptions must name the advisory and
record why the project is unaffected or what bounded risk is accepted; there
are no blanket advisory exceptions. Duplicate transitive crate versions are
warnings, since forcing dependency unification can break upstream contracts.
The license allowlist covers the locked target's permissive and Unicode data
licenses. It is a review policy, not a legal opinion or automatic notice delivery.

Dependabot manages Cargo, Dockerfile and GitHub Actions updates through
`.github/dependabot.yml`; it replaces the private deployment's Renovate setup.
Review updates through the same tests and AERB gate. Scanner pins in shell
scripts need a reviewed manual update. RustSec and Trivy detect known findings;
they do not certify the absence of malicious dependencies.

After the image smoke test, `scripts/scan_image.sh` runs a digest-pinned Trivy
scanner without exposing the Docker socket. It produces separate CycloneDX
inventories for the image's OS packages and the source Cargo lockfile. The
stripped Rust binary does not expose all crate metadata to the image scanner;
the lockfile inventory describes resolved source dependencies, including entries
that may not be linked into the supported target. Cargo Deny supplies the
target-aware Rust advisory gate. High and critical container findings fail the
image job, including findings without an available fix. Review lower-severity
findings in the full image SBOM; do not silently turn off the gate to publish.

Actions retains the JSON evidence as `image-evidence-<commit SHA>` for 90 days.
`build-record.json` records the checkout commit/tree, dirty-state flag, local
image configuration digest, and report hashes. It is unsigned build evidence,
not a signed provenance attestation or proof that a local image was built from
that checkout. On CI the reviewed workflow binds the sequence: build, smoke,
scan, then publish. The image configuration digest is not the registry manifest
digest used for pulls. Publication resolves each pushed tag through Docker Buildx,
reads the resulting immutable manifest, and checks that its configuration digest
matches the tested local image. Different tag digests or an unexpected image fail
verification without repeating a push. A verification failure can follow a
successful push; inspect the registry before rerunning the workflow.

The publication job uses the pinned official `actions/attest` action to sign
provenance for that verified registry digest, plus a separate attestation of the
image OS inventory. GitHub stores these attestations in its API. This repository
is public and qualifies under current GitHub plans; signatures use public
Sigstore infrastructure and expose build identity metadata. Private/internal
repositories require Enterprise Cloud. See [GitHub's attestation contract](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations).

The action does not push signatures to GHCR or create organization storage
records. It retains the signed bundles for independent verification. After
downloading dependency notices from the same workflow's test job, the publication
job assembles `release-evidence.zip` and its SHA-256 checksum. The bundle contains
the original scan reports, Rust notices and index, signed provenance/SBOM bundles,
and a release record binding their hashes to the source commit and immutable
image reference. Packaging rejects mismatched source, scan, notice, or attestation
subject data. Its subject check is not cryptographic signature verification.

`release-evidence-<source SHA>` is retained in Actions for 90 days. Attach that
bundle and checksum to the durable version release before advertising it.
Provenance signatures describe the build; they do not certify absence of
vulnerabilities or make the source lockfile inventory a list of linked crates.

## Repository setup and publication prerequisites

Workflow files do not configure repository permissions or protection. Before
merging, require the `dependencies`, `test`, `image`, and `pr-review/gate` checks on the current
pull-request head. Confirm their displayed names after the first run. Install
and authorize the AERB GitHub App for review status publication; the checked-in
review policy is in `.github/pr-review`. Keep dependency update tooling enabled
for the repository and assess advisory and dependency-review coverage separately.

The former private dependency threat-gate action is replaced by the public
checks above. Their advisory and license coverage does not reproduce private
malware heuristics or replace maintainer review.
The [initial review record](security-review.md) records the preparation
snapshot's license assessment and individual container-finding dispositions.

Repository visibility and GHCR package visibility are separate settings. The
source repository is public; this does not prove public image access. Before
advertising a public image, verify that a user with no registry credentials can
pull the documented digest. Complete the source/dependency review, security
reporting setup and runnable onboarding instructions before a public release.

## Release record

Before creating a version tag, use a reviewed main commit, verify its checks,
and prepare release notes describing user-visible changes, compatibility,
configuration changes, and known limitations. A first release must identify
the supported Linux container target and available access profiles accurately;
do not imply untested operating systems or clients are supported.

After the tag workflow succeeds, record the registry manifest digest and exact
source commit in the release notes. Download `release-evidence-<source SHA>`,
verify `sha256sum -c release-evidence.sha256`, and attach its archive and checksum
to the durable GitHub release record before advertising it. Actions retention is
temporary. Use the official verifier against the immutable image reference:

```sh
gh attestation verify oci://ghcr.io/chrisbennight/mcp-komodo-rs@sha256:<manifest-digest> \
  --repo chrisbennight/mcp-komodo-rs
```

The retained Sigstore bundles also support [offline verification](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations-offline).
Review the full OS report and base-image distribution obligations in addition
to Rust notices. Actual publication and signature verification must succeed for
the selected release; merging workflow source alone does not establish that.
