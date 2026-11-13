# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- A connect whose socket is already in `CLOSE_WAIT` when `TCP_INFO` is read (the
  target accepted and closed before the check) is `ok`, no longer `anomaly`: that
  state is reachable only after a completed handshake. It is counted as
  `peer_closed` (`probe` field, `selftest` detail). Seen as 1 anomaly in 1000 in a
  pod with a 1-CPU quota.
- The `selftest` loopback listener keeps the last 64 accepted connections open
  instead of closing each at once, so its FIN cannot reach the client before the
  check; `listen` will do the same (design §4.2).
- The binary uses the system allocator; mimalloc is removed. It brought no measurable
  benefit here and was the only C dependency, so the static musl build now needs no
  `musl-gcc` (CI no longer installs `musl-tools`).
- Design: the per-cycle NIC path check also compares RX/TX ring sizes; a change
  invalidates the cycle as `nic_changed`.
- Design: the NIC family is treated as a property of the host, not of the instance
  type. The NIC path is checked around every cycle (`nic_changed`), a family change
  between crossover periods is a context mismatch, the node context gains the CPU
  model, and the report states when one instance type occurs with several families.

### Added

- `listen` control port: `GET /v1/context` (node context with provenance per field:
  `observed`, `declared`, `unknown`; kernel, boot id, uptime, CPU model and
  microcode, own `cpu.max`, TCP sysctls, MTU, NIC path) and `GET /v1/snapshot`
  (monotonic time, boot id, `listener_instance`, accept counters, listener
  `SK_MEMINFO_DROPS`, netns `ListenOverflows`/`ListenDrops`, own `cpu.stat`, NIC
  path). The bodies are typed in `netmeasure-core::schema::control` and read back
  with `deny_unknown_fields`. New declared-context flags `--node-name` (default
  `$NODE_NAME`), `--zone`, `--instance-type`.
- `listen` control port (HTTP/1.1 + JSON on its own thread, tokio `current_thread` +
  hyper): `GET /startup`, `/live` (watchdog over the accept and main threads, 5 s),
  `/ready` (503 from the first shutdown signal on) and `/v1/info` (build info,
  `listener_instance` ULID, measured address, backlog). Unknown path 404, other
  methods 405, any query 400. `--control-port` (default 17701, must differ from
  `--port`, exit 2) and `--control-bind` (default `auto`: the IPv4 address of the
  default-route interface; exit 3 if there is none). `/v1/context` and
  `/v1/snapshot` follow.
- `listen` subcommand, measured port only (control port follows): `--port` (default
  17700), `--bind` (default `0.0.0.0`), `--backlog` (default 4096, effective
  `min(backlog, somaxconn)`, logged). A dedicated accept thread keeps the last 64
  connections open. SIGTERM / SIGINT stop it with exit 143 / 130, a second signal
  exits at once; a port that cannot be bound is exit 3. On shutdown it logs accepted,
  aborted and failed accepts and the listener's `SK_MEMINFO_DROPS`.
- Node facts list every physical interface with its driver and RX/TX ring sizes
  (current and maximum, `ethtool -g` equivalent, no capability needed); a size the
  driver does not report is `null`. `selftest --output` includes them.
- `selftest`: the loopback check lists the TCP state of every `anomaly`
  (`anomaly_states`).
- Node facts include the CPU model and microcode revision of the first processor
  (`/proc/cpuinfo`); `selftest --output` includes them.
- `selftest --output`: `node` is now the node context of `/v1/context` (every field
  with `value` and `source`).
- `--log-level` (`K8S_NETMEASURE_LOG_LEVEL`) on every subcommand: `error`, `warn`,
  `info` (default), `debug`, `trace`; `debug` logs parameter sources and full
  `selftest` check data.
- Atomic result write: temporary file, `fsync`, `rename`, then `fsync` of the
  directory (design §5), so a completed file also survives a node crash.
- `probe` subcommand: a few connects (`--count`, 1–100) to `--target` with the exact
  measurement code path; one JSON line per connect on stdout with outcome, duration,
  errno, `sync_connect`, SYN retransmissions. Exit 0 if every connect is `ok`, 7
  otherwise.
- `selftest` subcommand: verifies on the running kernel the SYN retransmission field,
  the listener drop indicator (`SO_MEMINFO`), SYN backoff, `tcpi_total_rto`, loopback
  connect behaviour and anomaly rate, pacing lateness, signal isolation of the
  measuring thread, and cgroup v2 visibility. Summary table on stdout, one JSON log
  line per check, `--output` for a JSON report. Exit 0, or 6 if a check was
  inconclusive. The node-listener check follows with `listen`.
- Measurement core: non-blocking connect with `ppoll` on an absolute deadline
  (`EINTR` never extends the timeout), `SO_ERROR` and `TCP_INFO` read outside the
  timed section, `ok` only with `ESTABLISHED`, outcome classes `ok`, `timeout`,
  `refused`, `net_error`, `local_error`, `anomaly`; a measuring thread with all
  asynchronous signals blocked and 1 ns timer slack; the pacer with the four overrun
  rules; slot and cycle plan. `TCP_INFO` and `SO_MEMINFO` offsets come from `libc`
  (`offset_of!`); a field beyond the length the kernel filled is `unknown`, never 0.
  Signal mask, `sigaction`, `kill`, `ppoll`, clocks and timer slack go through the
  safe `nix` API; `unsafe` remains only for installing the signal handler and for
  `getsockopt` of partially filled buffers.
- Configuration rules: every parameter also as `K8S_NETMEASURE_<FLAG>`; unknown or
  empty `K8S_NETMEASURE_*` variables and the same parameter on the command line and
  in the environment are errors (exit 2); variables of other subcommands are ignored
  and logged.

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
