# k8s-netmeasure-rs

Reproducible TCP connect-latency measurement in Kubernetes and OpenShift.

**Status: milestone 3 of the [Phase 1 implementation plan](docs/design/2026-11-13_phase1-design/11-implementation-plan.md)
in progress:** `probe`, `selftest` and `listen` (measured port and control port with
probes and `/v1/info`; `/v1/context` and `/v1/snapshot` follow). `run` and `report`
follow. The design is in
[docs/design/2026-11-13_phase1-design/](docs/design/2026-11-13_phase1-design/README.md).

## Subcommands so far

```sh
# Is this kernel suitable? Run once per node type before measuring (about 10 s).
k8s-netmeasure selftest [--output selftest.json]

# Measurement target: accepts on the measured port until SIGTERM/SIGINT (exit 143/130).
k8s-netmeasure listen [--port 17700] [--bind 0.0.0.0] [--backlog 4096] \
                      [--control-port 17701] [--control-bind auto]

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

`listen` accepts connections and never reads or writes on them. It keeps the last 64
accepted connections open instead of closing each at once, so its FIN cannot reach the
client before the client has checked the connection. The effective backlog is
`min(--backlog, net.core.somaxconn)` and is logged at start. A port that cannot be bound
ends the process with exit 3. On shutdown it logs the accepted, aborted and failed
accepts and the listener's SYN drops (`SK_MEMINFO_DROPS`).

The control port is plain HTTP/1.1 with JSON and is never measured:

| Path | Answer |
|---|---|
| `GET /startup` | 200 once both ports are bound, 503 before |
| `GET /live` | 200 unless the accept or main thread is stalled (> 5 s); never depends on the network |
| `GET /ready` | 200 while accepting; 503 before startup and from the first SIGTERM/SIGINT on |
| `GET /v1/info` | build info, `listener_instance` (ULID, new per start), measured address, backlog |

An unknown path is 404, any method other than `GET` 405, any query string 400.
`--control-bind auto` (default) binds the IPv4 address of the interface with the
default route: the pod IP in a pod, the node IP with `hostNetwork`. `127.0.0.1` is
then not served, so `oc port-forward` needs `--control-bind 0.0.0.0`. With
`hostNetwork`, NetworkPolicy does not apply and the control port is reachable for
anyone who can reach the node; it has no authentication. `--control-port` must
differ from `--port` (exit 2).

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
