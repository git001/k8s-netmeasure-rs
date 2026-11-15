# Phase 1 design — 11 Implementation plan and prerequisites

Part of the [Phase 1 design](README.md) · covers §14–§15 · previous: [Container image](10-container-image.md) · next: [Decisions and review log](12-decisions-and-reviews.md)

---

## 14. Implementation order (review round 1)

Validate the parts that everything else rests on before building the rest. Each
milestone ends with something that runs and is reviewed before the next one starts.

1. **Scaffold.** Workspace, toolchain, `deny.toml`, `xtask` (line limit), CI gates,
   `Containerfile`. No measurement logic yet.
2. **Measurement core + `probe` + `selftest`.** The §3.1 sequence with all
   edge-case tests from §11, pacing with overrun tests, `TCP_INFO`/`SO_MEMINFO`
   decoding. **Then run `selftest` on the real target cluster** (overlay and underlay
   pod, on nodes of every NIC type in the cluster). Also on the cluster:
   - verify the ancestor-throttling inference of §3.7 once via a debug shell on the
     node;
   - check the `anomaly` rate (§3.2) on real paths of both NIC families; it must be
     zero, or the cycle rule is revisited (§16);
   - take a first set of real baseline runs with `run`'s prototype, as the input for
     the semi-synthetic statistics check of §11.

   These findings may change §3.3, §3.7 or §4.2 before anything builds on them.
3. **`listen` + `run` + result file.** Listener, control port, cycles, validity,
   shutdown behaviour and its tests, atomic writing, the schema. Split into five
   reviewable steps: 3a `listen` with the measured port (accept thread, signals);
   3b control port (3b1: server, probes, `/v1/info`, watchdog; 3b2: `/v1/context`,
   `/v1/snapshot`); 3c `run` with own snapshots, validity and the
   result file; 3d `run` against targets' control ports and `--node-context-from`;
   3e shutdown deadline (3e1), example manifests in `deploy/examples/` and README
   (3e2). Zone and instance type stay declared in Phase 1; Phase 2 reads them from
   the node labels (`topology.kubernetes.io/zone`, `node.kubernetes.io/instance-type`)
   with the `kube` client (decided in 3e2: the Downward API cannot read node labels).
4. **Statistics with the simulation suite.** Quantiles, order-statistic CI, block
   bootstrap, verdicts. The §11 acceptance criteria must pass **before** step 5
   produces any verdict. Split into five reviewable steps (decided 2026-11-15, after
   the first cluster baselines): 4a sample series from result files, type-1
   quantiles with censoring, order-statistic interval, tail minimum; 4b stationary
   block bootstrap with Politis–White block length; 4c replication levels,
   comparison and verdicts, failure rates; 4d simulation suite (smoke grid per push,
   full grid weekly); 4e semi-synthetic check against real runs (harness committed,
   data local only). The evidence label (§10.5) and the NIC-family analysis (§10.7)
   belong to step 5.
   **Brought forward on 2026-11-16** for a kernel change on 2026-11-17: a first
   `report` (sequential design, factor `kernel`, text output, sides pooled per path
   with the source nodes as the top resampling level) and the mesh script
   (`deploy/examples/mesh/`). The simulation suite (4d) follows right after; until
   then every report header says the method is covered by unit tests only.
5. **`report`.** Declared designs and factors, crossover, comparability, NIC-family
   stratification and the kernel × NIC interaction (§10.7), text and JSON output.
6. **Background load, underlay diagnostics (ethtool, netstat), README.** The ethtool
   ring sizes (`ETHTOOL_GRINGPARAM`) already arrived in milestone 2; step 6 adds
   driver info, channels and the statistics allowlist.

---

## 15. Prerequisites found during Step 0

- Toolchain `1.100.0` is installed (`x86_64-unknown-linux-gnu`). The host glibc is
  2.44, newer than UBI 10's, hence the builder-container rule in §1.
- `cargo-llvm-cov`, `cargo-audit` and `cargo-deny` (0.20.2) are installed.
- `podman` is available on the host (not inside the sandboxed editor environment);
  local image builds run there, CI builds its own.
