# Rostfrei Tracer Studio

Explore what your domain does, one command at a time.

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

The theme picker in the top bar supports **Light**, **Dark**, and **System**.
System is the default; the browser preference is shared with the website through
`rostfrei-theme`. Reference ’15 colors, controls, typography, and duck branding
come from the workspace package `@rostfrei/ui`.

Open **Command** (or **Ctrl/⌘ Shift K**) to use the catalog-driven command form.
**Test bus** uses
the isolated command pipeline and can append events and trigger integrations.
Test bus is the default; submission is disabled when unavailable. Commands
remain available even when the catalog has no behavioral-test repository.
Identifiers are editable, discovered payload options refresh after Test runs,
and numeric options retain their exact JSON representation. An unconfirmed Test
submission keeps its idempotency key and payload for an explicit retry.
**Simulate handler** is an optional explicit choice when advertised. It shows
the real handler decision, starting stream versions and proposed domain events
in a separate **Uncommitted · no acceptance receipt** panel, never an observed
message graph. Injected dependencies are not automatically sandboxed; this does
not test delivery, persistence/concurrency or downstream integrations.
Start with the [bike-rental walkthrough](https://elblabs.github.io/rostfrei/docs/getting-started)
for a connected application. Studio requires a reachable Tracer API; synthetic
responses are limited to the browser-test and visual-inspection fixtures below.

The **Observe** panel continuously follows application-originated domain and
integration events. It supports Test/Production scope discovery, filtering,
flow selection, pause/resume, reconnect, and explicit partial/retention status.
See [continuous observation](../docs/continuous-observation.md) for backend wiring
and the optional `VITE_TRACER_INSPECTION_TOKEN` used for production observation.
It uses the same Canvas/Workbench message cards and inspector as command and
test execution. **Executions** returns to the previous execution view; closing
the observation sidebar does not stop capture.

The **Correlations** panel reads [retained correlation events](../docs/continuous-observation.md#retained-correlation-inspection)
that touched one business object.
Choose an anchor the Tracer advertises (or **Other aggregate type…** to type
`context/aggregate`), enter the anchor's aggregate ID and select a correlation,
or open a correlation ID directly. Its retained domain and integration events
appear with causation edges and their bounded context. Payloads for the anchor's
own events follow Tracer's payload policy; **Load payloads from** adds contexts,
including other aggregates in the anchor's context (`?include=context:<name>`,
which replays each touched stream). Only returned causes get graph edges; absent parents'
types and reasons for absence are unknown.
**Match payload field** narrows the list to one business entity: each row is
`context / event-type . field = value` (or a raw
`context/event-type.field=value` expression), rows are AND-ed and sent as
`match` parameters. A row on another context ticks its include automatically
and says so; applied filters appear as chips from Tracer's `matched` echo, with
Clear all. The last rows are remembered per anchor in this browser. Matching
sees payloads after the Tracer payload policy, so a redacting policy matches
nothing. Source cutoffs show exact sequence strings for each domain source
and the shared integration source. They are captured independently, not as an
atomic snapshot or proof of completion. Expired events are absent; truncated
reads may omit retained matches. Selecting an anchor chain uses that response's
events, payloads and cutoffs together. Refresh is manual. Correlation requests
use the read-only inspection token (`VITE_TRACER_INSPECTION_TOKEN`); the control
token (`VITE_TRACER_TOKEN`) keeps serving commands and tests and receives `403`
on these routes. These reads inspect Normal traffic, not isolated Test runs.

## Run locally

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm --dir studio dev
```

From `studio/`, run the Chrome/Puppeteer interaction smoke test with:

```sh
pnpm test:ui
```

The smoke test intercepts every Tracer request and uses deterministic HTTP
fixtures from `scripts/fixtures.mjs`, including connection failures and retry,
reruns, rejection, failed expectations, storage failure, and responsive layouts.
It also checks pre-paint theme initialization, theme selection/persistence,
system-theme changes, canvas colors, and the mobile theme picker above raised
command panels.
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
pnpm inspect:ui --scene all --theme light --out artifacts/after-light
pnpm inspect:ui --scene all --theme dark --out artifacts/after-dark
```

The inspector starts Vite on an available port and launches a fresh Chrome
session. Named scenarios intercept every API request and label their synthetic
responses as visual fixtures. They cover the workbench overview, command form,
accepted/rejected Test, pending and indeterminate results, a 25-message
branching graph, long payloads, continuous observation, anchored correlations
with their error states, and the narrow-screen inspector. Unknown API
requests fail rather than reaching a backend.

Each capture writes `screenshot.png`, `state.json`, `accessibility.json`, and
`diagnostics.json` under the gitignored `artifacts/inspect/` directory (or `--out`).
State includes the resolved theme, panel/card bounds, selection, inspector content, and viewport
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
