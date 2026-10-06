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
actually exposes. Keep domain events, integration events, Preview, and Test
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

Use Node.js 24 and pnpm. From `website/`:

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Routes live in `src/routes`. Documentation navigation and lazy MDX imports live
in `src/docs`, while authored documents live in `src/content/docs`.

After changing route files, regenerate the typed route tree:

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

GitHub Pages deployment is configured in
`.github/workflows/deploy-website.yml` at the repository root.
