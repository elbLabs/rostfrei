# Rostfrei Tracer Studio

Explore what your domain does, one command at a time.

Use the same Studio for development and production-hosted Tracer endpoints.
Choose a view directly: **Domain**, **Infrastructure**, **Read models**, **Live
flows**, **Correlations**, **Tests**, **Runs** or **Commands**. The endpoint
determines which application you're talking to. Each action determines which
state it uses: a behavioral test on a production-hosted Tracer uses that host’s
isolated Test runtime when configured; **Production dispatch** sends a real
command to its Normal runtime. Neither changing tabs nor browsing the domain
runs a test or sends a command. **Studio map** puts the available views around
a central hub. Choose a destination on the map or use its tab directly.

Give a production-hosted Test runtime its own storage, workers and downstream
test integrations. The framework isolates its Test traffic; a custom handler
that calls an external service still uses the dependencies you configured.

With an inspection token configured, Studio opens on **Domain**; a local setup
with only Test control opens on **Tests**. Existing builds configured with
`VITE_STUDIO_WORKSPACES=observe` keep the inspection-first preset: they open on
Infrastructure, omit Tests and Runs, and discover command controls on request.
That build setting changes the UI; the Tracer host enforces permissions.

Open the **Runs** tab to inspect a saved run on the canvas without
executing it again. **Run summary** opens its verdict, command outcome, capture
quality, and diagnostic details; **Back to graph** returns to the canvas. Reports
from the live behavioral-run endpoint contain no capture-window metadata and show
quality/settling unavailable. Separately saved finite captures retain their own
quality metadata; settled is not proof
of downstream completion. Select a message for its response and causal relationship;
expand request, metadata, scenario, or run details when needed. **Run again** is
an explicit action using the current test definition and isolated Test state.

Use **Run a test** in the **Tests** tab to choose and execute a test. Its floating panel
includes the optional Given/When/Then definition. Submitting uses the current
definition and resets isolated Test state to its fixture, then shows the result
on the canvas. Selecting a test from **Test library** in that tab
previews its expected flow without executing it.

**Ctrl+number** selects a view; arrow keys, Home and
End navigate its focused tab bar.
Switching views does not execute commands or tests. Commands, tests, runs,
live flows and correlations share the graph canvas.
Its desktop message inspector overlays the flow without changing node positions,
pan, or zoom. Click an empty area of the canvas to close it. Navigation and details
open on demand. Narrow screens use a message list and a back-to-flow detail view.

The theme picker in the top bar supports **Light**, **Dark**, and **System**.
System is the default; the browser preference is shared with the website through
`rostfrei-theme`. Open grey/rust surfaces, Signika headings, controls, and duck
branding come from `@rostfrei/ui`. Its UI-only evidence summary, disclosure, and
causal flow are also reusable in embedded readers; hosts own theme and data.
Message cards share a thin grey outline and a kind-colored left rail that remains
visible when selected. Fixture context retains a dashed rail in the full graph.

Open **Commands → Send command** (or **Ctrl/⌘ Shift K**) to use the catalog-driven command form.
**Test bus** uses
the isolated command pipeline and can append events and trigger integrations.
Test bus is the default; submission is disabled when unavailable. Commands
remain available even when the catalog has no behavioral-test repository.
Identifiers are editable, discovered payload options refresh after Test runs,
and numeric options retain their exact JSON representation. An unconfirmed Test
submission keeps its idempotency key and payload for an explicit retry.
**Simulate handler** is an optional explicit choice when advertised. It shows
the real handler decision, starting stream versions and proposed domain events
in a separate panel labelled **Uncommitted. There is no acceptance receipt**, never an observed
message graph. Injected dependencies are not automatically sandboxed; this does
not test delivery, persistence/concurrency or downstream integrations.
Start with the [bike-rental walkthrough](https://elblabs.github.io/rostfrei/docs/getting-started)
for a connected application. Studio requires a reachable Tracer API; synthetic
responses are limited to the browser-test and visual-inspection fixtures below.

**Infrastructure** takes a read-only sample of configured NATS sources.
Use it to check account quotas and usage, stream size, KV allocation, consumer
backlog, acknowledgements and the broker's evidence of a waiting or bound
consumer. A durable with no waiting pull could be busy, idle or offline. A
partial or inaccessible listing cannot establish that a resource is absent.
Hosts can also report individual NATS node health and process RAM.
**Domain** opens on a canvas of bounded contexts and aggregates. Open an
aggregate to see its entities and events grouped beneath it. Select a card to inspect its field types,
or select a bounded context and expand a registered command to see its payload;
close the overlay or press **Esc** to return to the same spot. The canvas works
on narrow screens too. Its links describe declarations, while **Live flows**
shows recorded messages.
**Read models** distinguishes declared storage policy
from the bytes reported by the backing broker, and can show a mapped container's
memory usage and limit separately. See [inventory setup and multi-service
coverage](../docs/nats-application-host.md#studio-operational-inventory).

**Production dispatch** is offered in Commands only when the host advertises it.
Enter the separate Dispatch credential for this
session and review the exact target and payload before publishing. An inspection
credential alone cannot dispatch. Do not put a dispatch credential in the Studio
build. A timeout after submission can leave the outcome unknown; retry only the
same request with its original idempotency key. Status refresh is read-only.

**Live flows** continuously follows application-originated domain and
integration events. It supports Test/Production scope discovery, filtering,
flow selection, pause/resume, reconnect, and explicit partial/retention status.
See [continuous observation](../docs/continuous-observation.md) for backend wiring
and the optional `VITE_TRACER_INSPECTION_TOKEN` used for production observation.
It uses the same canvas, message cards, and inspector as command and
test execution. **Commands** and **Tests** return to their saved view state; closing
the observation sidebar does not stop capture.

**Correlations** reads [retained correlation events](../docs/continuous-observation.md#retained-correlation-inspection)
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
the test panel, graph-first saved runs, reruns, rejection, failed expectations,
storage failure, and responsive views.
It also checks pre-paint theme initialization, theme selection/persistence,
system-theme changes, canvas colors, and the mobile theme picker above raised
command panels.
These fixtures are test-only and are not included in the application bundle.
The smoke test does not publish to a running Tracer. Set `CHROME_BIN` if Chrome
is installed outside the standard
locations; container environments that require it can set `CHROME_NO_SANDBOX=1`.

Use `pnpm test:ui --check saved-runs` to run just the saved-run inspection journey
at desktop and mobile widths. Selection failures retain screenshots, DOM state,
accessibility, and browser diagnostics under `artifacts/review/saved-run-inspection-<width>/`.

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
responses as visual fixtures. They cover the canvas overview, test panel, command form,
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
Anyone who can download the Studio bundle can read its `VITE_*` values. Serve a
token-configured build only to users entitled to that capability. For a private
inspection-only deployment, use an inspection-only host and omit the Control
and Dispatch credentials.

Run summaries are retained in browser local storage because Tracer does not yet
provide a run-history endpoint. Saved real runs can be inspected while disconnected,
but rerunning requires a connection and uses the current test definition. Previously
generated demo entries are excluded from history and cannot appear as real results.
