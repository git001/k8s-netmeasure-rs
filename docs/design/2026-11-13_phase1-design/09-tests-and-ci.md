# Phase 1 design — 09 Tests and quality gates

Part of the [Phase 1 design](README.md) · covers §11–§12 · previous: [Report model](08-report.md) · next: [Container image](10-container-image.md)

---

## 11. Tests (Phase 1)

Everything the prompt lists:

- Statistics against known inputs:
  - block vs naive bootstrap on a synthetic AR(1)/regime-switching series, where the
    naive interval must be measurably narrower;
  - censoring, tail below minimum; censored verdict rule (§10.4): `worse` only from a
    finite bound, never `equivalent` with an infinite bound;
  - all four verdicts, in particular `inconclusive` vs `equivalent`;
  - node-group mismatch rejection;
  - factor validation (§10.7): declared factor absent from the data → exit 5; an
    undeclared difference → exit 5 unless allowed;
  - NIC-family stratification: mixed families on one side are never pooled; the
    `unknown` stratum is reported separately;
  - NIC path per cycle: a VF that disappears, reappears or changes driver between
    snapshots → `nic_changed`; an unreachable node listener → `unknown`, never
    "unchanged";
  - crossover with a node whose family changes between periods → exit 5, or exclusion
    with a warning under `--allow-context-mismatch nic_family`;
  - "same instance type, different NIC family" is detected and stated in the header;
  - kernel × NIC interaction on synthetic data with an effect in one stratum only:
    the effect must be found there, `equivalent` in the control stratum, and the
    interaction interval must exclude zero;
  - bucket saturation, as a pure function for the later Prometheus path.
- Shutdown tests:
  - SIGTERM mid-cycle within and beyond the deadline;
  - second signal;
  - SIGKILL during the write, where the file must be either absent or parse as
    complete.

Additions:

- Pacing arithmetic, as pure unit tests.
- The SYN-drop integration test from §3.3, which also verifies `SO_MEMINFO`.
- A loopback end-to-end test `listen` + `run` + `report`.
- Statistics reference values cross-checked against R (`qbinom`, `binom.test`,
  `poisson.test`), recorded as constants in the tests.

**Measurement core, edge cases** (review round 1). All are unprivileged and run on
loopback. Each asserts that duration and outcome match:

| Case | How it is produced | Expected |
|---|---|---|
| immediate success | loopback listener | `ok`, `sync_connect` recorded, duration in the µs range |
| delayed handshake | accept queue full → SYN dropped, accept after ~1.5 s (§3.3) | `ok`, duration ≥ ~1 s, `syn_retrans` ≥ 1 |
| refused | closed loopback port | `refused` via `SO_ERROR`, short duration |
| timeout | full accept queue that is never drained, `--timeout 300ms` | `timeout`, duration ≈ 300 ms (± scheduling), never ≫ |
| EINTR | `pthread_kill` with a test signal **not** blocked by the measuring thread, sent mid-wait | the wait continues to the **same absolute deadline**; duration ≈ timeout, not timeout + extra |
| signal isolation | SIGUSR2 to the process (not SIGTERM, which would start the shutdown path) | delivered to another thread; the measuring thread sees no EINTR |
| `anomaly` classification | pure unit test in core with synthetic `SO_ERROR`/`tcpi_state` pairs | `SO_ERROR == 0` with any state other than `ESTABLISHED` → `anomaly`, never `ok` |

**Pacing** (review round 1, made exact in round 2). A fake prober in core (pure) and
the real delayed-handshake listener (integration) produce overruns. The assertions are
the four rules of §3.4, one each:
1. the overrunning sample completes and is recorded normally;
2. the next slot starts at once, with `scheduled = actual start` (late_ns ≈ 0) and the
   "after overrun" flag set;
3. the slot after that is scheduled exactly `interval` later;
4. no slot is made up: the number of slots in the cycle is unchanged, and the cycle
   takes longer by the overrun.

