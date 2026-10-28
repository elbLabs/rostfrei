# Domain model and kernel

**Model business meaning once, in executable domain code.**

Humans, execution, and tooling need the same model. A separately maintained model
or inferred ownership can disagree with the code that makes business decisions.

## What follows

- Make ownership and the aggregate's event vocabulary explicit in typed Rust
  declarations. Macros validate those declarations; source scraping is not authority.
- Keep actions, queries, policies, and invariants as ordinary Rust contracts.
  Policies interpret facts without effects; aggregates own deterministic state
  transitions. The runtime owns pending events, replay, concurrency, and persistence.
- Keep the kernel usable without macros, descriptors, or a transport. Adapters and
  explicit runtime registration connect it to the compiled domain model.
- Preserve historical meaning when decoding old events. A successful deserialize
  is not proof of compatible state or subsequent behavior. Convert only facts the
  old record actually contains; represent missing knowledge explicitly.
- Share the interpretation of stored facts across execution, fixtures, diagnostics,
  and event consumers. A different entry point must not silently change the codec.
- Import legacy state as a starting fact with provenance, never as an invented
  sequence of business events. Use the same [language](../glossary.md) in code,
  documentation, and tools.

## Tradeoff

Explicit ownership and registration cost wiring. In return, domain rules remain
testable without a broker and infrastructure can change without redefining them.
The framework can check structure; applications must verify business meaning.

See [domain declarations](../domain-macros/index.mdx) and
[project structure](../project-structure/index.mdx).
