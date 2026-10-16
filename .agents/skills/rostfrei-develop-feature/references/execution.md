# Execute with the Rust CLI against real NATS

Install a skills-enabled setup CLI once, then run explicit setup inside the
application with its dependency resolved in `Cargo.lock`:

```sh
cargo install --locked --git https://github.com/elbLabs/rostfrei \
  --rev <skills-enabled-sourceRevision> rostfrei-structure
cargo rostfrei skills install
# After upgrading the application's framework dependency and lockfile:
cargo rostfrei skills update
```

Setup uses the application's resolved dependency, obtains its exact matching CLI
when needed and caches it under `$CARGO_HOME/rostfrei/tools` (default
`$HOME/.cargo/rostfrei/tools`; `ROSTFREI_TOOLS_DIR` overrides). Skills go in the
application's `.agents/skills`. Commit the managed baseline alongside them.
Updates preserve user-added files and block all skills on edited-file conflicts.
Adding the library or starting the host never installs skills.

Initial compilation requires Rust 1.98+, Cargo, Git, a native linker and source
access; consumer setup/execution requires no Python. PR #106 must be merged or
selected by its reviewed revision; published `v0.0.5-alpha` predates this setup
capability. Do not substitute a newer CLI's bundle for an unsupported dependency.
From this checkout, `cargo install --locked --path crates/rostfrei-structure`
installs the setup tool. `skills info` reports that executable's own provenance;
`references/release.json` records the selected installed skills' provenance.

The native runner uses the already-cached matching CLI without downloads, Cargo,
Git or a source checkout. Keep the cache available to the runtime environment.
Optional `--kit` imports remain exact-release checked; `--destination` overrides
the application-local path (or explicitly installs this CLI's bundled skills
when used outside a Cargo project).

## Source discovery

When this skill is discovered in the framework checkout, no generated metadata,
schema or Python bootstrap is needed. Use the local Rust CLI:

```sh
cargo rostfrei-dev skills info
cargo rostfrei-dev test .agents/skills/rostfrei-develop-feature/assets/add-first-bicycle.json \
  --output source-evidence.json
```

The CLI identifies its **compiled** revision and contract versions; Cargo rebuilds
it after HEAD changes. The schema is obtained from the live catalog. Installed
kits additionally contain a runtime-schema snapshot. Old locally generated
metadata, if present beside an example, is checked rather than silently trusted;
move a stale local file aside deliberately or install the matching CLI.

## Runtime and isolation

Use the application's real handlers, supervised Test workers, registered fixtures
and NATS event store on NATS Server **2.12.1+ with JetStream**. A disposable broker
or uniquely owned application namespace is required; a run/correlation ID does
not isolate resources. Do not reset an unknown shared broker. Serialize Test runs
until independent run isolation is available.

Every behavior run resets **all configured Test resources**: paired authoritative
journal and completed-outcome state, command/response and integration streams, quarantine and consumers,
plus retained Test operation and correlation evidence. It applies the
selected fixture's domain events. Normal resources are separate; external
stores/read models require explicit application reset support. Reuse the already
authorized specific fixture/suite TDD scope without redundant confirmations.

The current baseline framework host is the implemented `bike-rental-api` binary.
One Rust-only local recipe, with distinct control/dispatch capabilities and an
owned unique application/epoch already available in the environment, is:

```sh
# Separate terminal; disposable broker, no persistent volume:
docker run --rm --name "$OWNED_BROKER" -p 127.0.0.1:14222:4222 nats:2.12.15-alpine -js
# Framework checkout, while that broker runs:
: "${ROSTFREI_APPLICATION:?Set a unique owned application name}"
: "${ROSTFREI_COMPLETED_OUTCOME_EPOCH:?Set a new explicit fresh-authority epoch}"
ROSTFREI_NATS_URL=nats://127.0.0.1:14222 \
  cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer
```

The host defaults to `127.0.0.1:1309` (`ROSTFREI_API_ADDR` overrides it). It requires
distinct `ROSTFREI_API_TOKEN` and `ROSTFREI_DISPATCH_TOKEN`, provisions both traffic
scopes, and resets Test at startup. Optional `ROSTFREI_INSPECTION_TOKEN` enables
read-only production observation/quarantine; Test inspection uses the control
token. Export the application and epoch variables before startup. The epoch
selects fresh writable split authority; a unique app alone is insufficient.
Existing namespaces remain read-only by default: do not use this fresh setup as
an implicit migration. Repeated Test resets must retain Normal history/authority.
Stop the host and the owned broker after use
(`docker stop "$OWNED_BROKER"`). A consuming application supplies its own
application-linked host. The shared host also supports `cargo run --locked -p
bike-rental -- --tracer` through its default binary. Without the flag it runs the
normal application with no Tracer HTTP; older baseline hosts always enabled
Tracer and ignored the flag. Startup does not install skills.

## Installed skill execution

Set `ROSTFREI_TRACER_URL` and `ROSTFREI_API_TOKEN` in the agent environment:

```sh
SKILL="$PWD/.agents/skills/rostfrei-develop-feature"
cargo rostfrei test "$SKILL/assets/add-first-bicycle.json" --output behavior-evidence.json
```

For application-owned definitions, add `--release "$SKILL/references/release.json"`.
The CLI checks framework version, source revision and independent protocol
versions. Use clean release builds; dirty build flags are retained in evidence.
Match the application's framework revision too: the catalog alone identifies a
protocol, not an application release.

Direct invocation without Cargo on the runtime PATH is also supported:
`/path/to/cargo-rostfrei rostfrei test <definition.json> --output <new-evidence.json>`.
The command follows advertised same-origin links, validates the exact JSON, reads
the fixture, submits once and captures the report plus finite operation series.
It disables proxies, redirects and automatic retries and uses bounded timeouts.
Existing evidence is never overwritten. No dispatch credential is used by this
client. Never display tokens, put them in arguments/URLs or persist them.

Exit **0** means a reported pass with complete retrieval; **1** means a reported
failed suite with complete retrieval; **2** is a typed local blocked/incomplete
execution. Inspect diagnostics to distinguish an assertion mismatch from a
runner/environment issue. Real isolated execution is the normal TDD path.
Optional Tracer **Simulate handler** is discovered through
`simulateHandlerHrefTemplate` for explicit rule diagnostics. It invokes the real
handler over configured history; dependencies are not automatically sandboxed.
Its uncommitted proposals and core simulation unit tests are never feature
acceptance or a fallback for unavailable real execution.

For “what happened?”, read the returned operation and finite capture. For actions
initiated by another client/worker, discover optional `observation[]` relations
and read list/detail snapshots with the scope's token. Inspect health, partial
window/fidelity metadata and quarantine before explaining missing activity.
The Tracer skill describes these HTTP JSON/SSE workflows; no MCP adapter or
native observation subcommand is currently implemented.

Errors are stderr JSON with `error.kind`, `message`, `phase` and `details`.
Rust error kinds are `incomplete_response`, `request_failed`, `invalid_response`
and `setup_failed`; no server-side error protocol is invented. An interrupted
run response retains `runState: submission-outcome-unknown`. A later capture
failure preserves the complete report/verdict, `runState: reported`, and
`captureState: incomplete`. Structured errors are retained with available
evidence, without raw response-body diagnostics or credentials. Never retry an
ambiguous submission automatically.

Report settling/fidelity and actual expected-versus-observed differences. A
partial capture does not prove absence. Causal edges require explicit matching
`causationId`/`messageId`, never timestamps or array order. Evidence may contain
business payloads; redact it before sharing beyond the team.
