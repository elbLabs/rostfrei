# Saved-evidence MCP Apps spike

A small, read-only **official MCP Apps** stdio server and self-contained viewer.
One explicitly configured saved CLI evidence file is loaded once per process.
It exposes `show_operation`, `inspect_message`, `select_message`, and
`get_selection`. Inspection does not connect to Tracer or NATS.

## Build and use

Use Node **24.22.2** and this experiment's lockfile, independently of the root
workspace. The source build consumes `packages/tracer-presentation/src/index.ts`.
Its public reader, selection resolver, and text renderer run on the server;
the browser uses their rendered strings, native buttons and inert `textContent`.
The core is bundled into the built server, including its JSON schemas.

```sh
cd experiments/studio-adapters/mcp-apps
npm ci --ignore-scripts
npm run check
npm run build
node /absolute/path/to/dist/server.js --evidence /absolute/path/to/saved.json
```

The final command speaks MCP on stdin/stdout. Configure it in a host using
absolute paths; no launch-time build is required. For Inspector 2.14.0, a local
read-only session config looks like:

```json
{
  "mcpServers": {
    "saved-evidence": {
      "command": "/absolute/path/to/node",
      "args": ["/absolute/path/to/dist/server.js", "--evidence", "/absolute/path/to/saved.json"],
      "protocolEra": "legacy"
    }
  }
}
```

Use the locally pinned Inspector launcher:

```sh
node node_modules/@modelcontextprotocol/inspector/clients/launcher/build/index.js \
  --web --config /absolute/path/to/session.json
```

For isolated acceptance, `npm run qualify` builds, packs, inspects, scratch-installs,
then tests the installed server from an unrelated directory. It writes logs,
transcripts and screenshots under `artifacts/studio-adapters/mcp-apps/` by default.
Set `EVIDENCE_DIR` to a directory containing the canonical accepted/rejected
tutorial filenames and select a provisioned browser with `CHROME_PATH` (or
`CHROME_BIN`):

```sh
EVIDENCE_DIR=/absolute/path/to/saved-captures \
CHROME_PATH=/absolute/path/to/chrome npm run qualify
```

For a fresh checkout, copy `packages/tracer-presentation/test/canonical/accepted.json`
and `rejected.json` into that directory as `rent-available-bicycle.json` and
`reject-unavailable-bicycle.json`. `EVIDENCE_DIR` also applies to `npm test`;
no ignored research inputs are needed. `ARTIFACT_DIR` changes the output directory.
`BROWSER_LIBS` optionally supplies nonstandard shared libraries; otherwise the
inherited library path is preserved. Set `BROWSER_NO_SANDBOX=1` only when required
by the test environment.

## Identity and fallback

- MCP initialization negotiates `io.modelcontextprotocol/ui` and
  `text/html;profile=mcp-app`; absent/wrong MIME gives ordinary text tools.
  This spike qualifies MCP **2025-11-25**, not the modern request-metadata era.
- `show_operation` always returns useful text, unchanged `evidenceJson`, a SHA-256
  byte identity and a random connection-local `viewId`. Its browser-only
  `presentation` is derived text, not a replacement evidence schema.
- Pass the complete returned `binding` to inspect/select/readback. Canonical
  message IDs are never replaced with graph-node IDs. The shared core's
  `snapshotId` remains server-local selection provenance as well.
- Only one view is active per connection. Reopening replaces the view ID and
  preserves the validated selection; superseded frames and foreign connections
  fail explicitly. Restart clears selection. Changing the configured file after
  startup does not change loaded bytes.
- The viewer inspects locally immediately. With negotiated host `serverTools`,
  writes are serialized. Local highlight, acknowledged write and confirmed
  `get_selection` readback are distinct states. Refusal preserves the last
  confirmed identity; transport or readback failure leaves sharing **unknown**,
  since the server selection may have changed. Reopen to read current state.
  Feedback appears beside details and clear controls, with a live announcement.
  Host receipt does not establish model consumption. Text/reader changes preserve
  selection. No model calls are used by the tests.

## Bounds and authority

The owner selects one absolute regular file, at most **1 MiB**; the final path
must not be a symlink. Parent directories are owner-controlled.
Fatal UTF-8 decoding rejects invalid encoding and a leading BOM explicitly;
every returned evidence string re-encodes to its SHA-256-identified bytes.
Nonblocking no-follow open and descriptor validation reject nonregular inputs
(including a writerless FIFO) before reading. The core enforces
its documented depth/schema/consistency checks; malformed captures retain
independent report facts where the core permits them. Complete tool results are
limited to **8 MiB**, with explicit errors rather than truncation.

The configured evidence is approved in full for this host: fixture inputs,
metadata, raw JSON and payload detail are available. A digest identifies bytes,
not authenticity or application/scope provenance. No filesystem path is accepted
through a tool, and no execution, reset, capture, dispatch or credential surface
exists. The resource declares no network/resource/frame domains or permissions.
Core validation stays on Node because its AJV runtime compilation is incompatible
with the host iframe's restrictive CSP.

## Qualification scope

The tests use real stdio subprocesses and the **actual official Inspector web
host**, not a custom renderer. Both real saved captures are used with the runtime
stopped. Synthetic corruption, hostile labels, large numbers, host selection
denial, failed readback/clear and container/text sizing controls are explicitly
labelled. Either canonical connection failure fails qualification; negative
browser probes exercise both gates. The installed source hashes and embedded
server source map must agree with current adapter/core sources after the run.

Inspector's native page has a wide minimum layout: a 390px browser viewport did
not produce a narrow app iframe. Narrow app reflow is tested by explicitly sizing
the real host's outer iframe (390px and 320px), retaining its real sandbox and
bridge. This does not qualify Inspector itself for mobile use. Screenshots cover
light/dark, wide/narrow app containers, selected details and 200% text; automated
focus and accessibility-tree checks do not replace screen-reader review.

Other hosts, modern MCP negotiation, model-consumed selection and full
accessibility certification remain unqualified. The qualification runner retains
package provenance, protocol transcripts and browser diagnostics in its output
directory; interpret each result at that tested host and surface.

The package retains the repository's EUPL-1.2 license. Bundled browser dependency
licenses and build/source hashes are emitted under `dist/`; server source maps
include the corresponding bundled source.
