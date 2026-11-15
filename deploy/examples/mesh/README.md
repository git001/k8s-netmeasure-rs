# Mesh: every node measures every other node

`mesh.sh` runs measurements across all nodes without hand-edited manifests: it finds
the ready listeners itself, starts one `run` Job per source node against every other
node, waits, copies the result files out and deletes the Jobs. It is meant for
before/after comparisons on many nodes, e.g. a kernel change.

> [!WARNING]
> **`listen` has only minimal protection. Never expose it to the internet.** The
> script creates no Service, Route or Ingress. Delete the namespace afterwards.

Requirements: `oc` (or `kubectl` via `OC=kubectl`) logged in with rights to create a
namespace, a Role/RoleBinding for the SCC `hostnetwork-v2` and Jobs; `bash`.

## Usage

```sh
export IMAGE=ghcr.io/git001/k8s-netmeasure-rs@sha256:<digest>
# Only if the nodes need them (otherwise leave unset):
export NODE_SELECTOR='type=example'                      # extra node labels, k=v,…
export TOLERATIONS='[{"key":"type","operator":"Equal","value":"example","effect":"NoSchedule"}]'

deploy/examples/mesh/mesh.sh deploy              # namespace, listeners on every selected worker
deploy/examples/mesh/mesh.sh inventory           # zone, instance type, kernel, NIC drivers and rings per node
deploy/examples/mesh/mesh.sh series before 5     # 5 rounds, overlay and underlay alternating
# … change (e.g. kernel), wait until every node is back …
deploy/examples/mesh/mesh.sh deploy              # listeners again (new pods, new pod IPs)
deploy/examples/mesh/mesh.sh inventory           # NIC family unchanged per node?
deploy/examples/mesh/mesh.sh series after 5
deploy/examples/mesh/mesh.sh cleanup --all       # delete the namespace
```

### Only some nodes

`NODES` restricts everything to the named nodes (comma-separated node names), e.g. two
per zone and machine type to see which NIC family each has. `nodes` lists the
candidates with their zone and instance type to pick from:

```sh
deploy/examples/mesh/mesh.sh nodes               # NAME … ZONE INSTANCE-TYPE
export NODES=node-1a,node-1b,node-2a,node-2b,node-3a,node-3b
deploy/examples/mesh/mesh.sh deploy              # listeners on these six only
deploy/examples/mesh/mesh.sh inventory
deploy/examples/mesh/mesh.sh series types 1      # optional: these nodes measure each other
```

The listeners stay DaemonSets; `NODES` becomes a node affinity on `metadata.name`, so
every named node gets exactly one listener. A name that gets none (misspelt, excluded
by `NODE_SELECTOR`, or a taint without toleration) is reported after `deploy`. Running
`deploy` again without `NODES` returns to all selected nodes.

`round <overlay|underlay> <phase> <n>` runs a single round. Results land in
`$OUT/<phase>/<path>/round-<n>/<node>/` (`OUT` defaults to `./mesh-results`): the
result file and `exit-code` (0 every cycle valid, 4 at least one invalid; `no-result`
with a `run.log` if the Job produced nothing).

| Variable | Default | Meaning |
|---|---|---|
| `IMAGE` | — | image reference, preferably by digest (required for `deploy`, `round`, `series`) |
| `NODE_SELECTOR` | none | extra node labels, `key=value,…`; `node-role.kubernetes.io/worker` is always required |
| `NODES` | all | only these nodes, comma-separated node names (node affinity on `metadata.name`) |
| `TOLERATIONS` | `[]` | JSON array of tolerations for tainted nodes, for listeners and Jobs |
| `SAMPLES` | `1000` | counted samples per target and run |
| `OUT` | `./mesh-results` | local result directory |
| `TIMEOUT_S` | `900` | seconds to wait for one round |
| `PENDING_S` | `120` | give up on a Job pod that is still `Pending` (e.g. `Insufficient cpu`), log the scheduler's reason |
| `RUN_CPU_REQUEST` | `250m` | CPU request of a run pod; the limit stays 1 CPU, the quota `run` checks |
| `OC` | `oc` | client binary |

## What one round does

- One Job per source node, all in parallel; each measures every other node's
  listener (overlay: pod IPs, underlay: node IPs). A node never measures itself.
- Every run declares its node's zone and instance type from the node labels
  (`topology.kubernetes.io/zone`, `node.kubernetes.io/instance-type`), so result
  files and `report` carry them; a node without these labels leaves them `unknown`.
- Overlay sources need the node listener on their own node; a node without one is
  skipped and logged.
- The Job writes into an `emptyDir`; the pod waits after the run so that the file
  can be copied (`oc cp`), then the Job is deleted. No volume claim is needed.
- A Job pod that cannot be scheduled within `PENDING_S` (a full node: `Insufficient
  cpu`) is given up with the scheduler's reason; the round goes on with the other
  nodes. The request is only 250m: `run` checks the limit (1 CPU), and CPU missing
  during a cycle shows up as `cpu_throttled` and invalidates it.

## Load and duration

Each `run` opens 100 connections per second (10 ms interval) in total, spread over
its targets; a connection is a handshake and a reset, about four packets of 60–70
bytes, no payload. With every node measuring every other node at once, each node
sends and receives about 100 connections per second, roughly 400 packets per second
and 30 KB/s, far below what probes, monitoring and logging already produce.

Per node: one `run` pod (request 250m CPU, limit 1) during a round, and two listeners
(100m each) for the whole series. With `n` nodes and the default 1000 samples, a
round takes about `1050 × (n − 1) × 11 ms`: about 2 min for 12 nodes.
