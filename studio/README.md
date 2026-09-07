# Tracer Studio

Tracer Studio is a React/Vite client for Tracer behavioral tests and causal
message-series visualization.

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
