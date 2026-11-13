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
   - take a first set of real baseline runs with `run`'s prototype, as the input for
     the semi-synthetic statistics check of §11.

   These findings may change §3.3, §3.7 or §4.2 before anything builds on them.
3. **`listen` + `run` + result file.** Listener, control port, cycles, validity,
   shutdown behaviour and its tests, atomic writing, the schema.
4. **Statistics with the simulation suite.** Quantiles, order-statistic CI, block
   bootstrap, verdicts. The §11 acceptance criteria must pass **before** step 5
   produces any verdict.
5. **`report`.** Declared designs and factors, crossover, comparability, NIC-family
   stratification and the kernel × NIC interaction (§10.7), text and JSON output.
6. **Background load, underlay diagnostics (ethtool, netstat), README.**

---

## 15. Prerequisites found during Step 0

- Toolchain `1.100.0` is installed (`x86_64-unknown-linux-gnu`). The host glibc is
  2.44, newer than UBI 10's, hence the builder-container rule in §1.
- `cargo-llvm-cov`, `cargo-audit` and `cargo-deny` (0.20.2) are installed.
- `podman` is available on the host (not inside the sandboxed editor environment);
  local image builds run there, CI builds its own.
