# Rostfrei website

The static Rostfrei project site documents the reference domain structure and
the current semantic macro surface.

```sh
pnpm install --frozen-lockfile
pnpm run dev
```

Before committing website changes, run:

```sh
pnpm run typecheck
pnpm run lint
pnpm run build
```

GitHub Pages builds the site through `.github/workflows/deploy-website.yml`.
