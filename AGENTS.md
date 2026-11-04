# Developing Rostfrei

This harness guides changes to the Rostfrei framework, adapters, tooling, and
examples in this repository. Start with [the development guide](docs/development.md)
and [the framework testing policy](docs/testing/strategy.md).

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
  disposable broker through `scripts/test_nats.py`. Missing test infrastructure
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

Use the current commands in `docs/testing/strategy.md`. Keep historical acceptance
documents as context and use current source and the testing policy for development
decisions.

For Studio work, also follow `studio/AGENTS.md`.
