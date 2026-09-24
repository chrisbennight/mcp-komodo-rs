# Visual identity

The **Stack** identity uses layered trays and a copper connection ring. It
represents the server's work with Komodo resources and shares the warm surfaces
and restrained geometry of Waygate and mcp-ssh-rs. The name remains
**mcp-komodo-rs**; command names and protocol identifiers are unchanged.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/header-dark.svg">
  <img src="assets/header-light.svg" width="960" alt="mcp-komodo-rs — Komodo operations for MCP clients">
</picture>

## Source of truth

The maintainer selected option 01 on 2026-09-23. The
[approved concept](reference/approved-concept.png) and
[image-generation prompts](reference/prompts.json) preserve that choice.
The built-in image generation tool produced the concept. Its approximate font
shapes, colors, and textures are inspiration; the editable sources and this
guide define the production identity.

[symbol.svg](symbol.svg) owns the mark geometry; [icons.svg](icons.svg) owns the
feature illustrations. [export.py](export.py) composes the layouts, applies the
palette, and outlines lettering from the bundled font. Update these sources
and regenerate, rather than editing exports or generating a new logo per page.

## Color and typography

| Color | Value | Role |
| --- | --- | --- |
| Ivory | `#F7F5F0` | Light background and lettering on dark |
| Ink | `#23201B` | Text, monochrome mark, dark background |
| Forest | `#1E5F46` | Mark on light backgrounds and feature illustrations |
| Copper | `#B66A45` | Connection ring, used sparingly |

Dark variants use ivory for the stack so its shape remains visible. Copper is
a decorative accent, not small text or a status indicator. Brand colors never
prove that an operation succeeded or was authorized. Pair operational state
with explicit words rather than relying on color.

Use **Manrope** at weight 700 for the wordmark and 500 for the short descriptor.
The unmodified variable font, its license, upstream Git blob, and SHA-256 are
stored in [fonts](fonts). SVG lettering is outlined so readers need no font
download. GitHub controls README body typography; prose and commands remain
ordinary Markdown and selectable text.

## Geometry and iconography

The symbol uses a 128-unit canvas. Leave at least 16 units of clear space
around the artwork; preserve proportions. Use the symbol alone where a full
wordmark would become too small. The avatar exports include space for circular
crops. Use the supplied single-ink variant when only one ink is available.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/capabilities-dark.svg">
  <img src="assets/capabilities-light.svg" width="720" alt="Outline icons for stacks, deployments, builds, and operations">
</picture>

Feature icons use a 24-unit grid, rounded line ends, and a common stroke weight.
Use them with visible labels. They illustrate capabilities; they are not
controls or promises that a caller has permission to invoke a tool. No browser
UI is implied by these assets. Informative standalone images need useful alt
text; purely decorative images beside equivalent text should use empty alt text.

## Surfaces and exports

| Asset | Use |
| --- | --- |
| `header-light.svg`, `header-dark.svg` | Shallow README and documentation headers |
| `wordmark-light.svg`, `wordmark-dark.svg` | Compact identity on mobile or in a documentation index |
| `symbol-light.svg`, `symbol-dark.svg`, `symbol-mono.svg` | Standalone marks on the named backgrounds |
| `avatar-light.png`, `avatar-dark.png` | Square identity images with crop space |
| `icon-light-16.png`, `icon-light-32.png`, dark equivalents | Small icon exports; inspect at actual size |
| `stacks.svg`, `deployments.svg`, `builds.svg`, `operations.svg` | Individual feature illustrations for light surfaces |
| `capabilities-light.svg`, `capabilities-dark.svg` | Labeled feature strip |
| `social-preview.svg`, `social-preview.png` | Editable source and opaque 1280 × 640 shared-link image |

Use [the asset directory](assets) for the generated files. Keep one compact
identity area in the README. Avoid gradients, glow, decorative dashboards,
badge walls, and text-heavy images. The purpose, next action, prerequisites,
and commands must remain readable without the images.

## Reproduce and verify

Install Python 3.11 or newer and [uv](https://docs.astral.sh/uv/), then run from
the repository root:

```sh
uv run docs/branding/export.py
uv run docs/branding/export.py --check
```

The script declares export-only dependencies on FontTools and resvg-py. These
are established font tooling and bindings for the resvg renderer, verified
against their registry and upstream documentation when selected. They do not
change the application build. The first run downloads these tools; subsequent
exports read the checked-in font and geometry and do not fetch remote assets.

Inspect headers on light and dark backgrounds and at mobile width, wordmark
spelling, single-ink appearance, actual 16/32-pixel icons, and circular avatar
crops. Check text contrast against its background, useful alt text, and image
byte size. `--check` detects stale exports; it does not replace visual review.
The shared-link PNG must stay below GitHub's 1 MB limit.

GitHub's social preview is a separate repository setting. Upload the PNG under
**Settings → Social preview** and verify the shared link; committing the file
does not update that setting. See [GitHub's requirements](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/customizing-your-repositorys-social-media-preview).

The original artwork and exporter use the project's [MIT license](../../LICENSE).
Manrope retains its [SIL Open Font License](fonts/OFL.txt); distribute the font
with that notice. Artwork does not imply endorsement by Komodo or another project.

## Writing and open-source guidance

The README introduces useful tasks, offers a first successful call, distinguishes
access profiles, and points readers to the complete reference material. Use
plain sentences, concrete examples, and verifiable claims. Explain limitations
where they affect the reader's next step. Keep internal implementation detail
in the contributor and architecture guides.

The guidance was refreshed through Kagi on 2026-09-23:

- [GitHub: About READMEs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes): purpose, usefulness, getting started, help, and maintainers.
- [Open Source Guides: Starting a project](https://opensource.guide/starting-a-project/): clear expectations, contribution guidance, licensing, and welcoming language as part of a project's identity.
- [Waygate design language](https://github.com/chrisbennight/waygate/blob/main/docs/design.md): restrained connection geometry, theme-aware assets, and factual introductory artwork.
- [mcp-ssh-rs visual identity](https://github.com/chrisbennight/mcp-ssh-rs/blob/main/docs/branding/README.md): warm surfaces, an independent mark, and a written design authority separate from generated concepts.

Public readiness also requires verified source and package visibility, a working
private security-reporting route, and durable release evidence. Those are
tracked in [builds and releases](../releases.md); visual polish does not establish
that they are complete. Contribution and support guidance state current routes
without inventing a governance body or response-time commitment.
