# Reference ’15: shared website and Studio rollout

## Goal

Make the selected Reference ’15 study the production visual system for the
website, documentation, and Tracer Studio: cool steel surfaces, rust actions and
selection, blue links, compact system typography, square panels, and the user's
duck. Support light, dark, and system themes through one implementation.

## Sequence and ownership

1. **Baseline (orchestrator): complete.** Fast-forwarded `website-rebuild` to
   `3ac3e63` on main, including #110 and the current Studio. Restored all design
   studies and original duck assets. Backup stash:
   `3f13dab98aec1aed80037818cbd27ed170a4b2ef`.
2. **Foundation (UI agent + orchestrator): complete.** Establish the root
   pnpm workspace and internal `@rostfrei/ui` package. Consolidate tokens,
   shadcn/Radix primitives, theme handling, and duck branding. Update installation,
   build, and CI entry points to use the shared lockfile.
3. **Website and docs (website agent): complete.** Reproduce the selected
   navigation-led layout using the shared library. Keep current technical copy
   and documentation routing. The live component gallery is a separate local
   library tool in `packages/ui/gallery` (`pnpm dev:ui`).
4. **Tracer Studio (Studio agent): complete.** Capture the current fixture
   baseline, then migrate chrome, controls, graph, forms, and inspector colors.
   Retain current execution, observation, navigation, and graph behavior.
5. **Integration (orchestrator): complete.** Review both migrations together,
   resolve shared API issues, and verify that both apps actually consume the same
   primitives, palette, branding, and theme behavior.
6. **Verification and preview (orchestrator): complete.** Run lint/build checks,
   Studio interaction tests and fixture captures, and website/gallery browser
   checks in both themes at desktop and narrow sizes. Inspect screenshots and
   publish local preview instructions.

## Shared contract

- Package sources live in `packages/ui/src`; React remains a peer dependency.
- Imports use `@rostfrei/ui/components/<name>`, `@rostfrei/ui/theme`,
  `@rostfrei/ui/brand`, `@rostfrei/ui/lib/utils`, and `@rostfrei/ui/styles.css`.
- Apps import Tailwind before the shared stylesheet and explicitly scan the
  package's source with Tailwind `@source`.
- Existing shadcn component APIs are retained. Sizes include compact and normal
  controls rather than separate website/Studio implementations.
- `ThemeProvider`, `useTheme`, and `ThemeToggle` share the `rostfrei-theme`
  storage key and `.light`/`.dark` root classes.
- Both Vite configs use `referenceTheme()` from `@rostfrei/ui/vite` to apply
  stored/system appearance before styles or React load. Shared portals use
  `--layer-overlay`; Studio isolates application panels in their own stacking
  context so raising a panel cannot cover a menu or tooltip.
- `Brand({ compact?, className? })` renders a noninteractive duck wordmark;
  `DuckMascot({ className?, alt? })` renders the full illustration. Parent links
  own navigation and accessible naming.
- App-specific graph layout, domain forms, documentation navigation, and business
  logic remain in their apps. Shared tokens also cover message kinds and statuses.
- A real-component gallery exercises controls, panels, inputs, statuses,
  overlays, focus, disabled/invalid states, and theme selection.

## Acceptance checks

- A frozen root install resolves all three packages; both production builds pass.
- Shared source is typechecked/linted, and app lint checks pass.
- Website/docs work with a deployment base path as well as `/`; the local
  component gallery runs independently from the public website.
- Light/dark/system selection and duck artwork work in both apps.
- Studio smoke tests retain graph dragging, panning/zooming, command results,
  observation and message inspection behavior.
- Fixture screenshots and website captures show readable contrast and no
  document overflow at mobile widths.

The original static study stays available at `/design-study/` as the selected
design reference. The live gallery documents the production components.

## Verification record

- Frozen root installation passed; both apps resolve the workspace library and
  React 19.2.8 from the shared lockfile.
- Root `pnpm lint` and `pnpm build` passed for all frontend packages.
- Website browser checks cover five production routes plus the independent local
  gallery, light/dark themes, and 1440/390/320px widths (36 combinations per run).
  Both `/rostfrei/` deployment paths and the root preview have been exercised.
  Gallery tabs, overlay dismissal/focus return, and live system-theme changes passed.
- Studio's full mocked-API interaction suite passed. Its 22 light/dark fixture
  captures include accepted/rejected results, command forms, observation, branching,
  loading, indeterminate outcomes, long payloads, and narrow layouts. Captures
  contain no document overflow, page exceptions, or unexpected request failures.
- Independent review identified and resolved a mobile theme-menu stacking issue
  and a light startup flash before React loads. Regression checks pass with app
  modules blocked during startup, and with the command panel raised above its
  normal layer. The mobile menu remains visible and clickable; its final capture
  is in `studio/artifacts/reference-final-theme-menu/`.

Local artifacts are gitignored: `artifacts/website/` and
`studio/artifacts/reference-{before,after-light,after-dark}/`.

## Review locally

```sh
pnpm build
pnpm --dir website preview --host 127.0.0.1 --port 4178 --strictPort
```

- Website: <http://localhost:4178/>
- Local component gallery: run `pnpm dev:ui`, then <http://localhost:4177/>
- Original design studies: <http://localhost:4178/design-study/>

On the development host, the built website preview is running independently in
the `rostfrei-design-preview` user service on port 4178. The existing SSH tunnel
continues to work. Studio's fixture captures document its presentation; normal
Studio development uses `pnpm --dir studio dev` with the configured Tracer API.

Follow-up: the component gallery was moved out of the website into the shared
library's local tooling. Public gallery links and its route were removed, along
with the metadata strip beneath the website header.
