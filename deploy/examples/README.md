# Example manifests

Minimal manifests for Phase 1; a Helm chart follows in Phase 2. How to use them, step
by step: [user guide, Running in Kubernetes and OpenShift](../../docs/user/05-kubernetes-openshift.md#example-manifests).

> [!WARNING]
> **`listen` has only minimal protection. Never expose it to the internet.** These
> manifests create no Service, Route or Ingress, and must not be given one.

| Directory | File | What |
|---|---|---|
| `listen/` | `namespace.yaml` | namespace `netmeasure` |
| | `rbac-underlay.yaml` | service account for hostNetwork pods; OpenShift: SCC `hostnetwork-v2` |
| | `listen-underlay.yaml` | DaemonSet, hostNetwork: underlay target and node listener on every worker |
| | `listen-overlay.yaml` | DaemonSet, normal pod: overlay target, NIC path via the node listener |
| | `networkpolicy-listen-overlay.yaml` | only overlay run pods reach the overlay listen pods |
| `run/` | `pvc-results.yaml` | volume for the result files |
| | `run-overlay.yaml` | Job: overlay measurement from one source node |
| | `run-underlay.yaml` | Job: underlay measurement from one source node |
| | `results-reader.yaml` | pod to fetch the result files from the volume |
| `mesh/` | `mesh.sh` | every node measures every other node, no hand-edited manifests ([README](mesh/README.md)) |

Placeholders to set: the image tag in both `kustomization.yaml` (`SET-ME`), and in
the Jobs `SOURCE-NODE`, the `--target` addresses and `PHASE`.

Check before applying (needs cluster access, changes nothing):

```sh
oc apply -k deploy/examples/listen --dry-run=server
```
