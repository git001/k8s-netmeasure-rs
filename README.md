# k8s-netmeasure-rs

Reproducible TCP connect-latency measurement in Kubernetes and OpenShift.

**Status: milestone 1 of the [Phase 1 implementation plan](docs/design/2026-11-13_phase1-design/11-implementation-plan.md)
(scaffold).** The binary has no measurement subcommands yet; `listen`, `run`,
`report`, `selftest` and `probe` follow in the next milestones. The design is in
[docs/design/2026-11-13_phase1-design/](docs/design/2026-11-13_phase1-design/README.md).

## Build and check

Toolchain: pinned by `rust-toolchain.toml` (Rust 1.100, edition 2024).

```sh
cargo build --release
cargo xtask ci        # every quality gate, exactly as CI runs them
cargo xtask <gate>    # one gate: fmt | clippy | line-limit | test | deny | audit | coverage
cargo xtask musl      # static musl build + tests (needs musl-gcc, cargo-auditable)
cargo xtask changelog <version>   # release notes from CHANGELOG.md
```

Changes are recorded in [CHANGELOG.md](CHANGELOG.md) (Keep a Changelog format); a
release's notes are its changelog section.

`cargo xtask ci` needs `cargo-deny`, `cargo-audit` and `cargo-llvm-cov` installed.

## Container image

```sh
podman build -f Containerfile --build-arg BUILD_GIT_SHA=$(git rev-parse --short=12 HEAD) .
```

CI publishes `ghcr.io/git001/k8s-netmeasure-rs`, signed keylessly with cosign, with an
SPDX SBOM attestation and SLSA build provenance; each release's notes list the
verification commands.
Reference it by digest; for clusters that pull only from an internal registry, mirror
it by digest first (`skopeo copy --all` or `oc image mirror`), so the attestation
stays valid.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
