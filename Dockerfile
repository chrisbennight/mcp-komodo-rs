# syntax=docker/dockerfile:1.27@sha256:bde3983e9c939224420ddaf6b784cc30e09b035a4dea01f581230c50809f372e

ARG RUST_VERSION=1.96.0

FROM rust:${RUST_VERSION}-slim-bookworm@sha256:4732ca96fd086cb9be682050c3f0176288eebaac2b80aa2bcefccfaf198e1950 AS builder
WORKDIR /app

# Where crates come from. This stage reaches the network, and the fleet wants
# that traffic through its caching proxy - but Docker gives a RUN step an
# environment built from this file rather than the caller's, so the address has
# to arrive as a build argument.
#
# Cargo reads no environment variable for a mirror, so unlike pip or npm it
# cannot simply be told: the redirect has to be a config file, written from this
# value below. The name stays outside cargo's own CARGO_ namespace on purpose -
# cargo maps CARGO_REGISTRY_INDEX onto its removed registry.index key and aborts
# every invocation, and an ARG is visible to RUN as an environment variable, so
# that spelling would break the build it was meant to route.
#
# Left unsupplied it stays unset, no config file is written, and cargo resolves
# from crates.io. That fallback is what keeps this image buildable away from the
# network the proxy lives on.
ARG CRATES_INDEX_URL

COPY . .

# Source replacement rather than an additional registry: it redirects the
# existing crates.io source instead of introducing a second one, so Cargo.lock
# goes on naming crates-io and a lock produced here still resolves from the
# public index.
#
# Appended rather than written, so whatever else .cargo/config.toml carries is
# preserved. The repository's copy deliberately no longer pins a source: a
# committed mirror address would be a hard dependency on a host that resolves
# only on the LAN, which is exactly what this argument exists to avoid.
RUN if [ -n "${CRATES_INDEX_URL}" ]; then \
      mkdir -p .cargo && \
      printf '\n[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "%s"\n' \
        "${CRATES_INDEX_URL}" >> .cargo/config.toml; \
    fi
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked --bin komodo-mcp-rs \
    && cp target/release/komodo-mcp-rs /usr/local/bin/komodo-mcp-rs

# Preserve the vetted GCC support library without shipping unused TLS libraries.
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f AS runtime-libraries

FROM gcr.io/distroless/base-nossl-debian12:nonroot@sha256:be40c00dfabd86576d92666e87e406714d5618342de1a0c213ad232de255172e AS runtime
COPY --from=runtime-libraries /lib/x86_64-linux-gnu/libgcc_s.so.1 /lib/x86_64-linux-gnu/libgcc_s.so.1
COPY --from=runtime-libraries /var/lib/dpkg/status.d/libgcc-s1 /var/lib/dpkg/status.d/libgcc-s1.md5sums /var/lib/dpkg/status.d/gcc-12-base /var/lib/dpkg/status.d/gcc-12-base.md5sums /var/lib/dpkg/status.d/
COPY --from=runtime-libraries /usr/share/doc/gcc-12-base/ /usr/share/doc/gcc-12-base/
COPY --from=builder /usr/local/bin/komodo-mcp-rs /komodo-mcp-rs

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
    CMD ["/komodo-mcp-rs", "--healthcheck"]

ENTRYPOINT ["/komodo-mcp-rs"]
