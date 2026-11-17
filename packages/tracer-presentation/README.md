# Tracer presentation core (provisional)

Open saved Tracer evidence, bind a message selection to its immutable snapshot,
and render bounded summaries, plain text or Mermaid. The core performs no I/O. Node **22.16+** is
required; installed-package qualification uses Node **24**.

## Use

Build in the checkout with `pnpm --dir packages/tracer-presentation build`.
The private package exports compiled JavaScript and declarations from `dist/`;
`pnpm pack` builds these automatically. An installed consumer imports:

```ts
import {
  readEvidence, createSelection, resolveSelection, renderText,
  summarizeEvidence, renderMermaid,
} from '@rostfrei/tracer-presentation'

const view = readEvidence(savedJsonText)
console.log(renderText(view))
console.log(summarizeEvidence(view))
if (view.kind === 'operation' && view.series) console.log(renderMermaid(view))

if (view.kind === 'operation' && view.series) {
  const selection = createSelection(view, chosenMessageId)
  const detail = resolveSelection(view, selection)
  console.log(renderText(view, { selection, includePayloads: true }))
}
```

File acquisition belongs to the caller. In checkout-only tools, the same exports
are available from `packages/tracer-presentation/src/index.ts` with Node's native
type stripping. Native Node does **not** strip installed TypeScript under
`node_modules`; consumers must use the built package export.

### Inspect a saved run

After building, export an existing file without a running application:

```sh
node packages/tracer-presentation/bin/render.mjs saved.json --output run.md
node packages/tracer-presentation/bin/render.mjs saved.json --format mermaid --output flow.mmd
```

The installed bin is `rostfrei-evidence`. Output files must be new; the adapter
never overwrites evidence. It reads original UTF-8 text from a regular file
(8 MiB maximum), retains exact numeric tokens and identifies the bytes by SHA-256.
Raw Mermaid retains that digest in an inert comment, outside the visible diagram.
Markdown leads with outcome, verdict and a small flow; details are expandable.
Unsupported/missing flow evidence stays explicit, with text available in Markdown.

`summarizeEvidence(view)` returns a bounded, payload-free `{ title, facts,
limitations, issues, diagnostics }` projection of strings suitable for transport. Render
these strings as text, escaping them for HTML/Markdown as appropriate. Conflicting
decisions remain qualified; passing a rejection test does not mean acceptance.
Titles use the command name. Capture summaries show gaps and anomalies; context,
schema versions and exact capture metadata remain in text and source details.
`issues` names concrete gaps from reader diagnostic codes without paths or IDs;
`diagnostics` retains the technical detail. Captures without diagnostics have no issues.

`renderMermaid(view, { maxNodes: 12 })` accepts only reader-produced operation
views with a finite capture. The limit is 1–80 messages. It uses `view.edges`,
generated aliases and escaped inert labels. Outcome, verdict and capture quality
are a caption, never causal nodes. Missing parents remain disconnected; omitted
messages/edges and clipped labels are explicit. A simulation or missing/invalid
capture raises a typed error. The original source remains authoritative.