Also checked: overruns and lateness per sample, and an effective connect rate in the
result file that matches the observed one.

**Statistics simulation suite** (review round 1, Monte-Carlo handling made explicit in
round 2). This is required before any real measurement is used in a support case. It
lives in `netmeasure-core` tests. Each scenario generates synthetic samples with a
**known** true effect, runs the complete report pipeline and repeats that R times.

The dimensions:

| Dimension | Values |
|---|---|
| dependence | i.i.d.; AR(1) with φ = 0.5, 0.9; regime switching (short bursts of seconds) |
| drift | none; linear trend within a run |
| nodes | homogeneous; heterogeneous node effects (concurrent design) |
| design | sequential; concurrent; concurrent + crossover with a period effect |
| true effect on p99 | 0; inside the margin; just outside the margin; clear regression |
| censoring | none; 0.5 % timeouts; 2 % timeouts (censored p99, §10.4) |

**Not the full factorial.** All combinations would be 576 scenarios, and even a
perfectly calibrated method would fail a few of them by Monte-Carlo chance alone.
Instead about **40 scenarios** are chosen so that every pair of dimension values
occurs at least once (pairwise design), plus the combinations that matter most for
support cases (dependent + crossover + effect near the margin). The list is fixed in
the test code.

**Monte-Carlo control.**
- Every scenario uses a **fixed seed**, so results are reproducible and a failure
  can be replayed exactly.
- Full grid: R = 2000 repetitions, B = 2000 bootstrap replicates each, run weekly.
  Standard error of an observed 95 % coverage: √(0.95·0.05/2000) ≈ 0.49 pp, so the
  band [93 %, 97 %] is ±4 standard errors. Chance failure of a correctly calibrated
  method: about 0.005 % per scenario, under 0.2 % for the whole grid.
- Estimated cost (corrected in review round 3): 40 × 2000 × 2000 = 1.6 × 10⁸
  bootstrap replicates at 20–50 µs each (resampling plus quantile selection over
  n ≈ 5000) is **1–2.5 CPU-hours** for the two-level designs, more with the third
  level of the concurrent design. Plan about **1 h wall time on a 4-core runner**.
- Per push: a smoke grid of 8 scenarios with R = 200. With SE ≈ 1.5 pp, the band
  there is [89 %, 100 %]. It only catches gross breakage; the weekly grid is the real
  gate.
  Implemented in `netmeasure-core/tests/sim.rs` (`cargo xtask sim`, part of `ci`):
  i.i.d., AR(1) 0.9, run effects, bursts (all without effect), a clear regression
  × 1.3, × 1.15 just outside a 10 % margin, 2 % timeouts, and the naive-bootstrap
  negative control. The negative control uses **one run per side**: with several
  runs the run level of the bootstrap already carries most of the variance, so a
  naive block inside a run still covers (≈ 92 % at five runs) and would not prove
  anything. With 2 % timeouts p99 is censored; only the verdict rules are checked.

Acceptance criteria (weekly grid):
- empirical coverage of the nominal 95 % intervals within **[93 %, 97 %]**; of the
  90 % TOST intervals within **[87.5 %, 92.5 %]** (±3.7 SE, because that coverage has a
  larger standard error);
- false `worse`/`better` rate at true effect 0 ≤ **6.5 %** (5 % + 3 SE);
- false `equivalent` rate at a true effect just outside the margin ≤ **6.5 %**;
- censored scenarios: no `equivalent` while a bound is infinite, and the false
  `worse`/`better` rate stays ≤ **6.5 %**;
- intersection-union reading (§10.7; fixed extra scenarios with two strata): when
  the control stratum has a true effect just outside the margin, the reading "effect
  only where declared" occurs in ≤ **6.5 %** of repetitions;
- the naive (non-block) bootstrap fails the coverage criterion on the dependent
  scenarios, which proves that the test can detect the problem.

