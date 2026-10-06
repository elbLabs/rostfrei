# Rostfrei website

The Rostfrei website contains the project landing page and MDX documentation.
It is a client-side React application built with Vite and TanStack Router.

## Public story and page metadata

The site introduces Rostfrei as **domain-driven, event-sourced Rust** with the
promise **Understand your code. Even when AI wrote it.** Lead with the human need
to understand and review an agent-assisted codebase. Support that promise with
specific capabilities: checked ownership, typed contracts, a compiled domain
model, event history, and inspectable command flows. Explain how developers and
coding agents use those capabilities, and keep claims tied to what the framework
actually exposes. Keep domain events, integration events, observation, and Test
distinct, and describe the current alpha status explicitly.

- Landing-page content lives in `src/components/page-shell`.
- The docs introduction explains the mental model; `getting-started.mdx` is the
  first-run path into the bike-rental example.
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

## Reference ’15 and shared components

The website and docs use `@rostfrei/ui` from `packages/ui`. Run `pnpm dev:ui`
from the repository root for the separate local gallery on port 4177. Its source
lives in `packages/ui/gallery` and exercises theme selection, inputs and
validation, button states, message badges, tabs, tables, popovers, dialogs, and
the inspector sheet. The preview example is local to the gallery.

- Shared tokens and primitives belong in `packages/ui`; app layout lives in
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
desktop/mobile widths. Set `CHROME_BIN`
when Chrome is not in a standard location. Captures go to `artifacts/website/`.

## Identity studies

Open `/design-study/` on the development server to compare website and Tracer
Studio design concepts in light and dark mode. Reference ’15 is the selected default;
Workshop ’12 and Reference ’15 explore 2010s developer-web aesthetics. The static
preview also runs with `python3 -m http.server 4178 --directory website/public` from the
repository root. The current visual contract is documented in
[`packages/ui/README.md`](../packages/ui/README.md).
