# `report`: before and after

[User guide](README.md) · previous: [Troubleshooting](06-troubleshooting.md)

`report` compares two sets of result files, e.g. before and after a kernel change,
and gives one verdict on the declared primary quantile per path.

```sh
k8s-netmeasure report --design sequential --factor kernel \
    --before mesh-results/before --after mesh-results/after \
    --equivalence-margin 10% --equivalence-rationale "p99 within ±10 % is no change for us" \
    [--primary p99] [--quantiles p50,p90,p99] [--replicates 10000] [--seed N] \
    [--allow-context-mismatch nic_family,cpu_model]
```

**State:** this first version supports the sequential design (`before` on the same
nodes, then `after`) with the factor `kernel`, and text output. The concurrent design
with crossover, the other factors, NIC-family strata and JSON output *follow*. The
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
| `--factor kernel` | required; what changed between the sides. Kernel differences are expected, everything else is checked |
| `--primary` | default `p99`: the **only** latency quantile that gets a verdict; the others are descriptive |
| `--equivalence-margin` | `10%` (on the ratio after/before) or `50us` (on the difference); needs `--equivalence-rationale`. Without a margin the verdict is never `equivalent` |
| `--before`, `--after` | files or directories (searched recursively; other JSON files are skipped) |

Choose the primary quantile and the margin **before** the measurement and write the
rationale down: chosen afterwards, they can be fitted to the result.

## Reading the output

- One section per path (overlay, underlay). Each side pools all real targets of its
  runs; the **source nodes** are the top level of the bootstrap, then the runs, then
  blocks of 5 s within each run.
- `verdict` on the primary quantile:
  - `WORSE` / `BETTER`: the interval excludes "no change";
  - `EQUIVALENT`: the TOST interval lies inside the margin ("statistically
    significant, practically irrelevant" if it also excludes "no change");
  - `INCONCLUSIVE`: neither; usually too few runs for the margin. More runs per side
    narrow the interval.
- Intervals of a relative margin are ratios (`1.000` = no change); the level is
  Bonferroni-adjusted over the paths compared (two paths: 97.5 %, TOST 95 %).
- `evidence`: `exploratory` with one run per node and side, `preliminary` with more,
  `strong` only with ≥ 5 runs per side **interleaved** in time (≥ 3 side switches),
  which a before/after kernel change cannot have.
- `failures` gets its own verdict only from 10 failures on both sides together; SYN
  retransmissions are diagnostics only.
- `local control`: the median of the loopback control. A significant shift means
  the measuring machine itself changed; weigh its size against the effect.
- `between-run spread`: the primary quantile of every run; if it is wide, more runs
  help more than more samples per run.
- `context, not verdict`: kernel, CPU model, NIC family, ring sizes and MTU of both
  sides, one `both:` line when they agree, `before:`/`after:` when they differ. `!`
  marks a difference outside the declared factor. A changed NIC family or
  CPU model **on the same node** refuses the comparison (exit 5), because the node
  then measures another host; `--allow-context-mismatch` accepts it, printed.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | report produced (whatever the verdict) |
| 1 | internal error |
| 2 | usage error: missing `--design`/`--factor`, a margin without rationale, unreadable or invalid files, the same run given twice on one side (e.g. one file in two archives) |
| 5 | comparison refused: the sides overlap in time, or a node's NIC family or CPU model changed |
