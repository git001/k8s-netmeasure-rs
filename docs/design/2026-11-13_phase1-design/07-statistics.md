# Phase 1 design — 07 Statistics

Part of the [Phase 1 design](README.md) · covers §10.1–§10.5 · previous: [Load, process behaviour and configuration](06-runtime.md) · next: [Report model](08-report.md)

---

## 10. Statistics

### 10.1 Per side

- **Quantile estimator:** inverse ECDF (Hyndman–Fan type 1), `x_(⌈np⌉)`. No
  interpolation, so it combines cleanly with censoring and with the order-statistic
  interval. At n ≥ 1000 the differences between estimator types are negligible against
  the interval widths.
- **Censoring:** a timeout enters as +∞ (meaning ≥ timeout). If the quantile's order
  statistic is censored, the output is `≥ <timeout>` instead of a number.
- **Distribution-free CI:** order statistics `(x_(l), x_(u))` with binomial coverage
  ≥ 95 %: `l` the largest rank with `F(l−1) ≤ 2.5 %`, `u` the smallest with
  `F(u−1) ≥ 97.5 %`, `F` the CDF of `Binomial(n, p)`; the exact coverage
  `F(u−1) − F(l−1)` is reported. The CDF is computed exactly in log space in the
  crate itself (milestone 4a), so this needs no statistics crate. It is labelled honestly: it **assumes independent samples** and is therefore
  optimistic under autocorrelation; it is shown for orientation, never used for a
  verdict.
- **Tail minimum `m = 40` exceedances** above the quantile per side. Justification:
  - Bahadur: a quantile's sampling error is driven by the count of exceedances; the
    relative standard error of that count is 1/√m, about 16 % at m = 40.
  - The default 5000 samples give 50 exceedances for p99, which leaves 20 % headroom
    for excluded invalid cycles. That headroom is the reason to choose 40 rather than 50.
    With 10 cycles per run (§3.5) the headroom covers **two** excluded cycles, not one
    (corrected in review round 2). That holds exactly only because every cycle carries
    the same number of counted samples and the warm-up is added to cycle 0 (corrected
    in review round 3, §3.5).
  - Below m, the output is a warning and the required sample count, `n = m / (1 − p)`,
    instead of an interval.
- **Series** (milestone 4a): per target of a run, the samples of every valid cycle
  without warm-up, in measurement order, one segment per cycle. `ok` enters as its
  duration, `timeout` as censored; `refused`/`net_error` count only as failures;
  `local_error`/`anomaly` (only in invalid cycles) count nowhere.

Wall time below: `pair` mode, 10 ms interval, warm-up 50, loopback every 10th slot
(§3.4).

| Quantile | n per target per side (m = 40) | wall time |
|---|---|---|
| p50 | 80 (bootstrap needs more; use ≥ 1000) | — |
| p90 | 400 | ≈ 5 s |
| p99 | 4,000 (default 5,000) | ≈ 56 s |
| p99.9 | 40,000 | ≈ 7.4 min |

### 10.2 Block bootstrap

- **Stationary bootstrap** (Politis & Romano 1994). Block lengths are geometric, so the
  result is less sensitive to the exact mean block length than a fixed-block method.
- **Mean block length: automatic, by Politis & White (2004)** with the Patton,
  Politis & White (2009) correction. It is applied to the **exceedance indicator series**
  `1{x_t > q̂_p}` of each target's samples in measurement order. Justification: by the
  Bahadur representation, `q̂_p − q_p ≈ −(F̂(q_p) − p) / f(q_p)`, so the variance of
  the quantile estimate is the variance of the mean of exactly that indicator series.
  Choosing the block length for it targets the right dependence structure, not the
  dependence of raw latencies.
- Bounds:
  - Floor: the number of samples spanning **5 s** of wall time (`--block-floor`). This
    is an **assumption** about the time scale of network disturbances, not a
    statistical constant (review round 1).
  - Cap: **n_run / 10**, so every run has ≥ 10 blocks. This cap addresses the prompt's
    concern that a whole cycle may be too long.
  - The largest estimate across a side's runs is used (conservative).
  - `--block-length <duration>|auto` overrides.
  - **Floor and cap can conflict** (review round 2). Floor ≤ cap holds only if a run
    lasts at least `10 × floor` = **50 s** of measurement per target series; for
    shorter runs the cap wins and the 5-s assumption about disturbance time scales
    cannot be applied. `run` warns at startup when the configured run is shorter. The
    report prints this as a **warning**, not as a note: "block-length assumption not
    applicable: run too short".
