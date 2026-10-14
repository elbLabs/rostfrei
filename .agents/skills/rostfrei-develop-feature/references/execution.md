# Execute with the Rust CLI against real NATS

Install the CLI from the kit's exact `sourceRevision` (read `references/release.json`
in an installed skill). Initial compilation needs Rust 1.98+, Cargo, Git, a native
linker and registry/Git network access, but no Python or system TLS development
packages:

```sh
cargo install --locked --git https://github.com/elbLabs/rostfrei \
  --rev <sourceRevision> rostfrei-structure
cargo rostfrei skills info
```

From a matching source checkout, `cargo install --locked --path
crates/rostfrei-structure` is equivalent. The installed `cargo-rostfrei` native
binary runs offline with respect to package registries and the source checkout;
execution needs only the application Tracer endpoint. The portable skills kit
does not carry a platform-specific executable. Cargo builds the CLI for the
consumer's platform.

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

Every behavior run resets **all configured Test resources**: event history and
receipts, command/response and integration streams, quarantine and consumers,
plus retained Test/Preview operation and correlation evidence. It applies the
selected fixture's domain events. Normal resources are separate; external
stores/read models require explicit application reset support. Reuse the already
authorized specific fixture/suite TDD scope without redundant confirmations.

The current baseline framework host is the implemented `bike-rental-api` binary.
One Rust-only local recipe, with distinct control/dispatch capabilities already
available in the environment, is:

```sh
# Separate terminal; disposable broker, no persistent volume:
docker run --rm --name rostfrei-skills-nats -p 127.0.0.1:14222:4222 nats:2.12.1-alpine -js
# Framework checkout, while that broker runs:
ROSTFREI_NATS_URL=nats://127.0.0.1:14222 \
  cargo run --locked -p bike-rental --bin bike-rental-api
```

The host defaults to `127.0.0.1:1309` (`ROSTFREI_API_ADDR` overrides it). It requires
distinct `ROSTFREI_API_TOKEN` and `ROSTFREI_DISPATCH_TOKEN`, provisions both traffic
scopes, and resets Test at startup. Stop the host and the owned broker after use
(`docker stop rostfrei-skills-nats`). A consuming application supplies its own
application-linked host. #107's standard `cargo run --locked -p bike-rental --
--tracer` startup belongs to that separate host revision; final combined release
and fresh-session qualification remain explicit integration gates.

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
runner/environment issue. Preview is prediction, never acceptance.

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
