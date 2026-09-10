# Tracer Studio

Tracer Studio is a React/Vite client for executing commands, running Tracer
behavioral tests, and visualizing causal message series. The Command pane offers
Preview, which reads isolated Test history without publishing, and Test bus
publication, which is the default and can append events and trigger Test-scoped
integrations. A valid Test push submits immediately, closes the Command pane,
and displays its message flow. Commands are discovered from Catalog v1 by
bounded context; identifiers are prefilled editable payload fields.

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Run the Chrome/Puppeteer interaction smoke test with:

```sh
pnpm test:ui
```

## Browser inspection

Capture a reproducible UI state with the installed Chrome browser (or set
`CHROME_BIN` to its executable):

```sh
pnpm inspect:ui --scene accepted
pnpm inspect:ui --scene all
pnpm inspect:ui --scene command --viewport 390x844
```

The inspector starts its own Vite server on an available port, opens a fresh
browser context, and drives the real Studio controls. Named scenarios mock all
API fetches, reuse the demo test definitions and fixtures, and label their test
and command names as visual fixtures. They run without a Tracer backend. Unknown
API requests fail instead of falling through to a backend.

| Scene           | Captured state                                         |
| --------------- | ------------------------------------------------------ |
| `overview`      | Expected rental flow with Tests open                   |
| `command`       | Populated catalog-driven command form                  |
| `accepted`      | Accepted Preview with command details open             |
| `rejected`      | Business rejection with command details open           |
| `loading`       | Pending Preview and observing indicator                |
| `indeterminate` | Mocked ambiguous Test submission and retry details     |
| `branching`     | 25 synthetic messages after clicking Fit graph to view |
| `long-payload`  | Long strings and nested properties in a command popup  |
| `narrow`        | Accepted command popup at 390 × 844                    |

Other scenes default to 1440 × 900. `--viewport WIDTHxHEIGHT` overrides either
default. Use `--list` or `--help` for the command reference.

Each capture writes four files to `artifacts/inspect/<scene>-<width>x<height>/`:

- `screenshot.png`: the rendered viewport, including clipping and overlays.
- `state.json`: scenario/source, browser version, panels, controls, graph nodes,
  edges, geometry, popup contents, and node-center visibility.
- `accessibility.json`: browser accessibility roles, names, and tree relationships.
- `diagnostics.json`: page exceptions, console warnings/errors, failed requests,
  HTTP errors, and unexpected fixture requests. The `indeterminate` scene declares
  its deliberate 503 in `expectedHttpErrors`.

Browser exceptions, failed requests, unexpected HTTP errors, and missing fixture
routes make the command fail after saving diagnostic artifacts. Console messages
are recorded for inspection. A successful capture verifies that the scenario
was reached; inspect the screenshot to assess the layout.

`artifacts/` is gitignored. Capturing the same scene and viewport replaces its
files; use `--out artifacts/before` and `--out artifacts/after` to retain both.
Fixture sessions freeze wall time and UUID generation and disable CSS motion.
Timers and React Flow interactions still run. Compare pixels using the same
Chrome version and operating system.

### Interactive and live inspection

```sh
pnpm inspect:ui --scene rejected --headed
pnpm inspect:ui --url http://127.0.0.1:5174 --headed
```

`--headed` keeps Chrome open for interaction. Press Enter in the terminal to
capture the current view again, or `q` to close it. The server and browser are
closed when the inspector exits.

`--url` selects the `live` scene and connects to an existing Studio, using its
configured API. Live inspection captures the initial view automatically; further
actions are manual. Its fresh browser context has separate local storage from
your normal browser. `state.json` records `source: "live"` or `source: "mocked"`;
the fixture connection badge is rendered by the real application.

### Extending the workflow

`scripts/browser.mjs` exports the shared Vite/Chrome launchers, `closeStudioBrowser`,
`observePage`, `settlePage`, and `capturePage` for custom Puppeteer interactions. The smoke test
uses the same launchers. `scripts/visual-scenarios.mjs` holds mock API responses
and named interaction recipes. Add a scenario there, then capture it using
`--scene NAME`. Keep scenario readiness checks tied to visible UI state.

## Tracer connection

The development server proxies `/api` to `http://127.0.0.1:1309`. Override the
target or control token when needed:

```sh
VITE_TRACER_TARGET=http://127.0.0.1:1309 \
VITE_TRACER_TOKEN=local-development-token \
pnpm dev
```

When Tracer is unavailable, the Studio uses clearly labelled demo data. Past
run summaries are retained in browser local storage because Tracer does not yet
provide a run-history endpoint.
