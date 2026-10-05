import { Link } from "@tanstack/react-router"

export function SiteFooter() {
  return (
    <footer className="border-t border-border/70">
      <div className="mx-auto flex max-w-[1180px] flex-col gap-4 px-5 py-8 text-xs text-muted-foreground sm:flex-row sm:items-center sm:justify-between sm:px-8">
        <div>
          <p className="font-medium text-foreground">
            Rostfrei · Understand your code. Even when AI wrote it.
          </p>
          <p className="mt-2">
            Open source under EUPL-1.2. Built by elbtech.dev.
          </p>
        </div>
        <nav aria-label="Footer navigation" className="flex flex-wrap gap-5">
          <Link className="hover:text-foreground" to="/docs">
            Documentation
          </Link>
          <a
            className="hover:text-foreground"
            href="https://github.com/elbLabs/rostfrei"
          >
            GitHub
          </a>
          <a
            className="hover:text-foreground"
            href="https://github.com/elbLabs/rostfrei/blob/main/CHANGELOG.md"
          >
            Changelog
          </a>
          <a
            className="hover:text-foreground"
            href="https://github.com/elbLabs/rostfrei/blob/main/LICENSE"
          >
            License
          </a>
        </nav>
      </div>
    </footer>
  )
}