- **Transparency** (review round 1). For every compared group the report prints:
  - the raw Politis–White estimate per run;
  - the value applied, and whether the floor or the cap determined it;
  - the lag-1…lag-k autocorrelation of the exceedance indicator.

  If the floor dominates in most runs, the report says that the block length rests
  on the assumption rather than on the data. The simulation suite (§11) checks that
  the floor does not cause under-coverage when the true dependence is shorter or
  longer.
- Blocks never cross an excluded cycle or a run boundary; each valid segment is
  resampled circularly within itself.
- B = 10,000 replicates, percentile intervals, RNG seed in the report JSON.

### 10.3 Replication levels

- **Sequential design:** two levels: runs with replacement, then stationary blocks
  within each drawn run. Sides are resampled independently.
- **Concurrent design:** three levels: nodes within group, then runs, then blocks.
  Sides are resampled independently.
  - **Only conservative under an assumption** (corrected in review round 1): the
    variance of a difference is `Var(A) + Var(B) − 2·Cov(A,B)`. Resampling the groups
    independently drops the covariance term. That overstates the variance, and is
    therefore conservative for `better`/`worse` and `equivalent` alike, **only if the
    covariance is ≥ 0**. Shared network events usually make it positive. Contention
    for the same target X could in principle make it negative, and then the interval
    is too narrow.
  - **Check:** the report computes the correlation of the two groups' exceedance
    indicators in aligned wall-clock windows of one block length. If it is
    significantly negative, the report warns and the evidence label drops to at most
    `preliminary`.
  - Joint resampling of synchronised wall-clock blocks would include the covariance
    and be tighter; it is a candidate for later, not Phase 1.
- Between-run spread is always shown: per-run quantile estimates and their range per
  side. With k runs, two-stage resampling underestimates between-run variance by
  about (k−1)/k; the evidence label reflects that.

### 10.4 Compared quantity, verdicts, margins

- Default quantiles p50, p90, p99; p99.9 only on request (`--quantiles`).
- **Absolute margin** (`--equivalence-margin 50us`): the compared quantity is
  `Δ = q_after − q_ref`.
- **Relative margin** (`--equivalence-margin 10%`): the compared quantity is the ratio
  `q_after / q_ref`, with equivalence bounds `[1 − m, 1 + m]`. Bootstrapping the ratio
  directly carries the reference value's own uncertainty into the interval.
- Verdicts exactly as in the prompt:
  - `better`/`worse`: the 95 % two-sided interval excludes 0 (or 1 for the ratio).
  - `equivalent`: TOST, the 90 % interval lies inside the margin. This takes
    precedence, annotated "statistically significant, practically irrelevant" when the
    95 % interval also excludes 0.
  - `inconclusive`: otherwise.
  - Without a margin, `equivalent` is never reported.
- **Verdicts under censoring** (added in review round 3). Timeouts enter the bootstrap
  as +∞ (§10.1); ordering, and therefore percentile intervals, stay well defined.
  - An interval bound is **infinite** when enough replicates are censored; a finite
    bound is still a valid bound, because the true value behind a timeout is only
    larger.
  - `worse`/`better` is allowed only if the bound on the side of that conclusion is
    finite and excludes 0 (or 1). Example: the test kernel's p99 is `≥ timeout`, the
    reference's is 4 ms; the lower bound of Δ is finite and positive → `worse`.
  - `equivalent` is **never** reported while any bound of the TOST interval is
    infinite.
  - Otherwise `inconclusive`, with the reason "censored" and the censored fraction per
    side. A ratio with both sides censored is undefined and also `inconclusive`.
  - At the default p99 this applies above 1 % timeouts; with `--quantiles p99.9`
    already above 0.1 %.
- `--equivalence-margin` requires `--equivalence-rationale`.
- Effect size (absolute and relative) is always printed.
- **Failures** (`timeout` + `refused` + `net_error` per connect; revised in review
  round 2). They are shown in the verdict section because the prompt counts failures
  as part of the verdict.
  - **Why not an exact test:** an exact conditional rate test assumes independent
    events. Timeouts cluster just like latencies, so such a test would be
    anti-conservative exactly for the quantity that gets a verdict.
  - **Method:** the failure *rate* (failures / attempts) per side, and its difference
    and ratio, go through the **same hierarchical block bootstrap** as the latency
    quantiles. The resampled blocks carry their own attempt counts, so unequal
    exposure after validity exclusions is handled automatically.
  - **Minimum count:** a verdict (`better`/`worse`/`inconclusive`) only if both sides
    together have **≥ 10 failures**. Below that the result is descriptive: counts,
    attempts and rates per side, with the plain exact interval clearly labelled
    "assumes independence, descriptive only".
