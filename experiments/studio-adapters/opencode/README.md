# OpenCode presentation experiment

OpenCode V2 server plugin, bounded HTTP RPC and native keyboard inspection of
**saved** Tracer evidence. The shared `@rostfrei/tracer-presentation` package owns
validation and causal associations. The isolated probe targets OpenCode **2.0.29**;
the supported probe scope is described below.

## Run

Requires Linux, Python 3, `opencode` on PATH and the already-built shared package
with its runtime dependencies installed. The runner copies compiled `dist/`
exports and dependencies into a disposable installation; it never builds core.

```sh
pnpm --filter @rostfrei/tracer-presentation build
python3 experiments/studio-adapters/opencode/probe.py \
  --evidence /absolute/path/to/accepted.json \
  --evidence /absolute/path/to/rejected.json \
  --tui --tui-size 90x28
```

Repeat `--evidence` for up to 16 immutable UTF-8 files, each at most 8 MiB.
Omit `--tui` for headless RPC. The native qualification visits **every capture**,
selecting its second message when available, otherwise its sole message. The
selected message must have a payload for the reveal check. Thus the canonical
accepted event is a nondefault message; the rejected command is the only message
in its capture. Repeat with `--tui-size 120x40` for the larger viewport.

Without `--evidence`, the adapter uses an explicitly synthetic fixture with the
real core. `--core-double` is a separate synthetic-only diagnostic mode: it refuses
evidence arguments and does not qualify shared validation or real captures.

Every run reserves a **new directory** under
`artifacts/studio-adapters/research/`, then exclusively creates the report and
all `.tui.ansi`, `.tui.txt`, `.tui.frames.json` and `.integrity.json` sidecars
**before starting any subprocess**. Optional `--output` must name a report in a
new directory there. Existing files/directories, symlinked paths and output-name
collisions are refused, including aliases of inputs. Outputs are write-once.
Input digests are recorded after report/terminal writes and checked again after
the integrity sidecar is written. Raw ANSI and reconstructed viewport frames are
retained; the small VT decoder is not a general terminal/accessibility evaluator.
Reports retain adapter source hashes, core source/build hashes, installed package
and dependency hashes, the core revision and lockfile hash. The installed package
contains compiled JS with AJV, without core source TypeScript. Headless checks
reassemble every original-source page and compare its UTF-8 bytes to the input.

The runner owns isolated HOME/XDG/database/config paths and explicit loopback
server connections, excludes inherited credentials, and denies provider use.
`/tmp/opencode` is preferred; owned `artifacts/tmp` is the fallback. Unexpected
ancestor configuration blocks startup. Cleanup stops owned process groups and
removes temporary installations, retaining startup errors and cleanup diagnostics.

## Native interaction

**Ctrl+G** or `/tracer-evidence` opens captures, then messages. Selected kind/name,
compact identity, validated parent or outcome, capture metadata and limitations
appear first. `captureStatus` and core diagnostics remain visible through the
capture diagnostics action, including when an invalid capture has no selectable
messages. Causal relations come only from core `view.edges` and resolved outcomes.
**Exact IDs**, **Limitations**, **Reveal payload** and **Reveal
recorded outcome** lead to bounded text pages. Type an action's title to filter
the native menu, then Enter; Next/Previous page and Escape are keyboard-reachable.
Summary/text rows are selectable read-only rows (Enter redisplays them); disabled
rows are omitted by this host's select dialog.

Payloads and full rejection bodies are hidden by default; a recorded rejection
code can appear in the summary. Reveal is explicit and labelled; missing/redacted
payloads are not replaced by null. Detail uses escaped JSON text, with 160-character
pages over a prefix bounded to 3,072 characters, depth 16 and 256 values. Truncation
is labelled; full original source remains separately pageable through opt-in RPC
in at most 4,096-character pages.
Escape returns to the selected summary, then the remembered message page/capture.
Reopening retains selection within the same owned instance. A busy guard prevents
overlapping invocations; failed detail preparation does not commit a new selection.

The private 90×28 or 120×40 PTY checks current visible content, all exact-ID,
payload, outcome and limitation pages, back/reopen/cancel and composer focus with
an **unsent** sentinel. Its 180-second interaction deadline includes RPC waits;
the overall host-check budget is 300 seconds, with separate bounded cleanup waits.
Headless checks and native results are reported separately; no model session may
be created.

## Protocol and scope

Call `listCaptures({})` to obtain `{ captureId, snapshotId, operationId }`, then
`listMessages({ ...capture, offset: 0 })` for a page of at most 16 identities.
Each item contains the original core `createSelection(view, messageId)` result
under `selection`. Send `{ captureId, ...message.selection }` back unchanged.
All reads/selections require the original capture digest and **snapshotId**, plus
operation ID; message reads also require message ID. Reopening the core view creates
a new snapshot. Old callbacks fail rather than being silently rebound.

```js
adapter.inspectMessage({ captureId, snapshotId, operationId, messageId }) // bounded summary
adapter.selectMessage({ captureId, snapshotId, operationId, messageId })
adapter.selectedMessage() // original four-field identity, or null
adapter.messageDetail({ ...identity, section: "payload", offset: 0, reveal: true })
adapter.sourcePage({ captureId, snapshotId, operationId, offset: 0, reveal: true })
```

`showOperation` returns bounded metadata; `renderOperation` uses core bounded text.
`captureDetail({ captureId, snapshotId, operationId, offset: 0 })` pages capture
diagnostics without requiring a selectable message.
Neither full local views nor raw-JSON objects cross the wire. Exact numeric lexemes
travel inside detail/source **strings**, not ordinary JSON numeric fields. The
adapter rejects identity fields above 256 characters rather than clipping them.

`rpc.js` exports `Presentation` (`rostfrei.presentation.spike`). Transport is
`POST /api/rpc/rostfrei.presentation.spike/<method>` with `{"input": ...}`;
no-argument methods take `{}`. File paths are configured server-side, never RPC
arguments. Three tools expose operation text, bounded inspection and selection:
`rostfrei_presentation_show_operation`, `rostfrei_presentation_inspect_message`,
`rostfrei_presentation_selected_message`.

**Selection belongs to one explicitly owned plugin instance, not an OpenCode
session.** This is not a session-safe shared deployment. Model-driven tool use,
conversational explanation, desktop GUI and rendered MCP Apps remain unqualified.

## Focused checks

```sh
python3 -B -m unittest discover -s experiments/studio-adapters/opencode -p 'test_probe_support.py'
node --experimental-vm-modules --test experiments/studio-adapters/opencode/boundary.test.mjs
```

These use actual adapter/probe code with explicit core/host/process doubles for
fault paths. Node 24 is required for native raw-JSON regression values. Test the
real copied compiled installation, unchanged canonical files, numeric fidelity,
degraded capture handling and withheld anomalous edges with:

```sh
python3 experiments/studio-adapters/opencode/test_adapter.py --node /path/to/node24
```
