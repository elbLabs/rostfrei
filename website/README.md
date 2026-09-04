# Rostfrei website

The Rostfrei website contains the project landing page and MDX documentation.
It is a client-side React application built with Vite and TanStack Router.

## Development

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
