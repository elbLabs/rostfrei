# Working on Tracer Studio

Run commands from `studio/` (or use `pnpm --dir studio ...` from the repo root).
Install workspace dependencies from the repository root with
`pnpm install --frozen-lockfile` when needed.

## Frontend feedback loop

1. Choose the closest visual scenario with `pnpm inspect:ui --list`.
2. Capture before changing layout: for example,
   `pnpm inspect:ui --scene accepted --out artifacts/before`.
3. Read the generated `screenshot.png` with the image-capable Read tool. Read
   `state.json` for panel/node bounds and `diagnostics.json` for browser failures.
4. Make the change and capture the same scene and viewport with
   `--out artifacts/after`. Inspect the resulting image, including panel overlap,
   popup clipping, text wrapping, contrast, and graph visibility.
5. For responsive changes, also use `--scene narrow` or specify
    `--viewport 390x844` on the affected scene.
   Capture both `--theme light` and `--theme dark` when changing presentation.

Screenshots are available through Puppeteer even when the session has no
dedicated browser tool. Use the existing inspector instead of assuming source
or API inspection alone establishes how the frontend looks.

## Browser tools

- `pnpm inspect:ui --scene all`: capture every named fixture scene.
- `pnpm inspect:ui --scene rejected --headed`: open a fixture for interaction;
  Enter recaptures, `q` exits.
- `pnpm inspect:ui --url http://127.0.0.1:5174`: inspect an existing Studio.
  Use its actual running URL. This is a fresh browser session, with separate
  local storage and selections from the user's tab.
- `scripts/browser.mjs`: reusable Vite/Chrome launch, diagnostics, layout
  settling, and screenshot/state/accessibility capture helpers.
- `scripts/visual-scenarios.mjs`: deterministic API mocks and UI interaction
  recipes. Unknown fixture requests fail instead of reaching Tracer.

The inspector creates its own Vite server on an available port and cleans up
its server/browser. Set `CHROME_BIN` when Chrome is outside the known locations.
For slow cold starts, `--timeout 120000` raises the navigation deadline in
milliseconds (default 20000).
If a local server must be stopped manually, identify listeners with
`lsof -ti :<port> -sTCP:LISTEN`.

Artifacts go under `artifacts/inspect/` by default and are gitignored. Their
`state.json` records the scene, viewport, browser version, and live/mocked source.
Named fixtures freeze wall time/UUIDs and disable CSS motion; use `test:ui` for
motion and interaction checks. A fixture screenshot demonstrates presentation
with synthetic responses; real command behavior is verified through Tracer.

## Checks

- `pnpm lint`
- `pnpm build` (TypeScript project build and Vite production bundle)
- `pnpm test:ui` for changes to interactions, graph behavior, or shared browser
  tooling. This checks command forms, outcomes, dragging, panning, zooming,
  message inspection, graph stability, and mobile behavior.
- Capture affected scenarios for visual changes. `inspect:ui` fails on page
  exceptions, failed requests, unexpected HTTP errors, and missing mock routes;
  it saves artifacts on scenario failures where the page remains available.

## Code and presentation

- `src/App.tsx`: catalog discovery, canvas, panels, command execution, and run state.
- `src/components/command-panel.tsx`: catalog-driven command forms and results.
- `src/components/test-panel.tsx`: test selection, definition disclosure, and explicit execution.
- `src/components/message-graph.tsx`: message cards, React Flow, and the mobile list.
- `src/components/message-inspector.tsx`: selected message request, response, and metadata.
- `src/components/run-inspection.tsx`: optional saved-run summary, verdict, flow, and disclosures.
- `src/components/studio-sidebar.tsx`: Tests and run-history navigation.
- `src/components/draggable-panel.tsx`: shared draggable Command and Test panel shell.
- `src/lib/graph.ts`: message-to-graph conversion and layout.
- `src/lib/api.ts`: Tracer HTTP client.
- `scripts/fixtures.mjs`: test-only imports of the canonical bike-rental fixtures
  and behavioral definitions. Runtime demo fallback is intentionally absent.
- `src/index.css`: Studio layout and visual styling.

Use the shared open grey/rust palette, Signika headings, system body typography,
and theme-aware message colors from `@rostfrei/ui`. Prefer open reading surfaces
and details on demand. Reuse `@rostfrei/ui/evidence` for result summaries, message
lists, and disclosure; those components display supplied facts without interpreting
evidence. Studio CSS owns application layout and graph presentation.
Prefer accessible control names and existing `data-graph-node`/`data-graph-edge`
attributes for automation. Preserve the distinction between causation, fixture
stream order, and fixture context when changing graph presentation.
