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