A scenario that misses its criterion blocks the release. It is fixed in the method,
not by loosening the threshold.

**Limit of the simulation, and the semi-synthetic check.** A simulation validates the
method only against the models it simulates; a passed grid does not prove correct
coverage on a real cluster network. Therefore, as soon as implementation step 2 (§14)
has produced real baseline runs:
- several real runs from the same nodes and time window are split into two pseudo
  sides;
- a **known** effect is imposed on one side, e.g. all latencies × 1.1, or a fixed
  added delay;
- the full pipeline must recover it with the stated coverage.

This tests the method against the real dependence structure of the target network.
The real data stays local and is never committed; the test harness is committed, the
data is not. The README states that coverage is validated against simulated models
plus this semi-synthetic check, not proven for every network.

---

## 12. Quality gates and CI

All gates run locally through `cargo xtask <check>` and identically in CI (GitHub
Actions, since the repository is on GitHub). The workflow logic lives in Rust in
`xtask`, not in shell or Python fragments inside YAML.

| Gate | Tool | Fails when |
|---|---|---|
| Format | `cargo fmt --check` | any diff |
| Lint | `cargo clippy --all-targets -- -D warnings` | any warning |
| File size | `cargo xtask line-limit` | any `.rs` file > 400 lines, or > 200 lines without a `//! line-limit: <reason>` line, counted outside `#[cfg(test)]` items (§2). Parsed with `syn`, so test modules are recognised exactly rather than by a line pattern |
| Argument count | `#![forbid(clippy::too_many_arguments)]` in every crate root (`deny` in `netmeasure-cli`, §2); `cargo xtask line-limit` checks the crate roots, rejects any `allow`/`expect`/`warn` of the lint or of `clippy::complexity` / `clippy::all`, and checks that no `clippy.toml` / `.clippy.toml` raises `too-many-arguments-threshold` | any function over the limit (7); any attempt to `allow` it or to raise the threshold |
| Config surface | unit test over the clap command tree (§9.1) | a parameter without env var, a misnamed env var, a missing unknown-env check, or a parameter set via CLI and env that is not rejected |
| Tests | `cargo test --locked` (unit + integration, loopback only) | any failure |
| Simulation smoke | `cargo xtask sim`: smoke grid (§11), release build, every push | any smoke-band criterion missed |
| Simulation | full statistics simulation grid (§11), weekly | any acceptance criterion missed |
| musl artifact | `cargo xtask musl`: `cargo build --release --target x86_64-unknown-linux-musl` on the runner (Rust musl target only, no `musl-gcc`), then the test suite against it (§1); not part of `cargo xtask ci`, because it adds the musl target and needs `cargo-auditable` | build or test failure |
| Coverage | `cargo llvm-cov` (lcov + HTML report as CI artifact) | line coverage below **90 %** in `netmeasure-core`, `netmeasure-util` and `netmeasure-cli`, or below **70 %** in `k8s-netmeasure` |
| Audit | `cargo audit` (RustSec advisory DB) | any unignored advisory; also runs on a daily schedule, so new advisories against unchanged code are caught |
| Dependency policy | `cargo deny check` (advisories, licenses, bans, sources) | license outside the allowlist (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode-3.0, Zlib), crate from a source other than crates.io, duplicate version of a **direct** dependency, or an unlisted transitive duplicate. Listed transitive exceptions (`skip` with reason and review date) are allowed; an exception past its review date fails |
| Freshness | weekly scheduled job: `cargo update` + full gate run; Dependabot for major versions | the updated graph fails a gate; the job then opens an issue |
| Image | build per §13; image vulnerability scan (e.g. `trivy`) on the final image | build failure; critical findings fail, others are reported |

The binary crate's coverage gate is lower because the syscall error paths (EMFILE, an
unreadable cgroup, ethtool on real NICs) cannot all be triggered on a CI runner.
Integration tests on loopback cover the main paths. The uncovered lines are listed in
the coverage report, not hidden.
