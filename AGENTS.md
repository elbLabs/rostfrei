# Developing Rostfrei

This harness guides changes to the Rostfrei framework, adapters, tooling, and
examples in this repository. Start with [the development guide](docs/development.md)
and [the framework testing policy](docs/testing/strategy.md).

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

## Testing decisions

- State the observable contract being changed and find its existing test owner
  before adding tests. Extend that suite instead of creating another
  `review_*`, `*_fixed`, or issue-number suite.
- Prefer real, isolated NATS integration tests for runtime behavior involving
  persistence, delivery, replay, concurrency, or recovery. Assert durable state
  and externally visible effects, including the absence of duplicate effects.
- Keep focused broker-free tests for algorithms, validation, codecs, policy,
  and public API/compile contracts. Choose representative valid, invalid, and
  boundary cases; add more only for a distinct failure mechanism or regression.
- Test a rule thoroughly at its owning layer. At another boundary, test the
  wiring or translation that boundary adds rather than repeating the whole matrix.
- Prefer a small set of decisive scenarios. Remove narrower tests when a retained
  contract already catches the same failure; moving every old case into a table
  or a faster layer does not reduce maintenance. Keep additional cases for distinct
  failure mechanisms, not every variation of the same input.
- Import compiled crates in integration tests. Keep private tests colocated;
  importing production `.rs` modules also duplicates their nested unit tests.
- Use deterministic gates for races, bounded waits for observations, and a
  disposable broker through `cargo xtask test`. Missing test infrastructure
  is a setup failure. Restart and fault tests must control their own resources.
- Preserve focused regressions for duplicate execution, partial commits,
  authority/scope confusion, expired-result disclosure, and ACK-before-durability.
  Retain a decisive assertion for each distinct mechanism rather than every
  historical reproduction of it.
- Keep independently failing scenarios independently runnable. Combining cases
  into one large test solely to lower the count is not consolidation.

## Development loop

1. Describe the expected behavior and select the smallest relevant existing suite.
2. For a behavior change or bug fix, add or adjust the decisive regression and
   establish that it detects the missing behavior before implementing the fix.
3. Implement, then run that suite and the affected integration boundary. For
   documentation or mechanical changes, use the relevant checks without inventing
   behavior tests.
4. Run formatting, affected lints, and the checks required by the changed area.
   Shared runtime or test-harness changes need broader workspace verification.
5. Report the commands and results, including any unexecuted checks. Stop repeating
   passing suites unless a subsequent change or unresolved concern warrants it.

Use the current commands in `docs/testing/strategy.md` and current source for
development decisions.

For Studio work, also follow `studio/AGENTS.md`.
