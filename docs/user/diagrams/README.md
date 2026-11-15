# Diagrams

Every diagram has a Graphviz source (`*.dot`) and a rendered `*.svg` next to it; the
documentation embeds the SVG. After changing a source, render it again with
Graphviz (any recent version; the committed files were rendered with 15.1.1):

```sh
for f in docs/user/diagrams/*.dot; do dot -Tsvg "$f" -o "${f%.dot}.svg"; done
```

| Source | Shows |
|---|---|
| [overview.dot](overview.dot) | measuring side (`run`) and target side (`listen`), ports and data flow |
| [workflow.dot](workflow.dot) | the recommended order of steps for a test series |
| [run-lifecycle.dot](run-lifecycle.dot) | `run` from start checks through the cycle loop to the exit code |
| [slots.dot](slots.dot) | slot schedule: round-robin, loopback control, warm-up, cycles |
| [cycle-validity.dot](cycle-validity.dot) | the validity reasons and annotations of a cycle |
| [listen-probes.dot](listen-probes.dot) | `listen` lifecycle and its Kubernetes probe answers |
| [node-listener.dot](node-listener.dot) | overlay measurement with a node listener (`--node-context-from`) |
| [deployment.dot](deployment.dot) | example manifests: overlay above underlay, measured paths and control requests between nodes |
| [threads.dot](threads.dot) | threads of `run` and `listen` and how signals are handled |
