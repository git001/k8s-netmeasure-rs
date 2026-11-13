# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Phase 1 design in `docs/design/2026-11-13_phase1-design/`, after three external
  review rounds.
- Cargo workspace (Rust 1.100, edition 2024) with the crates `netmeasure-util`,
  `netmeasure-core`, `netmeasure-cli`, `k8s-netmeasure` and `xtask`.
- `k8s-netmeasure` CLI frame: `--help`, and `--version` with build information (git
  revision, rustc, target, static or dynamic linking). Usage errors exit with code 2.
  No measurement subcommands yet.
- `netmeasure-cli`: clap command definitions and the config-surface check (every
  parameter has a `K8S_NETMEASURE_<FLAG>` env var).
- Quality gates as `cargo xtask <gate>`: `fmt`, `clippy`, `line-limit` (file-size and
  argument-count rules), `test`, `deny` (including review dates of duplicate-version
  exceptions), `audit`, `coverage` (per-crate thresholds), `ci` (all of them) and
  `musl`.
- `deny.toml`: license allowlist, crates.io only, no duplicate versions.
- `Containerfile`: built in UBI 10 (glibc 2.39), runtime UBI 10 minimal, both pinned by
  digest; non-root user 1001, works with a read-only root filesystem.
- GitHub Actions, pinned by commit SHA: quality gates and the musl build on every
  push and pull request; image build, Trivy scan, push to
  `ghcr.io/git001/k8s-netmeasure-rs` and build provenance attestation; a daily audit;
  a weekly dependency freshness job; the static musl binary as a release asset on
  version tags. Dependabot for major crate versions and GitHub Actions.
- Licensed under MIT OR Apache-2.0.
- Release workflow: the GitHub release notes are the `CHANGELOG.md` section of the
  tagged version (`cargo xtask changelog <version>`); a tag without a changelog
  section fails the release.
- Image supply-chain evidence: keyless cosign signature, SPDX SBOM (syft) as cosign
  attestation, SLSA build provenance; the musl release binary gets an SPDX SBOM,
  an SBOM attestation and build provenance. Release notes include the verification
  commands.
- Binaries are built with `cargo auditable`, so SBOM generation and the image scan
  see the Rust dependencies.
