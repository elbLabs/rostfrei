# Rostfrei Studio

The first Studio slice is a local-first explorer for the Tracer
`MessageSeriesDefinition` and `ObservedMessageSeries` contracts.

It provides:

- expected and observed sample documents;
- deterministic causal layout using explicit parent identities;
- source ordinals independent from causal order;
- graph timing and command-outcome inspection;
- contract and partial-observation diagnostics;
- JSON paste and file import without sending data to a service; and
- a responsive semantic tree for narrow screens.

Studio intentionally does not derive an observed causal graph from the current
Tracer SSE feed. That feed does not yet carry complete message and causation
identities for every event.

Imports are limited to 8 MiB, 256 expected graphs, 127 levels of JSON nesting,
and 100,000 JSON values. Numeric payload values that JavaScript cannot represent
exactly are retained and displayed without precision loss.

## Development

```sh
cd studio
bun install
bun run dev
```

## Checks

```sh
bun run lint
bun run typecheck
bun run build
```
