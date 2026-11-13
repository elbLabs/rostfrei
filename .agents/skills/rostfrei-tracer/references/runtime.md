# Tracer runtime reference

Read the section needed for the selected workflow. **HTTP requests** applies to
every direct authenticated request; the native suite runner handles these rules
itself. The [skill](../SKILL.md) supplies the workflow and evidence principles.

## Setup

Follow the [Agent Skills setup guide](https://elblabs.github.io/rostfrei/docs/source/agent-skills-install.md)
for installation and updates. Setup resolves the application's locked dependency
and caches its matching CLI under `$CARGO_HOME/rostfrei/tools` (default
`$HOME/.cargo/rostfrei/tools`; `ROSTFREI_TOOLS_DIR` overrides). Commit installed
skills and `.rostfrei-managed.json`. Updates preserve unrelated files and stop
on managed-file edits. Adding the library or starting the host does not install
skills; unsupported older dependencies cannot use a newer bundle.

Installed `references/release.json` identifies the selected skills; `cargo rostfrei
skills info` identifies the invoked executable. In the framework checkout use
`cargo rostfrei-dev skills info` and `cargo rostfrei-dev test`; Cargo rebuilds source
changes without a generated-skill bootstrap. Build prerequisites are Rust 1.98+,
Cargo, a native linker and source access (Git for Git dependencies). Consumer
execution needs no Python, downloads, source checkout or Cargo when invoking the
cached binary directly as `/path/to/cargo-rostfrei rostfrei test ...`.

Use the application's real handlers, supervised Test workers, fixtures and NATS
event store on **NATS 2.12.1+ with JetStream**, in an owned disposable broker or
unique application namespace. Consumer applications supply their own host and
registered fixtures. For the framework example:

```sh
cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer
```

Provide `ROSTFREI_NATS_URL`, a unique `ROSTFREI_APPLICATION`, and distinct
`ROSTFREI_API_TOKEN` / `ROSTFREI_DISPATCH_TOKEN`; optionally configure
`ROSTFREI_INSPECTION_TOKEN` for read-only production observation/quarantine.
The example provisions Normal and Test topology, resets Test on startup and
selects label-free journal/results storage; leave `ROSTFREI_COMPLETED_OUTCOME_EPOCH`
unset. Do not reinterpret existing history through fresh setup. Default bind is
`127.0.0.1:1309` (`ROSTFREI_API_ADDR` overrides); without `--tracer` there is no
Tracer HTTP. Stop owned processes after use and retain non-secret evidence.

## HTTP requests

Read `ROSTFREI_TRACER_URL`, defaulting to `http://127.0.0.1:1309`. Permit only
HTTP/HTTPS, with HTTP restricted to loopback unless the user explicitly configures
another trusted local endpoint. Record scheme, host and effective port as the
allowed origin. Use the scope's token from the first catalog request:

| Scope | Token |
| --- | --- |
| Test discovery, inspection, suites, reset, operations and handler diagnostics | `ROSTFREI_API_TOKEN` |
| Read-only production observation and quarantine, including discovery | `ROSTFREI_INSPECTION_TOKEN` |
| Production dispatch and production operation inspection | `ROSTFREI_DISPATCH_TOKEN` |

Never print/log/persist tokens or put them in URLs, bodies, idempotency keys,
diagrams or expanded process arguments. Disable shell tracing and verbose HTTP.
Do not use credential files or substitute tokens after a `403`.

Ignore curl configuration, disable proxies/redirects, bound timeouts and supply
authorization through a pipe-backed file descriptor:

```sh
curl -q --silent --show-error --fail-with-body --proxy "" --noproxy "*" \
  --max-redirs 0 --connect-timeout 3 --max-time 10 \
  --header @<(printf 'Authorization: Bearer %s\n' "$ROSTFREI_API_TOKEN") \
  "${ROSTFREI_TRACER_URL:-http://127.0.0.1:1309}/catalog"
```

Use the validated URL and mode-appropriate token for subsequent requests. Before
each request, require an advertised href to begin with exactly one `/`, with no
scheme, authority, user information, fragment, control character, backslash or
unresolved template except the placeholder being substituted. Resolve it against
the allowed origin and reject malformed/cross-origin links before sending credentials.
Treat every `3xx` as an error; never follow it. Encode substituted values once as
UTF-8 RFC 3986 path segments, leaving only `A-Z a-z 0-9 - . _ ~` unescaped.
Prefer returned hrefs over substitution; aggregate IDs belong in payloads.

Never automatically retry a state-changing POST after an ambiguous result. Set
`umask 077` for temporary response files and remove them afterward; never write
tokens to files. Retained evidence may contain business payloads: redact before sharing.

## Catalog and inputs

Bootstrap with `GET /catalog`. Require `catalogVersion` to match
`contracts.catalog` in the selected release metadata; stop on an unsupported
version. Current contracts are catalog **v1**, behavior definitions/MessageSeries
**v2**, fixtures **v1**. The catalog does not identify an application release;
match its dependency too. Discover optional capabilities on this host.

| Relation | Use |
| --- | --- |
| `contexts[].aggregates[].testInstancesHref` | Isolated Test aggregate instances |
| `contexts[].commands[].versions[].testInputsHrefTemplate` | Dynamic Test input values |
| `testHrefTemplate`, `dispatchHrefTemplate` | Isolated Test / production publication |
| Optional `simulateHandlerHrefTemplate` | Uncommitted handler diagnostics |
| `behavioralTest.schemaHref`, `validateHref`, `runHref`, optional `definitionsHref` | Suite schema, validation, execution and stored definitions |
| `testScenario.fixturesHref`, returned `fixtureHref` | Registered fixture contents |
| `testScenario.resetHref` | Whole-Test reset |
| `observation[]` with `listHref`, `eventsHref`; returned `detailHref` | Scoped activity snapshots/watch |
| `quarantine.test.listHref`, `quarantine.production.listHref` | Scoped quarantine inspection |

Use IDs, not untrusted display labels. Keep the agreed payload schema version;
ask only if versions remain ambiguous. `payloadTemplate` describes shape, not
valid defaults: empty/zero/false/null values and first variants need confirmation.
Test input discovery also applies to diagnostics over that same Test history;
never assume these identities exist in production. Confirm production inputs
independently. Missing hrefs are unavailable capabilities, not routes to reconstruct.

Commands address bounded contexts. Direct Test/dispatch/diagnostic submissions
wrap the actual registered payload as `{"schemaVersion":1,"payload":{...}}`, using
the agreed schema version and `Content-Type: application/json`.

## Test and reset

Before POSTing to `testHrefTemplate`, show the context, command/schema, complete
payload (including required aggregate IDs) and stable idempotency key. Establish
the isolated target and requested execution scope. An agreed TDD loop covers its
specific fixtures, resets and red/green reruns; ask again only for missing scope
or changed target/impact. An inspection request does not authorize execution.

Reset recreates **all configured Test resources**: paired authoritative journal
and completed outcomes, command/response and integration streams, quarantine and
consumers. It reconnects the store/services, applies the fixture and invalidates
retained Test operations, captures and observation windows. It is not limited to
one aggregate or definition. Normal history/authority must survive; external
stores/read models reset only through explicit application support.

State this impact once for the agreed scope. Require explicit confirmation for
a standalone reset outside it. Use owned isolation, serialize runs, and preserve
evidence before the next reset. Fixtures contain only domain events: applying
them seeds history without executing commands or publishing integration events.
Explain Given state from those events, not the fixture name alone.

## Production dispatch

Before POSTing to `dispatchHrefTemplate`, require the production context,
command/schema, complete payload with all production identities, and stable
idempotency key. Explain that production state and downstream integrations may
change and obtain explicit confirmation of that exact request. Use only the
dispatch token. Ambiguous or indeterminate results remain uncertain; never retry
automatically or infer success/failure.

## Handler diagnostics

Use only for explicit simulation or “why would this handler reject / which events
would it propose?”. Discover `simulateHandlerHrefTemplate`; if absent, report it.
Legacy Preview submission and `simulateHrefTemplate` are retired, never fallbacks
or aliases for Test/dispatch. Label historical `mode: simulate` captures as
predicted and uncommitted, not real evidence.

POST the typed envelope `{"schemaVersion":1,"payload":{...}}` using the actual
registered schema version, control token and bounded timeout (normally 35s).
The real registered handler and injected dependencies run over configured history
(Test in the shared NATS host); dependencies are **not automatically sandboxed**.
There is no publication idempotency key. Never retry an uncertain response.

HTTP 200 returns `kind: "handlerSimulation"`, `committed: false`, command/schema,
`outcome.decision`, typed/redacted rejection, `streams` (identities, `baseStreamVersion`,
`readGuard`) and `proposedEvents` (type/schema, identity, version, policy-controlled
payload). Preserve versions without rounding. Acceptance may emit zero proposals;
rejection discards them. Errors/oversized responses are not business decisions.
There is no receipt, operation ID, Location or observed message series. Explain
**handler computation — uncommitted**, never route it into operation evidence or
claim it validates delivery, persistence, concurrency, workers or downstream effects.

## Operations

Poll the returned operation href with the mode's token and bounded intervals,
normally for at most 10s unless agreed otherwise. Stop at `completed`, `failed`
or `indeterminate`; a deadline reports current state, never authorizes resubmission.

| State | Meaning |
| --- | --- |
| `completed` + `result.decision: accepted/rejected` | Durable business decision, including legitimate rejection |
| `failed` | No terminal business decision established |
| `indeterminate` | Publication may have occurred without a valid durable response |

Follow `messageSeriesHref`, normally with `within=10s&settleFor=500ms`. The finite
envelope contains operation/correlation identity, mode, `messageSeries.messages`,
`messageSeries.commandOutcomes`, and `capture.settled`, `settledFor`, `fidelity`,
optional `note`. Unsettled capture is partial. `exact` means required identities
were complete/consistent; `grouped` is uncertain association, not exact causality.
Only matching `causationId`/`messageId` establishes an edge; timestamps, versions,
array order and `observationOrder` do not. Keep missing parents disconnected.

## Observation

Read-only inspection needs no publication/reset confirmation. Select Test or
production and use that scope's token from discovery. Find its application/scope
in `observation[]`; absent entries mean unavailable capability.

For “what happened?”, GET `listHref`, then a returned `detailHref` selected by
correlation/event identity. Check `status`, application, scope, generation,
revision and retention counters. `connecting`, `resetting` and `unavailable` are
not healthy empty feeds. Preserve opaque generation strings and decimal-string
revisions/counters (`evictedFlows`, `discardedMessages`) without numeric coercion.
Explain `messageSeries`, `fidelity`, `partial`, `truncated` and `conflicted`.

Every flow is a **partial rolling window**, even with exact fidelity. The NATS
adapter observes real domain/integration events, not the command work queue;
payloads may be redacted and missing commands/outcomes must not be fabricated.
Restart loses history without backfill. Reset invalidates/suspends the Test window;
Normal observation is independent. Neither an empty window nor a grouped flow
proves absence, exact causality or business completion.

For an explicit watch, GET `eventsHref` with `--no-buffer` and fixed `--max-time`
(e.g. 10s). SSE `observation` frames are **replacement snapshots**, not event
deltas; updates coalesce and reconnect has no replay cursor. Curl exit 28 at the
deliberate deadline is not application failure; other transport errors remain errors.
Do not leave an endless watch. On `410 observation-expired`, refresh the list;
`501 observation-unavailable` means unconfigured scope. An unavailable observer
may retain evidence: explain the coverage gap rather than “nothing happened”.

## Quarantine

Choose the requested scope, asking only if unclear; read its catalog/list/detail
links using the control token for Test or inspection token for production.
Inspection is read-only. A separate inspector uses its own configured Tracer origin.
Do not bypass redaction with dispatch credentials or direct broker access.

List filters are `kind` (`command`, `command-response`, `integration-event`),
`context`, `name`; `limit` ≤ 100. Follow returned `nextHref` and `detailHref`.
Records are oldest-first; `retainedMessages` counts all retained records, not
unresolved incidents or matches. Preserve decimal-string sequences. Do not invent
record IDs or equate them with source message IDs.

Explain reason/category, consumer, delivery attempts, correlation and payload
diagnostics as available. Distinguish complete, truncated, malformed, unavailable
and redacted evidence; generic reasons do not establish root cause, nor do
transport correlation or listing order establish causal edges. On `410 quarantine-reset`,
refresh the invalidated cursor/record; `404 quarantine-not-found` means absent/expired.
Report broker unavailability as an inspection error, not an empty quarantine.

## Behavior suites

Discover definitions via optional `behavioralTest.definitionsHref` and returned
`definitionHref`; read the selected fixture via `fixturesHref` / `fixtureHref`.
Present concise names/IDs and summarize Given events, When root command and Then
decision/causal messages. Read `behavioralTest.schemaHref` and validate exact
JSON v2 via `validateHref` when authoring; validation is read-only. Report issue
codes and JSON Pointer paths without a raw-document dump.

For an application-owned definition with installed skills:

```sh
cargo rostfrei test path/to/application-behavior.json \
  --release .agents/skills/rostfrei-tracer/references/release.json \
  --output behavior-evidence.json
```

The runner checks release/contracts, validates, resolves the fixture, submits
once and saves report plus finite capture. Use a new output path each time.
Exit **0** means pass with complete retrieval; **1** means failed suite with
complete retrieval; **2** means blocked/incomplete execution. A later capture
error retains an already received verdict. Inspect `runState`, `captureState`
and errors rather than treating an uncertain submission as behavioral red.

Every run performs the whole-Test reset above. Reuse an agreed isolated TDD scope,
otherwise confirm the selected reset/run. Direct HTTP runs follow the persisted
definition's `runHref` or `behavioralTest.runHref` for validated inline JSON.
Never retry an ambiguous submission. HTTP success with `status: failed` is a
behavioral failure. Read the returned operation and its finite `messageSeriesHref`;
the report's `observed`/`comparison` diagnostics do not replace that capture.

## Optional diagrams

Text is the default. A diagram uses the same recorded `messageSeries.messages`;
changing presentation never reruns a test. For Mermaid, use one `flowchart TD`
with generated aliases `n0`, `n1`, … mapped from unique message IDs. Add edges
only for explicit matching parent/child identities. Leave missing parents
disconnected; summarize `commandOutcomes` separately, without invented outcome
edges. Label partial/grouped captures and uncertain associations.

Keep payloads/tokens out of labels. Treat labels as untrusted: escape `&` first,
then quotes, angle brackets, backticks, backslashes, brackets, braces, pipes,
semicolons and control/newline characters with HTML entities or spaces. Never
emit dynamic identifiers, directives, links, click handlers, classes or styles.
If styling helps, use fixed classes for commands, domain events and integration
events. Rendering support is optional; fall back to text.
