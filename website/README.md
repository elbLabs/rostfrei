# Rostfrei website

Rostfrei is a **batteries-included Rust framework for event-sourced applications**:
domain code, durable commands, messaging, read models, and runtime inspection.
Business rules stay in ordinary Rust.

## Explore the framework

- [Getting started](../docs/getting-started.mdx): copy a prompt into your coding
  agent to explore the bike-rental example with real Tracer evidence.
- [Introduction](../docs/introduction.mdx): the framework's pieces and execution model.
- [Documentation index](../docs/README.md): contracts, reference, and operations.
- [Agent tutorial guide](../docs/agent-tutorial.md): checkout-local setup and
  text-first guidance. Studio is an optional visual client.

The website presents these shared docs alongside the project landing page. It is
a client-side React application built with Vite and TanStack Router.

## Public story and page metadata

Introduce the whole application framework. Make “batteries included” concrete
through domain declarations, command execution, NATS and HTTP adapters, read
models, testing, and project tooling. Applications use one `rostfrei` dependency
and enable the adapters they need. Keep claims tied to current capabilities,
distinguish authoritative domain history from bounded runtime observations, and
describe the current alpha status explicitly.

- Landing-page content lives in `src/components/page-shell`.
- The docs introduction explains the mental model; `getting-started.mdx` is a
  short agent entry point with prerequisites and reference links. The tutorial
  takes place in the reader's agent, using the checkout's `docs/agent-tutorial.md`.
- `src/agent-prompt.ts` reads the prompt from the shared getting-started source.
  `AgentPrompt` supplies the landing and docs copy control, with a selectable
  text field and manual-copy fallback. Keep the root README's prompt aligned;
  the production browser check compares both clipboard results with it.
- `src/site.ts` owns page titles, descriptions, canonical URLs, and sharing
  metadata. Reference-page metadata follows the documentation navigation.
- `metadata-plugin.ts` emits an HTML entry for each docs URL so link previews
  receive page-specific metadata without running JavaScript. Page content still
  renders in React. `PageMetadata` updates the same tags on client navigation.
- The repository README, facade crate docs, and Studio introduction use the
  same terminology. Keep those entry points aligned when the positioning changes.

## Development

Use Node.js 24 and pnpm. From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm --dir website dev
```

Routes live in `src/routes`. All documentation is authored in the repository's
[`docs/`](../docs/README.md), shared by humans and agents. `src/docs/navigation.ts`
maps sources to URLs; `registry.ts` loads them directly. Add new pages to that
navigation and the shared index. Keep essential prose and examples in the source
files; MDX components may enhance their presentation.

Use relative file links in documentation. `docs-plugin.ts` resolves them to website
routes (preserving anchors), and sends repository-code links to GitHub. A production
build also emits `llms.txt` and raw sources under `docs/source/`, from the same files.
The macro explorer reads its authored examples from the documentation's Rust code
blocks, and the subject explorer reads its reference table from the shared Markdown.

After changing route files, regenerate the typed route tree from `website/`:

```sh
pnpm generate-routes
```

## Validation

Before committing website changes, run:

```sh
pnpm typecheck
pnpm lint
pnpm build
```

After the build, run `pnpm test:docs` from the repository root. Set `VITE_BASE_PATH`
to the build's base path. This checks source coverage, published raw pages,
the machine index, and relative-link routing.

GitHub Pages deployment is configured in
`.github/workflows/deploy-website.yml` at the repository root.

## Appearance and shared components

Keep the original `SiteLayout`, landing sections, docs sidebar, system typography,
and duck branding. The website's grey/rust palette lives in `src/theme.css`:
`#f4f4f5` / `#7e342a` in light mode and `#262626` / `#d3a498` in dark mode.
Reference pages use ordinary headings and visible code examples.

The website and docs use `@rostfrei/ui` from `packages/ui`. Run `pnpm dev:ui`
from the repository root for the separate local gallery on port 4177. Its source
lives in `packages/ui/gallery` and exercises theme selection, inputs and
validation, button states, message badges, tabs, tables, popovers, dialogs, and
the inspector sheet. The preview example is local to the gallery.

- Shared primitives and default tokens belong in `packages/ui`; app layout lives in
  `src/index.css` and `src/components/page-shell/SiteLayout.tsx`.
- `src/components/ui` compatibility exports and `src/lib/utils.ts` re-export the
  shared package. The carousel and sidebar are app-specific.
- The shared theme provider uses `rostfrei-theme` for light, dark, and system
  preferences. Code highlighting and diagrams consume the same CSS variables.
  The shared `referenceTheme()` Vite plugin applies the preference before first
  paint, including the generated documentation HTML entries.
- Use router links for internal routes and imported shared brand assets for
  artwork. `VITE_BASE_PATH=/rostfrei/ pnpm build` exercises deployment under a
  path prefix.

After building, run `pnpm test:website` from the repository root for production
browser checks of the website and local gallery in light/dark themes and
desktop/mobile widths. They cover header/sidebar/mobile navigation, theme boot,
real clipboard contents, denied and unavailable clipboard fallbacks, shared raw
documentation, and overflow. Set `CHROME_BIN`
when Chrome is not in a standard location. Captures go to `artifacts/website/`.

## Identity studies

Open `/design-study/` on the development server to compare website and Tracer
Studio design concepts in light and dark mode. Reference ’15 is the selected default;
Workshop ’12 and Reference ’15 explore 2010s developer-web aesthetics. The static
preview also runs with `python3 -m http.server 4178 --directory website/public` from the
repository root. The current visual contract is documented in
[`packages/ui/README.md`](../packages/ui/README.md).
