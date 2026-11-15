# `report`: before and after

[User guide](README.md) · previous: [Troubleshooting](06-troubleshooting.md)

`report` compares two sets of result files, e.g. before and after a kernel change,
and gives one verdict on the declared primary quantile per path.

```sh
k8s-netmeasure report --design sequential --factor kernel|nic-settings \
    --before mesh-results/before --after mesh-results/after \
    --equivalence-margin 10% --equivalence-rationale "p99 within ±10 % is no change for us" \
    [--primary p99] [--quantiles p50,p90,p99] [--replicates 10000] [--seed N] \
    [--allow-context-mismatch nic_family,cpu_model,kernel_release]
```

**State:** this version supports the sequential design (`before` on the same nodes,
then `after`) with the factors `kernel` and `nic-settings`, NIC-family strata and text
output. The concurrent design with crossover, the factors `nic` and `load` and JSON
output *follow*. The
statistical method is covered by unit tests and by the smoke grid of the simulation
suite (8 synthetic scenarios with a known effect, run on every push); the full grid
*follows*. On your own data, `cargo xtask semisynthetic <dir>` (in a checkout of the
repository) checks that the method recovers a known effect imposed on a random half of
your runs; it needs at least two runs per node, better a mesh series of five rounds.
The report header names what has validated the method.

## Declare before you measure

| Flag | Meaning |
|---|---|
| `--design sequential` | required; the result files must not overlap in wall-clock time (≤ 1 s), else exit 5 |
| `--factor` | required; what changed between the sides. `kernel`: kernel differences are expected, everything else is checked. `nic-settings`: ring sizes are expected to differ on the same nodes; a changed kernel on a node is refused (exit 5) like a changed NIC family or CPU model |
| `--primary` | default `p99`: the **only** latency quantile that gets a verdict; the others are descriptive |
| `--equivalence-margin` | `10%` (on the ratio after/before) or `50us` (on the difference); needs `--equivalence-rationale`. Without a margin the verdict is never `equivalent` |
| `--before`, `--after` | files or directories (searched recursively; other JSON files are skipped) |

**Measure both sides alike.** Within a path, every run of both sides must have the
same `--interval`, loopback share, `--timeout` and topology; otherwise `report`
refuses (exit 5) and names each parameter set with an example run. The sample count
may differ, it changes only the precision.

Choose the primary quantile and the margin **before** the measurement and write the
rationale down: chosen afterwards, they can be fitted to the result.

## Reading the output

- One section per path (overlay, underlay) with the local control, then one block per
  **NIC stratum** `source family → target family`, e.g. `A → A`, `A → B`. Strata are
  never pooled: a mixed cluster gets one verdict per stratum. The NIC family of a
  node is the set of drivers of its physical interfaces; a target without its own
  context takes the family its node reported as a source, else it is `unknown` (a
  warning in that block). Within a stratum each side pools the matching targets of
  its runs; the **source nodes** are the top level of the bootstrap, then the runs,
  then blocks of 5 s within each run.
- `verdict` on the primary quantile:
  - `WORSE` / `BETTER`: the interval excludes "no change";
  - `EQUIVALENT`: the TOST interval lies inside the margin ("statistically
    significant, practically irrelevant" if it also excludes "no change");
  - `INCONCLUSIVE`: neither; usually too few runs for the margin. More runs per side
    narrow the interval.
- Intervals of a relative margin are ratios (`1.000` = no change); the level is
  Bonferroni-adjusted over k = paths × strata (two paths, one stratum: 97.5 %, TOST
  95 %). The header prints k and how many times the samples the same power needs
  compared with k = 1 (four strata: ≈ 1.86 ×), so do not expect `equivalent` in many
  strata from a default run.
- An instance type that occurs with several NIC families is named in the header:
  the same machine type can sit on different NIC hardware, which is why results
  follow the observed family, never the instance type.
- `evidence`: `exploratory` with one run per node and side, `preliminary` with more,
  `strong` only with ≥ 5 runs per side **interleaved** in time (≥ 3 side switches),
  which a before/after kernel change cannot have. The sequential design compares the
  **same source nodes**: a `!` line per path counts the nodes on both sides and on one
  side only, and without a common node the label is `exploratory` (an observational
  comparison of two node groups, which differ in more than the declared factor).
- `failures` gets its own verdict only from 10 failures on both sides together; SYN
  retransmissions are diagnostics only.
- `local control`: the median of the loopback control. A significant shift means
  the measuring machine itself changed; weigh its size against the effect.
- `between-run spread`: the primary quantile of every run; if it is wide, more runs
  help more than more samples per run.
- `context, not verdict`: kernel, instance type (declared), CPU model, NIC family,
  ring sizes and MTU of both sides, one `both:` line when they agree,
  `before:`/`after:` when they differ. `!` marks a difference outside the declared
  factor. A changed NIC family or CPU model **on the same node** refuses the comparison (exit 5), because the node
  then measures another host; `--allow-context-mismatch` accepts it, printed.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | report produced (whatever the verdict) |
| 1 | internal error |
| 2 | usage error: missing `--design`/`--factor`, a margin without rationale, unreadable or invalid files, the same run given twice on one side (e.g. one file in two archives) |
| 5 | comparison refused: the sides overlap in time, a node's NIC family or CPU model changed, or the runs of a path differ in a load parameter |