The first-screen/detail split follows [progressive disclosure](https://www.nngroup.com/articles/progressive-disclosure/).
It is a presentation choice, not evidence of improved user comprehension.
Changing displays never executes or reruns a command.

### Inputs

`readEvidence(input, finiteCapture?)` accepts original JSON text or plain parsed
JSON objects. Supported inputs reuse the current Rust fields:

| Input | Returned `kind` |
| --- | --- |
| Saved CLI behavioral-test output (`report.operation`, nested finite `messageSeries`) | `operation` |
| `OperationSnapshot` plus optional `OperationMessageSeries`, as two arguments | `operation` |
| Saved execution before an operation was recorded | `incomplete` |
| Canonical `kind: "handlerSimulation"`, `committed: false` | `handlerSimulation` |

Finite `test` and `dispatch` captures retain their recorded mode. Continuous
observation documents, bare series, and historical `mode: "simulate"` operations
are unsupported. Simulation proposals receive no operation/message IDs or causal
edges. Studio-enriched proposal fields must not replace the canonical input.

## Selection and adapter migration

**A selection must include its original `snapshotId`.** Operation and message IDs
can recur across real executions, including events with different parents.

Replace old `{ operationId, messageId }` construction with
`createSelection(view, messageId)`. The helper returns a frozen
`{ snapshotId, operationId, messageId }`. Both `resolveSelection` and
`renderText({ selection })` enforce all three fields. Each `readEvidence` call
opens a distinct snapshot, even for identical bytes. Reopening requires explicit
re-selection; display changes reuse the existing view and selection.

For asynchronous UI callbacks, send the original three-field selection back to
the host and resolve it against the host-retained view. **Do not replace an old
callback's snapshot ID or call `createSelection` to silently rebind it.** Keep the
last valid selection when resolution fails. The snapshot ID is presentation
identity, not authorization: hosts still own their session/source access scope.

Views expose `snapshotId`, `sourceText`, optional `captureText`,
`provenance`, and `diagnostics`. Operation views also expose `captureStatus`,
`anomalousMessageIds`, and `unassociatedOutcomeIds`. Resolutions carry `path` and
message-relevant diagnostics. Renderers must use `view.edges`, not reconstruct
edges from every equality in raw records.

## Validation and evidence integrity

The reader uses Ajv 8 with bundled, byte-identical copies of Rust's generated
observed-v2 and behavioral-v2 schemas. Conformance tests check those copies
against `crates/rostfrei-tracer/schema/`. Runtime-only ordinal/identity and
rejection UTF-8/control-character checks supplement the schema in one shared
module. This validates supported presentation inputs, not application behavior.

- Accepted outcomes support omitted or explicit-null `value`; non-null content
  is invalid. Rejected outcomes retain their canonical rejection record.
- Explicit unsupported or mismatched release/CLI contract versions, catalog,
  definition, fixture or embedded schema versions fail closed. Missing release
  declarations produce `provenance: "unknown"`. `"supported"` describes declared
  contract compatibility, not authentication or proof of a matching source build.
- A malformed finite capture returns `captureStatus: "invalid"`, no `series`,
  and no edges. Independently valid operation/report facts and source text remain
  available. Selection is blocked. Root/provenance/acquisition failures throw.
- `captureStatus: "anomalous"` retains schema-valid records with diagnostics.
  Self-links, cycles and cross-correlation associations are withheld from normal
  causal edges. Outcomes with missing commands, correlation disagreement, or
  cross-source decision/identity conflicts are not returned as associated outcomes.
- Available operation, report command outcome, report observations and finite
  capture outcomes are reconciled by explicit command/response identities.
  Source-labelled discrepancies retain both records; no winner is inferred from
  suite verdict, ordering, event count or a redacted rejection.
- Missing parents stay disconnected. Expectations, fixture history and
  `report.observed` never substitute for a missing/invalid finite capture.

Diagnostics have stable `code`, `severity`, JSON-pointer `path`, and optional
`relatedPaths`/message identities. Important codes include `INVALID_CAPTURE`,
`INVALID_REPORT_SECTION`, `OUTCOME_CONFLICT`, `CAUSATION_CYCLE`,
`CROSS_CORRELATION`, `MISSING_COMMAND`, `MISSING_PARENT`, `PROVENANCE_UNKNOWN`,
and `PROVENANCE_DISAGREEMENT`. A two-argument capture uses `/captureInput` as its
source-pointer root. Bounded acquisition and identity errors expose `.code` and
`.path`, including `INPUT_LIMIT`, `DEPTH_LIMIT`, `UNSUPPORTED_CONTRACT`,
`STALE_SELECTION`, `INVALID_SELECTION`, `CAPTURE_UNAVAILABLE`, and `INVALID_VIEW`.

## Exact source, numbers and local views

Prefer original JSON text. `sourceText` preserves the supplied text unchanged;
`captureText` does the same for a separate text capture. `source` is a
detached frozen parsed representation. Both contain full evidence payloads.
Parsed-object inputs receive `SOURCE_LEXEMES_UNAVAILABLE`: serialization preserves
the supplied value, but cannot recover bytes or fractions already rounded by a
previous parser. Unsafe already-parsed integer values are rejected.

Every numeric token that would change on ordinary JSON serialization is retained
as a native `JSON.rawJSON` numeric value (`RawJsonNumber`). This includes
fractional rounding, underflow, exponent spelling and negative zero. Source
serialization and payload display preserve those tokens. Bounded integer metadata
is validated independently; operation event cursors and simulation stream versions
are exposed as exact decimal strings, never coerced from fractional tokens.

`Presentation` is a **local immutable view**, not a wire DTO. Native raw-JSON
values are not structured-cloneable. Transfer approved original JSON text and
selection identities through a host-owned protocol; do not put the whole view in
MCP structured content or an iframe message. Opening transferred text creates a
new view and requires explicit selection binding. Payload hiding in text does not
sanitize `source`, `sourceText`, report fields or the full view.

## Bounded plain text

`renderText(view, { includePayloads?, selection?, maxCharacters? })` starts with
the recorded verdict/decision and nearby limitations. Anomalous outcome records
are labelled separately. Selected detail includes its source pointer and, for a
valid edge, the named parent and exact causation/message-ID equality.

Payloads and rejection details require opt-in. Explicit null remains present;
omitted payloads are labelled **missing or redacted**. Settling, retrieval and
grouped capture limitations never imply downstream business completion.
Untrusted strings are quoted/escaped; use a plain-text sink, not HTML/Markdown.

Limits are enforced before indexing: 8 MiB combined serialized input, 64 nesting
levels, 200,000 JSON values, canonical 4,096 messages/outcomes each, and 4,096
simulation streams/proposals each. Oversize input is rejected, not clipped.
Text defaults to 16,000 characters (configurable 512–65,536), previews at most 40
records per section and 20 diagnostics/warnings, and bounds individual strings
and payload previews. Every truncation emits an omission marker/count. Complete
source text remains available locally for exact copy/inspection.

## Checks

```sh
pnpm --dir packages/tracer-presentation test
pnpm --dir packages/tracer-presentation typecheck
pnpm --dir packages/tracer-presentation test:package
```

The package probe packs, installs and imports the tarball from an unrelated
temporary consumer directory and checks the installed declarations. It needs
pnpm and access to its declared dependency cache/registry. Core tests are offline.

`test/canonical/README.md` records native fixture provenance and SHA-256 hashes.
Those unchanged accepted/rejected and earlier accepted captures are the
conformance baseline. Adversarial tests mutate separate in-memory copies;
`test/fixtures.mjs` remains explicitly synthetic. These checks validate the reader
and presentation, not a new live domain execution.