- **SYN retransmissions** per connect: diagnostics section only, never a verdict.
  Rates per side and a block-bootstrap interval for the ratio, with the same ≥ 10
  rule; below it, counts only.
- **Multiplicity — decided: one primary quantity, declared before the measurement.**
  - Problem: every `better`/`worse` verdict carries a 5 % false-alarm risk. With many
    verdicts in one report, the chance of at least one chance finding grows (3
    comparisons ≈ 14 %, 30 ≈ 79 % if independent).
  - **Primary quantity:** `--primary` (default `p99`) is the only latency quantile that
    gets a verdict. It is printed in the report header as "declared before measurement".
  - **k is defined here, once** (review round 3): the number of independent verdicts
    of one family that could each stand alone as a finding: compared (pair, path)
    groups × NIC-family strata (§10.7). With `--expect-effect-on`, the strata form one
    intersection-union hypothesis instead and count once (§10.7).
  - **Correction across those k:** with k compared groups, each verdict
    uses Bonferroni-adjusted levels: two-sided `1 − 0.05/k` instead of 95 %, and TOST at
    `α = 0.05/k` per side instead of 90 %. The report prints k and the adjusted levels.
    In `pair` mode k = 1, so the levels stay at exactly 95 % / 90 %.
  - **Other quantiles** (p50, p90, and p99.9 if requested) are **descriptive**: point
    estimate, effect size and an unadjusted 95 % interval, clearly marked "descriptive,
    no verdict". They never produce `better`/`worse`/`equivalent`.
  - **Failure-rate verdicts** form their own family across the same k pairs and are
    corrected separately the same way. They answer a different question ("does it fail
    more often?") and do not count against the latency verdict.
  - **The two families are coupled through timeouts** (review round 3). A timeout
    enters the latency distribution censored (§3.2) *and* counts as a failure. A
    change that shifts timeouts therefore moves both verdicts; the families are
    corrected separately, but they are not independent evidence. The report says so
    whenever timeouts are > 0 on either side.
  - The loopback local control and all diagnostics are outside every family; they warn
    or explain, they never decide.

### 10.5 Evidence label — exact thresholds

| Label | Sequential | Concurrent |
|---|---|---|
| `exploratory` | 1 run on either side | 1 node or 1 run on either side |
| `preliminary` | 2–4 runs per side, or ≥ 5 not interleaved | anything without crossover; also crossover with < 3 nodes per group |
| `strong` | ≥ 5 runs per side **and** interleaved: ≥ 3 side switches in time order (e.g. A B A B …) | ≥ 3 nodes per group, both groups rebooted, crossover present with both halves |

"Both groups rebooted" is decided from the node contexts. Two conditions must both
hold, **per crossover period** (review round 3): in a crossover every node has one
boot per period, and each period is checked against its own boot (`boot_id` of that
period's runs):
1. Every node's boot time falls within **6 h** of the other group's boot times
   (`--max-boot-skew`).
2. Every boot lies within **24 h** before that node's first run in that period
   (`--max-boot-age`).
   The reboot must belong to this experiment: two nodes booted an hour apart three
   weeks ago pass condition 1 but not this one (added in review round 2).

Three nodes is the smallest number at which node-to-node variance can be estimated
at all; it is a floor, not a recommendation.

**Additional conditions for `strong` in the concurrent design** (review round 2).
The counts alone are not enough:
- **same direction:** the effect has the same sign in both crossover periods, unless
  both period intervals contain zero;
- **leave-one-node-out stability:** the verdict is recomputed once without each node
  in turn. If removing any single node changes the verdict, one node drives the
  result, and the label is at most `preliminary`;
- **node heterogeneity is reported:** between-node spread of the per-node quantile
  estimates per group, next to the between-run spread.

**Caps that override the table** (review round 1). Each lowers the label to at most
`preliminary` and is printed with its reason:
- any run of a side has `container_kind: ephemeral` (CPU self-validation incomplete,
  §1);
- negative cross-group correlation in the concurrent design (§10.3);
- a significant period × treatment interaction in a crossover (§10.6);
- a failed same-direction or leave-one-node-out condition (above).

The table applies to `--factor kernel`, `nic-settings` and `load`. A NIC-family
comparison (`--factor nic`) cannot have a crossover and uses its own observational
labels (§10.7).
