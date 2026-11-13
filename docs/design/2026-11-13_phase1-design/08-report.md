# Phase 1 design — 08 Report model

Part of the [Phase 1 design](README.md) · covers §10.6–§10.7 · previous: [Statistics](07-statistics.md) · next: [Tests and quality gates](09-tests-and-ci.md)

---

### 10.6 Report model

- Inputs: `--before` / `--after` file sets (in the concurrent design read as
  reference / test group). Results are grouped by (path, target, source group) and
  **stratified by NIC family** (§10.7).
- **Two declarations are required before anything is compared:** `--design` (below)
  and `--factor` (§10.7), the variable the comparison is about.
- **The design is declared, not detected** (review round 1). The study design has to
  be fixed before the measurement, like the margin and the primary quantity.
  - `--design sequential|concurrent` is **required**; without it, exit 2.
  - The wall-clock data only **validates** the declaration. Overlap is defined per
    period (review round 2): each side's span runs from the start of its first run to
    the end of its last run, and overlap = |intersection| / |union| of the two spans.
    `concurrent` requires ≥ 0.8 in every period; `sequential` requires an intersection
    of at most **1 s** (review round 3: a clock step after a reboot can shift
    `realtime_at_origin` by milliseconds). Contradiction → exit 5, with the spans and
    the overlap printed; for `sequential` the message names a clock step as a possible
    cause and shows `clock.synchronized` of the runs involved (§5).
  - The output header states the declared design and the validation result.
- **Crossover is declared explicitly as well.** `report --design concurrent
  --crossover` requires every run to carry the label `period=1` or `period=2`; a
  missing or other value → exit 2. Group membership comes from the file sets (`--before`
  = reference kernel, `--after` = test kernel, across both periods). The report
  verifies that the node sets actually swap between the periods; otherwise exit 5.
  Periods, groups and node assignment are all printed and stored in the report JSON.
- **Crossover estimand, defined explicitly** (review round 1). For quantile `p` the
  kernel effect θ is written on the scale chosen by the margin (made consistent in
  review round 2):

  | Margin | Scale | Estimand | Interaction |
  |---|---|---|---|
  | absolute (`50us`) | raw | `θ = ½ · [ (q_test,P1 − q_ref,P1) + (q_test,P2 − q_ref,P2) ]` | `(q_test,P1 − q_ref,P1) − (q_test,P2 − q_ref,P2)` |
  | relative (`10%`) | log | `θ = √( (q_test,P1 / q_ref,P1) · (q_test,P2 / q_ref,P2) )`, the geometric mean of the two period ratios | `(q_test,P1 / q_ref,P1) / (q_test,P2 / q_ref,P2)` |

  This is the average of the within-period quantile differences (or ratios). It is
  **not** the quantile difference of the pooled data; for p99 the two can differ, and
  the report names which one it computes. θ removes period and node-group effects **if
  they act additively on the chosen scale**: on the log scale the assumption becomes
  multiplicative effects, which is more plausible for latency. The interval for θ
  comes from the same hierarchical bootstrap, resampled within each period and group.
  The **period × treatment interaction** is reported with its own interval. If it is
  significant, the two periods disagree and θ is not a single kernel effect; the report
  says so and caps the evidence (§10.5).
- **Comparability:**
  - Every context field of both sides is listed, always. A difference that belongs to
    the declared factor (§10.7) is shown without a warning marker; every other
    difference is shown with one. A kernel z-stream update that came along with a
    declared kernel change therefore stays visible.
  - A difference in a field that is neither part of the declared factor nor allowed
    for it (§10.7 table) is refused (exit 5) unless `--allow-context-mismatch
    <field,…>` is given; the override is printed.
  - `unknown` counts as unverifiable: refused in the concurrent design, warned in the
    sequential one. In the overlay variant this means the concurrent design needs a
    node listener on every node involved (§5).
  - `observed_via_node_listener` counts as observed.
  - Runs with different load parameters are never pooled.
  - The loopback control is compared on its **median** (block-bootstrap interval of
    the median difference), in its own "local control" section. A significant shift
    produces a warning. Its tail is shown descriptively only, because its share of the
    slots (§3.4) is too small for a reliable p99.
- Output: a benchstat-style text table (medians, quantiles with absolute and relative
  change, 95 % and 90 % intervals with method names, verdict, evidence label, design,
  factor, family size), one block per NIC-family stratum, then the interaction section
  where applicable (§10.7), then a separate **Diagnostics — context, not verdict**
  section, plus JSON with the same content.

### 10.7 NIC as comparison factor, stratification, kernel × NIC (decided in review round 2)

The tool has to answer two questions: whether a vendor change acts, and whether nodes
with different NIC families behave differently. The first design version treated a NIC
difference only as a confound to be refused. This section makes it a first-class
comparison. **[deviation]** `--factor` replaces the prompt's `--expect-change`; the
prompt's rule "never hide a difference, only suppress its warning" is kept unchanged.

