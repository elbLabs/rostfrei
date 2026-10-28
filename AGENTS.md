# Repository guidance

## NATS implementation

- Prefer native NATS/JetStream features and official client APIs for batching, ordering, retries, and recovery.
- Check native capabilities before building custom machinery; briefly document any concrete gap that requires it.
- Preserve Rostfrei's domain validation and consistency guarantees when adopting native features.

## Documentation

- `docs/README.md` is the shared entry point for humans and agents. `docs/` is the
  canonical product documentation; the website renders it directly. Read the
  relevant guide before changing its behavior and update that same guide afterward.
- Keep agent-only files focused on workflow and tool use. Product concepts and
  contracts belong in the shared docs, with examples readable without rendering.
- Write for someone using or maintaining the current system. Keep the shortest
  explanation that makes the behavior, constraint, or tradeoff clear.
- Update the existing guide instead of adding a document for each change. Keep
  one authoritative explanation and link to it from other entry points.
- Read the [architectural maxims](docs/adr/README.md) when implementing or reviewing
  a change. ADRs state lasting principles, their reasons, and accepted tradeoffs.
  Change a principle deliberately when the tradeoff changes. API renames,
  individual macros, fixes, optimizations, and implementation steps normally
  belong in existing guides, code, tests, or the changelog. Add an ADR only for a
  distinct architectural tradeoff; aim for one page.
- Keep plans, review diaries, test-run transcripts, temporary artifact paths,
  agent handoffs, and completed checklists in issues or PRs. Git preserves old
  designs; remove superseded material rather than accumulating historical notes.
- Preserve actionable setup, compatibility, migration, and recovery instructions.
  Link to tests instead of narrating everything they prove.
- When moving or removing docs, update incoming links, including website content.

See [the docs index](docs/README.md) and [development commands](docs/development.md).
