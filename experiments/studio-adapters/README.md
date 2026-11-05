# Saved-evidence adapter experiments

Opt-in source-checkout probes for presenting existing Tracer captures in agent
hosts. The [tutorial](../../docs/agent-tutorial.md#optional-saved-evidence-readers)
defines the evidence and selection contract. Ordinary conversation remains the
default; these experiments are not loaded into your running agent automatically.

## Shared reader

The private [presentation package](../../packages/tracer-presentation/README.md)
owns evidence interpretation. From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm --filter @rostfrei/tracer-presentation typecheck
pnpm test:presentation
```

## Host probes

- [OpenCode](opencode/README.md): isolated server plugin, RPC, and native terminal
  interaction using an explicitly selected executable and saved capture.
- [Claude Code](claude-code/README.md): plugin discovery and native-mod event tests
  using an explicitly selected executable. The native test engine is distinct
  from interactive screen rendering and from MCP Apps.
- [MCP Apps](mcp-apps/README.md): a packaged stdio evidence reader and embedded
  viewer, qualified separately in the official Inspector browser host.

Each probe documents its input, prerequisites, output and cleanup. Use the complete
saved CLI evidence from the tutorial for real-data checks; synthetic fixtures
exercise presentation failures and do not establish runtime behavior.

These probes inspect saved evidence after the runtime stops. They carry no
Tracer execution credentials. Keep their generated results under `artifacts/`;
design decisions, host qualification and review findings belong in the draft PR.