**Declared factor.** `report --factor kernel|nic|nic-settings|load` is **required**,
like `--design`, and fixed before the measurement. It says which context fields form
the treatment:

| `--factor` | Must differ between the sides | Allowed to differ, printed as a known confound | Everything else |
|---|---|---|---|
| `kernel` | kernel release / build | — | must match |
| `nic` | NIC model, driver, driver version, firmware | instance type, because the NIC generation is often tied to the instance or host generation | must match, including the kernel |
| `nic-settings` | the changed NIC settings (ring sizes, channels, …) | — | must match, including NIC model, driver and firmware |
| `load` | load parameters | — | must match |

- A factor field that does **not** differ between the sides is an error (exit 5,
  "declared factor not present in the data").
- Any other difference is refused unless `--allow-context-mismatch` names it (§10.6).

**NIC family and stratification.**
- The **NIC family** of a node is the driver of the interface that carries the
  measured traffic. Where a node has a synthetic NIC plus a VF (a common
  accelerated-networking setup), the VF's driver defines the family; both interfaces
  are recorded.
- A measured pair belongs to a **stratum** `(source family → target family)`.
- Results are **never pooled across strata**, neither within a side nor between the
  sides. A kernel report therefore prints one verdict block per stratum, e.g. A→A,
  B→B, A→B, B→A.
- Multiplicity: k is defined in §10.4. Without `--expect-effect-on`, every stratum
  verdict can stand alone as a finding, so k = pairs × strata. That is expensive: at
  four strata the TOST runs at α = 0.0125 per side, z rises from 1.645 to 2.24, and the
  same power needs **≈ 1.85 × the samples**. The report prints k and this factor; a
  default run should not be expected to give `equivalent` in four strata.
- A node whose family is `unknown` (overlay without a node listener, §5) forms the
  stratum `unknown`. It is excluded from `--factor nic` and reported separately with a
  warning for every other factor.

**Kernel × NIC interaction: the negative control.**
- With `--factor kernel` and at least two strata, the report computes the kernel
  effect θ per stratum and the **interaction**: the difference of the effects
  (absolute margin) or their ratio (relative margin), with an interval from the same
  hierarchical bootstrap.
- `--expect-effect-on <family>` (optional, declared before the measurement) names the
  family F whose driver the change touches. It assigns every stratum a role:
  - **effect stratum:** F→F;
  - **control strata:** neither end is F (e.g. B→B when F is A);
  - **mixed strata** (one end F): descriptive only, no verdict, because a partial
    effect is expected there and would blur both roles.
- **One joint hypothesis, no correction across strata** (review round 3). The claim
  "effect in F→F **and** `equivalent` in every control stratum **and** interaction
  interval excludes zero" holds only if every part holds. For such an
  intersection-union test (Berger 1982) each part at level α gives the joint claim at
  level α, so the strata count once in k (§10.4), and only the correction across pairs
  remains. The report states whether the observed pattern matches the declaration.
- A `better`/`worse` in a control stratum breaks the joint claim; on its own it is a
  warning marked "unadjusted, not a finding", because looking for effects in any of
  several strata is exactly the case Bonferroni exists for.

| Observed | Reading |
|---|---|
| effect in the declared family, `equivalent` in the others, interaction interval excludes zero | the change acts only where the touched driver is used: the strongest evidence that it works through that driver |
| the same effect in all families | the change acts independently of the NIC (e.g. generic kernel code) |
| an effect in a family that was not declared | unexpected; printed as a warning |
| `inconclusive` in the control families | no negative control: more runs or a margin are needed |

- `equivalent` in a control stratum needs a declared margin (§10.4), as everywhere.

**Comparing NIC families directly (`--factor nic`).**
- The comparison is **observational**: a NIC cannot be swapped between nodes, so
  there is no crossover. The report header always states "NIC family, together with
  everything tied to it (instance and host generation), observational comparison".
- **Design:** concurrent, same kernel on both families, both groups rebooted (§10.5),
  and — to keep the other end constant — the **same target nodes** for both source
  families. The reverse direction (same sources, targets of both families) isolates
  the receive side and is reported as its own comparison.
- **Evidence labels** for `--factor nic` replace the crossover condition of §10.5:
  - `exploratory`: < 3 nodes per family;
  - `preliminary`: ≥ 3 nodes per family;
  - `strong (observational)`: ≥ 5 nodes per family, same window, same kernel, both
    rebooted, same target set, leave-one-node-out stable.
- With a declared margin, `equivalent` is possible. It shows that the families do not
  differ beyond the margin on these paths, the formal version of "same distribution
  on both NIC types".
