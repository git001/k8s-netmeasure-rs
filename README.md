# k8s-netmeasure-rs

Reproducible TCP connect-latency measurement in Kubernetes and OpenShift.

**Status: milestone 2 of the [Phase 1 implementation plan](docs/design/2026-11-13_phase1-design/11-implementation-plan.md)
(measurement core, `probe`, `selftest`).** `listen`, `run` and `report` follow in the
next milestones. The design is in
[docs/design/2026-11-13_phase1-design/](docs/design/2026-11-13_phase1-design/README.md).

## Subcommands so far

```sh
# Is this kernel suitable? Run once per node type before measuring (about 10 s).
k8s-netmeasure selftest [--output selftest.json]

# Manual cross-check of one path, same code path as the measurement (not a measurement).
k8s-netmeasure probe --target 10.0.0.5:17700 [--count 5] [--timeout 5s]
```

Every parameter can also be set as `K8S_NETMEASURE_<FLAG>`, e.g.
`K8S_NETMEASURE_TARGET=10.0.0.5:17700`. Setting the same parameter both ways, an
unknown `K8S_NETMEASURE_*` variable, or an empty one is an error (exit 2).

Logs are JSON lines on stderr; results go to stdout. Every subcommand takes
`--log-level error|warn|info|debug|trace` (`K8S_NETMEASURE_LOG_LEVEL`, default `info`);
`debug` adds details such as the source of every parameter and the full data of each
`selftest` check.

`probe` measures TCP connection establishment only: one JSON line per connect with
the duration, the outcome, the errno, the SYN retransmissions and `peer_closed` (the
target had already closed the connection when it was checked; still `ok`). Without the tool, the
shell equivalent (no timing) is `exec 3<>/dev/tcp/<ip>/<port> && exec 3<&-`.

## Build and check

Toolchain: pinned by `rust-toolchain.toml` (Rust 1.100, edition 2024).

```sh
cargo build --release
cargo xtask ci        # every quality gate, exactly as CI runs them
cargo xtask <gate>    # one gate: fmt | clippy | line-limit | test | deny | audit | coverage
cargo xtask musl      # static musl build + tests (needs cargo-auditable)
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
