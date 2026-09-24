# Tracer Studio

Tracer Studio is a React/Vite client for command execution, Tracer behavioral
tests, and causal message-series visualization.

Select a test to read its Given/When/Then summary and expected message flow.
Run it to inspect the observed flow, test verdict, and command outcome. Message
cards expose payload summaries; selecting a card opens its request, response,
relationship, and metadata in a persistent inspector. Narrow screens use a
message list with a dedicated details view.

Use the **Canvas / Workbench** switch in the top bar to compare layouts. Canvas
gives the flow the full working area, with navigation and message details opened
on demand and a collapsible scenario summary. Workbench retains the expanded
summary and persistent inspector. Switching keeps the current test, run, and
message selection; the layout preference is saved in this browser.
Keyboard shortcuts: **1** selects Canvas and **2** selects Workbench. Shortcuts
are ignored while editing text or using modifier keys.

Open **Command** (or **Ctrl/⌘ Shift K**) to use the catalog-driven command form.
**Preview** reads isolated Test history without publishing; **Test bus** uses
the isolated command pipeline and can append events and trigger integrations.
Test bus is the default for commands that advertise that capability. Commands
remain available even when the catalog has no behavioral-test repository.
Identifiers are editable, discovered payload options refresh after Test runs,
and numeric options retain their exact JSON representation. An unconfirmed Test
submission keeps its idempotency key and payload for an explicit retry.

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Run the Chrome/Puppeteer interaction smoke test with:

```sh
pnpm test:ui
```

The smoke test intercepts every Tracer request and uses deterministic HTTP
fixtures from `scripts/fixtures.mjs`, including connection failures and retry,
reruns, rejection, failed expectations, storage failure, and responsive layouts.
These fixtures are test-only and are not included in the application bundle.
The smoke test does not publish to a running Tracer. Set `CHROME_BIN` if Chrome
is installed outside the standard
locations; container environments that require it can set `CHROME_NO_SANDBOX=1`.

## Browser inspection

Use the shared browser tools to capture named, deterministic UI scenarios:

```sh
pnpm inspect:ui --list
pnpm inspect:ui --scene accepted
pnpm inspect:ui --scene all
pnpm inspect:ui --scene command --viewport 390x844
pnpm inspect:ui --scene narrow --out artifacts/after
```

The inspector starts Vite on an available port and launches a fresh Chrome
session. Named scenarios intercept every API request and label their synthetic
responses as visual fixtures. They cover the workbench overview, command form,
accepted/rejected Preview, pending and indeterminate results, a 25-message
branching graph, long payloads, and the narrow-screen inspector. Unknown API
requests fail rather than reaching a backend.

Each capture writes `screenshot.png`, `state.json`, `accessibility.json`, and
`diagnostics.json` under the gitignored `artifacts/inspect/` directory (or `--out`).
State includes panel/card bounds, selection, inspector content, and viewport
visibility. Inspect the screenshot to evaluate wrapping, clipping, and contrast;
browser exceptions, failed requests, unexpected HTTP errors, and missing fixture
routes fail the capture. Fixture sessions freeze wall time/UUIDs and disable motion.

`--headed` keeps Chrome open: Enter recaptures and `q` exits. To inspect a running
Studio with its actual API instead of fixtures, use its URL:

```sh
pnpm inspect:ui --url http://127.0.0.1:4173 --headed
```

Live inspection uses a separate browser profile and initially only reads the UI;
further actions are manual. Artifacts identify live versus mocked sources.
`scripts/browser.mjs` provides the shared server/browser lifecycle, layout settling,
diagnostics, and capture helpers. `scripts/visual-scenarios.mjs` contains scenario
recipes, and `scripts/fixtures.mjs` imports the canonical bike-rental fixtures and
behavioral definitions. These test-only resources are not bundled into Studio.

## Tracer connection

The development and preview servers proxy `/api` to `http://127.0.0.1:1309`.
Override the target or control token when needed:

```sh
VITE_TRACER_TARGET=http://127.0.0.1:1309 \
VITE_TRACER_TOKEN=local-development-token \
pnpm dev
```

Studio discovers commands and optional behavioral tests through Catalog v1 and
follows advertised action and operation links. It requires a reachable Tracer API.
Connection failures display the failed
request, HTTP status when available, connection settings, and a **Retry connection**
action. Discovery requests time out after 10 seconds; test submissions after
60 seconds. Retrying a connection only reloads discovery and never executes a test.
There is no built-in demo execution or automatic fallback to sample messages.

In Docker, `127.0.0.1` refers to the Studio container itself. Configure
`VITE_TRACER_TARGET` to a reachable Tracer container/host address, or explicitly use
host networking for a local Tracer. Restart the dev/preview server after changing
the proxy target; token and other frontend environment changes require a rebuild.

Run summaries are retained in browser local storage because Tracer does not yet
provide a run-history endpoint. Saved real runs can be inspected while disconnected,
but rerunning requires a connection and uses the current test definition. Previously
generated demo entries are excluded from history and cannot appear as real results.
