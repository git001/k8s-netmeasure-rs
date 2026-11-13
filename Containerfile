# k8s-netmeasure image (design §13). Build from the repository root:
#   podman build -f Containerfile --build-arg BUILD_GIT_SHA=$(git rev-parse --short=12 HEAD) .
# Base images are pinned by digest; update digest and comment together.

# ubi10/ubi 10.3-1794519911 (2026-11-12)
ARG BUILDER_IMAGE=registry.access.redhat.com/ubi10/ubi@sha256:73ea54bfbfb586075bd3d45df6483a7f68875233c8cc7cebcf9b2bb42306c308
# ubi10/ubi-minimal 10.3-1794499233 (2026-11-12)
ARG RUNTIME_IMAGE=registry.access.redhat.com/ubi10/ubi-minimal@sha256:fb2f7f857cc60d88e3fad19da505083a682ca4569d7b55a61f64aaa43641f741

# --- builder: UBI 10 fixes the glibc baseline (2.39) of the binary (design §1) ---
FROM ${BUILDER_IMAGE} AS builder

ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55

# gcc: linker for the gnu target (no C code is compiled).
RUN dnf -y install --setopt=install_weak_deps=False --nodocs gcc \
 && dnf clean all

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin

RUN curl --proto '=https' --tlsv1.2 -sSfL -o /tmp/rustup-init \
      "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
 && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
 && chmod 0755 /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none \
 && rm /tmp/rustup-init

WORKDIR /src
# The toolchain comes from rust-toolchain.toml; its own layer, cached across builds.
COPY rust-toolchain.toml ./
RUN rustup toolchain install

# cargo-auditable embeds the dependency list into the binary, so that syft (SBOM) and
# Trivy (scan) see the Rust crates, not only the RPMs.
ARG CARGO_AUDITABLE_VERSION=0.7.7
RUN cargo install --locked --version "${CARGO_AUDITABLE_VERSION}" cargo-auditable

COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY xtask xtask

ARG BUILD_GIT_SHA=unknown
RUN BUILD_GIT_SHA="${BUILD_GIT_SHA}" cargo auditable build --release --locked --package k8s-netmeasure \
 && ldd --version | head -n1

# --- runtime ---
FROM ${RUNTIME_IMAGE}

ARG VERSION=0.1.0
ARG BUILD_GIT_SHA=unknown
# Overrides the base image's own `created` label, which would otherwise be inherited.
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="k8s-netmeasure" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.description="Reproducible TCP connect-latency measurement in Kubernetes and OpenShift" \
      org.opencontainers.image.source="https://github.com/git001/k8s-netmeasure-rs" \
      org.opencontainers.image.revision="${BUILD_GIT_SHA}" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

COPY --from=builder /src/target/release/k8s-netmeasure /usr/local/bin/k8s-netmeasure
COPY LICENSE-MIT LICENSE-APACHE /licenses/

# Numeric non-root user; runs under any arbitrary UID and with a read-only root
# filesystem, because nothing is written outside the --output volume.
USER 1001
ENTRYPOINT ["/usr/local/bin/k8s-netmeasure"]
CMD ["--help"]
