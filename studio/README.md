# Tracer Studio

Tracer Studio is a React/Vite client for Tracer behavioral tests and causal
message-series visualization.

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

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Run the Chrome/Puppeteer interaction smoke test with:

```sh
pnpm test:ui
```

The smoke test intercepts every Tracer request and exercises demo data and
deterministic API fixtures, including reruns, rejection, failed expectations,
browser storage failure, and responsive layouts. It does not publish to a
running Tracer. Set `CHROME_BIN` if Chrome is installed outside the standard
locations; container environments that require it can set `CHROME_NO_SANDBOX=1`.

The development server proxies `/api` to `http://127.0.0.1:1309`. Override the
target or control token when needed:

```sh
VITE_TRACER_TARGET=http://127.0.0.1:1309 \
VITE_TRACER_TOKEN=local-development-token \
pnpm dev
```

When Tracer is unavailable, the Studio uses clearly labelled demo data. Past
run summaries are retained in browser local storage because Tracer does not yet
provide a run-history endpoint. Rerunning history uses the current test
definition and connection; the action is labelled explicitly when switching
between demo data and connected isolated Test execution.
