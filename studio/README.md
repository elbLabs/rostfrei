# Rostfrei Studio

A deliberately small browser client for the authenticated Tracer catalog and
its advertised behavioral-test links. The control token remains in page memory;
the Studio does not put it in URLs or browser storage.

## Workflow

1. Start a Tracer HTTP API on `http://127.0.0.1:1309`.
2. Start the Studio and open the URL printed by Vite.
3. Enter the control token and load the catalog. The default `/api` base uses
   the Vite proxy, including for root-relative hrefs advertised by the API.
4. Paste raw JSON or choose an advertised persisted definition. Validate or run
   it, then inspect the report and linked event streams.

The editor submits raw text unchanged. A persisted definition replaces editor
text only when its representation advertises a definition or self href.

## Commands

```sh
npm ci
npm run dev
npm run lint
npm run typecheck
npm run build
npm run preview
```
